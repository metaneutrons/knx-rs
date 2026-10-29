// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP tunnel server — accepts incoming connections from ETS.
//!
//! Listens on UDP port 3671 and handles the server side of the
//! KNXnet/IP tunneling protocol. Supports simultaneous routing
//! (multicast) and tunneling (unicast) on the same port.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{
    ConnectionHeader, DEVICE_MGMT_CONNECTION, E_CONNECTION_ID, E_CONNECTION_TYPE, E_NO_ERROR,
    E_NO_MORE_CONNECTIONS, Hpai, KnxIpFrame, ServiceType, TUNNEL_IA_BASE, tunnel_crd,
};
use knx_rs_core::message::MessageCode;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};

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

/// How many events for the application the server holds back, beyond the
/// channel's own capacity, when the application does not keep up.
const MAX_EVENT_BACKLOG: usize = 1024;

/// How many confirmations a tunnel client may be owed at once. A client that
/// sends faster than the server can confirm does not make the server hold an
/// unbounded queue; the excess is not confirmed.
const MAX_PENDING_CONFIRMATIONS: usize = 64;

/// A tunnel client connection.
struct TunnelClient {
    channel_id: u8,
    ctrl_addr: SocketAddr,
    data_addr: SocketAddr,
    send_seq: u8,
    recv_seq: u8,
    last_heartbeat: tokio::time::Instant,
    /// `L_Data.con` frames owed to this client for the `L_Data.req` it sent, in order.
    pending_confirmations: VecDeque<Vec<u8>>,
}

/// Incoming frame from a tunnel client or multicast.
#[derive(Debug)]
pub enum ServerEvent {
    /// A CEMI frame received from a tunnel client.
    TunnelFrame(CemiFrame),
    /// A CEMI frame received from multicast routing.
    RoutingFrame(CemiFrame),
}

/// Events on their way to the application.
///
/// The server task must never wait for the application. If it did, an
/// application that sends from the task that receives (a bus access unit that
/// answers each event with frames) could deadlock with it: the server blocked
/// on the full event channel, the application blocked on the full command
/// channel. An event that does not fit into the channel is held back here, in
/// order, and handed over as the application makes room.
struct EventQueue {
    tx: mpsc::Sender<ServerEvent>,
    backlog: VecDeque<ServerEvent>,
}

impl EventQueue {
    const fn new(tx: mpsc::Sender<ServerEvent>) -> Self {
        Self {
            tx,
            backlog: VecDeque::new(),
        }
    }

    /// Hand `event` to the application without waiting.
    fn push(&mut self, event: ServerEvent) {
        if self.backlog.is_empty() {
            match self.tx.try_send(event) {
                Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
                Err(mpsc::error::TrySendError::Full(event)) => self.backlog.push_back(event),
            }
        } else if self.backlog.len() < MAX_EVENT_BACKLOG {
            self.backlog.push_back(event);
        } else {
            tracing::warn!("application is too far behind, event dropped");
        }
    }

    /// Whether the server may read another datagram. Beyond the backlog limit
    /// datagrams stay in the socket: a client's request is then not
    /// acknowledged and the client repeats it, instead of the event being lost.
    fn has_room(&self) -> bool {
        self.backlog.len() < MAX_EVENT_BACKLOG
    }

    fn has_backlog(&self) -> bool {
        !self.backlog.is_empty()
    }

    /// Move the oldest held-back event into the channel once it has room.
    ///
    /// Cancel-safe: the event leaves the backlog only after a slot is reserved.
    async fn deliver(&mut self) {
        match self.tx.reserve().await {
            Ok(permit) => {
                if let Some(event) = self.backlog.pop_front() {
                    permit.send(event);
                }
            }
            // The application is gone; nobody is waiting for these.
            Err(_) => self.backlog.clear(),
        }
    }
}

/// What the server task keeps between datagrams.
struct ServerState {
    tunnels: Vec<TunnelClient>,
    next_channel_id: u8,
    events: EventQueue,
    /// Turns `true` when the server is to stop. Its sender is owned by the
    /// [`DeviceServer`], so dropping the server stops the task, too.
    stop: watch::Receiver<bool>,
}

impl ServerState {
    /// Whether the server was asked to stop, or its handle is gone.
    fn stopping(&self) -> bool {
        *self.stop.borrow() || self.stop.has_changed().is_err()
    }
}

