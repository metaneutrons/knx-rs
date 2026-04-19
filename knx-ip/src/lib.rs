// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

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
//! use knx_ip::{connect, ConnectionSpec};
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

use knx_core::cemi::CemiFrame;

/// Trait for KNXnet/IP connections that can send and receive CEMI frames.
///
/// Both tunnel (unicast) and router (multicast) connections implement this.
#[allow(async_fn_in_trait)] // All impls are Send — we control the full set
pub trait KnxConnection: Send {
    /// Send a CEMI frame to the KNX bus.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the frame could not be sent.
    async fn send(&mut self, frame: CemiFrame) -> Result<(), KnxIpError>;

    /// Receive the next CEMI frame from the KNX bus.
    ///
    /// Returns `None` if the connection is closed.
    async fn recv(&mut self) -> Option<CemiFrame>;

    /// Close the connection gracefully.
    async fn close(&mut self);
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
    async fn send(&mut self, frame: CemiFrame) -> Result<(), KnxIpError> {
        match self {
            Self::Tunnel(c) => c.send(frame).await,
            Self::Router(c) => c.send(frame).await,
        }
    }

    async fn recv(&mut self) -> Option<CemiFrame> {
        match self {
            Self::Tunnel(c) => c.recv().await,
            Self::Router(c) => c.recv().await,
        }
    }

    async fn close(&mut self) {
        match self {
            Self::Tunnel(c) => c.close().await,
            Self::Router(c) => c.close().await,
        }
    }
}
