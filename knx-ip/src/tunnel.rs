// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNXnet/IP tunnel connection (unicast UDP).
//!
//! Implements the full KNXnet/IP tunneling protocol per KNX specification:
//! - Connect handshake with timeout
//! - Tunneling request/ack with sequence counting and 3× retry
//! - Heartbeat every 60s with failure counting (3 failures = disconnect)
//! - Graceful disconnect handshake
//! - Auto-reconnect with exponential backoff (optional)

use std::net::SocketAddr;
use std::pin::Pin;

use knx_core::cemi::CemiFrame;
use knx_core::knxip::{ConnectionHeader, HostProtocol, Hpai, KnxIpFrame, ServiceType};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};

use crate::KnxConnection;
use crate::error::KnxIpError;

/// KNX spec: connect request timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// KNX spec: heartbeat interval.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
/// KNX spec: tunneling request ack timeout (per attempt).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
/// KNX spec: maximum tunneling request retries.
const MAX_RETRIES: u8 = 3;
/// KNX spec: heartbeat failures before disconnect.
const MAX_HEARTBEAT_FAILURES: u8 = 3;

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
    async fn send(&self, frame: CemiFrame) -> Result<(), KnxIpError> {
        let (tx, rx) = oneshot::channel();
        self.tx_cmd
            .send(TunnelCmd::Send(frame, tx))
            .await
            .map_err(|_| KnxIpError::Closed)?;
        rx.await.map_err(|_| KnxIpError::Closed)?
    }

    async fn recv(&mut self) -> Option<CemiFrame> {
        self.rx.recv().await
    }

    async fn close(&mut self) {
        let _ = self.tx_cmd.send(TunnelCmd::Close).await;
    }
}

// ── Connection establishment ──────────────────────────────────

async fn establish(remote: &SocketAddr) -> Result<(UdpSocket, u8, SocketAddr), KnxIpError> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(remote).await?;
    let local_addr = socket.local_addr()?;
    let channel_id = do_connect(&socket, local_addr).await?;
    tracing::info!(%remote, channel_id, "KNXnet/IP tunnel connected");
    Ok((socket, channel_id, local_addr))
}

const fn build_hpai(addr: SocketAddr) -> Hpai {
    match addr {
        SocketAddr::V4(v4) => Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: v4.ip().octets(),
            port: v4.port(),
        },
        SocketAddr::V6(_) => Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [0, 0, 0, 0],
            port: 0,
        },
    }
}