/// Resolves when the server is to stop. Cancel-safe.
async fn stop_requested(stop: &mut watch::Receiver<bool>) {
    // Err means the sender is gone, which is a stop request as well.
    let _ = stop.wait_for(|stop| *stop).await;
}

/// KNXnet/IP device server — handles both routing and tunneling.
///
/// This is what makes a device programmable by ETS.
///
/// # Confirmations
///
/// A tunnel client's `L_Data.req` is confirmed, as a KNXnet/IP server does:
/// the server sends that client an `L_Data.con` (the same frame, confirm flag
/// clear) once it has taken the request over. The request still arrives as a
/// [`ServerEvent::TunnelFrame`]. Other message codes from a client are not
/// confirmed.
///
/// # Stopping
///
/// [`DeviceServer::shutdown`] stops the server and returns once its socket is
/// released, so the address can be bound again at once. [`DeviceServer::stop`]
/// and dropping the server request the same, and take effect as soon as the
/// server task next runs (also while it waits for a client's acknowledgement),
/// not after the queued frames have been sent. In all cases the tunnel clients
/// are sent a `DISCONNECT_REQUEST` on a best-effort basis.
pub struct DeviceServer {
    rx: mpsc::Receiver<ServerEvent>,
    tx_cmd: mpsc::Sender<ServerCmd>,
    local_addr: SocketAddr,
    stop: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

enum ServerCmd {
    /// Send a CEMI frame to all tunnel clients and/or multicast.
    SendFrame(CemiFrame),
    /// Send a CEMI frame to a specific tunnel client (response).
    SendToTunnel(u8, CemiFrame),
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

        let multicast_target = Some(SocketAddr::V4(SocketAddrV4::new(
            KNX_MULTICAST_ADDR,
            KNX_PORT,
        )));

        Ok(Self::spawn(socket, multicast_target, bound_addr))
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

        Ok(Self::spawn(socket, None, local_addr))
    }

    fn spawn(
        socket: UdpSocket,
        multicast_target: Option<SocketAddr>,
        local_addr: SocketAddr,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (stop_tx, stop_rx) = watch::channel(false);

        let task = tokio::spawn(server_task(
            socket,
            multicast_target,
            event_tx,
            cmd_rx,
            stop_rx,
        ));

        Self {
            rx: event_rx,
            tx_cmd: cmd_tx,
            local_addr,
            stop: stop_tx,
            task: Some(task),
        }
    }

    /// Local UDP address the server is bound to.
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Receive the next event (incoming frame from tunnel or multicast).
    ///
    /// The server never waits for the application: events are queued, in order,
    /// and the application may send frames from the task that receives. If it
    /// falls more than about a thousand events behind, the server stops reading
    /// datagrams (clients then repeat their requests) and, for frames that were
    /// already read, drops the excess and logs a warning.
    pub async fn recv(&mut self) -> Option<ServerEvent> {
        self.rx.recv().await
    }

    /// Send a CEMI frame to all connected tunnel clients and multicast.
    ///
    /// The frame goes out as an `L_Data.ind`: a frame built as `L_Data.req`
    /// (0x11), which is what a bus access unit queues, is sent with message
    /// code 0x29. Other message codes are sent as they are.
    ///
    /// Each tunnel client has to acknowledge the frame: it is sent up to three
    /// times, one second apart, and a client that never acknowledges is
    /// disconnected (and sent a `DISCONNECT_REQUEST` on a best-effort basis).
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
    /// An `L_Data.req` is sent as `L_Data.ind`, as for [`Self::send_frame`].
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

