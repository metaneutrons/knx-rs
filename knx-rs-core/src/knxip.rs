// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP frame types.
//!
//! Provides parsing and construction of KNXnet/IP protocol frames used for
//! IP-based KNX communication (tunneling, routing, discovery).
//!
//! # Wire Layout
//!
//! ```text
//! Offset  Field              Size
//! ──────  ─────              ────
//!   0     Header Length       1 byte  (always 0x06)
//!   1     Protocol Version    1 byte  (always 0x10)
//!   2     Service Type        2 bytes (big-endian)
//!   4     Total Length         2 bytes (big-endian, includes header)
//!   6     Body                variable
//! ```

use alloc::vec::Vec;
use core::fmt;

/// KNXnet/IP header length (always 6 bytes).
pub const HEADER_LEN: u8 = 0x06;

/// KNXnet/IP protocol version 1.0.
pub const PROTOCOL_VERSION_10: u8 = 0x10;

/// Well-known KNXnet/IP UDP port.
pub const KNX_PORT: u16 = 3671;

// ── Connection management (CRI/CRD + status) ─────────────────

/// Connection type byte for a tunnelling connection.
pub const TUNNEL_CONNECTION: u8 = 0x04;
/// Connection type byte for a device-management connection.
pub const DEVICE_MGMT_CONNECTION: u8 = 0x03;
/// KNX layer byte for a link-layer tunnel.
pub const TUNNEL_LINKLAYER: u8 = 0x02;
/// Length of a CRI/CRD block (structure length octet value).
pub const CRI_CRD_LEN: u8 = 0x04;
/// Default base for a tunnel server's assigned individual address (15.15.x).
pub const TUNNEL_IA_BASE: u16 = 0xFF00;

/// Connect/connection-state/disconnect status: no error.
pub const E_NO_ERROR: u8 = 0x00;
/// Status: the connection ID is not known to the server.
pub const E_CONNECTION_ID: u8 = 0x21;
/// Status: the requested connection type is not supported.
pub const E_CONNECTION_TYPE: u8 = 0x22;
/// Status: the server has no free connection slots.
pub const E_NO_MORE_CONNECTIONS: u8 = 0x24;

/// Build a tunnel Connection Request Information (CRI) block.
#[must_use]
pub const fn tunnel_cri() -> [u8; 4] {
    [CRI_CRD_LEN, TUNNEL_CONNECTION, TUNNEL_LINKLAYER, 0x00]
}

/// Build a tunnel Connection Response Data (CRD) block carrying the assigned
/// individual address.
#[must_use]
pub const fn tunnel_crd(individual_address: u16) -> [u8; 4] {
    let ia = individual_address.to_be_bytes();
    [CRI_CRD_LEN, TUNNEL_CONNECTION, ia[0], ia[1]]
}

/// KNXnet/IP service type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ServiceType {
    /// Search request.
    SearchRequest = 0x0201,
    /// Search response.
    SearchResponse = 0x0202,
    /// Description request.
    DescriptionRequest = 0x0203,
    /// Description response.
    DescriptionResponse = 0x0204,
    /// Connect request.
    ConnectRequest = 0x0205,
    /// Connect response.
    ConnectResponse = 0x0206,
    /// Connection state request.
    ConnectionStateRequest = 0x0207,
    /// Connection state response.
    ConnectionStateResponse = 0x0208,
    /// Disconnect request.
    DisconnectRequest = 0x0209,
    /// Disconnect response.
    DisconnectResponse = 0x020A,
    /// Extended search request.
    SearchRequestExtended = 0x020B,
    /// Extended search response.
    SearchResponseExtended = 0x020C,
    /// Device configuration request.
    DeviceConfigurationRequest = 0x0310,
    /// Device configuration acknowledgement.
    DeviceConfigurationAck = 0x0311,
    /// Tunneling request.
    TunnelingRequest = 0x0420,
    /// Tunneling acknowledgement.
    TunnelingAck = 0x0421,
    /// Routing indication (multicast).
    RoutingIndication = 0x0530,
    /// Routing lost message.
    RoutingLostMessage = 0x0531,
    /// Routing busy.
    RoutingBusy = 0x0532,
}

