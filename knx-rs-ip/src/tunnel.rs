// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP tunnel connection (unicast UDP).
//!
//! Implements the full KNXnet/IP tunneling protocol per KNX specification:
//! - Connect handshake with timeout
//! - Tunneling request/ack with sequence counting and 3× retry
//! - Heartbeat every 60s with failure counting (3 failures = disconnect)
//! - Graceful disconnect handshake
//! - Auto-reconnect with exponential backoff (optional)

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::pin::Pin;

use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{
    ConnectionHeader, E_NO_ERROR, HostProtocol, Hpai, KnxIpFrame, ServiceType, tunnel_cri,
};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};

use crate::error::KnxIpError;
use crate::{KnxConnection, KnxFuture};

/// KNX spec: connect request timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// KNX spec: heartbeat interval.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
/// KNX spec: tunneling request ack timeout (per attempt).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
/// KNX spec: maximum tunneling request retries.
pub const MAX_RETRIES: u8 = 3;
/// KNX spec: heartbeat failures before disconnect.
const MAX_HEARTBEAT_FAILURES: u8 = 3;

/// Receive buffer size for a single KNXnet/IP datagram.
pub const RECV_BUF_SIZE: usize = 1024;

/// Initial reconnect delay.
const RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(1);
/// Maximum reconnect delay.
const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(60);

/// Configuration for a tunnel connection.
#[derive(Debug, Clone)]
pub struct TunnelConfig {
    /// Remote gateway address.
    pub remote: SocketAddr,
    /// Enable auto-reconnect on connection loss.
    pub auto_reconnect: bool,
}

impl TunnelConfig {
    /// Create a config with default settings (no auto-reconnect).
    pub const fn new(remote: SocketAddr) -> Self {
        Self {
            remote,
            auto_reconnect: false,
        }
    }

    /// Enable auto-reconnect with exponential backoff.
    #[must_use]
    pub const fn with_auto_reconnect(mut self) -> Self {
        self.auto_reconnect = true;
        self
    }
}

/// A KNXnet/IP tunnel connection over unicast UDP.
///
/// Manages the full lifecycle: connect → exchange frames → heartbeat → disconnect.
/// Optionally auto-reconnects on connection loss.
pub struct TunnelConnection {
    rx: mpsc::Receiver<CemiFrame>,
    tx_cmd: mpsc::Sender<TunnelCmd>,
}

enum TunnelCmd {
    Send(CemiFrame, oneshot::Sender<Result<(), KnxIpError>>),
    Close,
}

impl TunnelConnection {
    /// Establish a tunnel connection to a KNXnet/IP gateway.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the connection cannot be established.
    pub async fn connect(remote: SocketAddr) -> Result<Self, KnxIpError> {
        Self::connect_with_config(TunnelConfig::new(remote)).await
    }

    /// Establish a tunnel connection with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the connection cannot be established.
    pub async fn connect_with_config(config: TunnelConfig) -> Result<Self, KnxIpError> {
        let (socket, channel_id, local_addr) = establish(&config.remote).await?;

        let (cemi_tx, cemi_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        tokio::spawn(tunnel_task(
            config, socket, channel_id, local_addr, cemi_tx, cmd_rx,
        ));

        Ok(Self {
            rx: cemi_rx,
            tx_cmd: cmd_tx,
        })
    }
}

impl KnxConnection for TunnelConnection {
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<(), KnxIpError>> {
        let tx_cmd = self.tx_cmd.clone();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            tx_cmd
                .send(TunnelCmd::Send(frame, tx))
                .await
                .map_err(|_| KnxIpError::Closed)?;
            rx.await.map_err(|_| KnxIpError::Closed)?
        })
    }

    fn recv(&mut self) -> KnxFuture<'_, Option<CemiFrame>> {
        Box::pin(async move { self.rx.recv().await })
    }

    fn close(&mut self) -> KnxFuture<'_, ()> {
        let tx_cmd = self.tx_cmd.clone();
        Box::pin(async move {
            let _ = tx_cmd.send(TunnelCmd::Close).await;
        })
    }
}

// ── Connection establishment ──────────────────────────────────

async fn establish(remote: &SocketAddr) -> Result<(UdpSocket, u8, SocketAddr), KnxIpError> {
    let socket = UdpSocket::bind(bind_addr_for_remote(remote)).await?;
    socket.connect(remote).await?;
    let local_addr = socket.local_addr()?;
    let channel_id = do_connect(&socket, local_addr).await?;
    tracing::info!(%remote, channel_id, "KNXnet/IP tunnel connected");
    Ok((socket, channel_id, local_addr))
}