    /// Ask the server to stop.
    ///
    /// Returns at once; the server task ends as soon as it runs next, which
    /// is not necessarily before this returns. Use [`DeviceServer::shutdown`]
    /// to wait until the socket is released.
    #[expect(
        clippy::unused_async,
        reason = "async since 0.9; callers await it, removing that would break them"
    )]
    pub async fn stop(&self) {
        self.stop.send_replace(true);
    }

    /// Stop the server and wait until its socket is released.
    ///
    /// When this returns the address can be bound again. Frames still queued
    /// for the tunnel clients are not sent.
    pub async fn shutdown(mut self) {
        self.stop.send_replace(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

// ── Server task ───────────────────────────────────────────────

async fn server_task(
    socket: UdpSocket,
    multicast_target: Option<SocketAddr>,
    event_tx: mpsc::Sender<ServerEvent>,
    mut cmd_rx: mpsc::Receiver<ServerCmd>,
    stop: watch::Receiver<bool>,
) {
    let mut state = ServerState {
        tunnels: Vec::new(),
        next_channel_id: 1,
        events: EventQueue::new(event_tx),
        stop,
    };
    let mut buf = [0u8; RECV_BUF_SIZE];
    let cleanup = tokio::time::interval(tokio::time::Duration::from_secs(CLEANUP_INTERVAL_SECS));
    tokio::pin!(cleanup);

    loop {
        // Not `biased`: with a fixed order, a flood of datagrams (the socket arm is
        // always ready) would starve the commands and the hand-over of events to the
        // application. The random order serves every ready arm; the stop request is
        // checked here as well, so it is seen after at most one more datagram.
        if state.stopping() {
            break;
        }
        tokio::select! {
            () = stop_requested(&mut state.stop) => break,

            result = socket.recv_from(&mut buf), if state.events.has_room() => {
                let (n, src) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "server recv error");
                        break;
                    }
                };
                handle_packet(&buf[..n], src, &socket, &mut state).await;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ServerCmd::SendFrame(cemi)) => {
                        send_to_all(&socket, multicast_target.as_ref(), &mut state, &cemi).await;
                    }
                    Some(ServerCmd::SendToTunnel(ch, cemi)) => {
                        send_to_tunnel_client(&socket, &mut state, ch, &cemi).await;
                    }
                    None => break,
                }
            }

            () = state.events.deliver(), if state.events.has_backlog() => {}

            _ = cleanup.tick() => {
                cleanup_stale_tunnels(&mut state.tunnels);
            }
        }

        // Requests may have arrived during any of the above, also while it waited
        // for an acknowledgement.
        send_confirmations(&socket, &mut state).await;
    }

    // Tell the tunnel clients, best effort: nobody waits for the answers.
    let port = socket.local_addr().map_or(KNX_PORT, |addr| addr.port());
    for tunnel in &state.tunnels {
        let request = KnxIpFrame::disconnect_request(tunnel.channel_id, Hpai::nat_udp(port));
        if let Some(bytes) = serialize_frame(&request) {
            let _ = socket.send_to(&bytes, tunnel.ctrl_addr).await;
        }
    }

    tracing::debug!("device server task ended");
}

