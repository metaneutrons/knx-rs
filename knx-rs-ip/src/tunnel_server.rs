// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP tunnel server — accepts incoming connections from ETS.
//!
//! Listens on UDP port 3671 and handles the server side of the
//! KNXnet/IP tunneling protocol. Supports simultaneous routing
//! (multicast) and tunneling (unicast) on the same port.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{
    ConnectionHeader, DEVICE_MGMT_CONNECTION, E_CONNECTION_ID, E_CONNECTION_TYPE, E_NO_ERROR,
    E_NO_MORE_CONNECTIONS, Hpai, KnxIpFrame, ServiceType, TUNNEL_IA_BASE, tunnel_crd,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::error::KnxIpError;
use crate::router::{KNX_MULTICAST_ADDR, KNX_PORT};
use crate::tunnel::{MAX_RETRIES, RECV_BUF_SIZE, REQUEST_TIMEOUT};

/// Maximum simultaneous tunnel connections.
const MAX_TUNNELS: usize = 4;

/// Tunnel timeout: close if no heartbeat in 120 seconds (per KNX spec).
const TUNNEL_TIMEOUT_SECS: u64 = 120;

/// How often the server sweeps for tunnels that have stopped sending
/// heartbeats; must be well below [`TUNNEL_TIMEOUT_SECS`].
const CLEANUP_INTERVAL_SECS: u64 = 30;

/// A tunnel client connection.
struct TunnelClient {
    channel_id: u8,
    ctrl_addr: SocketAddr,
    data_addr: SocketAddr,
    send_seq: u8,
    recv_seq: u8,
    last_heartbeat: tokio::time::Instant,
}

/// Incoming frame from a tunnel client or multicast.
#[derive(Debug)]
pub enum ServerEvent {
    /// A CEMI frame received from a tunnel client.
    TunnelFrame(CemiFrame),
    /// A CEMI frame received from multicast routing.
    RoutingFrame(CemiFrame),
}

/// KNXnet/IP device server — handles both routing and tunneling.
///
/// This is what makes a device programmable by ETS.
pub struct DeviceServer {
    rx: mpsc::Receiver<ServerEvent>,
    tx_cmd: mpsc::Sender<ServerCmd>,
    local_addr: SocketAddr,
}

enum ServerCmd {
    /// Send a CEMI frame to all tunnel clients and/or multicast.
    SendFrame(CemiFrame),
    /// Send a CEMI frame to a specific tunnel client (response).
    SendToTunnel(u8, CemiFrame),
    /// Stop the server.
    Stop,
}

impl DeviceServer {
    /// Start the device server.
    ///
    /// Binds to `0.0.0.0:3671`, joins the KNX multicast group, and
    /// accepts incoming tunnel connections.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created.
    pub async fn start(local_addr: Ipv4Addr) -> Result<Self, KnxIpError> {
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, KNX_PORT);
        let socket = UdpSocket::bind(bind_addr).await?;
        let bound_addr = socket.local_addr()?;

        socket
            .join_multicast_v4(KNX_MULTICAST_ADDR, local_addr)
            .map_err(|source| KnxIpError::Multicast {
                group: KNX_MULTICAST_ADDR.to_string(),
                source,
            })?;

        socket.set_multicast_loop_v4(false).ok();

        tracing::info!("KNXnet/IP device server started on port {KNX_PORT}");

        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        let multicast_target = Some(SocketAddr::V4(SocketAddrV4::new(
            KNX_MULTICAST_ADDR,
            KNX_PORT,
        )));

        tokio::spawn(server_task(socket, multicast_target, event_tx, cmd_rx));

