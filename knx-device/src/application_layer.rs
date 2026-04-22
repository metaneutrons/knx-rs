// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Application Layer — APDU service dispatch.
//!
//! Processes incoming APDUs and generates outgoing ones. This is the
//! bridge between the transport layer and the device's interface objects
//! and group objects.

use alloc::vec::Vec;

use knx_core::message::ApduType;

// ── APDU encoding (outgoing) ─────────────────────────────────

/// Encode a `GroupValueWrite` APDU payload.
pub fn encode_group_value_write(data: &[u8]) -> Vec<u8> {
    encode_group_value(0x00, 0x80, data)
}

/// Encode a `GroupValueResponse` APDU payload.
pub fn encode_group_value_response(data: &[u8]) -> Vec<u8> {
    encode_group_value(0x00, 0x40, data)
}

/// Encode a `GroupValueRead` APDU payload.
pub fn encode_group_value_read() -> Vec<u8> {
    alloc::vec![0x00, 0x00]
}

/// Encode an `IndividualAddressResponse` APDU payload.
pub fn encode_individual_address_response() -> Vec<u8> {
    alloc::vec![0x01, 0x40]
}

/// Encode a `DeviceDescriptorResponse` APDU payload for descriptor type 0.
pub fn encode_device_descriptor_response(mask_version: u16) -> Vec<u8> {
    let m = mask_version.to_be_bytes();
    alloc::vec![0x03, 0x40, m[0], m[1]]
}

/// Encode a `DeviceDescriptorResponse` for unsupported descriptor types (type 0x3F).
pub fn encode_device_descriptor_unsupported() -> Vec<u8> {
    alloc::vec![0x03, 0x7F]
}

/// Encode a `PropertyValueResponse` APDU payload.
pub fn encode_property_response(
    object_index: u8,
    property_id: u8,
    count: u8,
    start_index: u16,
    data: &[u8],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(6 + data.len());
    payload.push(0x03);
    payload.push(0xD6); // PropertyValueResponse
    payload.push(object_index);
    payload.push(property_id);
    let count_start = (u16::from(count) << 12) | (start_index & 0x0FFF);
    payload.extend_from_slice(&count_start.to_be_bytes());
    payload.extend_from_slice(data);
    payload
}

/// Encode a `MemoryResponse` APDU payload.
#[expect(
    clippy::cast_possible_truncation,
    reason = "data.len() is always <= 15"
)]
pub fn encode_memory_response(address: u16, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5 + data.len());
    payload.push(0x02);
    payload.push(0x40 | (data.len() as u8 & 0x0F));
    payload.extend_from_slice(&address.to_be_bytes());
    payload.extend_from_slice(data);
    payload
}

/// Shared helper for group value write/response encoding.
/// Applies the short-value optimization (≤6 bits packed into APCI byte).
fn encode_group_value(tpci: u8, apci: u8, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.push(tpci);
    if data.len() == 1 && data[0] <= 0x3F {
        payload.push(apci | (data[0] & 0x3F));
    } else {
        payload.push(apci);
        payload.extend_from_slice(data);
    }
    payload
}

// ── APDU parsing (incoming) ──────────────────────────────────

/// An incoming application-layer indication to be processed by the BAU.
#[derive(Debug, Clone)]
pub enum AppIndication {
    /// Group value write received from the bus.
    GroupValueWrite {
        /// ASAP (group object number).
        asap: u16,
        /// The value data.
        data: Vec<u8>,
    },
    /// Group value response received from the bus.
    /// Differs from write: checks `update_enable` (A-flag) instead of `write_enable` (S-flag).
    GroupValueResponse {
        /// ASAP (group object number).
        asap: u16,
        /// The value data.
        data: Vec<u8>,
    },
    /// Group value read request received.
    GroupValueRead {
        /// ASAP.
        asap: u16,
    },
    /// Property value read request (from ETS).
    PropertyValueRead {
        /// Object index.
        object_index: u8,
        /// Property ID.
        property_id: u8,
        /// Number of elements.
        count: u8,
        /// Start index.
        start_index: u16,
    },
    /// Property value write (from ETS).
    PropertyValueWrite {
        /// Object index.
        object_index: u8,
        /// Property ID.
        property_id: u8,
        /// Number of elements.
        count: u8,
        /// Start index.
        start_index: u16,
        /// Data.
        data: Vec<u8>,
    },
    /// Device descriptor read.
    DeviceDescriptorRead {
        /// Descriptor type (0 = mask version).
        descriptor_type: u8,
    },
    /// Memory read.
    MemoryRead {
        /// Number of bytes.
        count: u8,
        /// Memory address.
        address: u16,
    },
    /// Memory write.
    MemoryWrite {
        /// Number of bytes.
        count: u8,
        /// Memory address.
        address: u16,
        /// Data.
        data: Vec<u8>,
    },
    /// Restart request.
    Restart,
    /// Individual address write (programming mode).
    IndividualAddressWrite {
        /// New address.
        address: u16,
    },
    /// Individual address read (programming mode).
    IndividualAddressRead,
    /// Authorize request.
    AuthorizeRequest {
        /// Key.
        key: u32,
    },
}

