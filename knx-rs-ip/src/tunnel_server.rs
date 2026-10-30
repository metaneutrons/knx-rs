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

use knx_rs_core::address::IndividualAddress;
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::dib::{
    DeviceInformation, KnxMedium, ServiceFamily, supported_service_families_dib,
};
use knx_rs_core::knxip::srp::{SrpSequence, SrpType};
use knx_rs_core::knxip::{
    ConnectionHeader, DEVICE_MGMT_CONNECTION, E_CONNECTION_ID, E_CONNECTION_TYPE, E_NO_ERROR,
    E_NO_MORE_CONNECTIONS, HostProtocol, Hpai, KnxIpFrame, ServiceType, TUNNEL_IA_BASE, tunnel_crd,
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

/// How many frames may wait for one tunnel client, beyond the one on the wire.
///
/// A client that acknowledges slower than the application sends does not make
/// the server hold an unbounded queue: when it is full the oldest frame is
/// dropped. This covers the confirmations a client is owed as well, so a
/// client that sends faster than the server can confirm is not confirmed for
/// the excess.
const MAX_CLIENT_QUEUE: usize = 128;

/// A tunnel client connection.
struct TunnelClient {
    channel_id: u8,
    ctrl_addr: SocketAddr,
    data_addr: SocketAddr,
    send_seq: u8,
    recv_seq: u8,
    last_heartbeat: tokio::time::Instant,
    /// cEMI frames for this client, oldest first: indications, and the
    /// `L_Data.con` it is owed for each `L_Data.req` it sent. One of them is on
    /// the wire at a time ([`TunnelClient::in_flight`]).
    outbox: VecDeque<Vec<u8>>,
    /// The tunnelling request that waits for this client's acknowledgement.
    in_flight: Option<InFlight>,
    /// Whether an overflow of `outbox` was reported since it last drained.
    overflow_reported: bool,
}

impl TunnelClient {
    const fn new(
        channel_id: u8,
        ctrl_addr: SocketAddr,
        data_addr: SocketAddr,
        now: tokio::time::Instant,
    ) -> Self {
        Self {
            channel_id,
            ctrl_addr,
            data_addr,
            send_seq: 0,
            recv_seq: 0,
            last_heartbeat: now,
            outbox: VecDeque::new(),
            in_flight: None,
            overflow_reported: false,
        }
    }

    /// Queue a cEMI frame for this client. When [`MAX_CLIENT_QUEUE`] frames
    /// wait already, the oldest one is dropped.
    fn enqueue(&mut self, cemi: Vec<u8>) {
        if self.outbox.len() >= MAX_CLIENT_QUEUE {
            self.outbox.pop_front();
            if !self.overflow_reported {
                self.overflow_reported = true;
                tracing::warn!(
                    channel_id = self.channel_id,
                    client = %self.data_addr,
                    "tunnel client does not keep up, oldest frames dropped"
                );
            }
        }
        self.outbox.push_back(cemi);
    }
}

/// A tunnelling request on the wire.
struct InFlight {
    sequence: u8,
    /// The complete KNXnet/IP datagram, sent again unchanged on a retry.
    datagram: Vec<u8>,
    /// How often it was sent.
    attempts: u8,
    /// When the client has to have acknowledged it, or it is sent again.
    deadline: tokio::time::Instant,
    /// The error of the last send, if it failed.
    last_error: Option<String>,
}

/// What a [`DeviceServer`] tells a client that searches for it or asks for its
/// description: the device it presents itself as.
///
/// A server started without a description does not answer `SEARCH_REQUEST` and
/// `DESCRIPTION_REQUEST`, so ETS "Discover interfaces" and
/// [`discover`](crate::discovery::discover) do not find it, and it cannot be
/// described to a client that adds it by address. Start it with
/// [`DeviceServer::start_with`] or [`DeviceServer::start_at_with`], and keep the
/// individual address and the programming mode current with
/// [`DeviceServer::set_description`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescription {
    /// Friendly name, shown by discovery. At most 30 ISO-8859-1 bytes are sent.
    pub name: String,
    /// Individual address of the device.
    pub individual_address: IndividualAddress,
    /// Six-byte KNX serial number.
    pub serial_number: [u8; 6],
    /// Six-byte Ethernet MAC address.
    pub mac_address: [u8; 6],
    /// KNX medium. [`KnxMedium::Ip`] for a server on an IP network.
    pub medium: KnxMedium,
    /// Combined project-installation identifier; 0 if the device has none.
    pub project_installation_identifier: u16,
    /// Whether the device is in programming mode. ETS shows it, and a device in
    /// programming mode is the one an individual address is assigned to.
    pub programming_mode: bool,
}

