// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Application Layer — APDU service dispatch.
//!
//! Processes incoming APDUs and generates outgoing ones. This is the
//! bridge between the transport layer and the device's interface objects
//! and group objects.

use alloc::vec::Vec;

use knx_core::message::ApduType;

// ── APCI byte helpers (SSOT: derived from ApduType enum) ─────

/// Split an `ApduType` into its two APCI wire bytes `[high, low]`.
///
/// The 10-bit APCI value is encoded as:
/// - `high`: bits 9..8 (masked into the lower 2 bits)
/// - `low`: bits 7..0
#[expect(
    clippy::cast_possible_truncation,
    reason = "APCI is 10-bit, both halves fit in u8"
)]
const fn apci_bytes(t: ApduType) -> [u8; 2] {
    let v = t as u16;
    [(v >> 8) as u8, v as u8]
}

/// 6-bit mask for short APDU values and descriptor types.
const MASK_6BIT: u8 = 0x3F;
/// 4-bit mask for count fields and nibble extraction.
const MASK_4BIT: u8 = 0x0F;
/// 12-bit mask for `start_index` fields.
const MASK_12BIT: u16 = 0x0FFF;
/// Write-enable flag in property description type byte.
const WRITE_ENABLE_FLAG: u8 = 0x80;
/// Unsupported device descriptor type (0x3F = all bits set in 6-bit field).
const DESCRIPTOR_TYPE_UNSUPPORTED: u8 = 0x3F;

// ── APDU encoding (outgoing) ─────────────────────────────────

/// Encode a `GroupValueWrite` APDU payload.
pub fn encode_group_value_write(data: &[u8]) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::GroupValueWrite);
    encode_group_value(hi, lo, data)
}

/// Encode a `GroupValueResponse` APDU payload.
pub fn encode_group_value_response(data: &[u8]) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::GroupValueResponse);
    encode_group_value(hi, lo, data)
}

/// Encode a `GroupValueRead` APDU payload.
pub fn encode_group_value_read() -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::GroupValueRead);
    alloc::vec![hi, lo]
}

/// Encode an `IndividualAddressResponse` APDU payload.
pub fn encode_individual_address_response() -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::IndividualAddressResponse);
    alloc::vec![hi, lo]
}

/// Encode a `DeviceDescriptorResponse` APDU payload for descriptor type 0.
pub fn encode_device_descriptor_response(mask_version: u16) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::DeviceDescriptorResponse);
    let m = mask_version.to_be_bytes();
    alloc::vec![hi, lo, m[0], m[1]]
}

/// Encode a `DeviceDescriptorResponse` for unsupported descriptor types (type 0x3F).
pub fn encode_device_descriptor_unsupported() -> Vec<u8> {
    let [hi, _] = apci_bytes(ApduType::DeviceDescriptorResponse);
    alloc::vec![hi, DESCRIPTOR_TYPE_UNSUPPORTED]
}

/// Encode a `PropertyValueResponse` APDU payload.
pub fn encode_property_response(
    object_index: u8,
    property_id: u8,
    count: u8,
    start_index: u16,
    data: &[u8],
) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::PropertyValueResponse);
    let mut payload = Vec::with_capacity(6 + data.len());
    payload.push(hi);
    payload.push(lo);
    payload.push(object_index);
    payload.push(property_id);
    let count_start = (u16::from(count) << 12) | (start_index & MASK_12BIT);
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
    let [hi, lo] = apci_bytes(ApduType::MemoryResponse);
    let mut payload = Vec::with_capacity(5 + data.len());
    payload.push(hi);
    payload.push(lo | (data.len() as u8 & MASK_4BIT));
    payload.extend_from_slice(&address.to_be_bytes());
    payload.extend_from_slice(data);
    payload
}

/// Encode an `AuthorizeResponse` APDU payload.
pub fn encode_authorize_response(level: u8) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::AuthorizeResponse);
    alloc::vec![hi, lo, level]
}

/// Encode a `KeyResponse` APDU payload.
pub fn encode_key_response(level: u8) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::KeyResponse);
    alloc::vec![hi, lo, level]
}

