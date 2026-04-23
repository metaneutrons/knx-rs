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

/// Encode an `AuthorizeResponse` APDU payload.
pub fn encode_authorize_response(level: u8) -> Vec<u8> {
    alloc::vec![0x03, 0xD2, level]
}

/// Encode a `KeyResponse` APDU payload.
pub fn encode_key_response(level: u8) -> Vec<u8> {
    alloc::vec![0x03, 0xD4, level]
}

/// Encode a `RestartResponse` APDU payload (master reset response).
pub fn encode_restart_response(error_code: u8, process_time: u16) -> Vec<u8> {
    let t = process_time.to_be_bytes();
    alloc::vec![0x03, 0xA1, error_code, t[0], t[1]]
}

/// Encode a `PropertyDescriptionResponse` APDU payload.
pub fn encode_property_description_response(
    object_index: u8,
    property_id: u8,
    property_index: u8,
    write_enable: bool,
    pdt: u8,
    max_elements: u16,
    access: u8,
) -> Vec<u8> {
    let type_byte = if write_enable {
        0x80 | (pdt & 0x3F)
    } else {
        pdt & 0x3F
    };
    let max_hi = ((max_elements >> 8) & 0x0F) as u8;
    let max_lo = (max_elements & 0xFF) as u8;
    alloc::vec![
        0x03,
        0xD9,
        object_index,
        property_id,
        property_index,
        type_byte,
        max_hi,
        max_lo,
        access
    ]
}

/// Encode a `MemoryExtReadResponse` APDU payload.
pub fn encode_memory_ext_read_response(return_code: u8, address: u32, data: &[u8]) -> Vec<u8> {
    let a = address.to_be_bytes();
    let mut payload = Vec::with_capacity(6 + data.len());
    payload.push(0x01);
    payload.push(0xFE);
    payload.push(return_code);
    payload.extend_from_slice(&a[1..4]); // 24-bit address
    payload.extend_from_slice(data);
    payload
}

/// Encode a `MemoryExtWriteResponse` APDU payload.
pub fn encode_memory_ext_write_response(return_code: u8, address: u32) -> Vec<u8> {
    let a = address.to_be_bytes();
    alloc::vec![0x01, 0xFC, return_code, a[1], a[2], a[3]]
}

/// Encode an `IndividualAddressSerialNumberReadResponse` APDU payload.
pub fn encode_individual_address_serial_number_response(
    serial: [u8; 6],
    domain_address: u16,
) -> Vec<u8> {
    let d = domain_address.to_be_bytes();
    let mut payload = Vec::with_capacity(10);
    payload.push(0x03);
    payload.push(0xDD);
    payload.extend_from_slice(&serial);
    payload.extend_from_slice(&d);
    payload
}

/// Encode a `SystemNetworkParameterResponse` APDU payload.
pub fn encode_system_network_parameter_response(
    object_type: u16,
    property_id: u16,
    test_info: &[u8],
    test_result: &[u8],
) -> Vec<u8> {
    let ot = object_type.to_be_bytes();
    let pid_shifted = property_id << 4;
    let pid_bytes = pid_shifted.to_be_bytes();
    let mut payload = Vec::with_capacity(6 + test_info.len() + test_result.len());
    payload.push(0x01);
    payload.push(0xC9);
    payload.extend_from_slice(&ot);
    payload.extend_from_slice(&pid_bytes);
    payload.extend_from_slice(test_info);
    payload.extend_from_slice(test_result);
    payload
}

/// Encode an `AdcResponse` APDU payload.
pub fn encode_adc_response(channel: u8, count: u8, value: u16) -> Vec<u8> {
    let v = value.to_be_bytes();
    alloc::vec![0x01, 0xC0 | (channel & 0x3F), count, v[0], v[1]]
}

/// Encode a `FunctionPropertyStateResponse` APDU payload.
pub fn encode_function_property_state_response(
    object_index: u8,
    property_id: u8,
    result_data: &[u8],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + result_data.len());
    payload.push(0x02);
    payload.push(0xC9);
    payload.push(object_index);
    payload.push(property_id);
    payload.extend_from_slice(result_data);
    payload
}

/// Encode a `PropertyValueExtResponse` APDU payload.
pub fn encode_property_value_ext_response(
    object_type: u16,
    object_instance: u16,
    property_id: u16,
    count: u8,
    start_index: u16,
    data: &[u8],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(9 + data.len());
    payload.push(0x01);
    payload.push(0xCD);
    encode_ext_property_header(
        &mut payload,
        object_type,
        object_instance,
        property_id,
        count,
        start_index,
    );
    payload.extend_from_slice(data);
    payload
}