impl ServiceType {
    /// Try to convert a raw `u16` to a `ServiceType`.
    pub const fn from_raw(raw: u16) -> Option<Self> {
        Some(match raw {
            0x0201 => Self::SearchRequest,
            0x0202 => Self::SearchResponse,
            0x0203 => Self::DescriptionRequest,
            0x0204 => Self::DescriptionResponse,
            0x0205 => Self::ConnectRequest,
            0x0206 => Self::ConnectResponse,
            0x0207 => Self::ConnectionStateRequest,
            0x0208 => Self::ConnectionStateResponse,
            0x0209 => Self::DisconnectRequest,
            0x020A => Self::DisconnectResponse,
            0x020B => Self::SearchRequestExtended,
            0x020C => Self::SearchResponseExtended,
            0x0310 => Self::DeviceConfigurationRequest,
            0x0311 => Self::DeviceConfigurationAck,
            0x0420 => Self::TunnelingRequest,
            0x0421 => Self::TunnelingAck,
            0x0530 => Self::RoutingIndication,
            0x0531 => Self::RoutingLostMessage,
            0x0532 => Self::RoutingBusy,
            _ => return None,
        })
    }
}

/// Host protocol code for HPAI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HostProtocol {
    /// IPv4 over UDP.
    Ipv4Udp = 0x01,
    /// IPv4 over TCP.
    Ipv4Tcp = 0x02,
}

impl HostProtocol {
    /// Try to convert a raw host protocol code to a [`HostProtocol`].
    pub const fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            0x01 => Self::Ipv4Udp,
            0x02 => Self::Ipv4Tcp,
            _ => return None,
        })
    }
}

/// Error returned when parsing a KNXnet/IP frame fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnxIpParseError {
    /// Frame is shorter than the header.
    TooShort,
    /// Header length field is not 0x06.
    InvalidHeaderLength,
    /// Protocol version is not 0x10.
    InvalidProtocolVersion,
    /// Total length does not match actual data.
    LengthMismatch,
    /// Unknown service type.
    UnknownServiceType(u16),
    /// Serialized frame length exceeds the KNXnet/IP 16-bit length field.
    FrameTooLong(usize),
}

impl fmt::Display for KnxIpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("KNXnet/IP frame too short"),
            Self::InvalidHeaderLength => f.write_str("invalid KNXnet/IP header length"),
            Self::InvalidProtocolVersion => f.write_str("invalid KNXnet/IP protocol version"),
            Self::LengthMismatch => f.write_str("KNXnet/IP frame length mismatch"),
            Self::UnknownServiceType(st) => write!(f, "unknown KNXnet/IP service type: {st:#06x}"),
            Self::FrameTooLong(len) => write!(f, "KNXnet/IP frame too long: {len} bytes"),
        }
    }
}

impl core::error::Error for KnxIpParseError {}

/// Deprecated alias for [`KnxIpParseError`].
///
/// Renamed in 0.3 to avoid a name collision with the connection-error type
/// `knx_rs_ip::KnxIpError`. Kept for backward compatibility with 0.2.
#[deprecated(since = "0.3.1", note = "renamed to `KnxIpParseError`")]
pub type KnxIpError = KnxIpParseError;

/// A parsed KNXnet/IP frame header + body.
#[derive(Clone, PartialEq, Eq)]
pub struct KnxIpFrame {
    /// The service type.
    pub service_type: ServiceType,
    /// The frame body (after the 6-byte header).
    pub body: Vec<u8>,
}