async fn handle_packet(data: &[u8], src: SocketAddr, socket: &UdpSocket, state: &mut ServerState) {
    let Ok(frame) = KnxIpFrame::parse(data) else {
        return;
    };

    match frame.service_type {
        ServiceType::RoutingIndication => {
            if let Ok(cemi) = CemiFrame::parse(&frame.body) {
                state.events.push(ServerEvent::RoutingFrame(cemi));
            }
        }
        ServiceType::ConnectRequest => {
            handle_connect(
                socket,
                src,
                &frame,
                &mut state.tunnels,
                &mut state.next_channel_id,
            )
            .await;
        }
        ServiceType::ConnectionStateRequest => {
            handle_heartbeat(socket, src, &frame, &mut state.tunnels).await;
        }
        ServiceType::DisconnectRequest => {
            handle_disconnect(socket, src, &frame, &mut state.tunnels).await;
        }
        ServiceType::TunnelingRequest => {
            handle_tunneling_request(socket, src, &frame, &mut state.tunnels, &mut state.events)
                .await;
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
        pending_confirmations: VecDeque::new(),
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
    events: &mut EventQueue,
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
        // The server owes the client a confirmation for each L_Data.req. It is
        // sent by the server task once this packet is dealt with: sending needs
        // the client's acknowledgement, and this function does not wait.
        if let Some(confirmation) = confirmation_of(&cemi) {
            if tunnel.pending_confirmations.len() < MAX_PENDING_CONFIRMATIONS {
                tunnel.pending_confirmations.push_back(confirmation);
            } else {
                tracing::warn!(
                    channel_id = ch.channel_id,
                    "too many unconfirmed requests, confirmation skipped"
                );
            }
        }
        events.push(ServerEvent::TunnelFrame(cemi));
    }
}

/// The `L_Data.con` for an `L_Data.req` a tunnel client sent: the same frame
/// with message code 0x2E and the confirm flag cleared (no error). `None` for
/// any other message code, which the server does not confirm.
///
/// A server confirms what it took over from a client; knxd does it in its
/// tunnelling server (`eibnetserver.cpp`, right before it forwards the frame)
/// and the C++ reference in `DataLinkLayer::dataRequestFromTunnel`.
fn confirmation_of(cemi: &CemiFrame) -> Option<Vec<u8>> {
    const CONFIRM_FLAG: u8 = 0x01;
    let bytes = cemi.as_bytes();
    if bytes.first() != Some(&(MessageCode::LDataReq as u8)) {
        return None;
    }
    // Message code, additional information length, additional information, then
    // control field 1, whose lowest bit is the confirm flag.
    let control_1 = 2 + usize::from(*bytes.get(1)?);
    let mut confirmation = bytes.to_vec();
    confirmation[0] = MessageCode::LDataCon as u8;
    *confirmation.get_mut(control_1)? &= !CONFIRM_FLAG;
    Some(confirmation)
}

/// Send the confirmations the tunnel clients are owed, oldest first. Every send
/// waits for the client's acknowledgement and serves other packets meanwhile,
/// which may owe more confirmations; those are sent in the same pass.
async fn send_confirmations(socket: &UdpSocket, state: &mut ServerState) {
    loop {
        if state.stopping() {
            return;
        }
        let Some((channel_id, confirmation)) = state.tunnels.iter_mut().find_map(|t| {
            t.pending_confirmations
                .pop_front()
                .map(|confirmation| (t.channel_id, confirmation))
        }) else {
            return;
        };
        send_tunneling_to(socket, state, channel_id, &confirmation).await;
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
    state: &mut ServerState,
    cemi: &CemiFrame,
) {
    let cemi = as_indication(cemi);

    // Send as routing indication to multicast
    if let Some(multicast) = multicast {
        let routing = KnxIpFrame::routing_indication(&cemi);
        if let Some(bytes) = serialize_frame(&routing) {
            let _ = socket.send_to(&bytes, multicast).await;
        }
    }

    // Send as tunneling indication to all tunnel clients. The table changes
    // while the acknowledgements are awaited (clients connect and leave), so
    // work from the channels that exist now.
    let channels: Vec<u8> = state.tunnels.iter().map(|t| t.channel_id).collect();
    for channel_id in channels {
        if state.stopping() {
            return;
        }
        send_tunneling_to(socket, state, channel_id, &cemi).await;
    }
}

/// The cEMI bytes a server puts on the wire for `cemi`.
///
/// A server delivers what happened on the bus to its clients, as an
/// `L_Data.ind` in a tunnelling request or a routing indication. `L_Data.req`
/// (0x11) is the client-to-server direction, but it is what a bus access unit
/// queues when it sends, and what a caller of [`DeviceServer::send_frame`]
/// naturally holds. knxd's tunnelling client and routing driver discard
/// anything but 0x29 ("Unexpected CEMI Type"); the C++ reference sets
/// `L_data_ind` in `DataLinkLayer::sendTelegram`. Every other message code,
/// `L_Data.con` for instance, is passed through.
fn as_indication(cemi: &CemiFrame) -> Cow<'_, [u8]> {
    let bytes = cemi.as_bytes();
    if bytes.first() == Some(&(MessageCode::LDataReq as u8)) {
        let mut indication = bytes.to_vec();
        indication[0] = MessageCode::LDataInd as u8;
        Cow::Owned(indication)
    } else {
        Cow::Borrowed(bytes)
    }
}

async fn send_to_tunnel_client(
    socket: &UdpSocket,
    state: &mut ServerState,
    channel_id: u8,
    cemi: &CemiFrame,
) {
    send_tunneling_to(socket, state, channel_id, &as_indication(cemi)).await;
}

/// Forget a tunnel client whose acknowledgements stayed away and tell it, best
/// effort, so that a client that is merely cut off knows to reconnect.
async fn drop_lost_tunnel(
    socket: &UdpSocket,
    tunnels: &mut Vec<TunnelClient>,
    channel_id: u8,
    data_addr: SocketAddr,
) {
    let Some(position) = tunnels
        .iter()
        .position(|t| t.channel_id == channel_id && t.data_addr == data_addr)
    else {
        return;
    };
    let tunnel = tunnels.remove(position);
    tracing::warn!(
        channel_id,
        client = %tunnel.ctrl_addr,
        "tunnel client dropped: no acknowledgement after {MAX_RETRIES} attempts"
    );
    let port = socket.local_addr().map_or(KNX_PORT, |addr| addr.port());
    let request = KnxIpFrame::disconnect_request(channel_id, Hpai::nat_udp(port));
    if let Some(bytes) = serialize_frame(&request) {
        let _ = socket.send_to(&bytes, tunnel.ctrl_addr).await;
    }
}

/// Send a tunneling request to a client and wait for its ack, up to
/// [`MAX_RETRIES`] times. A client that never acknowledges is dropped.
///
/// The tunnel is looked up again by channel id wherever the table may have
/// changed: packets are served while the ack is awaited.
async fn send_tunneling_to(
    socket: &UdpSocket,
    state: &mut ServerState,
    channel_id: u8,
    cemi: &[u8],
) {
    let Some((seq, data_addr)) = state
        .tunnels
        .iter()
        .find(|t| t.channel_id == channel_id)
        .map(|t| (t.send_seq, t.data_addr))
    else {
        return;
    };
    let frame = KnxIpFrame::tunneling_request(channel_id, seq, cemi);
    let Some(frame_bytes) = serialize_frame(&frame) else {
        // Cannot happen for a cEMI frame that came out of a CemiFrame; the
        // client is not at fault.
        return;
    };

    for attempt in 0..MAX_RETRIES {
        if let Err(e) = socket.send_to(&frame_bytes, data_addr).await {
            tracing::debug!(channel = channel_id, attempt = attempt + 1, error = %e, "send failed");
            continue;
        }

        match wait_for_tunneling_ack(socket, state, channel_id, data_addr, seq).await {
            AckWait::Received => {
                if let Some(tunnel) = state
                    .tunnels
                    .iter_mut()
                    .find(|t| t.channel_id == channel_id && t.data_addr == data_addr)
                {
                    tunnel.send_seq = seq.wrapping_add(1);
                }
                return;
            }
            AckWait::Gone | AckWait::Stopped => return,
            AckWait::TimedOut => {
                tracing::debug!(channel = channel_id, attempt = attempt + 1, "ack timeout");
            }
        }
    }

    tracing::debug!(channel = channel_id, "no ack after {MAX_RETRIES} attempts");
    drop_lost_tunnel(socket, &mut state.tunnels, channel_id, data_addr).await;
}

/// How waiting for a tunnelling ack ended.
enum AckWait {
    Received,
    TimedOut,
    /// The tunnel left (disconnect request) while the ack was awaited.
    Gone,
    /// The server was asked to stop.
    Stopped,
}

/// Wait for a `TunnelingAck` matching the given channel and sequence.
///
/// Every other packet is served at once rather than set aside: a tunnelling
/// request of this very client is acknowledged now (the client may wait for
/// that before it reads our frame, and both would wait for each other),
/// heartbeats are answered, other clients can connect.
async fn wait_for_tunneling_ack(
    socket: &UdpSocket,
    state: &mut ServerState,
    channel_id: u8,
    expected_src: SocketAddr,
    seq: u8,
) -> AckWait {
    let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    let mut buf = [0u8; RECV_BUF_SIZE];

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return AckWait::TimedOut;
        }

        let received = tokio::select! {
            biased;
            () = stop_requested(&mut state.stop) => return AckWait::Stopped,
            received = tokio::time::timeout(remaining, socket.recv_from(&mut buf)) => received,
        };
        let Ok(Ok((n, src))) = received else {
            return AckWait::TimedOut;
        };

        // Check if this is our ack
        if let Ok(frame) = KnxIpFrame::parse(&buf[..n])
            && frame.service_type == ServiceType::TunnelingAck
            && let Some(ack_ch) = ConnectionHeader::parse(&frame.body)
            && src == expected_src
            && ack_ch.channel_id == channel_id
            && ack_ch.sequence_counter == seq
        {
            return AckWait::Received;
        }

        handle_packet(&buf[..n], src, socket, state).await;
        if !state
            .tunnels
            .iter()
            .any(|t| t.channel_id == channel_id && t.data_addr == expected_src)
        {
            return AckWait::Gone;
        }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
    use knx_rs_core::types::Priority;

    fn event(ga: u16) -> ServerEvent {
        ServerEvent::TunnelFrame(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            IndividualAddress::from_raw(0x1102),
            DestinationAddress::Group(GroupAddress::from_raw(ga)),
            Priority::Low,
            &[0x00, 0x81],
        ))
    }

    fn ga(event: &ServerEvent) -> u16 {
        match event {
            ServerEvent::TunnelFrame(f) | ServerEvent::RoutingFrame(f) => {
                f.destination_address_raw()
            }
        }
    }

    #[test]
    fn a_request_is_confirmed_with_the_confirm_flag_cleared() {
        let request = CemiFrame::new_l_data(
            MessageCode::LDataReq,
            IndividualAddress::from_raw(0x1102),
            DestinationAddress::Group(GroupAddress::from_raw(0x0901)),
            Priority::Low,
            &[0x00, 0x81],
        );
        let mut raw = request.as_bytes().to_vec();
        raw[2] |= 0x01; // a client that set the confirm flag in its request
        let request = CemiFrame::parse(&raw).unwrap();

        let confirmation = confirmation_of(&request).unwrap();
        assert_eq!(confirmation[0], MessageCode::LDataCon as u8);
        assert_eq!(confirmation[2] & 0x01, 0, "confirm flag clear: no error");
        assert_eq!(
            confirmation[3..],
            raw[3..],
            "the rest is the client's frame"
        );
        assert_eq!(
            confirmation[2] | 0x01,
            raw[2],
            "no other control bit changed"
        );
    }

    #[test]
    fn only_a_request_is_confirmed() {
        assert!(confirmation_of(&event_frame(MessageCode::LDataInd)).is_none());
        assert!(confirmation_of(&event_frame(MessageCode::LDataCon)).is_none());
        assert!(confirmation_of(&event_frame(MessageCode::LDataReq)).is_some());
    }

    fn event_frame(code: MessageCode) -> CemiFrame {
        CemiFrame::new_l_data(
            code,
            IndividualAddress::from_raw(0x1102),
            DestinationAddress::Group(GroupAddress::from_raw(1)),
            Priority::Low,
            &[0x00, 0x81],
        )
    }

    #[tokio::test]
    async fn event_queue_holds_back_what_does_not_fit_and_keeps_the_order() {
        let (tx, mut rx) = mpsc::channel(2);
        let mut queue = EventQueue::new(tx);
        for i in 0..5 {
            queue.push(event(i)); // two fit, three are held back
        }
        assert_eq!(queue.backlog.len(), 3);

        let mut seen = Vec::new();
        while seen.len() < 5 {
            seen.push(ga(&rx.recv().await.unwrap()));
            if queue.has_backlog() {
                queue.deliver().await;
            }
        }
        assert_eq!(seen, [0, 1, 2, 3, 4]);
        assert!(!queue.has_backlog());
    }

    #[tokio::test]
    async fn event_queue_stops_reading_and_then_drops_beyond_its_limit() {
        let (tx, _rx) = mpsc::channel(1);
        let mut queue = EventQueue::new(tx);
        for i in 0..=u16::try_from(MAX_EVENT_BACKLOG).unwrap() {
            queue.push(event(i));
        }
        // One event is in the channel, the backlog is at its limit.
        assert_eq!(queue.backlog.len(), MAX_EVENT_BACKLOG);
        assert!(!queue.has_room(), "the server must stop reading datagrams");
        queue.push(event(9999));
        assert_eq!(
            queue.backlog.len(),
            MAX_EVENT_BACKLOG,
            "the excess is dropped"
        );
    }

    #[tokio::test]
    async fn event_queue_discards_its_backlog_when_the_application_is_gone() {
        let (tx, rx) = mpsc::channel(1);
        let mut queue = EventQueue::new(tx);
        queue.push(event(1));
        queue.push(event(2));
        assert!(queue.has_backlog());
        drop(rx);
        queue.deliver().await;
        assert!(!queue.has_backlog());
        queue.push(event(3)); // must not panic or grow
        assert!(!queue.has_backlog());
    }
}