        Ok(Self {
            rx: event_rx,
            tx_cmd: cmd_tx,
            local_addr: bound_addr,
        })
    }

    /// Start the device server on a specific unicast address.
    ///
    /// This is useful for tests, constrained deployments, and IPv6 tunneling
    /// endpoints. No multicast group is joined; routing multicast is disabled
    /// for this server instance.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created.
    pub async fn start_at(bind_addr: SocketAddr) -> Result<Self, KnxIpError> {
        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;

        tracing::info!(%local_addr, "KNXnet/IP device server started");

        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        tokio::spawn(server_task(socket, None, event_tx, cmd_rx));

        Ok(Self {
            rx: event_rx,
            tx_cmd: cmd_tx,
            local_addr,
        })
    }

    /// Local UDP address the server is bound to.
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Receive the next event (incoming frame from tunnel or multicast).
    pub async fn recv(&mut self) -> Option<ServerEvent> {
        self.rx.recv().await
    }

    /// Send a CEMI frame to all connected tunnel clients and multicast.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the server is closed.
    pub async fn send_frame(&self, frame: CemiFrame) -> Result<(), KnxIpError> {
        self.tx_cmd
            .send(ServerCmd::SendFrame(frame))
            .await
            .map_err(|_| KnxIpError::Closed)
    }

    /// Send a response frame to a specific tunnel client.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the server is closed.
    pub async fn send_to_tunnel(&self, channel_id: u8, frame: CemiFrame) -> Result<(), KnxIpError> {
        self.tx_cmd
            .send(ServerCmd::SendToTunnel(channel_id, frame))
            .await
            .map_err(|_| KnxIpError::Closed)
    }

    /// Stop the server.
    pub async fn stop(&self) {
        let _ = self.tx_cmd.send(ServerCmd::Stop).await;
    }
}

// ── Server task ───────────────────────────────────────────────

async fn server_task(
    socket: UdpSocket,
    multicast_target: Option<SocketAddr>,
    event_tx: mpsc::Sender<ServerEvent>,
    mut cmd_rx: mpsc::Receiver<ServerCmd>,
) {
    let mut tunnels: Vec<TunnelClient> = Vec::new();
    let mut next_channel_id: u8 = 1;
    let mut buf = [0u8; RECV_BUF_SIZE];
    let cleanup = tokio::time::interval(tokio::time::Duration::from_secs(CLEANUP_INTERVAL_SECS));
    tokio::pin!(cleanup);

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                let (n, src) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "server recv error");
                        break;
                    }
                };
                handle_packet(
                    &buf[..n], src, &socket, &event_tx,
                    &mut tunnels, &mut next_channel_id,
                ).await;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ServerCmd::SendFrame(cemi)) => {
                        let stashed = send_to_all(
                            &socket,
                            multicast_target.as_ref(),
                            &mut tunnels,
                            &cemi,
                        ).await;
                        for (data, src) in stashed {
                            handle_packet(
                                &data, src, &socket, &event_tx,
                                &mut tunnels, &mut next_channel_id,
                            ).await;
                        }
                    }
                    Some(ServerCmd::SendToTunnel(ch, cemi)) => {
                        let stashed = send_to_tunnel_client(&socket, &mut tunnels, ch, &cemi).await;
                        for (data, src) in stashed {
                            handle_packet(
                                &data, src, &socket, &event_tx,
                                &mut tunnels, &mut next_channel_id,
                            ).await;
                        }
                    }
                    Some(ServerCmd::Stop) | None => break,
                }
            }

            _ = cleanup.tick() => {
                cleanup_stale_tunnels(&mut tunnels);
            }
        }
    }

    tracing::debug!("device server task ended");
}

async fn handle_packet(
    data: &[u8],
    src: SocketAddr,
    socket: &UdpSocket,
    event_tx: &mpsc::Sender<ServerEvent>,
    tunnels: &mut Vec<TunnelClient>,
    next_channel_id: &mut u8,
) {
    let Ok(frame) = KnxIpFrame::parse(data) else {
        return;
    };

    match frame.service_type {
        ServiceType::RoutingIndication => {
            if let Ok(cemi) = CemiFrame::parse(&frame.body) {
                let _ = event_tx.send(ServerEvent::RoutingFrame(cemi)).await;
            }
        }
        ServiceType::ConnectRequest => {
            handle_connect(socket, src, &frame, tunnels, next_channel_id).await;
        }
        ServiceType::ConnectionStateRequest => {
            handle_heartbeat(socket, src, &frame, tunnels).await;
        }
        ServiceType::DisconnectRequest => {
            handle_disconnect(socket, src, &frame, tunnels).await;
        }
        ServiceType::TunnelingRequest => {
            handle_tunneling_request(socket, src, &frame, tunnels, event_tx).await;
        }
        ServiceType::TunnelingAck => {} // client ack, nothing to do
        ServiceType::SearchRequest => {
            // Discovery — respond with device info (simplified)
            tracing::debug!("search request from {src}");
        }
        _ => {
            tracing::trace!(service = ?frame.service_type, "ignoring");
        }
    }
}

