// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNXnet/IP tunnel server — accepts incoming connections from ETS.
//!
//! Listens on UDP port 3671 and handles the server side of the
//! KNXnet/IP tunneling protocol. Supports simultaneous routing
//! (multicast) and tunneling (unicast) on the same port.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use knx_core::cemi::CemiFrame;
use knx_core::knxip::{ConnectionHeader, HostProtocol, Hpai, KnxIpFrame, ServiceType};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::error::KnxIpError;
use crate::router::{KNX_MULTICAST_ADDR, KNX_PORT};

/// Maximum simultaneous tunnel connections.
const MAX_TUNNELS: usize = 4;

/// Tunnel timeout: close if no heartbeat in 120 seconds (per KNX spec).
const TUNNEL_TIMEOUT_SECS: u64 = 120;

/// A tunnel client connection.
struct TunnelClient {
    channel_id: u8,
    ctrl_addr: SocketAddr,
    data_addr: SocketAddr,
    send_seq: u8,
    recv_seq: u8,
    last_heartbeat: tokio::time::Instant,
    _is_config: bool,
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

        socket
            .join_multicast_v4(KNX_MULTICAST_ADDR, local_addr)
            .map_err(|e| KnxIpError::Protocol(format!("join multicast: {e}")))?;

        socket.set_multicast_loop_v4(false).ok();

        tracing::info!("KNXnet/IP device server started on port {KNX_PORT}");

        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        tokio::spawn(server_task(socket, event_tx, cmd_rx));

        Ok(Self {
            rx: event_rx,
            tx_cmd: cmd_tx,
        })
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
    event_tx: mpsc::Sender<ServerEvent>,
    mut cmd_rx: mpsc::Receiver<ServerCmd>,
) {
    let mut tunnels: Vec<TunnelClient> = Vec::new();
    let mut next_channel_id: u8 = 1;
    let mut buf = [0u8; 1024];
    let multicast_target = SocketAddr::V4(SocketAddrV4::new(KNX_MULTICAST_ADDR, KNX_PORT));
    let cleanup = tokio::time::interval(tokio::time::Duration::from_secs(30));
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
                        let stashed = send_to_all(&socket, &multicast_target, &mut tunnels, &cemi).await;
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
            handle_heartbeat(socket, &frame, tunnels).await;
        }
        ServiceType::DisconnectRequest => {
            handle_disconnect(socket, &frame, tunnels).await;
        }
        ServiceType::TunnelingRequest => {
            handle_tunneling_request(socket, &frame, tunnels, event_tx).await;
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

    // Parse control HPAI
    let ctrl_hpai = Hpai::parse(&frame.body[..8]);
    let data_hpai = Hpai::parse(&frame.body[8..16]);

    let ctrl_addr = ctrl_hpai.map_or(src, |h| {
        if h.ip == [0, 0, 0, 0] {
            src
        } else {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(h.ip), h.port))
        }
    });

    let data_addr = data_hpai.map_or(src, |h| {
        if h.ip == [0, 0, 0, 0] {
            src
        } else {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(h.ip), h.port))
        }
    });

    // CRI: byte 16 = length, byte 17 = connection type
    let conn_type = frame.body.get(17).copied().unwrap_or(0);
    let is_config = conn_type == 0x03; // DEVICE_MGMT_CONNECTION

    if tunnels.len() >= MAX_TUNNELS {
        // No more connections available
        let resp = build_connect_response(0, 0x24, 0); // E_NO_MORE_CONNECTIONS
        let _ = socket.send_to(&resp, ctrl_addr).await;
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
        _is_config: is_config,
    });

    tracing::info!(channel_id, %ctrl_addr, config = is_config, "tunnel client connected");

    let resp = build_connect_response(channel_id, 0x00, 0xFF00 | u16::from(channel_id)); // E_NO_ERROR
    let _ = socket.send_to(&resp, ctrl_addr).await;
}

fn build_connect_response(channel_id: u8, status: u8, individual_addr: u16) -> Vec<u8> {
    let hpai = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [0, 0, 0, 0],
        port: KNX_PORT,
    };
    let mut body = Vec::with_capacity(12);
    body.push(channel_id);
    body.push(status);
    body.extend_from_slice(&hpai.to_bytes());
    // CRD: connection response data block (tunnel, link layer, individual address)
    let addr = individual_addr.to_be_bytes();
    body.extend_from_slice(&[0x04, 0x04, addr[0], addr[1]]);

    let frame = KnxIpFrame {
        service_type: ServiceType::ConnectResponse,
        body,
    };
    frame.to_bytes()
}