/// Helper to encode the common extended property header.
fn encode_ext_property_header(
    buf: &mut Vec<u8>,
    object_type: u16,
    object_instance: u16,
    property_id: u16,
    count: u8,
    start_index: u16,
) {
    let ot = object_type.to_be_bytes();
    buf.extend_from_slice(&ot);
    #[expect(clippy::cast_possible_truncation)]
    {
        buf.push(((object_instance >> 4) & 0xFF) as u8);
        buf.push((((object_instance & 0x0F) << 4) | ((property_id >> 8) & 0x0F)) as u8);
        buf.push((property_id & 0xFF) as u8);
    }
    buf.push(count);
    buf.extend_from_slice(&start_index.to_be_bytes());
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
    /// Restart master reset (from ETS).
    RestartMasterReset {
        /// Erase code.
        erase_code: u8,
        /// Channel number.
        channel: u8,
    },
    /// Property description read (from ETS).
    PropertyDescriptionRead {
        /// Object index.
        object_index: u8,
        /// Property ID (0 = by index).
        property_id: u8,
        /// Property index.
        property_index: u8,
    },
    /// Memory extended read (32-bit address).
    MemoryExtRead {
        /// Number of bytes.
        count: u8,
        /// 24-bit memory address.
        address: u32,
    },
    /// Memory extended write (32-bit address).
    MemoryExtWrite {
        /// Number of bytes.
        count: u8,
        /// 24-bit memory address.
        address: u32,
        /// Data.
        data: Vec<u8>,
    },
    /// Individual address serial number read (broadcast).
    IndividualAddressSerialNumberRead {
        /// Serial number (6 bytes).
        serial: [u8; 6],
    },
    /// Individual address serial number write (broadcast).
    IndividualAddressSerialNumberWrite {
        /// Serial number (6 bytes).
        serial: [u8; 6],
        /// New individual address.
        address: u16,
    },
    /// Key write (from ETS).
    KeyWrite {
        /// Access level.
        level: u8,
        /// Key value.
        key: u32,
    },
    /// Function property command.
    FunctionPropertyCommand {
        /// Object index.
        object_index: u8,
        /// Property ID.
        property_id: u8,
        /// Function input data.
        data: Vec<u8>,
    },
    /// Function property state read.
    FunctionPropertyState {
        /// Object index.
        object_index: u8,
        /// Property ID.
        property_id: u8,
        /// Function input data.
        data: Vec<u8>,
    },
    /// System network parameter read (broadcast).
    SystemNetworkParameterRead {
        /// Object type.
        object_type: u16,
        /// Property ID.
        property_id: u16,
        /// Test info data.
        test_info: Vec<u8>,
    },
    /// ADC read.
    AdcRead {
        /// Channel number.
        channel: u8,
        /// Read count.
        count: u8,
    },
    /// Property value extended read.
    PropertyValueExtRead {
        /// Object type.
        object_type: u16,
        /// Object instance.
        object_instance: u16,
        /// Property ID.
        property_id: u16,
        /// Number of elements.
        count: u8,
        /// Start index.
        start_index: u16,
    },
    /// Property value extended write (confirmed).
    PropertyValueExtWriteCon {
        /// Object type.
        object_type: u16,
        /// Object instance.
        object_instance: u16,
        /// Property ID.
        property_id: u16,
        /// Number of elements.
        count: u8,
        /// Start index.
        start_index: u16,
        /// Data.
        data: Vec<u8>,
    },
    /// Property value extended write (unconfirmed).
    PropertyValueExtWriteUnCon {
        /// Object type.
        object_type: u16,
        /// Object instance.
        object_instance: u16,
        /// Property ID.
        property_id: u16,
        /// Number of elements.
        count: u8,
        /// Start index.
        start_index: u16,
        /// Data.
        data: Vec<u8>,
    },
    /// Property extended description read.
    PropertyExtDescriptionRead {
        /// Object type.
        object_type: u16,
        /// Object instance.
        object_instance: u16,
        /// Property ID.
        property_id: u16,
        /// Description type.
        description_type: u8,
        /// Property index.
        property_index: u16,
    },
}