async fn handle_connect(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    tunnels: &mut Vec<TunnelClient>,
    next_channel_id: &mut u8,
) {
    if frame.body.len() < 20 {
        return;
    }

    // Parse control and data HPAI. IPv6 peers are represented by standard
    // KNXnet/IP NAT-mode HPAI (0.0.0.0:port) and authenticated by `src`.
    let Some(ctrl_hpai) = Hpai::parse(&frame.body) else {
        return;
    };
    let data_offset = usize::from(Hpai::LEN);
    let Some(data_hpai) = Hpai::parse(&frame.body[data_offset..]) else {
        return;
    };

    let ctrl_addr = hpai_to_socket_addr(ctrl_hpai, src);
    let data_addr = hpai_to_socket_addr(data_hpai, src);

    // CRI: byte 16 = length, byte 17 = connection type
    let cri_offset = data_offset + usize::from(Hpai::LEN);
    let conn_type = frame.body.get(cri_offset + 1).copied().unwrap_or(0);

    let port = socket.local_addr().map_or(KNX_PORT, |addr| addr.port());

    // Only tunnel connections are supported; reject device-management (and any
    // other) connection types explicitly rather than mis-handling them as tunnels.
    if conn_type == DEVICE_MGMT_CONNECTION {
        tracing::debug!(%ctrl_addr, "rejecting device-management connection");
        if let Some(resp) = build_connect_response(0, E_CONNECTION_TYPE, 0, port) {
            let _ = socket.send_to(&resp, ctrl_addr).await;
        }
        return;
    }

    if tunnels.len() >= MAX_TUNNELS {
        // No more connections available
        if let Some(resp) = build_connect_response(0, E_NO_MORE_CONNECTIONS, 0, port) {
            let _ = socket.send_to(&resp, ctrl_addr).await;
        }
        return;
    }

    // TUN-7: Find an unused channel ID (skip IDs already in use)
    let mut channel_id = *next_channel_id;
    let mut attempts = 0u16;
    while tunnels.iter().any(|t| t.channel_id == channel_id) {
        channel_id = channel_id.wrapping_add(1);
        if channel_id == 0 {
            channel_id = 1;
        }
        attempts += 1;
        if attempts > 255 {
            // All channel IDs exhausted — reject connection
            return;
        }
    }
    *next_channel_id = channel_id.wrapping_add(1);
    if *next_channel_id == 0 {
        *next_channel_id = 1;
    }

    tunnels.push(TunnelClient {
        channel_id,
        ctrl_addr,
        data_addr,
        send_seq: 0,
        recv_seq: 0,
        last_heartbeat: tokio::time::Instant::now(),
    });

    tracing::info!(channel_id, %ctrl_addr, "tunnel client connected");

    let assigned_ia = TUNNEL_IA_BASE | u16::from(channel_id);
    if let Some(resp) = build_connect_response(channel_id, E_NO_ERROR, assigned_ia, port) {
        let _ = socket.send_to(&resp, ctrl_addr).await;
    }
}

fn hpai_to_socket_addr(hpai: Hpai, src: SocketAddr) -> SocketAddr {
    if hpai.is_unspecified() {
        return socket_addr_with_port(src, hpai.port);
    }
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(hpai.ip), hpai.port))
}

const fn socket_addr_with_port(src: SocketAddr, port: u16) -> SocketAddr {
    let port = if port == 0 { src.port() } else { port };
    match src {
        SocketAddr::V4(v4) => SocketAddr::V4(SocketAddrV4::new(*v4.ip(), port)),
        SocketAddr::V6(v6) => SocketAddr::V6(SocketAddrV6::new(
            *v6.ip(),
            port,
            v6.flowinfo(),
            v6.scope_id(),
        )),
    }
}

fn serialize_frame(frame: &KnxIpFrame) -> Option<Vec<u8>> {
    frame.try_to_bytes().ok()
}