impl DeviceDescription {
    /// A description of an IP device that is not in programming mode and
    /// belongs to no project.
    pub fn new(
        name: impl Into<String>,
        individual_address: IndividualAddress,
        serial_number: [u8; 6],
        mac_address: [u8; 6],
    ) -> Self {
        Self {
            name: name.into(),
            individual_address,
            serial_number,
            mac_address,
            medium: KnxMedium::Ip,
            project_installation_identifier: 0,
            programming_mode: false,
        }
    }

    /// The service families the server supports, with their versions: core and
    /// tunnelling, and routing when it joined the multicast group.
    ///
    /// Core is advertised in version 1 although the server answers the extended
    /// search of version 2: it has nothing else of version 2 (TCP connections,
    /// secure sessions), and a client that sees version 2 expects them.
    fn families(routing: bool) -> Vec<(ServiceFamily, u8)> {
        let mut families = vec![(ServiceFamily::Core, 1), (ServiceFamily::Tunneling, 1)];
        if routing {
            families.push((ServiceFamily::Routing, 1));
        }
        families
    }

    /// The Device Information DIB and the Supported Service Families DIB that
    /// answer a search or description request.
    fn dibs(&self, routing: bool) -> Vec<u8> {
        let information = DeviceInformation {
            medium: self.medium,
            programming_mode: self.programming_mode,
            individual_address: self.individual_address.raw(),
            project_installation_identifier: self.project_installation_identifier,
            serial_number: self.serial_number,
            routing_multicast_address: if routing {
                KNX_MULTICAST_ADDR.octets()
            } else {
                [0; 4]
            },
            mac_address: self.mac_address,
            friendly_name: &self.name,
        };
        let mut dibs = information.to_bytes().to_vec();
        dibs.extend(supported_service_families_dib(&Self::families(routing)));
        dibs
    }

