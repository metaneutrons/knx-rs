// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! APDU parsing — incoming application-layer service dispatch.

use knx_core::message::ApduType;

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
    (object_type, object_instance, property_id, count, start_index)
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
        let ind =
            parse_indication(ApduType::PropertyValueRead, &[0x00, 0x01, 0x10, 0x01]).unwrap();
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
        let ind =
            parse_indication(ApduType::MemoryWrite, &[0x02, 0x00, 0x20, 0xAA, 0xBB]).unwrap();
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
}