fn build_connect_response(
    channel_id: u8,
    status: u8,
    individual_addr: u16,
    port: u16,
) -> Option<Vec<u8>> {
    let hpai = Hpai::nat_udp(port);
    let mut body = Vec::with_capacity(12);
    body.push(channel_id);
    body.push(status);
    body.extend_from_slice(&hpai.to_bytes());
    body.extend_from_slice(&tunnel_crd(individual_addr));

    let frame = KnxIpFrame {
        service_type: ServiceType::ConnectResponse,
        body,
    };
    serialize_frame(&frame)
}

async fn handle_heartbeat(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    tunnels: &mut [TunnelClient],
) {
    if frame.body.is_empty() {
        return;
    }
    let channel_id = frame.body[0];

    let mut dst = src;
    let tunnel = tunnels
        .iter_mut()
        .find(|t| t.channel_id == channel_id && t.ctrl_addr == src);
    let status = if let Some(t) = tunnel {
        t.last_heartbeat = tokio::time::Instant::now();
        dst = t.ctrl_addr;
        E_NO_ERROR
    } else {
        E_CONNECTION_ID
    };

    let resp = KnxIpFrame {
        service_type: ServiceType::ConnectionStateResponse,
        body: vec![channel_id, status],
    };
    if let Some(bytes) = serialize_frame(&resp) {
        let _ = socket.send_to(&bytes, dst).await;
    }
}

async fn handle_disconnect(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    tunnels: &mut Vec<TunnelClient>,
) {
    if frame.body.is_empty() {
        return;
    }
    let channel_id = frame.body[0];

    let ctrl_addr = tunnels
        .iter()
        .find(|t| t.channel_id == channel_id && t.ctrl_addr == src)
        .map(|t| t.ctrl_addr);

    let status = if ctrl_addr.is_some() {
        tunnels.retain(|t| t.channel_id != channel_id);
        tracing::info!(channel_id, "tunnel client disconnected");
        E_NO_ERROR
    } else {
        E_CONNECTION_ID
    };

    let resp = KnxIpFrame {
        service_type: ServiceType::DisconnectResponse,
        body: vec![channel_id, status],
    };
    if let Some(bytes) = serialize_frame(&resp) {
        let _ = socket.send_to(&bytes, ctrl_addr.unwrap_or(src)).await;
    }
}

async fn handle_tunneling_request(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    tunnels: &mut [TunnelClient],
    event_tx: &mpsc::Sender<ServerEvent>,
) {
    let Some(ch) = ConnectionHeader::parse(&frame.body) else {
        return;
    };

    let tunnel = tunnels.iter_mut().find(|t| t.channel_id == ch.channel_id);
    let Some(tunnel) = tunnel else {
        send_tunneling_ack(
            socket,
            src,
            ch.channel_id,
            ch.sequence_counter,
            E_CONNECTION_ID,
        )
        .await;
        return;
    };

    if tunnel.data_addr != src {
        send_tunneling_ack(
            socket,
            src,
            ch.channel_id,
            ch.sequence_counter,
            E_CONNECTION_ID,
        )
        .await;
        return;
    }

    // Send ACK
    send_tunneling_ack(
        socket,
        tunnel.data_addr,
        ch.channel_id,
        ch.sequence_counter,
        E_NO_ERROR,
    )
    .await;

    // Deduplicate
    if ch.sequence_counter != tunnel.recv_seq {
        return;
    }
    tunnel.recv_seq = tunnel.recv_seq.wrapping_add(1);

    // Parse CEMI
    let cemi_data = &frame.body[ConnectionHeader::LEN as usize..];
    if let Ok(cemi) = CemiFrame::parse(cemi_data) {
        let _ = event_tx.send(ServerEvent::TunnelFrame(cemi)).await;
    }
}

async fn send_tunneling_ack(
    socket: &UdpSocket,
    dst: SocketAddr,
    channel_id: u8,
    sequence_counter: u8,
    status: u8,
) {
    let ack = KnxIpFrame::tunneling_ack(channel_id, sequence_counter, status);
    if let Some(bytes) = serialize_frame(&ack) {
        let _ = socket.send_to(&bytes, dst).await;
    }
}