    /// Whether a `SEARCH_REQUEST_EXTENDED` with these search request
    /// parameters (the bytes after the discovery endpoint) is to be answered.
    ///
    /// The server answers only if it understands and satisfies every
    /// *mandatory* parameter: programming mode, MAC address and service
    /// selectors that match it, and no parameter of a kind it does not know. A
    /// parameter that is not mandatory never keeps the server from answering.
    /// The request for description blocks is met as far as the server can: the
    /// answer always has the Device Information and the Supported Service
    /// Families, and a block the server does not have is left out, as the
    /// protocol says. A block of parameters that cannot be parsed is not
    /// answered.
    fn answers_extended_search(&self, srps: &[u8], routing: bool) -> bool {
        let Ok(srps) = SrpSequence::parse(srps) else {
            return false;
        };
        let families = Self::families(routing);
        for srp in &srps {
            let data = srp.data();
            // `Some(true)`: satisfied, `Some(false)`: understood but not satisfied,
            // `None`: not understood, which includes data of the wrong size.
            let satisfied = match srp.srp_type() {
                Some(SrpType::SelectByProgrammingMode) if data.is_empty() => {
                    Some(self.programming_mode)
                }
                Some(SrpType::SelectByMacAddress) if data.len() == 6 => {
                    Some(data == self.mac_address)
                }
                Some(SrpType::SelectByService) if data.len() == 2 => Some(
                    families
                        .iter()
                        .any(|(family, version)| family.to_raw() == data[0] && *version >= data[1]),
                ),
                Some(SrpType::RequestDibs) if !data.is_empty() => Some(true),
                _ => None,
            };
            if satisfied != Some(true) && srp.is_mandatory() {
                return false;
            }
        }
        true
    }
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
    /// What the server answers to a search or description request; `None`: nothing.
    description: watch::Receiver<Option<DeviceDescription>>,
    /// Whether the server joined the routing multicast group.
    routing: bool,
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
/// # Slow clients
///
/// Every tunnel client has its own queue and one frame on the wire at a time,
/// as the tunnelling protocol demands, and the server never waits for an
/// acknowledgement: the next frame of that client goes out when the previous
/// one is acknowledged, and an acknowledgement that stays away is noticed at
/// its deadline. A client that does not acknowledge therefore delays only
/// itself. A frame it has not acknowledged after three attempts, one second
/// apart, costs it its tunnel. A send that fails counts as an attempt, too, and
/// the next one follows after the same interval; the error is reported when
/// the client is dropped.
///
/// At most 128 frames wait for a client. When it falls further behind, the
/// oldest frame is dropped, so a client that acknowledges, only slowly, cannot
/// make the server hold an unbounded queue.
///
/// # Discovery
///
/// Started with a [`DeviceDescription`] ([`DeviceServer::start_with`],
/// [`DeviceServer::start_at_with`]), the server answers `SEARCH_REQUEST` and
/// `DESCRIPTION_REQUEST` with its name, individual address, serial number, MAC
/// address and the service families it supports (core and tunnelling, and
/// routing when it joined the multicast group). The control endpoint in the
/// answer is an address the requester can reach: the bound address, or, for a
/// socket bound to all interfaces, the one the system sends from towards the
/// requester. Without a description the server does not answer.
///
/// The extended search (`SEARCH_REQUEST_EXTENDED`, which XKNX, and with it Home
/// Assistant, sends first) is answered with a `SEARCH_RESPONSE_EXTENDED` that has the same blocks. The
/// server honours the selectors a client may add (programming mode, MAC
/// address, supported service) and answers only if it satisfies every mandatory
/// one, or one it does not know. Description blocks a client asks for beyond
/// the Device Information and the Supported Service Families (IP
/// configuration, tunnelling slots, extended device information) are not
/// provided and are left out of the answer. The server advertises the core
/// family in version 1, as it has no other part of version 2.
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
    description: watch::Sender<Option<DeviceDescription>>,
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
        Self::start_described(local_addr, None).await
    }

    /// Start the device server as [`DeviceServer::start`] does, and answer
    /// search and description requests with `description`.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created.
    pub async fn start_with(
        local_addr: Ipv4Addr,
        description: DeviceDescription,
    ) -> Result<Self, KnxIpError> {
        Self::start_described(local_addr, Some(description)).await
    }

    async fn start_described(
        local_addr: Ipv4Addr,
        description: Option<DeviceDescription>,
    ) -> Result<Self, KnxIpError> {
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

        Ok(Self::spawn(
            socket,
            multicast_target,
            bound_addr,
            description,
        ))
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
        Self::start_at_described(bind_addr, None).await
    }

    /// Start the device server on a specific unicast address as
    /// [`DeviceServer::start_at`] does, and answer search and description
    /// requests with `description`. Without the multicast group, only a search
    /// request sent to this address reaches it.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created.
    pub async fn start_at_with(
        bind_addr: SocketAddr,
        description: DeviceDescription,
    ) -> Result<Self, KnxIpError> {
        Self::start_at_described(bind_addr, Some(description)).await
    }

    async fn start_at_described(
        bind_addr: SocketAddr,
        description: Option<DeviceDescription>,
    ) -> Result<Self, KnxIpError> {
        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;

        tracing::info!(%local_addr, "KNXnet/IP device server started");

        Ok(Self::spawn(socket, None, local_addr, description))
    }

    /// Replace what the server answers to search and description requests, for
    /// instance when ETS assigned a new individual address or the programming
    /// mode changed. Starts answering, if the server was started without a
    /// description.
    pub fn set_description(&self, description: DeviceDescription) {
        self.description.send_replace(Some(description));
    }

    fn spawn(
        socket: UdpSocket,
        multicast_target: Option<SocketAddr>,
        local_addr: SocketAddr,
        description: Option<DeviceDescription>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (description_tx, description_rx) = watch::channel(description);

        let task = tokio::spawn(server_task(
            socket,
            multicast_target,
            event_tx,
            cmd_rx,
            stop_rx,
            description_rx,
        ));

        Self {
            rx: event_rx,
            tx_cmd: cmd_tx,
            local_addr,
            stop: stop_tx,
            description: description_tx,
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
    /// The call returns once the frame is queued for the clients: each has its
    /// own queue, so a client that does not answer delays no frame to the
    /// others (see "Slow clients" on [`DeviceServer`]).
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
    description: watch::Receiver<Option<DeviceDescription>>,
) {
    let mut state = ServerState {
        tunnels: Vec::new(),
        next_channel_id: 1,
        events: EventQueue::new(event_tx),
        stop,
        description,
        routing: multicast_target.is_some(),
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
        let ack_deadline = next_ack_deadline(&state.tunnels);
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

            () = ack_due(ack_deadline) => {
                expire_acks(&socket, &mut state.tunnels).await;
            }

            _ = cleanup.tick() => {
                cleanup_stale_tunnels(&mut state.tunnels);
            }
        }
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
        ServiceType::TunnelingAck => {
            handle_tunneling_ack(socket, src, &frame, &mut state.tunnels).await;
        }
        ServiceType::SearchRequest => handle_search(socket, src, &frame, state).await,
        ServiceType::SearchRequestExtended => {
            handle_search_extended(socket, src, &frame, state).await;
        }
        ServiceType::DescriptionRequest => handle_description(socket, src, &frame, state).await,
        _ => {
            tracing::trace!(service = ?frame.service_type, "ignoring");
        }
    }
}

/// Answer a `SEARCH_REQUEST` with the control endpoint and the description of
/// the server, to the discovery endpoint the request names (the sender, for a
/// request in NAT mode). Without a description there is nothing to answer.
async fn handle_search(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    state: &ServerState,
) {
    let Some(description) = state.description.borrow().clone() else {
        tracing::trace!("search request from {src}: no description, not answered");
        return;
    };
    let Some(reply_to) = reply_endpoint(&frame.body, src) else {
        tracing::debug!("search request from {src}: no usable discovery endpoint");
        return;
    };
    tracing::debug!("search request from {src}, answered to {reply_to}");
    let response = KnxIpFrame::search_response(
        control_endpoint(socket, src),
        &description.dibs(state.routing),
    );
    if let Some(bytes) = serialize_frame(&response) {
        let _ = socket.send_to(&bytes, reply_to).await;
    }
}

