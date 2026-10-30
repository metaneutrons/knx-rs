// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP Search Request Parameters (SRPs).
//!
//! A `SEARCH_REQUEST_EXTENDED` carries the discovery endpoint of the client and
//! after it any number of SRPs. They narrow the set of servers that answer
//! (by programming mode, MAC address or supported service) and say which
//! description blocks the client wants in the answer.
//!
//! An SRP is `[structure length, type, data…]`. The highest bit of the type
//! byte is the mandatory flag: a server answers a request only if it
//! understands and satisfies every mandatory SRP in it.

use alloc::vec::Vec;
use core::fmt;
use core::iter::FusedIterator;

use super::dib::{DibType, ServiceFamily};

/// Size of the SRP header: structure length and type.
pub const SRP_HEADER_LEN: usize = 2;

const MANDATORY_FLAG: u8 = 0x80;
const TYPE_MASK: u8 = 0x7F;

/// Length of the data of a select-by-MAC-address SRP.
pub const MAC_ADDRESS_DATA_LEN: usize = 6;
/// Length of the data of a select-by-service SRP: family and minimum version.
pub const SERVICE_DATA_LEN: usize = 2;

/// Kind of a search request parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum SrpType {
    /// Not a parameter: used to test how a server treats one it does not know.
    Invalid = 0x00,
    /// Only servers that are in programming mode answer. No data.
    SelectByProgrammingMode = 0x01,
    /// Only the server with this MAC address answers. Six bytes of data.
    SelectByMacAddress = 0x02,
    /// Only servers that support a service family in at least a version answer.
    /// Data: the family and the minimum version.
    SelectByService = 0x03,
    /// The description types the client wants in the answer. Data: one byte each,
    /// padded with a zero to an even structure length.
    RequestDibs = 0x04,
}

impl SrpType {
    /// Convert a wire value (without the mandatory flag) to a known type.
    pub const fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            0x00 => Self::Invalid,
            0x01 => Self::SelectByProgrammingMode,
            0x02 => Self::SelectByMacAddress,
            0x03 => Self::SelectByService,
            0x04 => Self::RequestDibs,
            _ => return None,
        })
    }

    /// Return the type code used on the wire.
    pub const fn to_raw(self) -> u8 {
        self as u8
    }
}

/// Error returned while parsing a sequence of SRPs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrpParseError {
    /// Fewer bytes than an SRP header are left.
    TooShort {
        /// Bytes left.
        remaining: usize,
    },
    /// The structure length is smaller than the header.
    LengthBelowHeader {
        /// Declared structure length.
        length: usize,
    },
    /// The structure length is larger than the bytes that are left.
    LengthExceedsData {
        /// Declared structure length.
        length: usize,
        /// Bytes left.
        remaining: usize,
    },
}

impl fmt::Display for SrpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { remaining } => {
                write!(f, "{remaining} byte(s) left, too short for an SRP header")
            }
            Self::LengthBelowHeader { length } => {
                write!(f, "SRP structure length {length} is below the header size")
            }
            Self::LengthExceedsData { length, remaining } => write!(
                f,
                "SRP structure length {length} exceeds the {remaining} byte(s) left"
            ),
        }
    }
}

impl core::error::Error for SrpParseError {}

/// One validated search request parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Srp<'a> {
    bytes: &'a [u8],
}

impl<'a> Srp<'a> {
    /// Return the complete structure, header included.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Whether the mandatory flag is set: the server must understand and
    /// satisfy this SRP to answer.
    pub const fn is_mandatory(&self) -> bool {
        self.bytes[1] & MANDATORY_FLAG != 0
    }

    /// Return the type code without the mandatory flag.
    pub const fn type_raw(&self) -> u8 {
        self.bytes[1] & TYPE_MASK
    }

    /// Return the known type, or `None` for one this crate does not know.
    pub const fn srp_type(&self) -> Option<SrpType> {
        SrpType::from_raw(self.type_raw())
    }

    /// Return the data after the header.
    pub fn data(&self) -> &'a [u8] {
        &self.bytes[SRP_HEADER_LEN..]
    }
}