/// Encode a `RestartResponse` APDU payload (master reset response).
pub fn encode_restart_response(error_code: u8, process_time: u16) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::RestartMasterReset);
    let t = process_time.to_be_bytes();
    alloc::vec![hi, lo, error_code, t[0], t[1]]
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
    let [hi, lo] = apci_bytes(ApduType::PropertyDescriptionResponse);
    let type_byte = if write_enable {
        WRITE_ENABLE_FLAG | (pdt & MASK_6BIT)
    } else {
        pdt & MASK_6BIT
    };
    let max_hi = ((max_elements >> 8) & u16::from(MASK_4BIT)) as u8;
    let max_lo = (max_elements & 0xFF) as u8;
    alloc::vec![
        hi,
        lo,
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
    let [hi, lo] = apci_bytes(ApduType::MemoryExtReadResponse);
    let a = address.to_be_bytes();
    let mut payload = Vec::with_capacity(6 + data.len());
    payload.push(hi);
    payload.push(lo);
    payload.push(return_code);
    payload.extend_from_slice(&a[1..4]); // 24-bit address
    payload.extend_from_slice(data);
    payload
}

/// Encode a `MemoryExtWriteResponse` APDU payload.
pub fn encode_memory_ext_write_response(return_code: u8, address: u32) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::MemoryExtWriteResponse);
    let a = address.to_be_bytes();
    alloc::vec![hi, lo, return_code, a[1], a[2], a[3]]
}

/// Encode an `IndividualAddressSerialNumberReadResponse` APDU payload.
pub fn encode_individual_address_serial_number_response(
    serial: [u8; 6],
    domain_address: u16,
) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::IndividualAddressSerialNumberResponse);
    let d = domain_address.to_be_bytes();
    let mut payload = Vec::with_capacity(10);
    payload.push(hi);
    payload.push(lo);
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
    let [hi, lo] = apci_bytes(ApduType::SystemNetworkParameterResponse);
    let ot = object_type.to_be_bytes();
    let pid_shifted = property_id << 4;
    let pid_bytes = pid_shifted.to_be_bytes();
    let mut payload = Vec::with_capacity(6 + test_info.len() + test_result.len());
    payload.push(hi);
    payload.push(lo);
    payload.extend_from_slice(&ot);
    payload.extend_from_slice(&pid_bytes);
    payload.extend_from_slice(test_info);
    payload.extend_from_slice(test_result);
    payload
}

/// Encode an `AdcResponse` APDU payload.
pub fn encode_adc_response(channel: u8, count: u8, value: u16) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::AdcResponse);
    let v = value.to_be_bytes();
    alloc::vec![hi, lo | (channel & MASK_6BIT), count, v[0], v[1]]
}

/// Encode a `FunctionPropertyStateResponse` APDU payload.
pub fn encode_function_property_state_response(
    object_index: u8,
    property_id: u8,
    result_data: &[u8],
) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::FunctionPropertyStateResponse);
    let mut payload = Vec::with_capacity(4 + result_data.len());
    payload.push(hi);
    payload.push(lo);
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
    let [hi, lo] = apci_bytes(ApduType::PropertyValueExtResponse);
    let mut payload = Vec::with_capacity(9 + data.len());
    payload.push(hi);
    payload.push(lo);
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
        buf.push(
            ((object_instance & u16::from(MASK_4BIT)) << 4
                | (property_id >> 8) & u16::from(MASK_4BIT)) as u8,
        );
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
    if data.len() == 1 && data[0] <= MASK_6BIT {
        payload.push(apci | (data[0] & MASK_6BIT));
    } else {
        payload.push(apci);
        payload.extend_from_slice(data);
    }
    payload
}

/// Encode an APDU into raw bytes (for transport layer connected-mode).
pub fn encode_raw_apdu(apdu: &knx_core::apdu::Apdu) -> Vec<u8> {
    let apci = apdu.apdu_type as u16;
    let mut buf = Vec::with_capacity(2 + apdu.data.len());
    buf.push((apci >> 8) as u8);
    buf.push((apci & 0xFF) as u8);
    buf.extend_from_slice(&apdu.data);
    buf
}

/// Parse raw APDU bytes back into an `AppIndication` (for transport layer connected-mode).
///
/// # Errors
///
/// Returns `AppLayerError::MalformedData` if the bytes are too short or the APCI is unrecognized.
/// Propagates errors from [`parse_indication`].
pub fn parse_raw_apdu(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    if data.len() < 2 {
        return Err(AppLayerError::MalformedData);
    }
    let apci_raw = u16::from(data[0]) << 8 | u16::from(data[1]);
    let apdu_type = ApduType::from_raw(apci_raw).ok_or(AppLayerError::MalformedData)?;
    parse_indication(apdu_type, &data[1..])
}