/// Parse an APDU type + data into an `AppIndication`.
///
/// Returns `None` for unsupported or malformed APDUs.
#[expect(clippy::too_many_lines)]
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
        ApduType::RestartMasterReset if data.len() >= 3 => {
            Some(AppIndication::RestartMasterReset {
                erase_code: data[1],
                channel: data[2],
            })
        }
        ApduType::PropertyDescriptionRead if data.len() >= 3 => {
            Some(AppIndication::PropertyDescriptionRead {
                object_index: data[0],
                property_id: data[1],
                property_index: data[2],
            })
        }
        ApduType::MemoryExtRead if data.len() >= 4 => Some(AppIndication::MemoryExtRead {
            count: data[0],
            address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
        }),
        ApduType::MemoryExtWrite if data.len() >= 4 => Some(AppIndication::MemoryExtWrite {
            count: data[0],
            address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
            data: data[4..].to_vec(),
        }),
        ApduType::IndividualAddressSerialNumberRead if data.len() >= 6 => {
            let mut serial = [0u8; 6];
            serial.copy_from_slice(&data[0..6]);
            Some(AppIndication::IndividualAddressSerialNumberRead { serial })
        }
        ApduType::IndividualAddressSerialNumberWrite if data.len() >= 8 => {
            let mut serial = [0u8; 6];
            serial.copy_from_slice(&data[0..6]);
            Some(AppIndication::IndividualAddressSerialNumberWrite {
                serial,
                address: u16::from_be_bytes([data[6], data[7]]),
            })
        }
        ApduType::KeyWrite if data.len() >= 5 => Some(AppIndication::KeyWrite {
            level: data[0],
            key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
        }),
        ApduType::FunctionPropertyCommand if data.len() >= 2 => {
            Some(AppIndication::FunctionPropertyCommand {
                object_index: data[0],
                property_id: data[1],
                data: data[2..].to_vec(),
            })
        }
        ApduType::FunctionPropertyState if data.len() >= 2 => {
            Some(AppIndication::FunctionPropertyState {
                object_index: data[0],
                property_id: data[1],
                data: data[2..].to_vec(),
            })
        }
        ApduType::SystemNetworkParameterRead if data.len() >= 4 => {
            let object_type = u16::from_be_bytes([data[0], data[1]]);
            let pid_raw = u16::from_be_bytes([data[2], data[3]]);
            Some(AppIndication::SystemNetworkParameterRead {
                object_type,
                property_id: pid_raw >> 4,
                test_info: data[3..].to_vec(),
            })
        }
        ApduType::AdcRead => Some(AppIndication::AdcRead {
            channel: data.first().copied().unwrap_or(0) & 0x3F,
            count: data.get(1).copied().unwrap_or(1),
        }),
        ApduType::PropertyValueExtRead if data.len() >= 7 => {
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Some(AppIndication::PropertyValueExtRead {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
            })
        }
        ApduType::PropertyValueExtWriteCon if data.len() >= 7 => {
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Some(AppIndication::PropertyValueExtWriteCon {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
                data: data[7..].to_vec(),
            })
        }
        ApduType::PropertyValueExtWriteUnCon if data.len() >= 7 => {
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Some(AppIndication::PropertyValueExtWriteUnCon {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
                data: data[7..].to_vec(),
            })
        }
        ApduType::PropertyExtDescriptionRead if data.len() >= 7 => {
            let object_type = u16::from_be_bytes([data[0], data[1]]);
            let object_instance = (u16::from(data[2]) << 4) | (u16::from(data[3]) >> 4);
            let property_id = (u16::from(data[3] & 0x0F) << 8) | u16::from(data[4]);
            let description_type = data[5] >> 4;
            let property_index = (u16::from(data[5] & 0x0F) << 8) | u16::from(data[6]);
            Some(AppIndication::PropertyExtDescriptionRead {
                object_type,
                object_instance,
                property_id,
                description_type,
                property_index,
            })
        }
        _ => None,
    }
}

/// Parse the common header for extended property services.
/// Returns `(object_type, object_instance, property_id, count, start_index)`.
fn parse_ext_property_header(data: &[u8]) -> (u16, u16, u16, u8, u16) {
    let object_type = u16::from_be_bytes([data[0], data[1]]);
    let object_instance = (u16::from(data[2]) << 4) | (u16::from(data[3]) >> 4);
    let property_id = (u16::from(data[3] & 0x0F) << 8) | u16::from(data[4]);
    let count = data[5];
    let start_index = u16::from_be_bytes([data[6], data.get(7).copied().unwrap_or(0)]);
    (
        object_type,
        object_instance,
        property_id,
        count,
        start_index,
    )
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
