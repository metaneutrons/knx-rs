// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! APDU parsing — incoming application-layer service dispatch.

use knx_rs_core::message::ApduType;

use super::{AppIndication, AppLayerError, MASK_4BIT, MASK_6BIT};

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

/// Parse an APDU type + data into an `AppIndication`.
///
/// # Errors
///
/// Returns `AppLayerError::UnsupportedApdu` for unknown APDU types,
/// `AppLayerError::TruncatedPayload` if the data is too short.
pub fn parse_indication(apdu_type: ApduType, data: &[u8]) -> Result<AppIndication, AppLayerError> {
    match apdu_type {
        ApduType::GroupValueWrite => Ok(parse_group_value_write(data)),
        ApduType::GroupValueResponse => Ok(parse_group_value_response(data)),
        ApduType::GroupValueRead => Ok(parse_group_value_read(data)),
        ApduType::PropertyValueRead => parse_property_value_read(data),
        ApduType::PropertyValueWrite => parse_property_value_write(data),
        ApduType::DeviceDescriptorRead => Ok(parse_device_descriptor_read(data)),
        ApduType::MemoryRead => parse_memory_read(data),
        ApduType::MemoryWrite => parse_memory_write(data),
        ApduType::Restart => Ok(parse_restart(data)),
        ApduType::IndividualAddressWrite => parse_individual_address_write(data),
        ApduType::IndividualAddressRead => Ok(parse_individual_address_read(data)),
        ApduType::AuthorizeRequest => parse_authorize_request(data),
        ApduType::RestartMasterReset => parse_restart_master_reset(data),
        ApduType::PropertyDescriptionRead => parse_property_description_read(data),
        ApduType::MemoryExtRead => parse_memory_ext_read(data),
        ApduType::MemoryExtWrite => parse_memory_ext_write(data),
        ApduType::IndividualAddressSerialNumberRead => {
            parse_individual_address_serial_number_read(data)
        }
        ApduType::IndividualAddressSerialNumberWrite => {
            parse_individual_address_serial_number_write(data)
        }
        ApduType::KeyWrite => parse_key_write(data),
        ApduType::FunctionPropertyCommand => parse_function_property_command(data),
        ApduType::FunctionPropertyState => parse_function_property_state(data),
        ApduType::SystemNetworkParameterRead => parse_system_network_parameter_read(data),
        ApduType::AdcRead => parse_adc_read(data),
        ApduType::PropertyValueExtRead => parse_property_value_ext_read(data),
        ApduType::PropertyValueExtWriteCon => parse_property_value_ext_write_con(data),
        ApduType::PropertyValueExtWriteUnCon => parse_property_value_ext_write_uncon(data),
        ApduType::PropertyExtDescriptionRead => parse_property_ext_description_read(data),
        _ => Err(AppLayerError::UnsupportedApdu(apdu_type)),
    }
}

fn parse_group_value_write(data: &[u8]) -> AppIndication {
    AppIndication::GroupValueWrite {
        asap: 0,
        data: data.to_vec(),
    }
}

fn parse_group_value_response(data: &[u8]) -> AppIndication {
    AppIndication::GroupValueResponse {
        asap: 0,
        data: data.to_vec(),
    }
}

const fn parse_group_value_read(_data: &[u8]) -> AppIndication {
    AppIndication::GroupValueRead { asap: 0 }
}

fn parse_property_value_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 4)?;
    Ok(AppIndication::PropertyValueRead {
        object_index: data[0],
        property_id: data[1],
        count: (data[2] >> 4) & MASK_4BIT,
        start_index: u16::from(data[2] & MASK_4BIT) << 8 | u16::from(data[3]),
    })
}

fn parse_property_value_write(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 4)?;
    Ok(AppIndication::PropertyValueWrite {
        object_index: data[0],
        property_id: data[1],
        count: (data[2] >> 4) & MASK_4BIT,
        start_index: u16::from(data[2] & MASK_4BIT) << 8 | u16::from(data[3]),
        data: data[4..].to_vec(),
    })
}

fn parse_device_descriptor_read(data: &[u8]) -> AppIndication {
    AppIndication::DeviceDescriptorRead {
        descriptor_type: data.first().copied().unwrap_or(0) & MASK_6BIT,
    }
}