async fn send_to_all(
    socket: &UdpSocket,
    multicast: Option<&SocketAddr>,
    tunnels: &mut [TunnelClient],
    cemi: &CemiFrame,
) -> Vec<(Vec<u8>, SocketAddr)> {
    // Send as routing indication to multicast
    if let Some(multicast) = multicast {
        let routing = KnxIpFrame::routing_indication(cemi.as_bytes());
        if let Some(bytes) = serialize_frame(&routing) {
            let _ = socket.send_to(&bytes, multicast).await;
        }
    }

    // Send as tunneling indication to all tunnel clients
    let mut stashed = Vec::new();
    for tunnel in tunnels.iter_mut() {
        stashed.extend(send_tunneling_to(socket, tunnel, cemi).await);
    }
    stashed
}

async fn send_to_tunnel_client(
    socket: &UdpSocket,
    tunnels: &mut [TunnelClient],
    channel_id: u8,
    cemi: &CemiFrame,
) -> Vec<(Vec<u8>, SocketAddr)> {
    let tunnel = tunnels.iter_mut().find(|t| t.channel_id == channel_id);
    if let Some(tunnel) = tunnel {
        send_tunneling_to(socket, tunnel, cemi).await
    } else {
        Vec::new()
    }
}

/// Send a tunneling request to a client and wait for ack.
///
/// Returns any non-ack packets received during the ack wait so the caller
/// can re-process them. This is necessary because the server shares a single
/// UDP socket for all tunnels and routing.
async fn send_tunneling_to(
    socket: &UdpSocket,
    tunnel: &mut TunnelClient,
    cemi: &CemiFrame,
) -> Vec<(Vec<u8>, SocketAddr)> {
    let seq = tunnel.send_seq;
    let frame = KnxIpFrame::tunneling_request(tunnel.channel_id, seq, cemi.as_bytes());
    let Some(frame_bytes) = serialize_frame(&frame) else {
        return Vec::new();
    };
    let mut stashed: Vec<(Vec<u8>, SocketAddr)> = Vec::new();

    for attempt in 0..MAX_RETRIES {
        if let Err(e) = socket.send_to(&frame_bytes, tunnel.data_addr).await {
            tracing::debug!(channel = tunnel.channel_id, attempt = attempt + 1, error = %e, "send failed");
            continue;
        }

        // Wait for ack from this specific client
        match wait_for_tunneling_ack(
            socket,
            tunnel.data_addr,
            tunnel.channel_id,
            seq,
            &mut stashed,
        )
        .await
        {
            Ok(()) => {
                tunnel.send_seq = seq.wrapping_add(1);
                return stashed;
            }
            Err(()) => {
                tracing::debug!(
                    channel = tunnel.channel_id,
                    attempt = attempt + 1,
                    "ack timeout"
                );
            }
        }
    }

    tracing::warn!(
        channel = tunnel.channel_id,
        "no ack after {MAX_RETRIES} retries"
    );
    stashed
}

/// Wait for a `TunnelingAck` matching the given channel and sequence.
/// Any other packets received are stashed for later re-processing.
async fn wait_for_tunneling_ack(
    socket: &UdpSocket,
    expected_src: SocketAddr,
    channel_id: u8,
    seq: u8,
    stashed: &mut Vec<(Vec<u8>, SocketAddr)>,
) -> Result<(), ()> {
    let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    let mut buf = [0u8; RECV_BUF_SIZE];

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(());
        }

        let Ok(Ok((n, src))) = tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await
        else {
            return Err(());
        };

        // Check if this is our ack
        if let Ok(frame) = KnxIpFrame::parse(&buf[..n])
            && frame.service_type == ServiceType::TunnelingAck
            && let Some(ack_ch) = ConnectionHeader::parse(&frame.body)
            && src == expected_src
            && ack_ch.channel_id == channel_id
            && ack_ch.sequence_counter == seq
        {
            return Ok(());
        }

        // Not our ack — stash for re-processing
        stashed.push((buf[..n].to_vec(), src));
    }
}

fn cleanup_stale_tunnels(tunnels: &mut Vec<TunnelClient>) {
    let timeout = tokio::time::Duration::from_secs(TUNNEL_TIMEOUT_SECS);
    let now = tokio::time::Instant::now();
    tunnels.retain(|t| {
        let alive = now.duration_since(t.last_heartbeat) < timeout;
        if !alive {
            tracing::info!(channel_id = t.channel_id, "tunnel client timed out");
        }
        alive
    });
}