/// A validated sequence of SRPs, as it follows the discovery endpoint in a
/// `SEARCH_REQUEST_EXTENDED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SrpSequence<'a> {
    bytes: &'a [u8],
}

impl<'a> SrpSequence<'a> {
    /// Validate the structure of every SRP in `bytes`. An empty slice is an
    /// empty sequence.
    ///
    /// # Errors
    ///
    /// Returns [`SrpParseError`] for a truncated header or a structure length
    /// that is below the header or beyond the data.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, SrpParseError> {
        let mut rest = bytes;
        while !rest.is_empty() {
            let (_, tail) = split_first(rest)?;
            rest = tail;
        }
        Ok(Self { bytes })
    }

    /// Return the bytes of the whole sequence.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Whether there is no SRP.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Iterate over the SRPs.
    pub const fn iter(&self) -> SrpIterator<'a> {
        SrpIterator { rest: self.bytes }
    }
}

impl<'a> IntoIterator for &SrpSequence<'a> {
    type Item = Srp<'a>;
    type IntoIter = SrpIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over the SRPs of a validated [`SrpSequence`].
#[derive(Debug, Clone)]
pub struct SrpIterator<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for SrpIterator<'a> {
    type Item = Srp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let (srp, tail) = split_first(self.rest).ok()?;
        self.rest = tail;
        Some(srp)
    }
}

impl FusedIterator for SrpIterator<'_> {}

fn split_first(bytes: &[u8]) -> Result<(Srp<'_>, &[u8]), SrpParseError> {
    if bytes.len() < SRP_HEADER_LEN {
        return Err(SrpParseError::TooShort {
            remaining: bytes.len(),
        });
    }
    let length = usize::from(bytes[0]);
    if length < SRP_HEADER_LEN {
        return Err(SrpParseError::LengthBelowHeader { length });
    }
    if length > bytes.len() {
        return Err(SrpParseError::LengthExceedsData {
            length,
            remaining: bytes.len(),
        });
    }
    let (srp, tail) = bytes.split_at(length);
    Ok((Srp { bytes: srp }, tail))
}

// ── Building SRPs ────────────────────────────────────────────────────────────

fn srp(srp_type: SrpType, mandatory: bool, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SRP_HEADER_LEN + data.len());
    // Every caller passes at most 253 bytes of data, so the length fits in a byte.
    bytes.push(u8::try_from(SRP_HEADER_LEN + data.len()).unwrap_or(u8::MAX));
    bytes.push(srp_type.to_raw() | if mandatory { MANDATORY_FLAG } else { 0 });
    bytes.extend_from_slice(data);
    bytes
}

/// An SRP that limits the answers to servers in programming mode (mandatory).
pub fn programming_mode_srp() -> Vec<u8> {
    srp(SrpType::SelectByProgrammingMode, true, &[])
}

/// An SRP that limits the answers to the server with this MAC address (mandatory).
pub fn mac_address_srp(mac_address: [u8; MAC_ADDRESS_DATA_LEN]) -> Vec<u8> {
    srp(SrpType::SelectByMacAddress, true, &mac_address)
}

/// An SRP that limits the answers to servers that support `family` in at least
/// `minimum_version` (mandatory).
pub fn service_srp(family: ServiceFamily, minimum_version: u8) -> Vec<u8> {
    srp(
        SrpType::SelectByService,
        true,
        &[family.to_raw(), minimum_version],
    )
}

