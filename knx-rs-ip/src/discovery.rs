// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP gateway discovery.
//!
//! Sends a search request to the KNX multicast group and collects
//! responses from gateways on the local network.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use knx_rs_core::knxip::{HostProtocol, Hpai, KnxIpFrame, ServiceType};
use tokio::net::UdpSocket;
use tokio::time::{Duration, timeout};

use crate::error::KnxIpError;
use crate::router::{KNX_MULTICAST_ADDR, KNX_PORT};

/// Default discovery timeout.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

/// Information about a discovered KNXnet/IP gateway.
#[derive(Debug, Clone)]
pub struct GatewayInfo {
    /// The control endpoint address of the gateway.
    pub address: SocketAddr,
    /// Device friendly name (from DIB, if available).
    pub name: String,
    /// KNX individual address of the gateway (from DIB, if available).
    pub individual_address: u16,
    /// Raw search response body for further parsing.
    pub raw_body: Vec<u8>,
}

/// Discover KNXnet/IP gateways on the local network.
///
/// Sends a search request to the KNX multicast group and waits for responses.
/// Returns all gateways that respond within the timeout.
///
/// # Errors
///
/// Returns [`KnxIpError`] if the socket cannot be created.
pub async fn discover(local_addr: Ipv4Addr) -> Result<Vec<GatewayInfo>, KnxIpError> {
    discover_with_timeout(local_addr, DISCOVERY_TIMEOUT).await
}

/// Discover gateways with a custom timeout.
///
/// # Errors
///
/// Returns [`KnxIpError`] if the socket cannot be created.
pub async fn discover_with_timeout(
    local_addr: Ipv4Addr,
    duration: Duration,
) -> Result<Vec<GatewayInfo>, KnxIpError> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
    let local_port = socket.local_addr()?.port();

    // Build HPAI for our discovery endpoint
    let hpai = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: if local_addr.is_unspecified() {
            [0, 0, 0, 0]
        } else {
            local_addr.octets()
        },
        port: local_port,
    };

    let target = SocketAddr::V4(SocketAddrV4::new(KNX_MULTICAST_ADDR, KNX_PORT));
    discover_on(socket, hpai, target, duration).await
}

/// Discover KNXnet/IP gateways using an IPv6 multicast target.
///
/// KNXnet/IP HPAI is IPv4-only, so IPv6 discovery requests advertise a
/// standard NAT-mode HPAI and rely on the UDP source address for replies.
///
/// # Errors
///
/// Returns [`KnxIpError`] if the socket cannot be created.
pub async fn discover_v6(
    interface: u32,
    multicast: SocketAddrV6,
) -> Result<Vec<GatewayInfo>, KnxIpError> {
    discover_v6_with_timeout(interface, multicast, DISCOVERY_TIMEOUT).await
}

/// Discover gateways over IPv6 with a custom timeout.
///
/// # Errors
///
/// Returns [`KnxIpError`] if the socket cannot be created.
pub async fn discover_v6_with_timeout(
    interface: u32,
    multicast: SocketAddrV6,
    duration: Duration,
) -> Result<Vec<GatewayInfo>, KnxIpError> {
    if !multicast.ip().is_multicast() {
        return Err(KnxIpError::InvalidConfig(format!(
            "discovery target is not multicast: {multicast}"
        )));
    }
    let scope_id = if interface == 0 {
        multicast.scope_id()
    } else {
        interface
    };
    let socket = UdpSocket::bind(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, scope_id)).await?;
    let local_port = socket.local_addr()?.port();
    let hpai = Hpai::nat_udp(local_port);
    let target = SocketAddr::V6(SocketAddrV6::new(
        *multicast.ip(),
        multicast.port(),
        multicast.flowinfo(),
        scope_id,
    ));
    discover_on(socket, hpai, target, duration).await
}

async fn discover_on(
    socket: UdpSocket,
    hpai: Hpai,
    target: SocketAddr,
    duration: Duration,
) -> Result<Vec<GatewayInfo>, KnxIpError> {
    let frame = KnxIpFrame {
        service_type: ServiceType::SearchRequest,
        body: hpai.to_bytes().to_vec(),
    };
    let bytes = frame.try_to_bytes()?;
    socket.send_to(&bytes, target).await?;

    tracing::debug!("discovery search request sent");

    let mut gateways = Vec::new();
    let mut buf = [0u8; 512];
    let deadline = tokio::time::Instant::now() + duration;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, src))) => {
                if let Some(info) = parse_search_response(&buf[..n], src) {
                    tracing::debug!(name = %info.name, addr = %info.address, "discovered gateway");
                    gateways.push(info);
                }
            }
            Ok(Err(e)) => {
                tracing::trace!(error = %e, "discovery recv error");
            }
            Err(_) => break, // timeout
        }
    }

    Ok(gateways)
}

// DEVICE_INFO DIB field offsets (relative to the start of the DIB, which
// follows the control HPAI). Layout: length(1) type(1) medium(1) status(1)
// individual_addr(2) project_id(2) serial(6) multicast(4) mac(6) name(30).
const DIB_IA_OFFSET: usize = 4;
const DIB_NAME_OFFSET: usize = 24;
const DIB_NAME_LEN: usize = 30;
const MIN_SEARCH_RESPONSE_LEN: usize = Hpai::LEN as usize + DIB_NAME_OFFSET + DIB_NAME_LEN;

/// Parse a search response into gateway info.
fn parse_search_response(data: &[u8], src: SocketAddr) -> Option<GatewayInfo> {
    let frame = KnxIpFrame::parse(data).ok()?;

    if frame.service_type != ServiceType::SearchResponse {
        return None;
    }

    // Body: HPAI (8 bytes) + DIB device info (54 bytes) + DIB supported services (variable)
    let body = &frame.body;

    // Parse control endpoint HPAI
    let hpai = Hpai::parse(body)?;
    let address = if hpai.is_unspecified() {
        // NAT mode: use source address
        socket_addr_with_port(src, hpai.port)
    } else {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(hpai.ip), hpai.port))
    };

    // Parse the DEVICE_INFO DIB (starts after the control HPAI).
    let (name, individual_address) = if body.len() >= MIN_SEARCH_RESPONSE_LEN {
        let dib = &body[usize::from(Hpai::LEN)..];
        let ia = u16::from_be_bytes([dib[DIB_IA_OFFSET], dib[DIB_IA_OFFSET + 1]]);
        let name_bytes = &dib[DIB_NAME_OFFSET..DIB_NAME_OFFSET + DIB_NAME_LEN];
        let name = core::str::from_utf8(name_bytes)
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();
        (name, ia)
    } else {
        (String::new(), 0)
    };

    Some(GatewayInfo {
        address,
        name,
        individual_address,
        raw_body: frame.body.clone(),
    })
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_response_too_short() {
        // Should not panic on short data
        assert!(
            parse_search_response(
                &[0x06, 0x10, 0x02, 0x02, 0x00, 0x06],
                "0.0.0.0:0".parse().unwrap()
            )
            .is_none()
        );
    }
}