/// Answer a `SEARCH_REQUEST_EXTENDED` with the same blocks as a plain search
/// response, in a `SEARCH_RESPONSE_EXTENDED`, if the search request parameters
/// allow it ([`DeviceDescription::answers_extended_search`]). The discovery
/// endpoint is at the start of the body, the parameters follow it.
async fn handle_search_extended(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    state: &ServerState,
) {
    let Some(description) = state.description.borrow().clone() else {
        tracing::trace!("extended search request from {src}: no description, not answered");
        return;
    };
    let Some(reply_to) = reply_endpoint(&frame.body, src) else {
        tracing::debug!("extended search request from {src}: no usable discovery endpoint");
        return;
    };
    let parameters = frame.body.get(usize::from(Hpai::LEN)..).unwrap_or_default();
    if !description.answers_extended_search(parameters, state.routing) {
        tracing::debug!("extended search request from {src}: its parameters exclude this server");
        return;
    }
    tracing::debug!("extended search request from {src}, answered to {reply_to}");
    let response = KnxIpFrame::search_response_extended(
        control_endpoint(socket, src),
        &description.dibs(state.routing),
    );
    if let Some(bytes) = serialize_frame(&response) {
        let _ = socket.send_to(&bytes, reply_to).await;
    }
}

/// Answer a `DESCRIPTION_REQUEST` with the description of the server, to the
/// control endpoint the request names (the sender, for a request in NAT mode).
async fn handle_description(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    state: &ServerState,
) {
    let Some(description) = state.description.borrow().clone() else {
        tracing::trace!("description request from {src}: no description, not answered");
        return;
    };
    let Some(reply_to) = reply_endpoint(&frame.body, src) else {
        tracing::debug!("description request from {src}: no usable control endpoint");
        return;
    };
    tracing::debug!("description request from {src}, answered to {reply_to}");
    let response = KnxIpFrame::description_response(&description.dibs(state.routing));
    if let Some(bytes) = serialize_frame(&response) {
        let _ = socket.send_to(&bytes, reply_to).await;
    }
}

/// Where the answer to a request goes: the UDP endpoint in the request's HPAI,
/// or the sender for a request in NAT mode. `None` for a body that holds no
/// HPAI, or one for another protocol than UDP.
fn reply_endpoint(body: &[u8], src: SocketAddr) -> Option<SocketAddr> {
    let hpai = Hpai::parse(body)?;
    (hpai.protocol == HostProtocol::Ipv4Udp).then(|| hpai_to_socket_addr(hpai, src))
}

/// The control endpoint a response names: an address the requester can reach
/// the server at. A socket bound to one address says it; a socket bound to all
/// interfaces does not, so it is the address the system would send from to
/// reach the requester. KNXnet/IP HPAI carries IPv4 only, so for an IPv6
/// requester, or when no address can be found, it is the NAT-mode endpoint
/// (the requester then uses the source of the answer).
fn control_endpoint(socket: &UdpSocket, requester: SocketAddr) -> Hpai {
    let port = socket.local_addr().map_or(KNX_PORT, |addr| addr.port());
    let SocketAddr::V4(_) = requester else {
        return Hpai::nat_udp(port);
    };
    let ip = match socket.local_addr() {
        Ok(SocketAddr::V4(bound)) if !bound.ip().is_unspecified() => Some(*bound.ip()),
        _ => source_address_towards(requester),
    };
    ip.map_or_else(
        || Hpai::nat_udp(port),
        |ip| Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: ip.octets(),
            port,
        },
    )
}

