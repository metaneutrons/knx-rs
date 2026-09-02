// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP gateway discovery.
//!
//! Sends a search request to the KNX multicast group and collects
//! responses from gateways on the local network.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use knx_rs_core::knxip::dib::{DeviceInformationDib, DibParseError, DibSequence, DibType};
use knx_rs_core::knxip::{HostProtocol, Hpai, KnxIpFrame, KnxIpParseError, ServiceType};
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
    /// Device friendly name from the Device Information DIB.
    pub name: String,
    /// KNX individual address of the gateway from the Device Information DIB.
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
            Ok(Ok((n, src))) => match parse_search_response(&buf[..n], src) {
                Ok(Some(info)) => {
                    tracing::debug!(name = %info.name, addr = %info.address, "discovered gateway");
                    gateways.push(info);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::trace!(%error, source = %src, "ignoring invalid discovery response");
                }
            },
            Ok(Err(e)) => {
                tracing::trace!(error = %e, "discovery recv error");
            }
            Err(_) => break, // timeout
        }
    }

    Ok(gateways)
}

#[derive(Debug, thiserror::Error)]
enum SearchResponseParseError {
    #[error("invalid KNXnet/IP frame: {0}")]
    Frame(#[from] KnxIpParseError),
    #[error("invalid control endpoint HPAI")]
    ControlEndpoint,
    #[error("invalid description information blocks: {0}")]
    Dib(#[from] DibParseError),
    #[error("search response has no Device Information DIB")]
    MissingDeviceInformation,
    #[error("search response has no Supported Service Families DIB")]
    MissingSupportedServiceFamilies,
}

/// Parse a search response into gateway info.
fn parse_search_response(
    data: &[u8],
    src: SocketAddr,
) -> Result<Option<GatewayInfo>, SearchResponseParseError> {
    let frame = KnxIpFrame::parse(data)?;

    if frame.service_type != ServiceType::SearchResponse {
        return Ok(None);
    }

    // Body: HPAI (8 bytes) + DIB device info (54 bytes) + DIB supported services (variable)
    let body = &frame.body;

    // Parse control endpoint HPAI
    let hpai = Hpai::parse(body).ok_or(SearchResponseParseError::ControlEndpoint)?;
    let address = if hpai.is_unspecified() {
        // NAT mode: use source address
        socket_addr_with_port(src, hpai.port)
    } else {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(hpai.ip), hpai.port))
    };

    let dibs = DibSequence::parse(&body[usize::from(Hpai::LEN)..])?;
    let device_information = dibs
        .get(DibType::DeviceInformation)
        .ok_or(SearchResponseParseError::MissingDeviceInformation)
        .and_then(|dib| DeviceInformationDib::parse(dib).map_err(Into::into))?;
    dibs.get(DibType::SupportedServiceFamilies)
        .ok_or(SearchResponseParseError::MissingSupportedServiceFamilies)?;

    Ok(Some(GatewayInfo {
        address,
        name: device_information.friendly_name(),
        individual_address: device_information.individual_address(),
        raw_body: frame.body,
    }))
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
#[allow(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    // Cross-implementation vector from XKNX's SearchResponse test fixture.
    const XKNX_SEARCH_RESPONSE: [u8; 80] = [
        0x06, 0x10, 0x02, 0x02, 0x00, 0x50, 0x08, 0x01, 0xC0, 0xA8, 0x2A, 0x0A, 0x0E, 0x57, 0x36,
        0x01, 0x02, 0x00, 0x11, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0xE0, 0x00,
        0x17, 0x0C, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x47, 0x69, 0x72, 0x61, 0x20, 0x4B, 0x4E,
        0x58, 0x2F, 0x49, 0x50, 0x2D, 0x52, 0x6F, 0x75, 0x74, 0x65, 0x72, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x02, 0x02, 0x01, 0x03, 0x02, 0x04,
        0x01, 0x05, 0x01, 0x07, 0x01,
    ];

