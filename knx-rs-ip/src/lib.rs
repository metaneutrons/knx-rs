// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! `knx-ip` — async KNXnet/IP tunnel and router connections.
//!
//! This crate provides a complete async KNXnet/IP implementation:
//!
//! - [`TunnelConnection`] — unicast tunnel with retry, heartbeat, auto-reconnect
//! - [`RouterConnection`] — multicast routing with rate limiting (50 pkt/s)
//! - [`discovery`] — gateway discovery on the local network
//! - [`Multiplexer`] / [`MultiplexHandle`] — fan out one connection to many handles
//! - [`connect`] / [`parse_url`] — URL-based connection factory
//!
//! # Example
//!
//! ```rust,no_run
//! use knx_rs_ip::{connect, ConnectionSpec};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let spec = ConnectionSpec::Tunnel("192.168.1.50:3671".parse()?);
//! let mut conn = connect(spec).await?;
//! // conn.send(...) / conn.recv() ...
//! # Ok(())
//! # }
//! ```

mod error;
mod router;
mod tunnel;
pub mod tunnel_server;
mod url;

pub mod discovery;
pub mod multiplex;
pub mod ops;

pub use error::{KnxIpError, Result};
pub use multiplex::{MultiplexHandle, Multiplexer};
pub use router::{KNX_MULTICAST_ADDR, KNX_PORT, RouterConnection};
pub use tunnel::{TunnelConfig, TunnelConnection};
pub use tunnel_server::{DeviceServer, ServerEvent};
pub use url::{ConnectionSpec, connect, parse_url};

use core::future::Future;
use core::pin::Pin;

use knx_rs_core::cemi::CemiFrame;
use std::net::SocketAddr;

/// A KNXnet/IP router report that routing frames were lost.
///
/// The count comes from the sending router. It is not a count of every lost
/// KNX bus telegram or of events dropped by a local application subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingLostMessage {
    /// UDP sender of the diagnostic.
    pub source: SocketAddr,
    /// Router device-state byte, retained without interpreting reserved bits.
    pub device_state: u8,
    /// Number of routing messages reported lost by this router.
    pub lost_messages: u16,
}

/// A received cEMI frame or KNXnet/IP routing diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnxReceiveEvent {
    /// A cEMI telegram from a tunnel or router.
    Frame(CemiFrame),
    /// A router-reported lost-message diagnostic (service `0x0531`).
    RoutingLostMessage(RoutingLostMessage),
}

/// Boxed `Send` future returned by KNX connection traits.
pub type KnxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for KNXnet/IP connections that can send and receive CEMI frames.
///
/// Both tunnel (unicast) and router (multicast) connections implement this.
pub trait KnxConnection: Send {
    /// Send a CEMI frame to the KNX bus.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the frame could not be sent.
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<()>>;

    /// Receive the next CEMI frame from the KNX bus.
    ///
    /// Returns `None` if the connection is closed.
    fn recv(&mut self) -> KnxFuture<'_, Option<CemiFrame>>;

    /// Receive the next frame or routing diagnostic.
    ///
    /// Non-routing implementations return only [`KnxReceiveEvent::Frame`].
    /// The existing [`Self::recv`] method remains frame-only for callers that
    /// do not consume diagnostics.
    fn recv_event(&mut self) -> KnxFuture<'_, Option<KnxReceiveEvent>> {
        Box::pin(async move { self.recv().await.map(KnxReceiveEvent::Frame) })
    }

    /// Close the connection gracefully.
    fn close(&mut self) -> KnxFuture<'_, ()>;
}

/// A type-erased KNX connection — either tunnel or router.
///
/// Returned by [`connect()`] when the connection type is determined at runtime.
pub enum AnyConnection {
    /// Tunnel connection.
    Tunnel(TunnelConnection),
    /// Router connection.
    Router(RouterConnection),
}

impl KnxConnection for AnyConnection {
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<()>> {
        match self {
            Self::Tunnel(c) => c.send(frame),
            Self::Router(c) => c.send(frame),
        }
    }

    fn recv(&mut self) -> KnxFuture<'_, Option<CemiFrame>> {
        match self {
            Self::Tunnel(c) => c.recv(),
            Self::Router(c) => c.recv(),
        }
    }

    fn recv_event(&mut self) -> KnxFuture<'_, Option<KnxReceiveEvent>> {
        match self {
            Self::Tunnel(c) => c.recv_event(),
            Self::Router(c) => c.recv_event(),
        }
    }

    fn close(&mut self) -> KnxFuture<'_, ()> {
        match self {
            Self::Tunnel(c) => c.close(),
            Self::Router(c) => c.close(),
        }
    }
}