impl KnxIpFrame {
    /// Parse a KNXnet/IP frame from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpParseError`] if the frame is malformed.
    pub fn parse(data: &[u8]) -> Result<Self, KnxIpParseError> {
        if data.len() < HEADER_LEN as usize {
            return Err(KnxIpParseError::TooShort);
        }
        if data[0] != HEADER_LEN {
            return Err(KnxIpParseError::InvalidHeaderLength);
        }
        if data[1] != PROTOCOL_VERSION_10 {
            return Err(KnxIpParseError::InvalidProtocolVersion);
        }

        let service_raw = u16::from_be_bytes([data[2], data[3]]);
        let total_len = u16::from_be_bytes([data[4], data[5]]) as usize;

        if data.len() != total_len {
            return Err(KnxIpParseError::LengthMismatch);
        }

        let service_type = ServiceType::from_raw(service_raw)
            .ok_or(KnxIpParseError::UnknownServiceType(service_raw))?;

        Ok(Self {
            service_type,
            body: data[HEADER_LEN as usize..total_len].to_vec(),
        })
    }

    /// Serialize the frame to bytes (header + body).
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpParseError::FrameTooLong`] if the frame body cannot fit in
    /// the 16-bit KNXnet/IP length field.
    pub fn try_to_bytes(&self) -> Result<Vec<u8>, KnxIpParseError> {
        let total_len = HEADER_LEN as usize + self.body.len();
        let total_len_u16 =
            u16::try_from(total_len).map_err(|_| KnxIpParseError::FrameTooLong(total_len))?;
        let mut buf = Vec::with_capacity(total_len);
        buf.push(HEADER_LEN);
        buf.push(PROTOCOL_VERSION_10);
        buf.extend_from_slice(&(self.service_type as u16).to_be_bytes());
        buf.extend_from_slice(&total_len_u16.to_be_bytes());
        buf.extend_from_slice(&self.body);
        Ok(buf)
    }

    /// Serialize the frame to bytes (header + body).
    ///
    /// # Panics
    ///
    /// Panics if the frame body cannot fit in the 16-bit KNXnet/IP length
    /// field. Use [`Self::try_to_bytes`] when the body length is not statically
    /// bounded.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self.try_to_bytes() {
            Ok(bytes) => bytes,
            Err(KnxIpParseError::FrameTooLong(len)) => {
                panic!("KNXnet/IP frame length {len} exceeds u16::MAX")
            }
            Err(_) => unreachable!("serializing a well-typed frame cannot fail for this reason"),
        }
    }

    // ── Typed frame constructors ──────────────────────────────
    //
    // Single source for the common outgoing-frame body layouts, so client and
    // server do not hand-roll the same `extend_from_slice` sequences.

    /// Build a routing indication carrying a cEMI telegram.
    #[must_use]
    pub fn routing_indication(cemi: &[u8]) -> Self {
        Self {
            service_type: ServiceType::RoutingIndication,
            body: cemi.to_vec(),
        }
    }

    /// Build a tunneling request: connection header + cEMI telegram.
    #[must_use]
    pub fn tunneling_request(channel_id: u8, sequence: u8, cemi: &[u8]) -> Self {
        let header = ConnectionHeader {
            channel_id,
            sequence_counter: sequence,
            status: E_NO_ERROR,
        };
        let mut body = Vec::with_capacity(ConnectionHeader::LEN as usize + cemi.len());
        body.extend_from_slice(&header.to_bytes());
        body.extend_from_slice(cemi);
        Self {
            service_type: ServiceType::TunnelingRequest,
            body,
        }
    }

    /// Build a tunneling acknowledgement.
    #[must_use]
    pub fn tunneling_ack(channel_id: u8, sequence: u8, status: u8) -> Self {
        let header = ConnectionHeader {
            channel_id,
            sequence_counter: sequence,
            status,
        };
        Self {
            service_type: ServiceType::TunnelingAck,
            body: header.to_bytes().to_vec(),
        }
    }

    /// Build a connection-state (heartbeat) request.
    #[must_use]
    pub fn connection_state_request(channel_id: u8, control: Hpai) -> Self {
        Self {
            service_type: ServiceType::ConnectionStateRequest,
            body: connection_management_body(channel_id, control),
        }
    }

    /// Build a disconnect request.
    #[must_use]
    pub fn disconnect_request(channel_id: u8, control: Hpai) -> Self {
        Self {
            service_type: ServiceType::DisconnectRequest,
            body: connection_management_body(channel_id, control),
        }
    }

    /// Build a connect request from control/data HPAIs and a CRI block.
    #[must_use]
    pub fn connect_request(control: Hpai, data: Hpai, cri: &[u8]) -> Self {
        let mut body = Vec::with_capacity(2 * Hpai::LEN as usize + cri.len());
        body.extend_from_slice(&control.to_bytes());
        body.extend_from_slice(&data.to_bytes());
        body.extend_from_slice(cri);
        Self {
            service_type: ServiceType::ConnectRequest,
            body,
        }
    }
}

/// Connection-management request body: channel id, reserved zero, control HPAI.
///
/// Shared by connection-state and disconnect requests.
fn connection_management_body(channel_id: u8, control: Hpai) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + Hpai::LEN as usize);
    body.push(channel_id);
    body.push(0);
    body.extend_from_slice(&control.to_bytes());
    body
}

impl fmt::Debug for KnxIpFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KnxIpFrame")
            .field("service_type", &self.service_type)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// KNXnet/IP Connection Header (4 bytes).
///
/// Used in tunneling request/ack frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionHeader {
    /// Channel ID.
    pub channel_id: u8,
    /// Sequence counter.
    pub sequence_counter: u8,
    /// Status code.
    pub status: u8,
}

impl ConnectionHeader {
    /// Header length on the wire (always 4).
    pub const LEN: u8 = 4;

    /// Parse from a 4-byte slice.
    pub const fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::LEN as usize {
            return None;
        }
        Some(Self {
            channel_id: data[1],
            sequence_counter: data[2],
            status: data[3],
        })
    }

    /// Serialize to 4 bytes.
    pub const fn to_bytes(self) -> [u8; 4] {
        [
            Self::LEN,
            self.channel_id,
            self.sequence_counter,
            self.status,
        ]
    }
}

/// Host Protocol Address Information (HPAI) — 8 bytes.
///
/// Identifies an IP endpoint (address + port).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hpai {
    /// Host protocol (UDP or TCP).
    pub protocol: HostProtocol,
    /// IPv4 address as 4 bytes.
    pub ip: [u8; 4],
    /// Port number.
    pub port: u16,
}

impl Hpai {
    /// HPAI length on the wire (always 8).
    pub const LEN: u8 = 8;

    /// Parse from an 8-byte slice.
    pub const fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::LEN as usize || data[0] != Self::LEN {
            return None;
        }
        let Some(protocol) = HostProtocol::from_raw(data[1]) else {
            return None;
        };
        Some(Self {
            protocol,
            ip: [data[2], data[3], data[4], data[5]],
            port: u16::from_be_bytes([data[6], data[7]]),
        })
    }

    /// Create a UDP HPAI in NAT mode.
    ///
    /// KNXnet/IP HPAI carries IPv4 endpoints only. For IPv6 sockets, callers
    /// should use this NAT-mode representation and rely on the UDP source
    /// address as the authoritative endpoint.
    pub const fn nat_udp(port: u16) -> Self {
        Self {
            protocol: HostProtocol::Ipv4Udp,
            ip: [0, 0, 0, 0],
            port,
        }
    }

    /// Whether this HPAI uses the wildcard IPv4 address.
    pub fn is_unspecified(self) -> bool {
        self.ip == [0, 0, 0, 0]
    }

    /// Serialize to 8 bytes.
    pub const fn to_bytes(self) -> [u8; 8] {
        let port = self.port.to_be_bytes();
        [
            Self::LEN,
            self.protocol as u8,
            self.ip[0],
            self.ip[1],
            self.ip[2],
            self.ip[3],
            port[0],
            port[1],
        ]
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn parse_routing_indication() {
        // KNXnet/IP header + CEMI frame (routing indication)
        let mut frame_data = Vec::new();
        frame_data.push(0x06); // header length
        frame_data.push(0x10); // protocol version
        frame_data.extend_from_slice(&0x0530u16.to_be_bytes()); // RoutingIndication
        let cemi = [
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81,
        ];
        let total_len = u16::from(HEADER_LEN) + cemi.len() as u16;
        frame_data.extend_from_slice(&total_len.to_be_bytes());
        frame_data.extend_from_slice(&cemi);

        let frame = KnxIpFrame::parse(&frame_data).unwrap();
        assert_eq!(frame.service_type, ServiceType::RoutingIndication);
        assert_eq!(frame.body, cemi);
    }

    #[test]
    fn roundtrip_tunneling_request() {
        let cemi = [
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81,
        ];
        let ch = ConnectionHeader {
            channel_id: 1,
            sequence_counter: 5,
            status: 0,
        };

        let mut body = Vec::new();
        body.extend_from_slice(&ch.to_bytes());
        body.extend_from_slice(&cemi);

        let frame = KnxIpFrame {
            service_type: ServiceType::TunnelingRequest,
            body,
        };

        let bytes = frame.to_bytes();
        let reparsed = KnxIpFrame::parse(&bytes).unwrap();
        assert_eq!(reparsed.service_type, ServiceType::TunnelingRequest);

        let ch2 = ConnectionHeader::parse(&reparsed.body).unwrap();
        assert_eq!(ch2.channel_id, 1);
        assert_eq!(ch2.sequence_counter, 5);
        assert_eq!(ch2.status, 0);
    }

    #[test]
    fn roundtrip_tunneling_ack() {
        let ch = ConnectionHeader {
            channel_id: 1,
            sequence_counter: 5,
            status: 0,
        };
        let frame = KnxIpFrame {
            service_type: ServiceType::TunnelingAck,
            body: ch.to_bytes().to_vec(),
        };
        let bytes = frame.to_bytes();
        assert_eq!(
            bytes.len(),
            HEADER_LEN as usize + ConnectionHeader::LEN as usize
        );

        let reparsed = KnxIpFrame::parse(&bytes).unwrap();
        assert_eq!(reparsed.service_type, ServiceType::TunnelingAck);
    }

    #[test]
    fn parse_hpai() {
        let data = [0x08, 0x01, 192, 168, 1, 50, 0x0E, 0x57]; // UDP, 192.168.1.50:3671
        let hpai = Hpai::parse(&data).unwrap();
        assert_eq!(hpai.protocol, HostProtocol::Ipv4Udp);
        assert_eq!(hpai.ip, [192, 168, 1, 50]);
        assert_eq!(hpai.port, 3671);
        assert!(!hpai.is_unspecified());
        assert_eq!(hpai.to_bytes(), data);
    }

    #[test]
    fn hpai_nat_udp_roundtrip() {
        let hpai = Hpai::nat_udp(3671);
        assert_eq!(hpai.protocol, HostProtocol::Ipv4Udp);
        assert!(hpai.is_unspecified());
        assert_eq!(Hpai::parse(&hpai.to_bytes()), Some(hpai));
    }

    #[test]
    fn parse_too_short() {
        assert!(KnxIpFrame::parse(&[0x06, 0x10]).is_err());
    }

    #[test]
    fn parse_bad_header_length() {
        let data = [0x05, 0x10, 0x05, 0x30, 0x00, 0x06];
        assert!(matches!(
            KnxIpFrame::parse(&data),
            Err(KnxIpParseError::InvalidHeaderLength)
        ));
    }

    #[test]
    fn parse_bad_version() {
        let data = [0x06, 0x11, 0x05, 0x30, 0x00, 0x06];
        assert!(matches!(
            KnxIpFrame::parse(&data),
            Err(KnxIpParseError::InvalidProtocolVersion)
        ));
    }

    #[test]
    fn parse_unknown_service() {
        let data = [0x06, 0x10, 0xFF, 0xFF, 0x00, 0x06];
        assert!(matches!(
            KnxIpFrame::parse(&data),
            Err(KnxIpParseError::UnknownServiceType(0xFFFF))
        ));
    }

    #[test]
    fn parse_rejects_trailing_bytes() {
        let data = [0x06, 0x10, 0x05, 0x30, 0x00, 0x06, 0x00];
        assert!(matches!(
            KnxIpFrame::parse(&data),
            Err(KnxIpParseError::LengthMismatch)
        ));
    }

    #[test]
    fn try_to_bytes_rejects_oversized_frame() {
        let frame = KnxIpFrame {
            service_type: ServiceType::RoutingIndication,
            body: vec![0; usize::from(u16::MAX)],
        };
        assert!(matches!(
            frame.try_to_bytes(),
            Err(KnxIpParseError::FrameTooLong(_))
        ));
    }

    #[test]
    fn service_type_roundtrip_all_variants() {
        // Drift guard: every discriminant must round-trip through from_raw so a
        // new variant or a typo in the reverse map is caught at test time.
        const ALL: &[ServiceType] = &[
            ServiceType::SearchRequest,
            ServiceType::SearchResponse,
            ServiceType::DescriptionRequest,
            ServiceType::DescriptionResponse,
            ServiceType::ConnectRequest,
            ServiceType::ConnectResponse,
            ServiceType::ConnectionStateRequest,
            ServiceType::ConnectionStateResponse,
            ServiceType::DisconnectRequest,
            ServiceType::DisconnectResponse,
            ServiceType::SearchRequestExtended,
            ServiceType::SearchResponseExtended,
            ServiceType::DeviceConfigurationRequest,
            ServiceType::DeviceConfigurationAck,
            ServiceType::TunnelingRequest,
            ServiceType::TunnelingAck,
            ServiceType::RoutingIndication,
            ServiceType::RoutingLostMessage,
            ServiceType::RoutingBusy,
        ];
        for &st in ALL {
            assert_eq!(ServiceType::from_raw(st as u16), Some(st), "{st:?}");
        }
    }

    #[test]
    fn host_protocol_roundtrip_all_variants() {
        for &hp in &[HostProtocol::Ipv4Udp, HostProtocol::Ipv4Tcp] {
            assert_eq!(HostProtocol::from_raw(hp as u8), Some(hp), "{hp:?}");
        }
    }

    #[test]
    fn tunnel_cri_crd_builders() {
        assert_eq!(tunnel_cri(), [0x04, 0x04, 0x02, 0x00]);
        assert_eq!(tunnel_crd(0xFF00), [0x04, 0x04, 0xFF, 0x00]);
    }

    #[test]
    fn typed_constructors_roundtrip() {
        let cemi = [0x29, 0x00, 0xBC];
        let req = KnxIpFrame::tunneling_request(7, 3, &cemi);
        let bytes = req.to_bytes();
        let parsed = KnxIpFrame::parse(&bytes).unwrap();
        assert_eq!(parsed.service_type, ServiceType::TunnelingRequest);
        let ch = ConnectionHeader::parse(&parsed.body).unwrap();
        assert_eq!((ch.channel_id, ch.sequence_counter), (7, 3));
        assert_eq!(&parsed.body[ConnectionHeader::LEN as usize..], &cemi);

        let hpai = Hpai::nat_udp(KNX_PORT);
        let dc = KnxIpFrame::disconnect_request(7, hpai);
        assert_eq!(dc.service_type, ServiceType::DisconnectRequest);
        assert_eq!(dc.body[0], 7);
    }

    #[test]
    fn connection_header_roundtrip() {
        let ch = ConnectionHeader {
            channel_id: 42,
            sequence_counter: 7,
            status: 0,
        };
        let bytes = ch.to_bytes();
        assert_eq!(bytes, [4, 42, 7, 0]);
        let parsed = ConnectionHeader::parse(&bytes).unwrap();
        assert_eq!(parsed, ch);
    }
}