/// The IPv4 address the system sends from to reach `target`, found by
/// connecting a throwaway UDP socket (which sends nothing).
fn source_address_towards(target: SocketAddr) -> Option<Ipv4Addr> {
    let probe = std::net::UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect(target).ok()?;
    match probe.local_addr().ok()? {
        SocketAddr::V4(local) if !local.ip().is_unspecified() => Some(*local.ip()),
        _ => None,
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

    tunnels.push(TunnelClient::new(
        channel_id,
        ctrl_addr,
        data_addr,
        tokio::time::Instant::now(),
    ));

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
        // The server owes the client a confirmation for each L_Data.req. It joins the
        // client's queue behind what is already waiting, and goes out as soon as the
        // client has acknowledged what is on the wire.
        if let Some(confirmation) = confirmation_of(&cemi) {
            tunnel.enqueue(confirmation);
            pump(socket, tunnel).await;
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

    // Queue it for every tunnel client. Each client has its own queue and its own
    // acknowledgement, so one that does not answer holds up only itself.
    for tunnel in &mut state.tunnels {
        tunnel.enqueue(cemi.to_vec());
        pump(socket, tunnel).await;
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
    if let Some(tunnel) = state
        .tunnels
        .iter_mut()
        .find(|t| t.channel_id == channel_id)
    {
        tunnel.enqueue(as_indication(cemi).into_owned());
        pump(socket, tunnel).await;
    }
}

/// Put the next queued frame of `tunnel` on the wire, unless one is waiting for
/// its acknowledgement. Never waits: the acknowledgement is handled when it
/// arrives ([`handle_tunneling_ack`]) or missing at its deadline
/// ([`expire_acks`]).
async fn pump(socket: &UdpSocket, tunnel: &mut TunnelClient) {
    if tunnel.in_flight.is_some() {
        return;
    }
    let sequence = tunnel.send_seq;
    // A frame that cannot be serialized cannot happen for cEMI that came out of a
    // CemiFrame, and the client is not at fault; it is skipped.
    let datagram = loop {
        let Some(cemi) = tunnel.outbox.pop_front() else {
            tunnel.overflow_reported = false;
            return;
        };
        let request = KnxIpFrame::tunneling_request(tunnel.channel_id, sequence, &cemi);
        if let Some(datagram) = serialize_frame(&request) {
            break datagram;
        }
    };
    tunnel.in_flight = Some(InFlight {
        sequence,
        datagram,
        attempts: 0,
        deadline: tokio::time::Instant::now(),
        last_error: None,
    });
    transmit(socket, tunnel).await;
}

/// Send the request in flight (again) and set when it has to be acknowledged.
///
/// A failed send counts as an attempt like an unanswered one, and the next one
/// follows after the usual interval, not at once: an error that goes away
/// (a full socket buffer, a route that is briefly missing) does not cost a
/// healthy client its tunnel, and one that stays is reported with its cause.
async fn transmit(socket: &UdpSocket, tunnel: &mut TunnelClient) {
    let Some(in_flight) = tunnel.in_flight.as_mut() else {
        return;
    };
    in_flight.attempts += 1;
    in_flight.deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    match socket.send_to(&in_flight.datagram, tunnel.data_addr).await {
        Ok(_) => in_flight.last_error = None,
        Err(e) => {
            tracing::debug!(
                channel = tunnel.channel_id,
                attempt = in_flight.attempts,
                error = %e,
                "send failed"
            );
            in_flight.last_error = Some(e.to_string());
        }
    }
}

/// The client acknowledged what is on the wire: the next frame may go.
async fn handle_tunneling_ack(
    socket: &UdpSocket,
    src: SocketAddr,
    frame: &KnxIpFrame,
    tunnels: &mut [TunnelClient],
) {
    let Some(header) = ConnectionHeader::parse(&frame.body) else {
        return;
    };
    let Some(tunnel) = tunnels
        .iter_mut()
        .find(|t| t.channel_id == header.channel_id && t.data_addr == src)
    else {
        return;
    };
    if tunnel
        .in_flight
        .as_ref()
        .is_some_and(|f| f.sequence == header.sequence_counter)
    {
        tunnel.in_flight = None;
        tunnel.send_seq = header.sequence_counter.wrapping_add(1);
        pump(socket, tunnel).await;
    }
}

/// When the first request that waits for an acknowledgement is due, if any.
fn next_ack_deadline(tunnels: &[TunnelClient]) -> Option<tokio::time::Instant> {
    tunnels
        .iter()
        .filter_map(|t| t.in_flight.as_ref().map(|f| f.deadline))
        .min()
}

/// Resolves at `deadline`; never, if there is none. Cancel-safe.
async fn ack_due(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Send again what a client has not acknowledged in time, up to
/// [`MAX_RETRIES`] times. A client that never acknowledges is dropped.
async fn expire_acks(socket: &UdpSocket, tunnels: &mut Vec<TunnelClient>) {
    let now = tokio::time::Instant::now();
    let mut lost = Vec::new();
    for tunnel in tunnels.iter_mut() {
        let Some(in_flight) = tunnel.in_flight.as_ref() else {
            continue;
        };
        if in_flight.deadline > now {
            continue;
        }
        if in_flight.attempts < MAX_RETRIES {
            tracing::debug!(
                channel = tunnel.channel_id,
                attempt = in_flight.attempts,
                "ack timeout"
            );
            transmit(socket, tunnel).await;
        } else {
            lost.push(tunnel.channel_id);
        }
    }
    for channel_id in lost {
        drop_lost_tunnel(socket, tunnels, channel_id).await;
    }
}

/// Forget a tunnel client whose acknowledgements stayed away and tell it, best
/// effort, so that a client that is merely cut off knows to reconnect.
async fn drop_lost_tunnel(socket: &UdpSocket, tunnels: &mut Vec<TunnelClient>, channel_id: u8) {
    let Some(position) = tunnels.iter().position(|t| t.channel_id == channel_id) else {
        return;
    };
    let tunnel = tunnels.remove(position);
    let failed_send = tunnel
        .in_flight
        .as_ref()
        .and_then(|f| f.last_error.as_deref());
    if let Some(error) = failed_send {
        tracing::warn!(
            channel_id,
            client = %tunnel.data_addr,
            error,
            "tunnel client dropped: sending to it failed {MAX_RETRIES} times"
        );
    } else {
        tracing::warn!(
            channel_id,
            client = %tunnel.ctrl_addr,
            "tunnel client dropped: no acknowledgement after {MAX_RETRIES} attempts"
        );
    }
    let port = socket.local_addr().map_or(KNX_PORT, |addr| addr.port());
    let request = KnxIpFrame::disconnect_request(channel_id, Hpai::nat_udp(port));
    if let Some(bytes) = serialize_frame(&request) {
        let _ = socket.send_to(&bytes, tunnel.ctrl_addr).await;
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
    use std::time::Duration;

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

    // ── Per-client sending ───────────────────────────────────────

    const LOOPBACK: &str = "127.0.0.1:0";

    async fn socket() -> UdpSocket {
        UdpSocket::bind(LOOPBACK).await.unwrap()
    }

    fn client(channel_id: u8, data_addr: SocketAddr) -> TunnelClient {
        TunnelClient::new(
            channel_id,
            data_addr,
            data_addr,
            tokio::time::Instant::now(),
        )
    }

    #[test]
    fn a_full_queue_drops_the_oldest_frame_and_keeps_the_order() {
        let mut tunnel = client(1, "127.0.0.1:9".parse().unwrap());
        for n in 0..=MAX_CLIENT_QUEUE {
            tunnel.enqueue(vec![u8::try_from(n % 256).unwrap(), 0xAA]);
        }
        assert_eq!(
            tunnel.outbox.len(),
            MAX_CLIENT_QUEUE,
            "the queue is bounded"
        );
        assert_eq!(tunnel.outbox.front().unwrap()[0], 1, "frame 0 was dropped");
        assert_eq!(
            tunnel.outbox.back().unwrap()[0],
            u8::try_from(MAX_CLIENT_QUEUE % 256).unwrap(),
            "the newest frame is kept"
        );
        assert!(tunnel.overflow_reported, "the overflow is reported once");
    }

    #[tokio::test]
    async fn one_frame_is_on_the_wire_and_an_ack_releases_the_next_with_the_next_sequence() {
        let server = socket().await;
        let peer = socket().await;
        let mut tunnel = client(7, peer.local_addr().unwrap());
        tunnel.enqueue(vec![0x29, 0x00]);
        tunnel.enqueue(vec![0x29, 0x01]);
        pump(&server, &mut tunnel).await;

        let mut buf = [0u8; 64];
        let (n, _) = peer.recv_from(&mut buf).await.unwrap();
        let first = KnxIpFrame::parse(&buf[..n]).unwrap();
        assert_eq!(
            ConnectionHeader::parse(&first.body)
                .unwrap()
                .sequence_counter,
            0
        );
        assert_eq!(tunnel.outbox.len(), 1, "the second frame waits for the ack");
        assert!(
            tokio::time::timeout(Duration::from_millis(150), peer.recv_from(&mut buf))
                .await
                .is_err(),
            "nothing else is sent before the ack"
        );

        // An ack for another sequence counter does not release it.
        let stale = KnxIpFrame::tunneling_ack(7, 9, E_NO_ERROR);
        handle_tunneling_ack(
            &server,
            peer.local_addr().unwrap(),
            &stale,
            std::slice::from_mut(&mut tunnel),
        )
        .await;
        assert!(tunnel.in_flight.is_some());
        assert_eq!(tunnel.send_seq, 0);

        let ack = KnxIpFrame::tunneling_ack(7, 0, E_NO_ERROR);
        handle_tunneling_ack(
            &server,
            peer.local_addr().unwrap(),
            &ack,
            std::slice::from_mut(&mut tunnel),
        )
        .await;
        let (n, _) = peer.recv_from(&mut buf).await.unwrap();
        let second = KnxIpFrame::parse(&buf[..n]).unwrap();
        assert_eq!(
            ConnectionHeader::parse(&second.body)
                .unwrap()
                .sequence_counter,
            1
        );
        assert!(tunnel.outbox.is_empty());
    }

    #[tokio::test]
    async fn an_ack_from_another_address_does_not_count() {
        let server = socket().await;
        let peer = socket().await;
        let stranger = socket().await;
        let mut tunnel = client(7, peer.local_addr().unwrap());
        tunnel.enqueue(vec![0x29, 0x00]);
        pump(&server, &mut tunnel).await;
        let ack = KnxIpFrame::tunneling_ack(7, 0, E_NO_ERROR);
        handle_tunneling_ack(
            &server,
            stranger.local_addr().unwrap(),
            &ack,
            std::slice::from_mut(&mut tunnel),
        )
        .await;
        assert!(
            tunnel.in_flight.is_some(),
            "only the client's own address may acknowledge"
        );
    }

    /// A send that fails is an attempt, and the next one follows after the usual
    /// interval, not at once: three immediate errors used to drop a client and blame
    /// its acknowledgement.
    #[tokio::test(start_paused = true)]
    async fn a_failing_send_is_retried_after_the_interval_and_the_cause_is_kept() {
        let server = socket().await;
        // An IPv6 destination from an IPv4 socket: the send fails at once, everywhere.
        let mut tunnels = vec![client(3, "[::1]:9".parse().unwrap())];
        tunnels[0].enqueue(vec![0x29, 0x00]);
        pump(&server, &mut tunnels[0]).await;

        let in_flight = tunnels[0].in_flight.as_ref().unwrap();
        assert_eq!(in_flight.attempts, 1);
        assert!(in_flight.last_error.is_some(), "the send error is recorded");
        let due = in_flight.deadline;
        assert!(
            due >= tokio::time::Instant::now() + REQUEST_TIMEOUT,
            "the retry waits one interval"
        );

        // Nothing happens before the deadline.
        expire_acks(&server, &mut tunnels).await;
        assert_eq!(tunnels[0].in_flight.as_ref().unwrap().attempts, 1);

        for attempt in 2..=MAX_RETRIES {
            tokio::time::advance(REQUEST_TIMEOUT).await;
            expire_acks(&server, &mut tunnels).await;
            assert_eq!(tunnels.len(), 1, "still connected after attempt {attempt}");
            assert_eq!(tunnels[0].in_flight.as_ref().unwrap().attempts, attempt);
        }
        tokio::time::advance(REQUEST_TIMEOUT).await;
        expire_acks(&server, &mut tunnels).await;
        assert!(
            tunnels.is_empty(),
            "dropped after {MAX_RETRIES} failed attempts"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_client_is_retried_three_times_with_the_same_sequence_and_then_dropped() {
        let server = socket().await;
        let peer = socket().await;
        let mut tunnels = vec![client(5, peer.local_addr().unwrap())];
        tunnels[0].enqueue(vec![0x29, 0x00]);
        pump(&server, &mut tunnels[0]).await;
        for _ in 1..MAX_RETRIES {
            tokio::time::advance(REQUEST_TIMEOUT).await;
            expire_acks(&server, &mut tunnels).await;
        }
        let mut buf = [0u8; 64];
        for _ in 0..MAX_RETRIES {
            let (n, _) = peer.recv_from(&mut buf).await.unwrap();
            let frame = KnxIpFrame::parse(&buf[..n]).unwrap();
            assert_eq!(
                ConnectionHeader::parse(&frame.body)
                    .unwrap()
                    .sequence_counter,
                0
            );
        }
        tokio::time::advance(REQUEST_TIMEOUT).await;
        expire_acks(&server, &mut tunnels).await;
        assert!(tunnels.is_empty());
        // And the client is told, as far as that is possible.
        let (n, _) = peer.recv_from(&mut buf).await.unwrap();
        assert_eq!(
            KnxIpFrame::parse(&buf[..n]).unwrap().service_type,
            ServiceType::DisconnectRequest
        );
    }

    #[test]
    fn the_next_deadline_is_the_earliest_of_all_clients() {
        let now = tokio::time::Instant::now();
        let mut tunnels = vec![
            client(1, "127.0.0.1:9".parse().unwrap()),
            client(2, "127.0.0.1:9".parse().unwrap()),
        ];
        assert_eq!(next_ack_deadline(&tunnels), None);
        let flight = |after: u64| InFlight {
            sequence: 0,
            datagram: Vec::new(),
            attempts: 1,
            deadline: now + Duration::from_secs(after),
            last_error: None,
        };
        tunnels[0].in_flight = Some(flight(5));
        tunnels[1].in_flight = Some(flight(2));
        assert_eq!(
            next_ack_deadline(&tunnels),
            Some(now + Duration::from_secs(2))
        );
    }

    // ── Extended search parameters ───────────────────────────────

    fn described(programming_mode: bool) -> DeviceDescription {
        let mut description = DeviceDescription::new(
            "Test",
            IndividualAddress::from_raw(0x1164),
            [0, 1, 2, 3, 4, 5],
            [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10],
        );
        description.programming_mode = programming_mode;
        description
    }

    #[test]
    fn a_request_without_parameters_is_answered() {
        assert!(described(false).answers_extended_search(&[], false));
    }

    #[test]
    fn the_programming_mode_selector_needs_a_server_in_programming_mode() {
        use knx_rs_core::knxip::srp::programming_mode_srp;
        assert!(!described(false).answers_extended_search(&programming_mode_srp(), false));
        assert!(described(true).answers_extended_search(&programming_mode_srp(), false));
        // Not mandatory (flag cleared): it narrows nothing.
        assert!(described(false).answers_extended_search(&[0x02, 0x01], false));
        // Data where there is none: not understood, so a mandatory one excludes.
        assert!(!described(true).answers_extended_search(&[0x03, 0x81, 0x00], false));
        assert!(described(true).answers_extended_search(&[0x03, 0x01, 0x00], false));
    }

    #[test]
    fn the_mac_selector_needs_the_servers_own_address() {
        use knx_rs_core::knxip::srp::mac_address_srp;
        let own = [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10];
        assert!(described(false).answers_extended_search(&mac_address_srp(own), false));
        assert!(
            !described(false).answers_extended_search(&mac_address_srp([1, 2, 3, 4, 5, 6]), false)
        );
        // A MAC of the wrong length is not understood.
        assert!(!described(false).answers_extended_search(&[0x07, 0x82, 1, 2, 3, 4, 5], false));
    }

    #[test]
    fn the_service_selector_needs_the_family_in_at_least_the_version() {
        use knx_rs_core::knxip::srp::service_srp;
        let answers = |srp: &[u8], routing| described(false).answers_extended_search(srp, routing);
        assert!(answers(&service_srp(ServiceFamily::Tunneling, 1), false));
        // Tunnelling version 2 is the TCP form, which the server does not have.
        assert!(!answers(&service_srp(ServiceFamily::Tunneling, 2), false));
        // Core is advertised in version 1.
        assert!(answers(&service_srp(ServiceFamily::Core, 1), false));
        assert!(!answers(&service_srp(ServiceFamily::Core, 2), false));
        // Routing only with the multicast group.
        assert!(!answers(&service_srp(ServiceFamily::Routing, 1), false));
        assert!(answers(&service_srp(ServiceFamily::Routing, 1), true));
        assert!(!answers(
            &service_srp(ServiceFamily::DeviceManagement, 1),
            true
        ));
    }

    #[test]
    fn an_srp_the_server_does_not_know_excludes_it_only_if_it_is_mandatory() {
        let answers = |srp: &[u8]| described(false).answers_extended_search(srp, false);
        // The reference "invalid" SRP (type 0), and a type nobody defined.
        assert!(!answers(&[0x02, 0x80]));
        assert!(answers(&[0x02, 0x00]));
        assert!(!answers(&[0x03, 0xFE, 0xAA]));
        assert!(answers(&[0x03, 0x7E, 0xAA]));
    }

    #[test]
    fn a_request_for_description_blocks_never_keeps_the_server_from_answering() {
        use knx_rs_core::knxip::dib::DibType;
        use knx_rs_core::knxip::srp::request_dibs_srp;
        // What XKNX asks for: device information, service families, secured service
        // families and tunnelling information; the last two the server does not have.
        let srp = request_dibs_srp(&[
            DibType::DeviceInformation,
            DibType::SupportedServiceFamilies,
            DibType::SecuredServiceFamilies,
            DibType::TunnelingInformation,
        ])
        .unwrap();
        assert!(described(false).answers_extended_search(&srp, false));
        // With no description type in it, it asks for nothing and is not understood.
        assert!(described(false).answers_extended_search(&[0x02, 0x04], false));
        assert!(!described(false).answers_extended_search(&[0x02, 0x84], false));
    }

    #[test]
    fn every_mandatory_parameter_has_to_be_satisfied() {
        use knx_rs_core::knxip::srp::{mac_address_srp, programming_mode_srp, service_srp};
        let own = [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10];
        let mut all = programming_mode_srp();
        all.extend(mac_address_srp(own));
        all.extend(service_srp(ServiceFamily::Tunneling, 1));
        assert!(described(true).answers_extended_search(&all, false));
        assert!(
            !described(false).answers_extended_search(&all, false),
            "not in programming mode"
        );
        let mut other_mac = programming_mode_srp();
        other_mac.extend(mac_address_srp([9; 6]));
        assert!(!described(true).answers_extended_search(&other_mac, false));
    }

    #[test]
    fn a_block_of_parameters_that_cannot_be_parsed_is_not_answered() {
        let answers = |srps: &[u8]| described(true).answers_extended_search(srps, false);
        assert!(!answers(&[0x08, 0x82, 1, 2]), "length beyond the data");
        assert!(!answers(&[0x01, 0x81]), "length below the header");
        assert!(!answers(&[0x02]), "a lone length byte");
        assert!(
            !answers(&[0x02, 0x81, 0x05, 0x01]),
            "a good parameter does not excuse a bad one"
        );
    }
}