async fn do_connect(socket: &UdpSocket, local_addr: SocketAddr) -> Result<u8, KnxIpError> {
    let hpai = build_hpai(local_addr);
    let hpai_bytes = hpai.to_bytes();

    // CRI: tunnel connection (0x04), TP link layer (0x02)
    let cri = [0x04, 0x04, 0x02, 0x00];

    let mut body = Vec::with_capacity(20);
    body.extend_from_slice(&hpai_bytes); // control endpoint
    body.extend_from_slice(&hpai_bytes); // data endpoint
    body.extend_from_slice(&cri);

    let frame = KnxIpFrame {
        service_type: ServiceType::ConnectRequest,
        body,
    };
    socket.send(&frame.to_bytes()).await?;

    let mut buf = [0u8; 256];
    let n = timeout(CONNECT_TIMEOUT, socket.recv(&mut buf))
        .await
        .map_err(|_| KnxIpError::Timeout("connect response"))?
        .map_err(KnxIpError::Io)?;

    let resp = KnxIpFrame::parse(&buf[..n])
        .map_err(|e| KnxIpError::Protocol(format!("connect response: {e}")))?;

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

    if status != 0 {
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
                state.handle_incoming(&buf[..n], &cemi_tx).await;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(TunnelCmd::Send(frame, reply)) => {
                        let result = state.send_with_retry(&frame).await;
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
                if let Err(e) = state.send_heartbeat().await {
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
                if matches!(cmd, Some(TunnelCmd::Close) | None) {
                    tracing::info!("reconnect cancelled by close");
                    return false;
                }
                // Ignore other commands during reconnect
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
    async fn handle_incoming(&mut self, data: &[u8], cemi_tx: &mpsc::Sender<CemiFrame>) {
        let frame = match KnxIpFrame::parse(data) {
            Ok(f) => f,
            Err(e) => {
                tracing::trace!(error = %e, "ignoring malformed frame");
                return;
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
                    body: vec![self.channel_id, 0],
                };
                let _ = self.socket.send(&resp.to_bytes()).await;
            }
            _ => {
                tracing::trace!(service = ?frame.service_type, "ignoring frame");
            }
        }
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
        let ack_ch = ConnectionHeader {
            channel_id: self.channel_id,
            sequence_counter: ch.sequence_counter,
            status: 0,
        };
        let ack = KnxIpFrame {
            service_type: ServiceType::TunnelingAck,
            body: ack_ch.to_bytes().to_vec(),
        };
        let _ = self.socket.send(&ack.to_bytes()).await;

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
    async fn send_with_retry(&mut self, cemi: &CemiFrame) -> Result<(), KnxIpError> {
        let ch = ConnectionHeader {
            channel_id: self.channel_id,
            sequence_counter: self.send_seq,
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

        for attempt in 0..MAX_RETRIES {
            self.socket.send(&frame_bytes).await?;

            match self.wait_for_ack().await {
                Ok(()) => {
                    self.send_seq = self.send_seq.wrapping_add(1);
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

    /// Wait for a tunneling ack matching our channel and sequence.
    async fn wait_for_ack(&self) -> Result<(), KnxIpError> {
        let mut buf = [0u8; 256];
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(KnxIpError::Timeout("tunneling ack"));
            }

            let n = timeout(remaining, self.socket.recv(&mut buf))
                .await
                .map_err(|_| KnxIpError::Timeout("tunneling ack"))?
                .map_err(KnxIpError::Io)?;

            if let Ok(resp) = KnxIpFrame::parse(&buf[..n]) {
                if resp.service_type == ServiceType::TunnelingAck {
                    if let Some(ch) = ConnectionHeader::parse(&resp.body) {
                        let channel_matches = ch.channel_id == self.channel_id;
                        let seq_matches = ch.sequence_counter == self.send_seq;
                        if channel_matches && seq_matches {
                            if ch.status != 0 {
                                return Err(KnxIpError::Protocol(format!(
                                    "tunneling ack error: {:#04x}",
                                    ch.status
                                )));
                            }
                            return Ok(());
                        }
                    }
                }
                // Not our ack — keep waiting
            }
        }
    }

    async fn send_heartbeat(&self) -> Result<(), KnxIpError> {
        let hpai = build_hpai(self.local_addr);
        let mut body = Vec::with_capacity(10);
        body.push(self.channel_id);
        body.push(0);
        body.extend_from_slice(&hpai.to_bytes());

        let frame = KnxIpFrame {
            service_type: ServiceType::ConnectionStateRequest,
            body,
        };
        self.socket.send(&frame.to_bytes()).await?;

        let mut buf = [0u8; 64];
        let n = timeout(REQUEST_TIMEOUT, self.socket.recv(&mut buf))
            .await
            .map_err(|_| KnxIpError::Timeout("heartbeat response"))?
            .map_err(KnxIpError::Io)?;

        let resp = KnxIpFrame::parse(&buf[..n])
            .map_err(|e| KnxIpError::Protocol(format!("heartbeat response: {e}")))?;

        if resp.service_type == ServiceType::ConnectionStateResponse
            && resp.body.len() >= 2
            && resp.body[0] == self.channel_id
        {
            let status = resp.body[1];
            if status != 0 {
                return Err(KnxIpError::Protocol(format!(
                    "heartbeat rejected: {status:#04x}"
                )));
            }
        }

        Ok(())
    }

    async fn send_disconnect(&self) -> Result<(), KnxIpError> {
        let hpai = build_hpai(self.local_addr);
        let mut body = Vec::with_capacity(10);
        body.push(self.channel_id);
        body.push(0);
        body.extend_from_slice(&hpai.to_bytes());

        let frame = KnxIpFrame {
            service_type: ServiceType::DisconnectRequest,
            body,
        };
        self.socket.send(&frame.to_bytes()).await?;
        tracing::debug!(channel_id = self.channel_id, "disconnect sent");
        Ok(())
    }
}
