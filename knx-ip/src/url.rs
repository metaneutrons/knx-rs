// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! URL parsing and connection factory.
//!
//! Supports URLs like:
//! - `udp://192.168.1.50:3671` → tunnel connection
//! - `udp://224.0.23.12:3671` → router connection (multicast auto-detected)
//! - `tunnel://192.168.1.50:3671` → explicit tunnel
//! - `router://224.0.23.12:3671` → explicit router

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::AnyConnection;
use crate::error::KnxIpError;
use crate::router::RouterConnection;
use crate::tunnel::TunnelConnection;

/// Specifies how to connect to a KNXnet/IP endpoint.
#[derive(Debug, Clone)]
pub enum ConnectionSpec {
    /// Unicast tunnel connection to a gateway.
    Tunnel(SocketAddr),
    /// Multicast router connection.
    Router(SocketAddrV4),
}

/// Parse a KNX URL into a [`ConnectionSpec`].
///
/// # Supported formats
///
/// - `udp://host:port` — auto-detects tunnel vs router based on multicast address
/// - `tunnel://host:port` — explicit tunnel
/// - `router://host:port` — explicit router
///
/// # Errors
///
/// Returns [`KnxIpError::InvalidUrl`] if the URL cannot be parsed.
pub fn parse_url(url: &str) -> Result<ConnectionSpec, KnxIpError> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| KnxIpError::InvalidUrl("missing ://".into()))?;

    let addr_str = rest;
    let sock_addr: SocketAddr = addr_str
        .parse()
        .map_err(|e| KnxIpError::InvalidUrl(format!("invalid address '{addr_str}': {e}")))?;

    match scheme {
        "tunnel" => Ok(ConnectionSpec::Tunnel(sock_addr)),
        "router" => {
            let v4 = to_v4(sock_addr)?;
            Ok(ConnectionSpec::Router(v4))
        }
        "udp" => {
            // Auto-detect: multicast addresses → router, otherwise → tunnel
            if let SocketAddr::V4(v4) = sock_addr {
                if v4.ip().is_multicast() {
                    return Ok(ConnectionSpec::Router(v4));
                }
            }
            Ok(ConnectionSpec::Tunnel(sock_addr))
        }
        _ => Err(KnxIpError::InvalidUrl(format!(
            "unsupported scheme '{scheme}'"
        ))),
    }
}

/// Connect to a KNXnet/IP endpoint using the given specification.
///
/// Returns an [`AnyConnection`] that can send and receive CEMI frames.
///
/// # Errors
///
/// Returns [`KnxIpError`] if the connection cannot be established.
pub async fn connect(spec: ConnectionSpec) -> Result<AnyConnection, KnxIpError> {
    match spec {
        ConnectionSpec::Tunnel(addr) => {
            let conn = TunnelConnection::connect(addr).await?;
            Ok(AnyConnection::Tunnel(conn))
        }
        ConnectionSpec::Router(multicast) => {
            let conn = RouterConnection::connect(Ipv4Addr::UNSPECIFIED, multicast).await?;
            Ok(AnyConnection::Router(conn))
        }
    }
}

fn to_v4(addr: SocketAddr) -> Result<SocketAddrV4, KnxIpError> {
    match addr {
        SocketAddr::V4(v4) => Ok(v4),
        SocketAddr::V6(_) => Err(KnxIpError::InvalidUrl("IPv6 not supported".into())),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_tunnel_url() {
        let spec = parse_url("udp://192.168.1.50:3671").unwrap();
        assert!(matches!(spec, ConnectionSpec::Tunnel(_)));
    }

    #[test]
    fn parse_router_url_auto() {
        let spec = parse_url("udp://224.0.23.12:3671").unwrap();
        assert!(matches!(spec, ConnectionSpec::Router(_)));
    }

    #[test]
    fn parse_explicit_tunnel() {
        let spec = parse_url("tunnel://192.168.1.50:3671").unwrap();
        assert!(matches!(spec, ConnectionSpec::Tunnel(_)));
    }

    #[test]
    fn parse_explicit_router() {
        let spec = parse_url("router://224.0.23.12:3671").unwrap();
        assert!(matches!(spec, ConnectionSpec::Router(_)));
    }

    #[test]
    fn parse_invalid_url() {
        assert!(parse_url("192.168.1.50:3671").is_err());
        assert!(parse_url("http://192.168.1.50:3671").is_err());
        assert!(parse_url("udp://not-an-address").is_err());
    }
}
