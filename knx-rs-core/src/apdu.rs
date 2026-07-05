// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Application Protocol Data Unit (APDU).
//!
//! The APDU carries the application-layer service type and data.
//! It is encoded in the TPDU payload, starting at the TPCI/APCI bytes.
//!
//! # Wire encoding
//!
//! The first two bytes of the TPDU data contain the TPCI and APCI:
//!
//! ```text
//! Byte 0: [TPCI bits 7..2] [APCI bits 9..8]
//! Byte 1: [APCI bits 7..0]
//! ```
//!
//! For "short" APCIs (group value read/response/write), the lower 6 bits
//! of byte 1 carry small data values directly.

use alloc::vec::Vec;

use crate::message::ApduType;

/// Mask for the 10-bit APCI field carried in the two TPCI/APCI bytes.
pub const APCI_MASK: u16 = 0x03FF;
/// Mask isolating the opcode bits of a "short" APCI (drops the 6 data bits).
pub const APCI_SHORT_TYPE_MASK: u16 = 0x03C0;
/// Mask for the 6-bit inline value carried by a "short" APCI.
pub const APCI_SHORT_DATA_MASK: u8 = 0x3F;

/// Bit position separating an APCI's opcode family from its 6 data bits.
const APCI_FAMILY_SHIFT: u16 = 6;
/// Opcode families at or above this index use the full (long) APCI encoding.
const APCI_SHORT_FAMILY_MAX: u16 = 11;
/// Opcode family `7` (the `0x1Cx` escape range) is long despite being below the
/// short-family threshold.
const APCI_LONG_ESCAPE_FAMILY: u16 = 7;

/// A parsed Application Protocol Data Unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apdu {
    /// The APDU service type.
    pub apdu_type: ApduType,
    /// The APDU data bytes (excluding the APCI encoding).
    ///
    /// For short APDUs (e.g. `GroupValueWrite` with ≤6 bits), this contains
    /// the small value in `data[0] & 0x3F`. For longer APDUs, this is the
    /// payload starting after the 2-byte APCI header.
    pub data: Vec<u8>,
}

impl Apdu {
    /// Parse an APDU from raw TPDU payload bytes.
    ///
    /// `payload` starts at the TPCI byte (first byte of the TPDU data).
    /// `npdu_length` is the octet count from the CEMI frame.
    ///
    /// # Errors
    ///
    /// Returns `None` if the payload is too short or the APCI is unrecognized.
    pub fn parse(payload: &[u8], npdu_length: u8) -> Option<Self> {
        if payload.len() < 2 {
            return None;
        }

        let apci_raw = u16::from_be_bytes([payload[0], payload[1]]) & 0x03FF;
        let (apdu_type, data) = decode_apci(apci_raw, payload, npdu_length)?;

        Some(Self { apdu_type, data })
    }

    /// Encode the APDU into TPDU payload bytes.
    ///
    /// Returns the bytes starting from the TPCI/APCI position.
    pub fn to_bytes(&self, tpci_bits: u8) -> Vec<u8> {
        let apci = self.apdu_type as u16;
        let byte0 = (tpci_bits & 0xFC) | ((apci >> 8) as u8 & 0x03);
        #[expect(clippy::cast_possible_truncation)]
        let apci_low = apci as u8;

        if uses_short_form(apci, &self.data) {
            // Short APDU: a single 6-bit value packed into the lower bits of
            // byte 1. Empty data encodes a value of 0 (the inverse of decode,
            // which always yields one data byte for the short form).
            let value = self.data.first().copied().unwrap_or(0);
            let byte1 = (apci_low & 0xC0) | (value & APCI_SHORT_DATA_MASK);
            alloc::vec![byte0, byte1]
        } else {
            // Long APDU: 2-byte APCI header + data
            let mut buf = alloc::vec![byte0, apci_low];
            buf.extend_from_slice(&self.data);
            buf
        }
    }
}

/// Determine if an APCI value uses the "short" encoding (6-bit data in byte 1).
///
/// Per the C++ reference: APCI values whose opcode family is below
/// [`APCI_SHORT_FAMILY_MAX`] and is not the [`APCI_LONG_ESCAPE_FAMILY`] are
/// short — the lower 6 bits carry inline data and are masked off for type
/// identification.
const fn is_short_apci(apci: u16) -> bool {
    let family = apci >> APCI_FAMILY_SHIFT;
    family < APCI_SHORT_FAMILY_MAX && family != APCI_LONG_ESCAPE_FAMILY
}

