// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP gateway discovery.
//!
//! Sends a search request to the KNX multicast group and collects
//! responses from gateways on the local network.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use knx_core::knxip::{HostProtocol, Hpai, KnxIpFrame, ServiceType};
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

    let frame = KnxIpFrame {
        service_type: ServiceType::SearchRequest,
        body: hpai.to_bytes().to_vec(),
    };

    let target = SocketAddr::V4(SocketAddrV4::new(KNX_MULTICAST_ADDR, KNX_PORT));
    socket.send_to(&frame.to_bytes(), target).await?;

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
    let address = if hpai.ip == [0, 0, 0, 0] {
        // NAT mode: use source address
        src
    } else {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(hpai.ip), hpai.port))
    };

    // Parse device info DIB (starts at offset 8)
    let (name, individual_address) = if body.len() >= 62 {
        let dib = &body[Hpai::LEN as usize..];
        // DIB structure: length(1) + type(1) + medium(1) + status(1) + individual_addr(2) + ...
        // + serial(6) + multicast(4) + mac(6) + name(30)
        let ia = u16::from_be_bytes([dib[4], dib[5]]);
        let name_bytes = &dib[22..52];
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
