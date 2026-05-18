// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP router connection (multicast UDP).
//!
//! Joins the KNX multicast group (default `224.0.23.12:3671`) and
//! sends/receives routing indications with rate limiting per KNX spec.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{KnxIpFrame, ServiceType};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

use crate::error::KnxIpError;
use crate::{KnxConnection, KnxFuture};

/// Default KNX multicast address.
pub const KNX_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 23, 12);

/// Default KNX port.
pub const KNX_PORT: u16 = 3671;

/// KNX spec: max 50 routing indications per second (KNX 3.2.6 p.6).
const MAX_PACKETS_PER_SEC: u32 = 50;

/// A KNXnet/IP router connection over multicast UDP.
pub struct RouterConnection {
    rx: mpsc::Receiver<CemiFrame>,
    tx_cmd: mpsc::Sender<RouterCmd>,
}

enum RouterCmd {
    Send(
        CemiFrame,
        tokio::sync::oneshot::Sender<Result<(), KnxIpError>>,
    ),
    Close,
}

impl RouterConnection {
    /// Join the KNX multicast group and start receiving routing indications.
    ///
    /// `local_addr` is the local interface to bind to (use `0.0.0.0` for any).
    /// `multicast` is the multicast group + port (default `224.0.23.12:3671`).
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created or joined.
    pub async fn connect(
        local_addr: Ipv4Addr,
        multicast: SocketAddrV4,
    ) -> Result<Self, KnxIpError> {
        Self::connect_v4(local_addr, multicast).await
    }

    /// Join an IPv4 KNX multicast group and start receiving routing indications.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created or joined.
    pub async fn connect_v4(
        local_addr: Ipv4Addr,
        multicast: SocketAddrV4,
    ) -> Result<Self, KnxIpError> {
        if !multicast.ip().is_multicast() {
            return Err(KnxIpError::Protocol(format!(
                "router target is not multicast: {multicast}"
            )));
        }
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, multicast.port());
        let socket = UdpSocket::bind(bind_addr).await?;

        socket
            .join_multicast_v4(*multicast.ip(), local_addr)
            .map_err(|e| KnxIpError::Protocol(format!("join multicast {}: {e}", multicast.ip())))?;

        socket.set_multicast_loop_v4(false).ok();
        Ok(Self::spawn(socket, SocketAddr::V4(multicast)))
    }

    /// Join an IPv6 multicast group and start receiving routing indications.
    ///
    /// Use the target address scope id or pass an explicit interface index for
    /// link-local multicast groups.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created or joined.
    pub async fn connect_v6(interface: u32, multicast: SocketAddrV6) -> Result<Self, KnxIpError> {
        if !multicast.ip().is_multicast() {
            return Err(KnxIpError::Protocol(format!(
                "router target is not multicast: {multicast}"
            )));
        }
        let interface = if interface == 0 {
            multicast.scope_id()
        } else {
            interface
        };
        let bind_addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, multicast.port(), 0, interface);
        let socket = UdpSocket::bind(bind_addr).await?;

        socket
            .join_multicast_v6(multicast.ip(), interface)
            .map_err(|e| KnxIpError::Protocol(format!("join multicast {}: {e}", multicast.ip())))?;

        socket.set_multicast_loop_v6(false).ok();
        Ok(Self::spawn(socket, SocketAddr::V6(multicast)))
    }

    /// Join a KNX multicast group from a generic socket address.
    ///
    /// IPv4 uses `0.0.0.0` as the interface selector. IPv6 uses the target
    /// scope id as the interface index when present.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created or joined.
    pub async fn connect_multicast(multicast: SocketAddr) -> Result<Self, KnxIpError> {
        match multicast {
            SocketAddr::V4(v4) => Self::connect_v4(Ipv4Addr::UNSPECIFIED, v4).await,
            SocketAddr::V6(v6) => Self::connect_v6(v6.scope_id(), v6).await,
        }
    }

    /// Connect to the default KNX multicast group (`224.0.23.12:3671`).
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the socket cannot be created.
    pub async fn connect_default(local_addr: Ipv4Addr) -> Result<Self, KnxIpError> {
        Self::connect(local_addr, SocketAddrV4::new(KNX_MULTICAST_ADDR, KNX_PORT)).await
    }

    fn spawn(socket: UdpSocket, target: SocketAddr) -> Self {
        tracing::info!(%target, "KNXnet/IP router joined multicast");

        let (cemi_tx, cemi_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        tokio::spawn(router_task(socket, target, cemi_tx, cmd_rx));

        Self {
            rx: cemi_rx,
            tx_cmd: cmd_tx,
        }
    }
}

impl KnxConnection for RouterConnection {
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<(), KnxIpError>> {
        let tx_cmd = self.tx_cmd.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            tx_cmd
                .send(RouterCmd::Send(frame, tx))
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
            let _ = tx_cmd.send(RouterCmd::Close).await;
        })
    }
}