/// Whether an APDU encodes in the short form (a single ≤6-bit value packed into
/// the APCI byte).
///
/// Requires a short APCI and at most one data byte that fits in 6 bits — a
/// single byte greater than [`APCI_SHORT_DATA_MASK`] (e.g. a full-octet DPT 5
/// value) must use the long form to avoid losing its high bits.
const fn uses_short_form(apci: u16, data: &[u8]) -> bool {
    if !is_short_apci(apci) || data.len() > 1 {
        return false;
    }
    match data.first() {
        Some(&value) => value <= APCI_SHORT_DATA_MASK,
        None => true,
    }
}

/// Normalize a raw 16-bit APCI field to the value used for type identification.
///
/// Applies the 10-bit [`APCI_MASK`], then drops the inline data bits of a short
/// APCI. This is the single source of the masking applied during parsing and
/// must be used by every raw-APCI → [`ApduType`] conversion.
const fn normalize_apci(raw: u16) -> u16 {
    let apci = raw & APCI_MASK;
    if is_short_apci(apci) {
        apci & APCI_SHORT_TYPE_MASK
    } else {
        apci
    }
}

/// Decode APCI value and extract data from payload.
fn decode_apci(apci_raw: u16, payload: &[u8], npdu_length: u8) -> Option<(ApduType, Vec<u8>)> {
    let apdu_type = match_apdu_type(normalize_apci(apci_raw))?;

    let data = if is_short_apci(apci_raw) && npdu_length <= 1 {
        // Short APDU: small value in lower 6 bits of byte 1
        alloc::vec![payload[1] & APCI_SHORT_DATA_MASK]
    } else if matches!(
        apdu_type,
        ApduType::MemoryRead
            | ApduType::MemoryWrite
            | ApduType::MemoryResponse
            | ApduType::AdcRead
            | ApduType::AdcResponse
    ) && payload.len() > 1
    {
        // Basic Memory (read/write/response) and ADC pack a 6-bit field (byte
        // count / channel) into byte 1's low bits AND carry trailing octets
        // (address/data). Keep byte 1 so the parser reads that field at data[0] —
        // mirrors the C++ `apdu.data()`, which points at the 2nd APCI byte. The
        // long-family path below strips both APCI bytes, which for these services
        // drops the count and shifts address/data by one byte — corrupting every
        // real ETS memory operation (the core of a download).
        payload[1..].to_vec()
    } else if payload.len() > 2 {
        // Long APDU: data after the 2-byte APCI header
        payload[2..].to_vec()
    } else {
        Vec::new()
    };

    Some((apdu_type, data))
}

/// Try to convert a raw APCI value to an `ApduType`.
///
/// Applies `normalize_apci` (the same masking used by [`Apdu::parse`]) before
/// matching, so a short APCI carrying inline data still resolves to its type.
pub const fn apdu_type_from_raw(raw: u16) -> Option<ApduType> {
    match_apdu_type(normalize_apci(raw))
}

