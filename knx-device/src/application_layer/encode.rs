// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! APDU encoding — outgoing application-layer payloads.

use alloc::vec::Vec;

use knx_core::message::ApduType;

use super::{
    DESCRIPTOR_TYPE_UNSUPPORTED, MASK_4BIT, MASK_6BIT, MASK_12BIT, WRITE_ENABLE_FLAG, apci_bytes,
};

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
    debug_assert!(data.len() <= 15, "MemoryResponse data must be <= 15 bytes");
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

/// Encode the common 3-field extended header: `object_type` (2) + `object_instance`/`property_id` (3).
#[expect(clippy::cast_possible_truncation)]
fn encode_ext_ot_oi_pid(buf: &mut Vec<u8>, object_type: u16, object_instance: u16, property_id: u16) {
    let ot = object_type.to_be_bytes();
    buf.extend_from_slice(&ot);
    buf.push(((object_instance >> 4) & 0xFF) as u8);
    buf.push(
        ((object_instance & u16::from(MASK_4BIT)) << 4
            | (property_id >> 8) & u16::from(MASK_4BIT)) as u8,
    );
    buf.push((property_id & 0xFF) as u8);
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
    encode_ext_ot_oi_pid(buf, object_type, object_instance, property_id);
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

/// Encode a `PropertyExtDescriptionResponse` APDU payload.
///
/// Uses the shared extended property header for `object_type`/`object_instance`/`property_id`,
/// then appends `description_type`, `property_index`, and the property description fields.
#[expect(clippy::too_many_arguments, reason = "each parameter maps to a KNX wire format field")]
pub fn encode_property_ext_description_response(
    object_type: u16,
    object_instance: u16,
    property_id: u16,
    property_index: u16,
    description_type: u8,
    write_enable: bool,
    pdt: u8,
    max_elements: u16,
    access: u8,
) -> Vec<u8> {
    let [hi, lo] = apci_bytes(ApduType::PropertyExtDescriptionResponse);
    let mut payload = Vec::with_capacity(13);
    payload.push(hi);
    payload.push(lo);
    // Reuse shared header for ot/oi/pid (first 5 bytes)
    encode_ext_ot_oi_pid(&mut payload, object_type, object_instance, property_id);
    // description_type (4 bits) + property_index (12 bits)
    let desc_idx = ((description_type & MASK_4BIT) << 4) | ((property_index >> 8) as u8 & MASK_4BIT);
    payload.push(desc_idx);
    payload.push((property_index & 0xFF) as u8);
    // Property description fields (same as standard PropertyDescriptionResponse)
    let type_byte = if write_enable {
        WRITE_ENABLE_FLAG | (pdt & MASK_6BIT)
    } else {
        pdt & MASK_6BIT
    };
    let max_hi = ((max_elements >> 8) & u16::from(MASK_4BIT)) as u8;
    let max_lo = (max_elements & 0xFF) as u8;
    payload.push(type_byte);
    payload.push(max_hi);
    payload.push(max_lo);
    payload.push(access);
    payload
}

/// Encode an APDU into raw bytes (for transport layer connected-mode).
pub fn encode_raw_apdu(apdu: &knx_core::apdu::Apdu) -> Vec<u8> {
    let [hi, lo] = apci_bytes(apdu.apdu_type);
    let mut buf = Vec::with_capacity(2 + apdu.data.len());
    buf.push(hi);
    buf.push(lo);
    buf.extend_from_slice(&apdu.data);
    buf
}