    fn device_information(name: &[u8]) -> Vec<u8> {
        let mut dib = vec![0; DeviceInformationDib::LEN];
        dib[0] = DeviceInformationDib::LEN as u8;
        dib[1] = DeviceInformationDib::TYPE.to_raw();
        dib[2] = 0x02;
        dib[4..6].copy_from_slice(&0x1234_u16.to_be_bytes());
        dib[18..24].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, b'E', b'F']);
        dib[24..24 + name.len()].copy_from_slice(name);
        dib
    }

    fn search_response(hpai: Hpai, dibs: &[u8]) -> Vec<u8> {
        let mut body = hpai.to_bytes().to_vec();
        body.extend_from_slice(dibs);
        KnxIpFrame {
            service_type: ServiceType::SearchResponse,
            body,
        }
        .try_to_bytes()
        .unwrap()
    }

    fn complete_search_response(hpai: Hpai, device_information: &[u8]) -> Vec<u8> {
        let mut dibs = device_information.to_vec();
        dibs.extend_from_slice(&[4, DibType::SupportedServiceFamilies.to_raw(), 0x02, 0x01]);
        search_response(hpai, &dibs)
    }

    #[test]
    fn parses_reference_search_response() {
        let info =
            parse_search_response(&XKNX_SEARCH_RESPONSE, "198.51.100.1:1234".parse().unwrap())
                .unwrap()
                .unwrap();

        assert_eq!(info.name, "Gira KNX/IP-Router");
        assert_eq!(info.individual_address, 0x1100);
        assert_eq!(info.address, "192.168.42.10:3671".parse().unwrap());
    }

    #[test]
    fn parses_name_after_complete_mac_address() {
        let data = complete_search_response(
            Hpai {
                protocol: HostProtocol::Ipv4Udp,
                ip: [192, 0, 2, 10],
                port: 3671,
            },
            &device_information(b"Gateway"),
        );
        let info = parse_search_response(&data, "198.51.100.1:1234".parse().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(info.name, "Gateway");
        assert_eq!(info.individual_address, 0x1234);
    }

    #[test]
    fn parses_latin1_name() {
        let data = complete_search_response(Hpai::nat_udp(3671), &device_information(b"Ger\xE4t"));
        let info = parse_search_response(&data, "198.51.100.1:1234".parse().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(info.name, "Ger\u{e4}t");
    }

    #[test]
    fn finds_device_information_after_another_dib() {
        let mut dibs = vec![4, 0x02, 0x02, 0x01];
        dibs.extend_from_slice(&device_information(b"Gateway"));
        let data = search_response(Hpai::nat_udp(3671), &dibs);
        let info = parse_search_response(&data, "198.51.100.1:1234".parse().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(info.name, "Gateway");
        assert_eq!(info.individual_address, 0x1234);
    }

    #[test]
    fn does_not_interpret_another_dib_type_as_device_information() {
        let mut dib = device_information(b"Gateway");
        dib[1] = 0x02;
        let data = search_response(Hpai::nat_udp(3671), &dib);
        assert!(matches!(
            parse_search_response(&data, "198.51.100.1:1234".parse().unwrap()),
            Err(SearchResponseParseError::MissingDeviceInformation)
        ));
    }

    #[test]
    fn rejects_malformed_dib_sequence() {
        let data = search_response(Hpai::nat_udp(3671), &[4, 0x01]);

        assert!(matches!(
            parse_search_response(&data, "198.51.100.1:1234".parse().unwrap()),
            Err(SearchResponseParseError::Dib(
                DibParseError::TruncatedStructure { .. }
            ))
        ));
    }

    #[test]
    fn rejects_response_without_supported_service_families() {
        let data = search_response(Hpai::nat_udp(3671), &device_information(b"Gateway"));

        assert!(matches!(
            parse_search_response(&data, "198.51.100.1:1234".parse().unwrap()),
            Err(SearchResponseParseError::MissingSupportedServiceFamilies)
        ));
    }

    #[test]
    fn preserves_ipv6_source_for_nat_mode_response() {
        let data =
            complete_search_response(Hpai::nat_udp(3671), &device_information(b"IPv6 Gateway"));
        let source = SocketAddr::V6(SocketAddrV6::new(
            "fe80::1234".parse().unwrap(),
            45678,
            9,
            7,
        ));
        let info = parse_search_response(&data, source).unwrap().unwrap();

        assert_eq!(
            info.address,
            SocketAddr::V6(SocketAddrV6::new("fe80::1234".parse().unwrap(), 3671, 9, 7,))
        );
        assert_eq!(info.name, "IPv6 Gateway");
    }

    #[test]
    fn uses_ipv6_source_port_when_nat_hpai_port_is_zero() {
        let data = complete_search_response(Hpai::nat_udp(0), &device_information(b"IPv6 Gateway"));
        let source = SocketAddr::V6(SocketAddrV6::new(
            "2001:db8::1".parse().unwrap(),
            45678,
            0,
            0,
        ));
        let info = parse_search_response(&data, source).unwrap().unwrap();

        assert_eq!(info.address, source);
    }

    #[test]
    fn parse_search_response_too_short() {
        // Should not panic on short data
        assert!(matches!(
            parse_search_response(
                &[0x06, 0x10, 0x02, 0x02, 0x00, 0x06],
                "0.0.0.0:0".parse().unwrap()
            ),
            Err(SearchResponseParseError::ControlEndpoint)
        ));
    }
}