/// Map a (masked) APCI value to an `ApduType` enum variant.
const fn match_apdu_type(bits: u16) -> Option<ApduType> {
    // This covers all variants from the C++ knx_types.h
    Some(match bits {
        0x000 => ApduType::GroupValueRead,
        0x040 => ApduType::GroupValueResponse,
        0x080 => ApduType::GroupValueWrite,
        0x0C0 => ApduType::IndividualAddressWrite,
        0x100 => ApduType::IndividualAddressRead,
        0x140 => ApduType::IndividualAddressResponse,
        0x180 => ApduType::AdcRead,
        0x1C0 => ApduType::AdcResponse,
        0x1C8 => ApduType::SystemNetworkParameterRead,
        0x1C9 => ApduType::SystemNetworkParameterResponse,
        0x1CA => ApduType::SystemNetworkParameterWrite,
        0x1CC => ApduType::PropertyValueExtRead,
        0x1CD => ApduType::PropertyValueExtResponse,
        0x1CE => ApduType::PropertyValueExtWriteCon,
        0x1CF => ApduType::PropertyValueExtWriteConResponse,
        0x1D0 => ApduType::PropertyValueExtWriteUnCon,
        0x1D2 => ApduType::PropertyExtDescriptionRead,
        0x1D3 => ApduType::PropertyExtDescriptionResponse,
        0x1D4 => ApduType::FunctionPropertyExtCommand,
        0x1D5 => ApduType::FunctionPropertyExtState,
        0x1D6 => ApduType::FunctionPropertyExtStateResponse,
        0x1FB => ApduType::MemoryExtWrite,
        0x1FC => ApduType::MemoryExtWriteResponse,
        0x1FD => ApduType::MemoryExtRead,
        0x1FE => ApduType::MemoryExtReadResponse,
        0x200 => ApduType::MemoryRead,
        0x240 => ApduType::MemoryResponse,
        0x280 => ApduType::MemoryWrite,
        0x2C0 => ApduType::UserMemoryRead,
        0x2C1 => ApduType::UserMemoryResponse,
        0x2C2 => ApduType::UserMemoryWrite,
        0x2C5 => ApduType::UserManufacturerInfoRead,
        0x2C6 => ApduType::UserManufacturerInfoResponse,
        0x2C7 => ApduType::FunctionPropertyCommand,
        0x2C8 => ApduType::FunctionPropertyState,
        0x2C9 => ApduType::FunctionPropertyStateResponse,
        0x300 => ApduType::DeviceDescriptorRead,
        0x340 => ApduType::DeviceDescriptorResponse,
        0x380 => ApduType::Restart,
        0x381 => ApduType::RestartMasterReset,
        0x3C0 => ApduType::RoutingTableOpen,
        0x3C1 => ApduType::RoutingTableRead,
        0x3C2 => ApduType::RoutingTableReadResponse,
        0x3C3 => ApduType::RoutingTableWrite,
        0x3C9 => ApduType::MemoryRouterReadResponse,
        0x3CA => ApduType::MemoryRouterWrite,
        0x3D1 => ApduType::AuthorizeRequest,
        0x3D2 => ApduType::AuthorizeResponse,
        0x3D3 => ApduType::KeyWrite,
        0x3D4 => ApduType::KeyResponse,
        0x3D5 => ApduType::PropertyValueRead,
        0x3D6 => ApduType::PropertyValueResponse,
        0x3D7 => ApduType::PropertyValueWrite,
        0x3D8 => ApduType::PropertyDescriptionRead,
        0x3D9 => ApduType::PropertyDescriptionResponse,
        0x3DC => ApduType::IndividualAddressSerialNumberRead,
        0x3DD => ApduType::IndividualAddressSerialNumberResponse,
        0x3DE => ApduType::IndividualAddressSerialNumberWrite,
        0x3E0 => ApduType::DomainAddressWrite,
        0x3E1 => ApduType::DomainAddressRead,
        0x3E2 => ApduType::DomainAddressResponse,
        0x3E3 => ApduType::DomainAddressSelectiveRead,
        0x3EC => ApduType::DomainAddressSerialNumberRead,
        0x3ED => ApduType::DomainAddressSerialNumberResponse,
        0x3EE => ApduType::DomainAddressSerialNumberWrite,
        0x3F1 => ApduType::SecureService,
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_group_value_write_short() {
        // GroupValueWrite with value=1 (short APDU, npdu_length=1)
        let payload = &[0x00, 0x81]; // TPCI=0x00, APCI=0x0080 | data=0x01
        let apdu = Apdu::parse(payload, 1).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::GroupValueWrite);
        assert_eq!(apdu.data, &[0x01]);
    }

    #[test]
    fn parse_group_value_read() {
        let payload = &[0x00, 0x00]; // GroupValueRead
        let apdu = Apdu::parse(payload, 0).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::GroupValueRead);
    }

    #[test]
    fn parse_group_value_response_short() {
        let payload = &[0x00, 0x41]; // GroupValueResponse, value=1
        let apdu = Apdu::parse(payload, 1).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::GroupValueResponse);
        assert_eq!(apdu.data, &[0x01]);
    }

    #[test]
    fn parse_group_value_write_long() {
        // GroupValueWrite with 2-byte DPT9 value (npdu_length=3)
        let payload = &[0x00, 0x80, 0x0C, 0x1A];
        let apdu = Apdu::parse(payload, 3).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::GroupValueWrite);
        assert_eq!(apdu.data, &[0x0C, 0x1A]);
    }

    #[test]
    fn roundtrip_short_apdu() {
        let apdu = Apdu {
            apdu_type: ApduType::GroupValueWrite,
            data: alloc::vec![0x01],
        };
        let bytes = apdu.to_bytes(0x00);
        assert_eq!(bytes, &[0x00, 0x81]);

        let parsed = Apdu::parse(&bytes, 1).unwrap();
        assert_eq!(parsed.apdu_type, ApduType::GroupValueWrite);
        assert_eq!(parsed.data, &[0x01]);
    }

    #[test]
    fn roundtrip_long_apdu() {
        let apdu = Apdu {
            apdu_type: ApduType::GroupValueWrite,
            data: alloc::vec![0x0C, 0x1A],
        };
        let bytes = apdu.to_bytes(0x00);
        assert_eq!(bytes, &[0x00, 0x80, 0x0C, 0x1A]);
    }

    #[test]
    fn parse_property_value_read() {
        // PropertyValueRead = 0x3D5 — long APCI
        let payload = &[0x03, 0xD5, 0x01, 0x02, 0x03];
        let apdu = Apdu::parse(payload, 4).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::PropertyValueRead);
        assert_eq!(apdu.data, &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn parse_device_descriptor_read() {
        // DeviceDescriptorRead = 0x300, descriptor type in lower 6 bits
        let payload = &[0x03, 0x00];
        let apdu = Apdu::parse(payload, 1).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::DeviceDescriptorRead);
    }

    #[test]
    fn parse_too_short() {
        assert!(Apdu::parse(&[0x00], 0).is_none());
        assert!(Apdu::parse(&[], 0).is_none());
    }

    #[test]
    fn apdu_type_from_raw_masks_short_inline_data() {
        // 0x081 = GroupValueWrite (0x080) + 1 bit of inline data. Must resolve
        // to GroupValueWrite, not None (regression for missing short-APCI mask).
        assert_eq!(ApduType::from_raw(0x081), Some(ApduType::GroupValueWrite));
        assert_eq!(
            ApduType::from_raw(0x041),
            Some(ApduType::GroupValueResponse)
        );
        // Unmasked extra high bits (e.g. a TPCI byte) must not defeat matching.
        assert_eq!(ApduType::from_raw(0xC081), Some(ApduType::GroupValueWrite));
        // Long APCIs still resolve exactly.
        assert_eq!(ApduType::from_raw(0x3D5), Some(ApduType::PropertyValueRead));
    }

    #[test]
    fn roundtrip_short_apdu_empty_data() {
        // A short APCI with empty data encodes as the short form (value 0) and
        // decodes back to a single zero byte — encode/decode share one boundary.
        let apdu = Apdu {
            apdu_type: ApduType::GroupValueWrite,
            data: Vec::new(),
        };
        let bytes = apdu.to_bytes(0x00);
        assert_eq!(bytes, &[0x00, 0x80]);
        let parsed = Apdu::parse(&bytes, 1).unwrap();
        assert_eq!(parsed.apdu_type, ApduType::GroupValueWrite);
        assert_eq!(parsed.data, &[0x00]);
    }

    #[test]
    fn single_byte_over_6bit_uses_long_form() {
        // A full-octet value (e.g. DPT 5 = 200) must not be short-encoded, which
        // would mask off its high bits; it uses the long form and round-trips.
        let apdu = Apdu {
            apdu_type: ApduType::GroupValueWrite,
            data: alloc::vec![0xC8],
        };
        let bytes = apdu.to_bytes(0x00);
        assert_eq!(bytes, &[0x00, 0x80, 0xC8]);
        let parsed = Apdu::parse(&bytes, 2).unwrap();
        assert_eq!(parsed.data, &[0xC8]);
    }

    #[test]
    fn to_apci_bytes_matches_discriminant() {
        assert_eq!(ApduType::GroupValueWrite.to_apci_bytes(), [0x00, 0x80]);
        assert_eq!(ApduType::PropertyValueRead.to_apci_bytes(), [0x03, 0xD5]);
    }

    #[test]
    fn memory_write_keeps_count_byte_and_aligns_address() {
        // A_Memory_Write, count=6 @ 0x1234 with 6 data bytes. The count packs into
        // byte 1's low 6 bits (0x80 | 6 = 0x86); the address and data follow.
        // Byte 1 MUST survive into `data` so the parser reads count at data[0] and
        // the address at data[1..3]. The buggy path stripped both APCI bytes,
        // shifting count/address/data by one octet — the core download corruption.
        let payload = [0x02, 0x86, 0x12, 0x34, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let apdu = Apdu::parse(&payload, u8::try_from(payload.len() - 1).unwrap()).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::MemoryWrite);
        assert_eq!(
            apdu.data,
            &[0x86, 0x12, 0x34, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
        );
        assert_eq!(apdu.data[0] & 0x3F, 6, "count preserved in byte 1");
        assert_eq!(&apdu.data[1..3], &[0x12, 0x34], "address not shifted");
    }

    #[test]
    fn memory_read_keeps_count_byte() {
        // A_Memory_Read, count=3 @ 0x0010 → [0x02, 0x03, 0x00, 0x10].
        let apdu = Apdu::parse(&[0x02, 0x03, 0x00, 0x10], 3).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::MemoryRead);
        assert_eq!(apdu.data, &[0x03, 0x00, 0x10]);
    }

    #[test]
    fn adc_read_keeps_channel_byte() {
        // A_ADC_Read, channel=5, read-count=8 → [0x01, 0x85, 0x08]. The channel
        // packs into byte 1's low 6 bits and must survive at data[0].
        let apdu = Apdu::parse(&[0x01, 0x85, 0x08], 2).unwrap();
        assert_eq!(apdu.apdu_type, ApduType::AdcRead);
        assert_eq!(apdu.data, &[0x85, 0x08]);
        assert_eq!(apdu.data[0] & 0x3F, 5, "channel preserved in byte 1");
    }
}