fn parse_memory_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 3)?;
    Ok(AppIndication::MemoryRead {
        count: data[0] & MASK_4BIT,
        address: u16::from_be_bytes([data[1], data[2]]),
    })
}

fn parse_memory_write(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 3)?;
    Ok(AppIndication::MemoryWrite {
        count: data[0] & MASK_4BIT,
        address: u16::from_be_bytes([data[1], data[2]]),
        data: data[3..].to_vec(),
    })
}

const fn parse_restart(_data: &[u8]) -> AppIndication {
    AppIndication::Restart
}

fn parse_individual_address_write(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 2)?;
    Ok(AppIndication::IndividualAddressWrite {
        address: u16::from_be_bytes([data[0], data[1]]),
    })
}

const fn parse_individual_address_read(_data: &[u8]) -> AppIndication {
    AppIndication::IndividualAddressRead
}

fn parse_authorize_request(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 5)?;
    Ok(AppIndication::AuthorizeRequest {
        key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
    })
}

fn parse_restart_master_reset(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 3)?;
    Ok(AppIndication::RestartMasterReset {
        erase_code: data[1],
        channel: data[2],
    })
}

fn parse_property_description_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 3)?;
    Ok(AppIndication::PropertyDescriptionRead {
        object_index: data[0],
        property_id: data[1],
        property_index: data[2],
    })
}

fn parse_memory_ext_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 4)?;
    Ok(AppIndication::MemoryExtRead {
        count: data[0],
        address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
    })
}

fn parse_memory_ext_write(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 4)?;
    Ok(AppIndication::MemoryExtWrite {
        count: data[0],
        address: u32::from_be_bytes([0, data[1], data[2], data[3]]),
        data: data[4..].to_vec(),
    })
}

fn parse_individual_address_serial_number_read(
    data: &[u8],
) -> Result<AppIndication, AppLayerError> {
    check_len(data, 6)?;
    let mut serial = [0u8; 6];
    serial.copy_from_slice(&data[0..6]);
    Ok(AppIndication::IndividualAddressSerialNumberRead { serial })
}

fn parse_individual_address_serial_number_write(
    data: &[u8],
) -> Result<AppIndication, AppLayerError> {
    check_len(data, 8)?;
    let mut serial = [0u8; 6];
    serial.copy_from_slice(&data[0..6]);
    Ok(AppIndication::IndividualAddressSerialNumberWrite {
        serial,
        address: u16::from_be_bytes([data[6], data[7]]),
    })
}

fn parse_key_write(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 5)?;
    Ok(AppIndication::KeyWrite {
        level: data[0],
        key: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
    })
}

fn parse_function_property_command(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 2)?;
    Ok(AppIndication::FunctionPropertyCommand {
        object_index: data[0],
        property_id: data[1],
        data: data[2..].to_vec(),
    })
}

fn parse_function_property_state(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 2)?;
    Ok(AppIndication::FunctionPropertyState {
        object_index: data[0],
        property_id: data[1],
        data: data[2..].to_vec(),
    })
}

fn parse_system_network_parameter_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 4)?;
    let object_type = u16::from_be_bytes([data[0], data[1]]);
    let pid_raw = u16::from_be_bytes([data[2], data[3]]);
    Ok(AppIndication::SystemNetworkParameterRead {
        object_type,
        property_id: pid_raw >> 4,
        test_info: data[3..].to_vec(),
    })
}

fn parse_adc_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 2)?;
    Ok(AppIndication::AdcRead {
        channel: data[0] & MASK_6BIT,
        count: data[1],
    })
}

fn parse_property_value_ext_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 8)?;
    let (ot, oi, pid, count, si) = parse_ext_property_header(data);
    Ok(AppIndication::PropertyValueExtRead {
        object_type: ot,
        object_instance: oi,
        property_id: pid,
        count,
        start_index: si,
    })
}

fn parse_property_value_ext_write_con(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 8)?;
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

fn parse_property_value_ext_write_uncon(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 8)?;
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