// ── Rate limiter ──────────────────────────────────────────────

/// Sliding-window rate limiter: max N events per 1-second window.
struct RateLimiter {
    timestamps: std::collections::VecDeque<Instant>,
    max_per_sec: u32,
}

impl RateLimiter {
    fn new(max_per_sec: u32) -> Self {
        Self {
            timestamps: std::collections::VecDeque::with_capacity(max_per_sec as usize),
            max_per_sec,
        }
    }

    /// Check if a send is allowed. If not, returns the duration to wait.
    fn check(&mut self) -> Option<Duration> {
        let now = Instant::now();
        let window_start = now - Duration::from_secs(1);

        // Remove timestamps older than 1 second
        while self.timestamps.front().is_some_and(|&t| t < window_start) {
            self.timestamps.pop_front();
        }

        if self.timestamps.len() < self.max_per_sec as usize {
            self.timestamps.push_back(now);
            None // allowed
        } else {
            // Must wait until the oldest timestamp exits the window
            self.timestamps
                .front()
                .map(|&oldest| (oldest + Duration::from_secs(1)) - now)
        }
    }

    /// Force a pause on the next send (used by `RoutingBusy` handling).
    fn pause(&mut self, duration: Duration) {
        // Fill the window with future timestamps to block sends for `duration`
        let future = Instant::now() + duration;
        self.timestamps.clear();
        for _ in 0..self.max_per_sec {
            self.timestamps.push_back(future);
        }
    }
}

// ── Background task ───────────────────────────────────────────

async fn router_task(
    socket: UdpSocket,
    target: SocketAddr,
    cemi_tx: mpsc::Sender<CemiFrame>,
    mut cmd_rx: mpsc::Receiver<RouterCmd>,
) {
    let mut buf = [0u8; 1024];
    let mut rate_limiter = RateLimiter::new(MAX_PACKETS_PER_SEC);

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                let (n, _src) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "router recv error");
                        break;
                    }
                };
                handle_routing_indication(&buf[..n], &cemi_tx, &mut rate_limiter).await;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(RouterCmd::Send(cemi, reply)) => {
                        let result = rate_limited_send(
                            &socket, &target, &cemi, &mut rate_limiter,
                        ).await;
                        let _ = reply.send(result);
                    }
                    Some(RouterCmd::Close) | None => break,
                }
            }
        }
    }

    tracing::debug!("router task ended");
}

async fn rate_limited_send(
    socket: &UdpSocket,
    target: &SocketAddr,
    cemi: &CemiFrame,
    limiter: &mut RateLimiter,
) -> Result<(), KnxIpError> {
    // Wait if rate limit exceeded
    if let Some(wait) = limiter.check() {
        tracing::debug!(wait_ms = wait.as_millis(), "rate limit: waiting");
        tokio::time::sleep(wait).await;
        // Re-check after waiting (the check also records the timestamp)
        if let Some(extra_wait) = limiter.check() {
            tokio::time::sleep(extra_wait).await;
            let _ = limiter.check(); // record
        }
    }

    let frame = KnxIpFrame {
        service_type: ServiceType::RoutingIndication,
        body: cemi.as_bytes().to_vec(),
    };
    let bytes = frame
        .try_to_bytes()
        .map_err(|e| KnxIpError::Protocol(e.to_string()))?;
    socket.send_to(&bytes, target).await?;
    Ok(())
}

async fn handle_routing_indication(
    data: &[u8],
    cemi_tx: &mpsc::Sender<CemiFrame>,
    rate_limiter: &mut RateLimiter,
) {
    let frame = match KnxIpFrame::parse(data) {
        Ok(f) => f,
        Err(e) => {
            tracing::trace!(error = %e, "ignoring malformed frame");
            return;
        }
    };

    match frame.service_type {
        ServiceType::RoutingIndication => {
            if let Ok(cemi) = CemiFrame::parse(&frame.body) {
                let _ = cemi_tx.send(cemi).await;
            }
        }
        ServiceType::RoutingBusy => {
            // KNX 3.2.6 §4.4: pause sending for the specified wait time
            let wait_ms = if frame.body.len() >= 6 {
                u16::from_be_bytes([frame.body[4], frame.body[5]])
            } else {
                50 // default 50ms per spec
            };
            tracing::debug!(wait_ms, "received RoutingBusy, pausing sends");
            // Drain the rate limiter to force a pause on next send
            rate_limiter.pause(Duration::from_millis(u64::from(wait_ms)));
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(3);
        assert!(limiter.check().is_none());
        assert!(limiter.check().is_none());
        assert!(limiter.check().is_none());
        // 4th should be rate-limited
        assert!(limiter.check().is_some());
    }
}