async fn handle_heartbeat(socket: &UdpSocket, frame: &KnxIpFrame, tunnels: &mut [TunnelClient]) {
    if frame.body.is_empty() {
        return;
    }
    let channel_id = frame.body[0];

    let tunnel = tunnels.iter_mut().find(|t| t.channel_id == channel_id);
    let status = if let Some(t) = tunnel {
        t.last_heartbeat = tokio::time::Instant::now();
        0x00 // E_NO_ERROR
    } else {
        0x21 // E_CONNECTION_ID
    };

    let resp = KnxIpFrame {
        service_type: ServiceType::ConnectionStateResponse,
        body: vec![channel_id, status],
    };
    let _ = socket
        .send_to(
            &resp.to_bytes(),
            frame
                .body
                .get(2..)
                .and_then(|b| {
                    Hpai::parse(b).map(|h| {
                        if h.ip == [0, 0, 0, 0] {
                            // Can't determine source from HPAI, use channel's ctrl_addr
                            tunnels.iter().find(|t| t.channel_id == channel_id).map_or(
                                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
                                |t| t.ctrl_addr,
                            )
                        } else {
                            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(h.ip), h.port))
                        }
                    })
                })
                .unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))),
        )
        .await;
}

async fn handle_disconnect(
    socket: &UdpSocket,
    frame: &KnxIpFrame,
    tunnels: &mut Vec<TunnelClient>,
) {
    if frame.body.is_empty() {
        return;
    }
    let channel_id = frame.body[0];

    let ctrl_addr = tunnels
        .iter()
        .find(|t| t.channel_id == channel_id)
        .map(|t| t.ctrl_addr);

    tunnels.retain(|t| t.channel_id != channel_id);
    tracing::info!(channel_id, "tunnel client disconnected");

    let resp = KnxIpFrame {
        service_type: ServiceType::DisconnectResponse,
        body: vec![channel_id, 0x00],
    };
    if let Some(addr) = ctrl_addr {
        let _ = socket.send_to(&resp.to_bytes(), addr).await;
    }
}

async fn handle_tunneling_request(
    socket: &UdpSocket,
    frame: &KnxIpFrame,
    tunnels: &mut [TunnelClient],
    event_tx: &mpsc::Sender<ServerEvent>,
) {
    let Some(ch) = ConnectionHeader::parse(&frame.body) else {
        return;
    };

    let tunnel = tunnels.iter_mut().find(|t| t.channel_id == ch.channel_id);
    let Some(tunnel) = tunnel else { return };

    // Send ACK
    let ack_ch = ConnectionHeader {
        channel_id: ch.channel_id,
        sequence_counter: ch.sequence_counter,
        status: 0,
    };
    let ack = KnxIpFrame {
        service_type: ServiceType::TunnelingAck,
        body: ack_ch.to_bytes().to_vec(),
    };
    let _ = socket.send_to(&ack.to_bytes(), tunnel.data_addr).await;

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

async fn send_to_all(
    socket: &UdpSocket,
    multicast: &SocketAddr,
    tunnels: &mut [TunnelClient],
    cemi: &CemiFrame,
) -> Vec<(Vec<u8>, SocketAddr)> {
    // Send as routing indication to multicast
    let routing = KnxIpFrame {
        service_type: ServiceType::RoutingIndication,
        body: cemi.as_bytes().to_vec(),
    };
    let _ = socket.send_to(&routing.to_bytes(), multicast).await;

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

/// KNXnet/IP tunneling ack timeout (1 second per spec).
const TUNNELING_ACK_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(1);

/// Maximum tunneling request retries.
const TUNNELING_MAX_RETRIES: u8 = 3;

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
    let ch = ConnectionHeader {
        channel_id: tunnel.channel_id,
        sequence_counter: seq,
        status: 0,
    };

    let mut body = Vec::with_capacity(ConnectionHeader::LEN as usize + cemi.total_length());
    body.extend_from_slice(&ch.to_bytes());
    body.extend_from_slice(cemi.as_bytes());

    let frame = KnxIpFrame {
        service_type: ServiceType::TunnelingRequest,
        body,
    };
    let frame_bytes = frame.to_bytes();
    let mut stashed: Vec<(Vec<u8>, SocketAddr)> = Vec::new();

    for attempt in 0..TUNNELING_MAX_RETRIES {
        if let Err(e) = socket.send_to(&frame_bytes, tunnel.data_addr).await {
            tracing::debug!(channel = tunnel.channel_id, attempt = attempt + 1, error = %e, "send failed");
            continue;
        }

        // Wait for ack from this specific client
        match wait_for_tunneling_ack(socket, tunnel.channel_id, seq, &mut stashed).await {
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
        "no ack after {TUNNELING_MAX_RETRIES} retries"
    );
    stashed
}

/// Wait for a `TunnelingAck` matching the given channel and sequence.
/// Any other packets received are stashed for later re-processing.
async fn wait_for_tunneling_ack(
    socket: &UdpSocket,
    channel_id: u8,
    seq: u8,
    stashed: &mut Vec<(Vec<u8>, SocketAddr)>,
) -> Result<(), ()> {
    let deadline = tokio::time::Instant::now() + TUNNELING_ACK_TIMEOUT;
    let mut buf = [0u8; 1024];

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
        if let Ok(frame) = KnxIpFrame::parse(&buf[..n]) {
            if frame.service_type == ServiceType::TunnelingAck {
                if let Some(ack_ch) = ConnectionHeader::parse(&frame.body) {
                    if ack_ch.channel_id == channel_id && ack_ch.sequence_counter == seq {
                        return Ok(());
                    }
                }
            }
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