/// An SRP that asks for these description blocks in the answer (not mandatory).
///
/// An odd number is padded with a zero, as the structure length has to be even.
/// `None` for an empty list: such an SRP is not valid.
pub fn request_dibs_srp(types: &[DibType]) -> Option<Vec<u8>> {
    if types.is_empty() {
        return None;
    }
    // 2 header bytes, and the padded data must fit into the length byte.
    let types = &types[..types.len().min(252)];
    let mut data: Vec<u8> = types.iter().map(|t| t.to_raw()).collect();
    if data.len() % 2 == 1 {
        data.push(0);
    }
    Some(srp(SrpType::RequestDibs, false, &data))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use alloc::vec;

    use super::*;

    // Encodings from XKNX's SRP tests (test/knxip_tests/srp_test.py), an independent
    // implementation of the same specification.
    #[test]
    fn the_builders_reproduce_the_reference_encodings() {
        assert_eq!(
            mac_address_srp([1, 2, 3, 4, 5, 6]),
            [0x08, 0x82, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            service_srp(ServiceFamily::Tunneling, 2),
            [0x04, 0x83, 0x04, 0x02]
        );
        assert_eq!(programming_mode_srp(), [0x02, 0x81]);
        assert_eq!(
            request_dibs_srp(&[
                DibType::SupportedServiceFamilies,
                DibType::TunnelingInformation
            ]),
            Some(vec![0x04, 0x04, 0x02, 0x07])
        );
        // An odd number of types is padded with a zero.
        assert_eq!(
            request_dibs_srp(&[DibType::SupportedServiceFamilies]),
            Some(vec![0x04, 0x04, 0x02, 0x00])
        );
        assert_eq!(request_dibs_srp(&[]), None);
    }

    #[test]
    fn the_reference_encodings_parse_back_to_what_was_built() {
        let bytes = [0x08, 0x82, 1, 2, 3, 4, 5, 6];
        let sequence = SrpSequence::parse(&bytes).unwrap();
        let srp = sequence.iter().next().unwrap();
        assert_eq!(srp.srp_type(), Some(SrpType::SelectByMacAddress));
        assert!(srp.is_mandatory());
        assert_eq!(srp.data(), [1, 2, 3, 4, 5, 6]);

        let bytes = [0x04, 0x04, 0x02, 0x07];
        let srp = SrpSequence::parse(&bytes).unwrap().iter().next().unwrap();
        assert_eq!(srp.srp_type(), Some(SrpType::RequestDibs));
        assert!(!srp.is_mandatory());
        assert_eq!(srp.data(), [0x02, 0x07]);
    }

    #[test]
    fn several_srps_are_read_in_order() {
        // The programming mode SRP followed by the MAC SRP of the reference request.
        let bytes = [0x02, 0x81, 0x08, 0x82, 1, 2, 3, 4, 5, 6];
        let sequence = SrpSequence::parse(&bytes).unwrap();
        let types: Vec<_> = sequence.iter().map(|s| s.srp_type()).collect();
        assert_eq!(
            types,
            [
                Some(SrpType::SelectByProgrammingMode),
                Some(SrpType::SelectByMacAddress)
            ]
        );
        assert!(!sequence.is_empty());
        assert!(SrpSequence::parse(&[]).unwrap().is_empty());
        assert_eq!(SrpSequence::parse(&[]).unwrap().iter().count(), 0);
    }

    #[test]
    fn an_unknown_type_is_kept_with_its_flag() {
        let bytes = [0x03, 0xFE, 0xAA];
        let srp = SrpSequence::parse(&bytes).unwrap().iter().next().unwrap();
        assert_eq!(srp.srp_type(), None);
        assert_eq!(srp.type_raw(), 0x7E);
        assert!(srp.is_mandatory());
        assert_eq!(srp.data(), [0xAA]);
    }

    #[test]
    fn a_malformed_structure_is_an_error() {
        // From the reference tests: a length beyond the data, and a lone length byte.
        assert_eq!(
            SrpSequence::parse(&[0x08, 0x82]),
            Err(SrpParseError::LengthExceedsData {
                length: 8,
                remaining: 2
            })
        );
        assert_eq!(
            SrpSequence::parse(&[0x08]),
            Err(SrpParseError::TooShort { remaining: 1 })
        );
        // A length below the header would never advance.
        assert_eq!(
            SrpSequence::parse(&[0x01, 0x81]),
            Err(SrpParseError::LengthBelowHeader { length: 1 })
        );
        assert_eq!(
            SrpSequence::parse(&[0x00, 0x00]),
            Err(SrpParseError::LengthBelowHeader { length: 0 })
        );
        // One good SRP does not excuse a bad one after it.
        assert!(SrpSequence::parse(&[0x02, 0x81, 0x05, 0x01]).is_err());
    }
}