const fn bind_addr_for_remote(remote: &SocketAddr) -> SocketAddr {
    match remote {
        SocketAddr::V4(_) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(v6) => SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::UNSPECIFIED,
            0,
            0,
            v6.scope_id(),
        )),
    }
}

const fn build_hpai(addr: SocketAddr) -> Hpai {
    match addr {
        SocketAddr::V4(v4) => Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: v4.ip().octets(),
            port: v4.port(),
        },
        SocketAddr::V6(v6) => Hpai::nat_udp(v6.port()),
    }
}

fn serialize_frame(frame: &KnxIpFrame) -> Result<Vec<u8>, KnxIpError> {
    Ok(frame.try_to_bytes()?)
}

async fn do_connect(socket: &UdpSocket, local_addr: SocketAddr) -> Result<u8, KnxIpError> {
    let hpai = build_hpai(local_addr);
    // Control and data endpoints share the local HPAI.
    let frame = KnxIpFrame::connect_request(hpai, hpai, &tunnel_cri());
    socket.send(&serialize_frame(&frame)?).await?;

    let mut buf = [0u8; RECV_BUF_SIZE];
    let n = timeout(CONNECT_TIMEOUT, socket.recv(&mut buf))
        .await
        .map_err(|_| KnxIpError::Timeout("connect response"))?
        .map_err(KnxIpError::Io)?;

    let resp = KnxIpFrame::parse(&buf[..n])?;

    if resp.service_type != ServiceType::ConnectResponse {
        return Err(KnxIpError::Protocol(format!(
            "expected ConnectResponse, got {:?}",
            resp.service_type
        )));
    }

    if resp.body.len() < 2 {
        return Err(KnxIpError::Protocol("connect response too short".into()));
    }

    let channel_id = resp.body[0];
    let status = resp.body[1];

    if status != E_NO_ERROR {
        return Err(KnxIpError::ConnectionRejected(status));
    }

    Ok(channel_id)
}

// ── Background task ───────────────────────────────────────────

async fn tunnel_task(
    config: TunnelConfig,
    socket: UdpSocket,
    channel_id: u8,
    local_addr: SocketAddr,
    cemi_tx: mpsc::Sender<CemiFrame>,
    mut cmd_rx: mpsc::Receiver<TunnelCmd>,
) {
    let mut state = TunnelState {
        socket,
        channel_id,
        local_addr,
        send_seq: 0,
        recv_seq: 0,
        heartbeat_failures: 0,
    };

    let heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    tokio::pin!(heartbeat);
    let mut buf = [0u8; 1024];

    loop {
        tokio::select! {
            result = state.socket.recv(&mut buf) => {
                let n = match result {
                    Ok(n) => n,
                    Err(e) => {
                        tracing::warn!(error = %e, "tunnel recv error");
                        if !try_reconnect(&config, &mut state, &mut heartbeat, &mut cmd_rx).await {
                            break;
                        }
                        continue;
                    }
                };
                if !state.handle_incoming(&buf[..n], &cemi_tx).await
                    && !try_reconnect(&config, &mut state, &mut heartbeat, &mut cmd_rx).await
                {
                    break;
                }
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(TunnelCmd::Send(frame, reply)) => {
                        let result = state.send_with_retry(&frame, &cemi_tx).await;
                        if result.is_err() && config.auto_reconnect {
                            let _ = reply.send(result);
                            if !try_reconnect(&config, &mut state, &mut heartbeat, &mut cmd_rx).await {
                                break;
                            }
                            continue;
                        }
                        let _ = reply.send(result);
                    }
                    Some(TunnelCmd::Close) | None => {
                        let _ = state.send_disconnect().await;
                        break;
                    }
                }
            }

            _ = heartbeat.tick() => {
                if let Err(e) = state.send_heartbeat(&cemi_tx).await {
                    state.heartbeat_failures += 1;
                    tracing::warn!(
                        error = %e,
                        failures = state.heartbeat_failures,
                        "heartbeat failed"
                    );
                    if state.heartbeat_failures >= MAX_HEARTBEAT_FAILURES {
                        tracing::error!("max heartbeat failures reached, disconnecting");
                        if !try_reconnect(&config, &mut state, &mut heartbeat, &mut cmd_rx).await {
                            break;
                        }
                    }
                } else {
                    state.heartbeat_failures = 0;
                }
            }
        }
    }

    tracing::debug!(channel_id = state.channel_id, "tunnel task ended");
}