/// Parse an APDU type + data into an `AppIndication`.
///
/// Returns `None` for unsupported or malformed APDUs.
pub fn parse_indication(apdu_type: ApduType, data: &[u8]) -> Option<AppIndication> {
    match apdu_type {
        ApduType::GroupValueWrite => Some(AppIndication::GroupValueWrite {
            asap: 0,
            data: data.to_vec(),
        }),
        ApduType::GroupValueResponse => Some(AppIndication::GroupValueResponse {
            asap: 0,
            data: data.to_vec(),
        }),
        ApduType::GroupValueRead => Some(AppIndication::GroupValueRead { asap: 0 }),
        ApduType::PropertyValueRead if data.len() >= 3 => Some(AppIndication::PropertyValueRead {
            object_index: data[0],
            property_id: data[1],
            count: (data[2] >> 4) & 0x0F,
            start_index: u16::from(data[2] & 0x0F) << 8 | u16::from(*data.get(3).unwrap_or(&0)),
        }),
        ApduType::PropertyValueWrite if data.len() >= 4 => {
            Some(AppIndication::PropertyValueWrite {
                object_index: data[0],
                property_id: data[1],
                count: (data[2] >> 4) & 0x0F,
                start_index: u16::from(data[2] & 0x0F) << 8 | u16::from(data[3]),
                data: data[4..].to_vec(),
            })
        }
        ApduType::DeviceDescriptorRead => Some(AppIndication::DeviceDescriptorRead {
            descriptor_type: data.first().copied().unwrap_or(0) & 0x3F,
        }),
        ApduType::MemoryRead if data.len() >= 3 => Some(AppIndication::MemoryRead {
            count: data[0] & 0x0F,
            address: u16::from_be_bytes([data[1], data[2]]),
        }),
        ApduType::MemoryWrite if data.len() >= 3 => Some(AppIndication::MemoryWrite {
            count: data[0] & 0x0F,
            address: u16::from_be_bytes([data[1], data[2]]),
            data: data[3..].to_vec(),
        }),
        ApduType::Restart => Some(AppIndication::Restart),
        ApduType::IndividualAddressWrite if data.len() >= 2 => {
            Some(AppIndication::IndividualAddressWrite {
                address: u16::from_be_bytes([data[0], data[1]]),
            })
        }
        ApduType::IndividualAddressRead => Some(AppIndication::IndividualAddressRead),
        ApduType::AuthorizeRequest if data.len() >= 5 => Some(AppIndication::AuthorizeRequest {
            key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
        }),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_group_value_write() {
        let ind = parse_indication(ApduType::GroupValueWrite, &[0x01]).unwrap();
        assert!(matches!(ind, AppIndication::GroupValueWrite { data, .. } if data == [0x01]));
    }

    #[test]
    fn parse_property_read() {
        // object_index=0, property_id=1, count=1, start_index=1
        let ind = parse_indication(ApduType::PropertyValueRead, &[0x00, 0x01, 0x10, 0x01]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueRead {
                object_index: 0,
                property_id: 1,
                count: 1,
                start_index: 1,
            }
        ));
    }

    #[test]
    fn parse_device_descriptor_read() {
        let ind = parse_indication(ApduType::DeviceDescriptorRead, &[0x00]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::DeviceDescriptorRead { descriptor_type: 0 }
        ));
    }

    #[test]
    fn parse_restart() {
        let ind = parse_indication(ApduType::Restart, &[]).unwrap();
        assert!(matches!(ind, AppIndication::Restart));
    }

    #[test]
    fn parse_unsupported() {
        assert!(parse_indication(ApduType::SecureService, &[]).is_none());
    }

    #[test]
    fn parse_memory_read() {
        let ind = parse_indication(ApduType::MemoryRead, &[0x03, 0x00, 0x10]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::MemoryRead {
                count: 3,
                address: 0x0010,
            }
        ));
    }

    #[test]
    fn parse_memory_write() {
        let ind = parse_indication(ApduType::MemoryWrite, &[0x02, 0x00, 0x20, 0xAA, 0xBB]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::MemoryWrite {
                count: 2,
                address: 0x0020,
                ..
            }
        ));
        if let AppIndication::MemoryWrite { data, .. } = ind {
            assert_eq!(data, &[0xAA, 0xBB]);
        }
    }

    #[test]
    fn parse_individual_address_write() {
        let ind = parse_indication(ApduType::IndividualAddressWrite, &[0x11, 0x05]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::IndividualAddressWrite { address: 0x1105 }
        ));
    }

    #[test]
    fn parse_individual_address_read() {
        let ind = parse_indication(ApduType::IndividualAddressRead, &[]).unwrap();
        assert!(matches!(ind, AppIndication::IndividualAddressRead));
    }

    #[test]
    fn parse_authorize_request() {
        let ind =
            parse_indication(ApduType::AuthorizeRequest, &[0x00, 0x00, 0x00, 0x00, 0xFF]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::AuthorizeRequest { key: 0x0000_00FF }
        ));
    }

    #[test]
    fn parse_property_write() {
        let ind = parse_indication(
            ApduType::PropertyValueWrite,
            &[0x00, 0x36, 0x10, 0x01, 0x01],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueWrite {
                object_index: 0,
                property_id: 0x36,
                count: 1,
                start_index: 1,
                ..
            }
        ));
    }
}