// ── APDU parsing (incoming) ──────────────────────────────────

/// Error type for application-layer APDU parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppLayerError {
    /// The APDU type is not supported by this device.
    UnsupportedApdu(ApduType),
    /// The payload is too short for the given APDU type.
    TruncatedPayload {
        /// Minimum number of bytes expected.
        expected: usize,
        /// Actual number of bytes received.
        got: usize,
    },
    /// The raw APDU bytes could not be decoded.
    MalformedData,
}

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

/// Check that `data` has at least `expected` bytes, or return a truncation error.
const fn check_len(data: &[u8], expected: usize) -> Result<(), AppLayerError> {
    if data.len() >= expected {
        Ok(())
    } else {
        Err(AppLayerError::TruncatedPayload {
            expected,
            got: data.len(),
        })
    }
}

/// Parse an APDU type + data into an `AppIndication`.
///
/// # Errors
///
/// Returns `AppLayerError::UnsupportedApdu` for unknown APDU types,
/// `AppLayerError::TruncatedPayload` if the data is too short.
#[expect(clippy::too_many_lines)]
pub fn parse_indication(
    apdu_type: ApduType,
    data: &[u8],
) -> Result<AppIndication, AppLayerError> {
    match apdu_type {
        ApduType::GroupValueWrite => Ok(AppIndication::GroupValueWrite {
            asap: 0,
            data: data.to_vec(),
        }),
        ApduType::GroupValueResponse => Ok(AppIndication::GroupValueResponse {
            asap: 0,
            data: data.to_vec(),
        }),
        ApduType::GroupValueRead => Ok(AppIndication::GroupValueRead { asap: 0 }),
        ApduType::PropertyValueRead => {
            check_len(data, 3)?;
            Ok(AppIndication::PropertyValueRead {
                object_index: data[0],
                property_id: data[1],
                count: (data[2] >> 4) & MASK_4BIT,
                start_index: u16::from(data[2] & MASK_4BIT) << 8
                    | u16::from(*data.get(3).unwrap_or(&0)),
            })
        }
        ApduType::PropertyValueWrite => {
            check_len(data, 4)?;
            Ok(AppIndication::PropertyValueWrite {
                object_index: data[0],
                property_id: data[1],
                count: (data[2] >> 4) & MASK_4BIT,
                start_index: u16::from(data[2] & MASK_4BIT) << 8 | u16::from(data[3]),
                data: data[4..].to_vec(),
            })
        }
        ApduType::DeviceDescriptorRead => Ok(AppIndication::DeviceDescriptorRead {
            descriptor_type: data.first().copied().unwrap_or(0) & MASK_6BIT,
        }),
        ApduType::MemoryRead => {
            check_len(data, 3)?;
            Ok(AppIndication::MemoryRead {
                count: data[0] & MASK_4BIT,
                address: u16::from_be_bytes([data[1], data[2]]),
            })
        }
        ApduType::MemoryWrite => {
            check_len(data, 3)?;
            Ok(AppIndication::MemoryWrite {
                count: data[0] & MASK_4BIT,
                address: u16::from_be_bytes([data[1], data[2]]),
                data: data[3..].to_vec(),
            })
        }
        ApduType::Restart => Ok(AppIndication::Restart),
        ApduType::IndividualAddressWrite => {
            check_len(data, 2)?;
            Ok(AppIndication::IndividualAddressWrite {
                address: u16::from_be_bytes([data[0], data[1]]),
            })
        }
        ApduType::IndividualAddressRead => Ok(AppIndication::IndividualAddressRead),
        ApduType::AuthorizeRequest => {
            check_len(data, 5)?;
            Ok(AppIndication::AuthorizeRequest {
                key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
            })
        }
        ApduType::RestartMasterReset => {
            check_len(data, 3)?;
            Ok(AppIndication::RestartMasterReset {
                erase_code: data[1],
                channel: data[2],
            })
        }
        ApduType::PropertyDescriptionRead => {
            check_len(data, 3)?;
            Ok(AppIndication::PropertyDescriptionRead {
                object_index: data[0],
                property_id: data[1],
                property_index: data[2],
            })
        }
        ApduType::MemoryExtRead => {
            check_len(data, 4)?;
            Ok(AppIndication::MemoryExtRead {
                count: data[0],
                address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
            })
        }
        ApduType::MemoryExtWrite => {
            check_len(data, 4)?;
            Ok(AppIndication::MemoryExtWrite {
                count: data[0],
                address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
                data: data[4..].to_vec(),
            })
        }
        ApduType::IndividualAddressSerialNumberRead => {
            check_len(data, 6)?;
            let mut serial = [0u8; 6];
            serial.copy_from_slice(&data[0..6]);
            Ok(AppIndication::IndividualAddressSerialNumberRead { serial })
        }
        ApduType::IndividualAddressSerialNumberWrite => {
            check_len(data, 8)?;
            let mut serial = [0u8; 6];
            serial.copy_from_slice(&data[0..6]);
            Ok(AppIndication::IndividualAddressSerialNumberWrite {
                serial,
                address: u16::from_be_bytes([data[6], data[7]]),
            })
        }
        ApduType::KeyWrite => {
            check_len(data, 5)?;
            Ok(AppIndication::KeyWrite {
                level: data[0],
                key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
            })
        }
        ApduType::FunctionPropertyCommand => {
            check_len(data, 2)?;
            Ok(AppIndication::FunctionPropertyCommand {
                object_index: data[0],
                property_id: data[1],
                data: data[2..].to_vec(),
            })
        }
        ApduType::FunctionPropertyState => {
            check_len(data, 2)?;
            Ok(AppIndication::FunctionPropertyState {
                object_index: data[0],
                property_id: data[1],
                data: data[2..].to_vec(),
            })
        }
        ApduType::SystemNetworkParameterRead => {
            check_len(data, 4)?;
            let object_type = u16::from_be_bytes([data[0], data[1]]);
            let pid_raw = u16::from_be_bytes([data[2], data[3]]);
            Ok(AppIndication::SystemNetworkParameterRead {
                object_type,
                property_id: pid_raw >> 4,
                test_info: data[3..].to_vec(),
            })
        }
        ApduType::AdcRead => Ok(AppIndication::AdcRead {
            channel: data.first().copied().unwrap_or(0) & MASK_6BIT,
            count: data.get(1).copied().unwrap_or(1),
        }),
        ApduType::PropertyValueExtRead => {
            check_len(data, 7)?;
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Ok(AppIndication::PropertyValueExtRead {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
            })
        }
        ApduType::PropertyValueExtWriteCon => {
            check_len(data, 7)?;
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Ok(AppIndication::PropertyValueExtWriteCon {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
                data: data[7..].to_vec(),
            })
        }
        ApduType::PropertyValueExtWriteUnCon => {
            check_len(data, 7)?;
            let (ot, oi, pid, count, si) = parse_ext_property_header(data);
            Ok(AppIndication::PropertyValueExtWriteUnCon {
                object_type: ot,
                object_instance: oi,
                property_id: pid,
                count,
                start_index: si,
                data: data[7..].to_vec(),
            })
        }
        ApduType::PropertyExtDescriptionRead => {
            check_len(data, 7)?;
            let object_type = u16::from_be_bytes([data[0], data[1]]);
            let object_instance = (u16::from(data[2]) << 4) | (u16::from(data[3]) >> 4);
            let property_id = (u16::from(data[3] & MASK_4BIT) << 8) | u16::from(data[4]);
            let description_type = data[5] >> 4;
            let property_index = (u16::from(data[5] & MASK_4BIT) << 8) | u16::from(data[6]);
            Ok(AppIndication::PropertyExtDescriptionRead {
                object_type,
                object_instance,
                property_id,
                description_type,
                property_index,
            })
        }
        _ => Err(AppLayerError::UnsupportedApdu(apdu_type)),
    }
}

