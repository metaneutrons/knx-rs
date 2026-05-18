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

pub use error::KnxIpError;
pub use multiplex::{MultiplexHandle, Multiplexer};
pub use router::{KNX_MULTICAST_ADDR, KNX_PORT, RouterConnection};
pub use tunnel::{TunnelConfig, TunnelConnection};
pub use tunnel_server::{DeviceServer, ServerEvent};
pub use url::{ConnectionSpec, connect, parse_url};

use core::future::Future;
use core::pin::Pin;

use knx_rs_core::cemi::CemiFrame;

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
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<(), KnxIpError>>;

    /// Receive the next CEMI frame from the KNX bus.
    ///
    /// Returns `None` if the connection is closed.
    fn recv(&mut self) -> KnxFuture<'_, Option<CemiFrame>>;

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
    fn send(&self, frame: CemiFrame) -> KnxFuture<'_, Result<(), KnxIpError>> {
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

    fn close(&mut self) -> KnxFuture<'_, ()> {
        match self {
            Self::Tunnel(c) => c.close(),
            Self::Router(c) => c.close(),
        }
    }
}