async fn try_reconnect(
    config: &TunnelConfig,
    state: &mut TunnelState,
    heartbeat: &mut Pin<&mut tokio::time::Interval>,
    cmd_rx: &mut mpsc::Receiver<TunnelCmd>,
) -> bool {
    if !config.auto_reconnect {
        return false;
    }

    tracing::info!("attempting reconnect...");
    let mut delay = RECONNECT_DELAY_INITIAL;

    loop {
        // TUN-3: Check for close command during reconnect delay
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(TunnelCmd::Close) | None => {
                        tracing::info!("reconnect cancelled by close");
                        return false;
                    }
                    Some(TunnelCmd::Send(_, reply)) => {
                        // Don't silently drop the reply (which the caller would
                        // see as a permanent Closed): report a transient,
                        // retryable error while reconnecting.
                        let _ = reply.send(Err(KnxIpError::Timeout("reconnecting")));
                    }
                }
            }
        }

        match establish(&config.remote).await {
            Ok((socket, channel_id, local_addr)) => {
                state.socket = socket;
                state.channel_id = channel_id;
                state.local_addr = local_addr;
                state.send_seq = 0;
                state.recv_seq = 0;
                state.heartbeat_failures = 0;
                heartbeat.as_mut().reset();
                tracing::info!(channel_id, "reconnected");
                return true;
            }
            Err(e) => {
                tracing::warn!(error = %e, delay_secs = delay.as_secs(), "reconnect failed");
                delay = (delay * 2).min(RECONNECT_DELAY_MAX);
            }
        }
    }
}

// ── Tunnel state ──────────────────────────────────────────────

struct TunnelState {
    socket: UdpSocket,
    channel_id: u8,
    local_addr: SocketAddr,
    send_seq: u8,
    recv_seq: u8,
    heartbeat_failures: u8,
}

impl TunnelState {
    async fn handle_incoming(&mut self, data: &[u8], cemi_tx: &mpsc::Sender<CemiFrame>) -> bool {
        let frame = match KnxIpFrame::parse(data) {
            Ok(f) => f,
            Err(e) => {
                tracing::trace!(error = %e, "ignoring malformed frame");
                return true;
            }
        };

        match frame.service_type {
            ServiceType::TunnelingRequest => {
                self.handle_tunneling_request(&frame, cemi_tx).await;
            }
            ServiceType::TunnelingAck => {
                // Handled inline by send_with_retry
            }
            ServiceType::DisconnectRequest => {
                tracing::info!("remote disconnect");
                let resp = KnxIpFrame {
                    service_type: ServiceType::DisconnectResponse,
                    body: vec![self.channel_id, E_NO_ERROR],
                };
                if let Ok(bytes) = serialize_frame(&resp) {
                    let _ = self.socket.send(&bytes).await;
                }
                return false;
            }
            _ => {
                tracing::trace!(service = ?frame.service_type, "ignoring frame");
            }
        }
        true
    }

    async fn handle_tunneling_request(
        &mut self,
        frame: &KnxIpFrame,
        cemi_tx: &mpsc::Sender<CemiFrame>,
    ) {
        let Some(ch) = ConnectionHeader::parse(&frame.body) else {
            return;
        };
        if ch.channel_id != self.channel_id {
            return;
        }

        // Always ACK
        let ack = KnxIpFrame::tunneling_ack(self.channel_id, ch.sequence_counter, E_NO_ERROR);
        if let Ok(bytes) = serialize_frame(&ack) {
            let _ = self.socket.send(&bytes).await;
        }

        // Deduplicate: only process if sequence matches expected
        if ch.sequence_counter != self.recv_seq {
            return;
        }
        self.recv_seq = self.recv_seq.wrapping_add(1);

        // Parse CEMI from body after connection header
        let cemi_data = &frame.body[ConnectionHeader::LEN as usize..];
        if let Ok(cemi) = CemiFrame::parse(cemi_data) {
            let _ = cemi_tx.send(cemi).await;
        }
    }