/// Parse the common header for extended property services.
/// Returns `(object_type, object_instance, property_id, count, start_index)`.
fn parse_ext_property_header(data: &[u8]) -> (u16, u16, u16, u8, u16) {
    let object_type = u16::from_be_bytes([data[0], data[1]]);
    let object_instance = (u16::from(data[2]) << 4) | (u16::from(data[3]) >> 4);
    let property_id = (u16::from(data[3] & MASK_4BIT) << 8) | u16::from(data[4]);
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
        let err = parse_indication(ApduType::SecureService, &[]).unwrap_err();
        assert!(matches!(err, AppLayerError::UnsupportedApdu(ApduType::SecureService)));
    }

    #[test]
    fn parse_truncated_property_read() {
        let err = parse_indication(ApduType::PropertyValueRead, &[0x00, 0x01]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 3,
                got: 2
            }
        ));
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

    // ── Encoding tests ───────────────────────────────────────

    /// Helper: expected APCI wire bytes for a given `ApduType`.
    fn expected_apci(t: ApduType) -> [u8; 2] {
        let v = t as u16;
        [(v >> 8) as u8, v as u8]
    }

    #[test]
    fn encode_group_value_write_short() {
        let [hi, lo] = expected_apci(ApduType::GroupValueWrite);
        let result = encode_group_value_write(&[0x01]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo | 0x01);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn encode_group_value_write_long() {
        let [hi, lo] = expected_apci(ApduType::GroupValueWrite);
        let result = encode_group_value_write(&[0xAA, 0xBB]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..], &[0xAA, 0xBB]);
    }

    #[test]
    fn encode_group_value_response_short() {
        let [hi, lo] = expected_apci(ApduType::GroupValueResponse);
        let result = encode_group_value_response(&[0x3F]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo | 0x3F);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn encode_group_value_response_long() {
        let [hi, lo] = expected_apci(ApduType::GroupValueResponse);
        let result = encode_group_value_response(&[0xFF, 0x01]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..], &[0xFF, 0x01]);
    }

    #[test]
    fn encode_group_value_read_bytes() {
        let result = encode_group_value_read();
        assert_eq!(result, &[0x00, 0x00]);
    }

    #[test]
    fn encode_device_descriptor_response_mask() {
        let [hi, lo] = expected_apci(ApduType::DeviceDescriptorResponse);
        let result = encode_device_descriptor_response(0x07B0);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..], &[0x07, 0xB0]);
    }

    #[test]
    fn encode_property_response_encoding() {
        let [hi, lo] = expected_apci(ApduType::PropertyValueResponse);
        // count=1, start_index=1 → count_start = (1 << 12) | 1 = 0x1001
        let result = encode_property_response(0, 0x36, 1, 1, &[0xAA]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00); // object_index
        assert_eq!(result[3], 0x36); // property_id
        assert_eq!(&result[4..6], &[0x10, 0x01]); // count_start BE
        assert_eq!(result[6], 0xAA);
    }

    #[test]
    fn encode_memory_response_encoding() {
        let [hi, lo] = expected_apci(ApduType::MemoryResponse);
        let result = encode_memory_response(0x0010, &[0xDE, 0xAD]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo | 0x02); // data.len() = 2
        assert_eq!(&result[2..4], &[0x00, 0x10]); // address BE
        assert_eq!(&result[4..], &[0xDE, 0xAD]);
    }

    #[test]
    fn encode_authorize_response_level() {
        let [hi, lo] = expected_apci(ApduType::AuthorizeResponse);
        let result = encode_authorize_response(0x03);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x03);
    }

    #[test]
    fn encode_restart_response_encoding() {
        let [hi, lo] = expected_apci(ApduType::RestartMasterReset);
        let result = encode_restart_response(0x01, 0x0064);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x01); // error_code
        assert_eq!(&result[3..5], &[0x00, 0x64]); // process_time BE
    }

    #[test]
    fn encode_memory_ext_read_response_24bit_addr() {
        let [hi, lo] = expected_apci(ApduType::MemoryExtReadResponse);
        let result = encode_memory_ext_read_response(0x00, 0x00_12_34_56, &[0xFF]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00); // return_code
        assert_eq!(&result[3..6], &[0x12, 0x34, 0x56]); // 24-bit address
        assert_eq!(result[6], 0xFF);
    }

    #[test]
    fn encode_memory_ext_write_response_24bit_addr() {
        let [hi, lo] = expected_apci(ApduType::MemoryExtWriteResponse);
        let result = encode_memory_ext_write_response(0x00, 0x00_AB_CD_EF);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00); // return_code
        assert_eq!(&result[3..6], &[0xAB, 0xCD, 0xEF]); // 24-bit address
    }
}