fn parse_property_ext_description_read(data: &[u8]) -> Result<AppIndication, AppLayerError> {
    check_len(data, 8)?;
    let (object_type, object_instance, property_id) = parse_ext_ot_oi_pid(data);
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

/// Parse the common 3-field extended header: `(object_type, object_instance, property_id)`.
fn parse_ext_ot_oi_pid(data: &[u8]) -> (u16, u16, u16) {
    let object_type = u16::from_be_bytes([data[0], data[1]]);
    let object_instance = (u16::from(data[2]) << 4) | (u16::from(data[3]) >> 4);
    let property_id = (u16::from(data[3] & MASK_4BIT) << 8) | u16::from(data[4]);
    (object_type, object_instance, property_id)
}

/// Parse the common header for extended property services.
/// Returns `(object_type, object_instance, property_id, count, start_index)`.
fn parse_ext_property_header(data: &[u8]) -> (u16, u16, u16, u8, u16) {
    let (object_type, object_instance, property_id) = parse_ext_ot_oi_pid(data);
    let count = data[5];
    let start_index = u16::from_be_bytes([data[6], data[7]]);
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
    use crate::application_layer::encode::*;

    #[test]
    fn parse_group_value_write() {
        let ind = parse_indication(ApduType::GroupValueWrite, &[0x01]).unwrap();
        assert!(matches!(ind, AppIndication::GroupValueWrite { data, .. } if data == [0x01]));
    }

    #[test]
    fn parse_property_read() {
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
        assert!(matches!(
            err,
            AppLayerError::UnsupportedApdu(ApduType::SecureService)
        ));
    }

    #[test]
    fn parse_truncated_property_read() {
        let err = parse_indication(ApduType::PropertyValueRead, &[0x00, 0x01]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 4,
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
        let result = encode_property_response(0, 0x36, 1, 1, &[0xAA]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00);
        assert_eq!(result[3], 0x36);
        assert_eq!(&result[4..6], &[0x10, 0x01]);
        assert_eq!(result[6], 0xAA);
    }

    #[test]
    fn encode_memory_response_encoding() {
        let [hi, lo] = expected_apci(ApduType::MemoryResponse);
        let result = encode_memory_response(0x0010, &[0xDE, 0xAD]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo | 0x02);
        assert_eq!(&result[2..4], &[0x00, 0x10]);
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
        assert_eq!(result[2], 0x01);
        assert_eq!(&result[3..5], &[0x00, 0x64]);
    }

    #[test]
    fn encode_memory_ext_read_response_24bit_addr() {
        let [hi, lo] = expected_apci(ApduType::MemoryExtReadResponse);
        let result = encode_memory_ext_read_response(0x00, 0x00_12_34_56, &[0xFF]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00);
        assert_eq!(&result[3..6], &[0x12, 0x34, 0x56]);
        assert_eq!(result[6], 0xFF);
    }

    #[test]
    fn encode_memory_ext_write_response_24bit_addr() {
        let [hi, lo] = expected_apci(ApduType::MemoryExtWriteResponse);
        let result = encode_memory_ext_write_response(0x00, 0x00_AB_CD_EF);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x00);
        assert_eq!(&result[3..6], &[0xAB, 0xCD, 0xEF]);
    }

    // ── Additional encode tests ──────────────────────────────

    #[test]
    fn encode_individual_address_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::IndividualAddressResponse);
        let result = encode_individual_address_response();
        assert_eq!(result, &[hi, lo]);
    }

    #[test]
    fn encode_key_response_level() {
        let [hi, lo] = expected_apci(ApduType::KeyResponse);
        let result = encode_key_response(0x02);
        assert_eq!(result, &[hi, lo, 0x02]);
    }

    #[test]
    fn encode_property_description_response_all_bytes() {
        let [hi, lo] = expected_apci(ApduType::PropertyDescriptionResponse);
        let result =
            encode_property_description_response(0x01, 0x0B, 0x03, true, 0x11, 0x0100, 0x37);
        assert_eq!(result.len(), 9);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x01); // object_index
        assert_eq!(result[3], 0x0B); // property_id
        assert_eq!(result[4], 0x03); // property_index
        assert_eq!(result[5], 0x80 | 0x11); // write_enable | pdt
        assert_eq!(result[6], 0x01); // max_elements high
        assert_eq!(result[7], 0x00); // max_elements low
        assert_eq!(result[8], 0x37); // access
    }

    #[test]
    fn encode_individual_address_serial_number_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::IndividualAddressSerialNumberResponse);
        let serial = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let result = encode_individual_address_serial_number_response(serial, 0xABCD);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..8], &serial);
        assert_eq!(&result[8..10], &[0xAB, 0xCD]);
    }

    #[test]
    fn encode_system_network_parameter_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::SystemNetworkParameterResponse);
        let result =
            encode_system_network_parameter_response(0x0001, 0x000C, &[0xAA], &[0xBB, 0xCC]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..4], &[0x00, 0x01]); // object_type
        assert_eq!(&result[4..6], &[0x00, 0xC0]); // pid << 4
        assert_eq!(result[6], 0xAA); // test_info
        assert_eq!(&result[7..], &[0xBB, 0xCC]); // test_result
    }

    #[test]
    fn encode_adc_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::AdcResponse);
        let result = encode_adc_response(0x05, 0x08, 0x1234);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo | 0x05); // channel in low bits
        assert_eq!(result[2], 0x08); // count
        assert_eq!(&result[3..5], &[0x12, 0x34]); // value
    }

    #[test]
    fn encode_function_property_state_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::FunctionPropertyStateResponse);
        let result = encode_function_property_state_response(0x02, 0x0A, &[0xDE, 0xAD]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(result[2], 0x02); // object_index
        assert_eq!(result[3], 0x0A); // property_id
        assert_eq!(&result[4..], &[0xDE, 0xAD]); // data
    }

    #[test]
    fn encode_property_value_ext_response_bytes() {
        let [hi, lo] = expected_apci(ApduType::PropertyValueExtResponse);
        let result = encode_property_value_ext_response(0x0001, 0x001, 0x00B, 1, 0x0001, &[0xFF]);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        // ext header: object_type(2) + oi_hi(1) + oi_lo|pid_hi(1) + pid_lo(1) + count(1) + start_index(2)
        assert_eq!(&result[2..4], &[0x00, 0x01]); // object_type
        assert_eq!(result[4], 0x00); // oi >> 4
        assert_eq!(result[5], 0x10); // (oi & 0x0F) << 4 | (pid >> 8)
        assert_eq!(result[6], 0x0B); // pid low
        assert_eq!(result[7], 0x01); // count
        assert_eq!(&result[8..10], &[0x00, 0x01]); // start_index
        assert_eq!(result[10], 0xFF); // data
    }

    #[test]
    fn encode_device_descriptor_unsupported_bytes() {
        let [hi, _] = expected_apci(ApduType::DeviceDescriptorResponse);
        let result = encode_device_descriptor_unsupported();
        assert_eq!(result[0], hi);
        assert_eq!(result[1], 0x3F);
    }

    #[test]
    fn encode_raw_apdu_passthrough() {
        let apdu = knx_rs_core::apdu::Apdu {
            apdu_type: ApduType::GroupValueWrite,
            data: alloc::vec![0xAA, 0xBB],
        };
        let [hi, lo] = expected_apci(ApduType::GroupValueWrite);
        let result = encode_raw_apdu(&apdu);
        assert_eq!(result[0], hi);
        assert_eq!(result[1], lo);
        assert_eq!(&result[2..], &[0xAA, 0xBB]);
    }

    // ── Roundtrip tests ──────────────────────────────────────

    #[test]
    fn roundtrip_group_value_write() {
        let encoded = encode_group_value_write(&[0xAA, 0xBB]);
        // parse_raw_apdu strips byte 0, passes &data[1..] to parse_indication
        // parse_group_value_write keeps the APCI low byte + trailing data
        let parsed = parse_raw_apdu(&encoded).unwrap();
        if let AppIndication::GroupValueWrite { data, .. } = parsed {
            // data[0] is APCI low byte (0x80), then payload
            assert_eq!(&data[1..], &[0xAA, 0xBB]);
        } else {
            panic!("expected GroupValueWrite");
        }
    }

    #[test]
    fn roundtrip_memory_response() {
        let encoded = encode_memory_response(0x0100, &[0xDE, 0xAD]);
        let [hi, lo] = expected_apci(ApduType::MemoryResponse);
        assert_eq!(encoded[0], hi);
        assert_eq!(encoded[1], lo | 0x02); // count = 2
        assert_eq!(&encoded[2..4], &[0x01, 0x00]); // address
        assert_eq!(&encoded[4..], &[0xDE, 0xAD]); // data
    }

    #[test]
    fn roundtrip_device_descriptor_response() {
        let encoded = encode_device_descriptor_response(0x07B0);
        let [hi, lo] = expected_apci(ApduType::DeviceDescriptorResponse);
        assert_eq!(encoded, &[hi, lo, 0x07, 0xB0]);
    }

    #[test]
    fn roundtrip_memory_ext_read_response() {
        let encoded = encode_memory_ext_read_response(0x00, 0x00_12_34_56, &[0xAA, 0xBB]);
        let [hi, lo] = expected_apci(ApduType::MemoryExtReadResponse);
        assert_eq!(encoded[0], hi);
        assert_eq!(encoded[1], lo);
        assert_eq!(encoded[2], 0x00); // return_code
        assert_eq!(&encoded[3..6], &[0x12, 0x34, 0x56]); // 24-bit address
        assert_eq!(&encoded[6..], &[0xAA, 0xBB]); // data
    }

    #[test]
    fn roundtrip_property_value_response() {
        let encoded = encode_property_response(0x00, 0x36, 1, 1, &[0xAA]);
        let [hi, lo] = expected_apci(ApduType::PropertyValueResponse);
        assert_eq!(encoded[0], hi);
        assert_eq!(encoded[1], lo);
        assert_eq!(encoded[2], 0x00); // object_index
        assert_eq!(encoded[3], 0x36); // property_id
        assert_eq!(&encoded[4..6], &[0x10, 0x01]); // count=1, start_index=1
        assert_eq!(encoded[6], 0xAA); // data
    }

    // ── Additional parse tests ───────────────────────────────

    #[test]
    fn parse_restart_master_reset() {
        let ind = parse_indication(ApduType::RestartMasterReset, &[0x00, 0x01, 0x02]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::RestartMasterReset {
                erase_code: 0x01,
                channel: 0x02,
            }
        ));
    }

    #[test]
    fn parse_restart_master_reset_truncated() {
        let err = parse_indication(ApduType::RestartMasterReset, &[0x00, 0x01]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 3,
                got: 2
            }
        ));
    }

    #[test]
    fn parse_property_description_read() {
        let ind = parse_indication(ApduType::PropertyDescriptionRead, &[0x01, 0x36, 0x03]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyDescriptionRead {
                object_index: 0x01,
                property_id: 0x36,
                property_index: 0x03,
            }
        ));
    }

    #[test]
    fn parse_property_description_read_truncated() {
        let err = parse_indication(ApduType::PropertyDescriptionRead, &[0x01, 0x36]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 3,
                got: 2
            }
        ));
    }

    #[test]
    fn parse_memory_ext_read() {
        let ind = parse_indication(ApduType::MemoryExtRead, &[0x04, 0x12, 0x34, 0x56]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::MemoryExtRead {
                count: 0x04,
                address: 0x00_12_34_56,
            }
        ));
    }

    #[test]
    fn parse_memory_ext_read_truncated() {
        let err = parse_indication(ApduType::MemoryExtRead, &[0x04, 0x12, 0x34]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 4,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_memory_ext_write() {
        let ind = parse_indication(
            ApduType::MemoryExtWrite,
            &[0x02, 0xAB, 0xCD, 0xEF, 0xDE, 0xAD],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::MemoryExtWrite {
                count: 0x02,
                address: 0x00_AB_CD_EF,
                ..
            }
        ));
        if let AppIndication::MemoryExtWrite { data, .. } = ind {
            assert_eq!(data, &[0xDE, 0xAD]);
        }
    }

    #[test]
    fn parse_memory_ext_write_truncated() {
        let err = parse_indication(ApduType::MemoryExtWrite, &[0x02, 0xAB, 0xCD]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 4,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_individual_address_serial_number_read() {
        let ind = parse_indication(
            ApduType::IndividualAddressSerialNumberRead,
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::IndividualAddressSerialNumberRead {
                serial: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            }
        ));
    }

    #[test]
    fn parse_individual_address_serial_number_read_truncated() {
        let err = parse_indication(
            ApduType::IndividualAddressSerialNumberRead,
            &[0x01, 0x02, 0x03],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 6,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_individual_address_serial_number_write() {
        let ind = parse_indication(
            ApduType::IndividualAddressSerialNumberWrite,
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x11, 0x05],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::IndividualAddressSerialNumberWrite {
                serial: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
                address: 0x1105,
            }
        ));
    }

    #[test]
    fn parse_individual_address_serial_number_write_truncated() {
        let err = parse_indication(
            ApduType::IndividualAddressSerialNumberWrite,
            &[0x01, 0x02, 0x03, 0x04, 0x05],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 5
            }
        ));
    }

    #[test]
    fn parse_key_write() {
        let ind = parse_indication(ApduType::KeyWrite, &[0x03, 0x00, 0x00, 0x00, 0xFF]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::KeyWrite {
                level: 0x03,
                key: 0x0000_00FF,
            }
        ));
    }

    #[test]
    fn parse_key_write_truncated() {
        let err = parse_indication(ApduType::KeyWrite, &[0x03, 0x00, 0x00]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 5,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_function_property_command() {
        let ind =
            parse_indication(ApduType::FunctionPropertyCommand, &[0x01, 0x02, 0xAA, 0xBB]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::FunctionPropertyCommand {
                object_index: 0x01,
                property_id: 0x02,
                ..
            }
        ));
        if let AppIndication::FunctionPropertyCommand { data, .. } = ind {
            assert_eq!(data, &[0xAA, 0xBB]);
        }
    }

    #[test]
    fn parse_function_property_command_truncated() {
        let err = parse_indication(ApduType::FunctionPropertyCommand, &[0x01]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 2,
                got: 1
            }
        ));
    }

    #[test]
    fn parse_function_property_state() {
        let ind = parse_indication(ApduType::FunctionPropertyState, &[0x03, 0x04, 0xCC]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::FunctionPropertyState {
                object_index: 0x03,
                property_id: 0x04,
                ..
            }
        ));
        if let AppIndication::FunctionPropertyState { data, .. } = ind {
            assert_eq!(data, &[0xCC]);
        }
    }

    #[test]
    fn parse_function_property_state_truncated() {
        let err = parse_indication(ApduType::FunctionPropertyState, &[0x03]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 2,
                got: 1
            }
        ));
    }

    #[test]
    fn parse_system_network_parameter_read() {
        let ind = parse_indication(
            ApduType::SystemNetworkParameterRead,
            &[0x00, 0x07, 0x01, 0x10],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::SystemNetworkParameterRead {
                object_type: 0x0007,
                property_id: 0x0011,
                ..
            }
        ));
        if let AppIndication::SystemNetworkParameterRead { test_info, .. } = ind {
            assert_eq!(test_info, &[0x10]);
        }
    }

    #[test]
    fn parse_system_network_parameter_read_truncated() {
        let err = parse_indication(ApduType::SystemNetworkParameterRead, &[0x00, 0x07, 0x01])
            .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 4,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_adc_read() {
        let ind = parse_indication(ApduType::AdcRead, &[0x05, 0x03]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::AdcRead {
                channel: 0x05,
                count: 0x03,
            }
        ));
    }

    #[test]
    fn parse_property_value_ext_read() {
        let ind = parse_indication(
            ApduType::PropertyValueExtRead,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x01, 0x00, 0x01],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueExtRead {
                object_type: 0x0001,
                object_instance: 0x012,
                property_id: 0x003,
                count: 0x01,
                start_index: 0x0001,
            }
        ));
    }

    #[test]
    fn parse_property_value_ext_read_truncated() {
        let err = parse_indication(
            ApduType::PropertyValueExtRead,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x01],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 6
            }
        ));
    }

    #[test]
    fn parse_property_value_ext_write_con() {
        let ind = parse_indication(
            ApduType::PropertyValueExtWriteCon,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x01, 0x00, 0x01, 0xAA],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueExtWriteCon {
                object_type: 0x0001,
                object_instance: 0x012,
                property_id: 0x003,
                count: 0x01,
                start_index: 0x0001,
                ..
            }
        ));
        if let AppIndication::PropertyValueExtWriteCon { data, .. } = ind {
            assert_eq!(data, &[0x01, 0xAA]);
        }
    }

    #[test]
    fn parse_property_value_ext_write_con_truncated() {
        let err = parse_indication(
            ApduType::PropertyValueExtWriteCon,
            &[0x00, 0x01, 0x01, 0x20, 0x03],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 5
            }
        ));
    }

    #[test]
    fn parse_property_value_ext_write_uncon() {
        let ind = parse_indication(
            ApduType::PropertyValueExtWriteUnCon,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x01, 0x00, 0x01, 0xBB],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueExtWriteUnCon {
                object_type: 0x0001,
                object_instance: 0x012,
                property_id: 0x003,
                count: 0x01,
                start_index: 0x0001,
                ..
            }
        ));
        if let AppIndication::PropertyValueExtWriteUnCon { data, .. } = ind {
            assert_eq!(data, &[0x01, 0xBB]);
        }
    }

    #[test]
    fn parse_property_value_ext_write_uncon_truncated() {
        let err = parse_indication(ApduType::PropertyValueExtWriteUnCon, &[0x00, 0x01, 0x01])
            .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_property_ext_description_read() {
        let ind = parse_indication(
            ApduType::PropertyExtDescriptionRead,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x10, 0x05, 0x00],
        )
        .unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyExtDescriptionRead {
                object_type: 0x0001,
                object_instance: 0x012,
                property_id: 0x003,
                description_type: 0x01,
                property_index: 0x005,
            }
        ));
    }

    #[test]
    fn parse_property_ext_description_read_truncated() {
        let err = parse_indication(
            ApduType::PropertyExtDescriptionRead,
            &[0x00, 0x01, 0x01, 0x20],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 4
            }
        ));
    }

    #[test]
    fn parse_group_value_response() {
        let ind = parse_indication(ApduType::GroupValueResponse, &[0x01, 0x02]).unwrap();
        assert!(
            matches!(ind, AppIndication::GroupValueResponse { data, .. } if data == [0x01, 0x02])
        );
    }

    #[test]
    fn parse_property_value_read_3bytes_is_error() {
        let err = parse_indication(ApduType::PropertyValueRead, &[0x00, 0x01, 0x10]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 4,
                got: 3
            }
        ));
    }

    #[test]
    fn parse_adc_read_truncated() {
        let err = parse_indication(ApduType::AdcRead, &[0x05]).unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 2,
                got: 1
            }
        ));
    }

    #[test]
    fn parse_ext_property_7bytes_is_error() {
        let err = parse_indication(
            ApduType::PropertyValueExtRead,
            &[0x00, 0x01, 0x01, 0x20, 0x03, 0x01, 0x00],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppLayerError::TruncatedPayload {
                expected: 8,
                got: 7
            }
        ));
    }

    #[test]
    fn parse_max_length_memory_write() {
        // MemoryWrite with 15 bytes of data (max for 4-bit count field)
        let mut payload = alloc::vec![0x0F, 0x00, 0x10]; // count=15, address=0x0010
        payload.extend_from_slice(&[0xAA; 15]);
        let ind = parse_indication(ApduType::MemoryWrite, &payload).unwrap();
        if let AppIndication::MemoryWrite {
            count,
            address,
            data,
        } = ind
        {
            assert_eq!(count, 15, "count should be 15 (max 4-bit value)");
            assert_eq!(address, 0x0010);
            assert_eq!(data.len(), 15);
            assert!(data.iter().all(|&b| b == 0xAA));
        } else {
            panic!("expected MemoryWrite");
        }
    }

    #[test]
    fn parse_all_zero_payload() {
        // Various APDU types with all-zero data — verify no crash
        let zeros_3 = [0x00, 0x00, 0x00];
        let zeros_4 = [0x00, 0x00, 0x00, 0x00];
        let zeros_5 = [0x00, 0x00, 0x00, 0x00, 0x00];
        let zeros_8 = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        assert!(parse_indication(ApduType::MemoryRead, &zeros_3).is_ok());
        assert!(parse_indication(ApduType::MemoryWrite, &zeros_3).is_ok());
        assert!(parse_indication(ApduType::PropertyValueRead, &zeros_4).is_ok());
        assert!(parse_indication(ApduType::AuthorizeRequest, &zeros_5).is_ok());
        assert!(parse_indication(ApduType::PropertyValueExtRead, &zeros_8).is_ok());
        assert!(parse_indication(ApduType::GroupValueWrite, &[0x00]).is_ok());
        assert!(parse_indication(ApduType::GroupValueRead, &[]).is_ok());
        assert!(parse_indication(ApduType::Restart, &[]).is_ok());
    }

    #[test]
    fn parse_property_value_read_exact_minimum() {
        // Exactly 4 bytes (minimum required), verify correct parsing
        let ind = parse_indication(ApduType::PropertyValueRead, &[0x02, 0x0B, 0x31, 0x05]).unwrap();
        assert!(matches!(
            ind,
            AppIndication::PropertyValueRead {
                object_index: 0x02,
                property_id: 0x0B,
                count: 3,
                start_index: 0x0105,
            }
        ));
    }
}