    /// Send a tunneling request with up to `MAX_RETRIES` attempts.
    /// Returns any frames received while waiting for ack (for re-processing).
    async fn send_with_retry(
        &mut self,
        cemi: &CemiFrame,
        cemi_tx: &mpsc::Sender<CemiFrame>,
    ) -> Result<(), KnxIpError> {
        let frame = KnxIpFrame::tunneling_request(self.channel_id, self.send_seq, cemi.as_bytes());
        let frame_bytes = serialize_frame(&frame)?;

        for attempt in 0..MAX_RETRIES {
            self.socket.send(&frame_bytes).await?;

            match self.wait_for_ack().await {
                Ok(buffered) => {
                    self.send_seq = self.send_seq.wrapping_add(1);
                    // TUN-1: Re-process any frames received while waiting for ack
                    for data in buffered {
                        if !self.handle_incoming(&data, cemi_tx).await {
                            return Err(KnxIpError::Closed);
                        }
                    }
                    return Ok(());
                }
                Err(KnxIpError::Timeout(_)) => {
                    tracing::debug!(attempt = attempt + 1, "tunneling ack timeout, retrying");
                }
                Err(e) => return Err(e),
            }
        }

        Err(KnxIpError::Timeout("tunneling ack after max retries"))
    }

    /// Receive one datagram into `buf` before `deadline`, mapping timeouts and
    /// I/O errors to typed errors with `label`.
    async fn recv_until(
        &self,
        buf: &mut [u8],
        deadline: tokio::time::Instant,
        label: &'static str,
    ) -> Result<usize, KnxIpError> {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(KnxIpError::Timeout(label));
        }
        timeout(remaining, self.socket.recv(buf))
            .await
            .map_err(|_| KnxIpError::Timeout(label))?
            .map_err(KnxIpError::Io)
    }

    /// Wait for a tunneling ack matching our channel and sequence.
    /// Returns any non-ack frames received while waiting (TUN-1: no frame loss).
    async fn wait_for_ack(&self) -> Result<Vec<Vec<u8>>, KnxIpError> {
        let mut buf = [0u8; RECV_BUF_SIZE];
        let mut buffered_frames = Vec::new();
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;

        loop {
            let n = self.recv_until(&mut buf, deadline, "tunneling ack").await?;

            if let Ok(resp) = KnxIpFrame::parse(&buf[..n]) {
                if resp.service_type == ServiceType::TunnelingAck {
                    if let Some(ch) = ConnectionHeader::parse(&resp.body) {
                        let channel_matches = ch.channel_id == self.channel_id;
                        let seq_matches = ch.sequence_counter == self.send_seq;
                        if channel_matches && seq_matches {
                            if ch.status != E_NO_ERROR {
                                return Err(KnxIpError::Protocol(format!(
                                    "tunneling ack error: {:#04x}",
                                    ch.status
                                )));
                            }
                            return Ok(buffered_frames);
                        }
                    }
                }
                // Not our ack — buffer for later processing
                buffered_frames.push(buf[..n].to_vec());
            }
        }
    }

    async fn send_heartbeat(
        &mut self,
        cemi_tx: &mpsc::Sender<CemiFrame>,
    ) -> Result<(), KnxIpError> {
        let frame =
            KnxIpFrame::connection_state_request(self.channel_id, build_hpai(self.local_addr));
        self.socket.send(&serialize_frame(&frame)?).await?;

        let mut buf = [0u8; RECV_BUF_SIZE];
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;

        loop {
            let n = self
                .recv_until(&mut buf, deadline, "heartbeat response")
                .await?;

            let Ok(resp) = KnxIpFrame::parse(&buf[..n]) else {
                continue;
            };

            if resp.service_type == ServiceType::ConnectionStateResponse
                && resp.body.len() >= 2
                && resp.body[0] == self.channel_id
            {
                let status = resp.body[1];
                if status != E_NO_ERROR {
                    return Err(KnxIpError::Protocol(format!(
                        "heartbeat rejected: {status:#04x}"
                    )));
                }
                return Ok(());
            }

            // Not our heartbeat response — process as incoming frame
            if !self.handle_incoming(&buf[..n], cemi_tx).await {
                return Err(KnxIpError::Closed);
            }
        }
    }

    async fn send_disconnect(&self) -> Result<(), KnxIpError> {
        let frame = KnxIpFrame::disconnect_request(self.channel_id, build_hpai(self.local_addr));
        self.socket.send(&serialize_frame(&frame)?).await?;
        tracing::debug!(channel_id = self.channel_id, "disconnect sent");
        Ok(())
    }
}
