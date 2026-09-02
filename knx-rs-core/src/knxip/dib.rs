// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP Description Information Blocks (DIBs).
//!
//! DIBs are length-prefixed structures used by KNXnet/IP discovery and
//! description services. This module validates the common envelope once and
//! exposes typed access to the fixed-layout Device Information DIB.

use alloc::string::String;
use core::fmt;
use core::iter::FusedIterator;

/// Size of the common DIB header: structure length and description type.
pub const DIB_HEADER_LEN: usize = 2;

/// KNXnet/IP DIB description type code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum DibType {
    /// Device information.
    DeviceInformation = 0x01,
    /// Supported service families.
    SupportedServiceFamilies = 0x02,
    /// Configured IP parameters.
    IpConfiguration = 0x03,
    /// Current IP parameters.
    IpCurrentConfiguration = 0x04,
    /// Additional KNX individual addresses.
    KnxAddresses = 0x05,
    /// Secured service families.
    SecuredServiceFamilies = 0x06,
    /// Tunneling slot information.
    TunnelingInformation = 0x07,
    /// Extended device information.
    ExtendedDeviceInformation = 0x08,
    /// Manufacturer-specific data.
    ManufacturerData = 0xFE,
}

impl DibType {
    /// Convert a wire value to a known DIB type.
    pub const fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            0x01 => Self::DeviceInformation,
            0x02 => Self::SupportedServiceFamilies,
            0x03 => Self::IpConfiguration,
            0x04 => Self::IpCurrentConfiguration,
            0x05 => Self::KnxAddresses,
            0x06 => Self::SecuredServiceFamilies,
            0x07 => Self::TunnelingInformation,
            0x08 => Self::ExtendedDeviceInformation,
            0xFE => Self::ManufacturerData,
            _ => return None,
        })
    }

    /// Return the description type code used on the wire.
    pub const fn to_raw(self) -> u8 {
        self as u8
    }
}

/// KNX medium code carried by a Device Information DIB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum KnxMedium {
    /// Twisted Pair 1.
    Tp1 = 0x02,
    /// Powerline 110 kHz.
    Pl110 = 0x04,
    /// KNX radio frequency.
    RadioFrequency = 0x10,
    /// KNX IP.
    Ip = 0x20,
}

impl KnxMedium {
    /// Convert a wire value to a known KNX medium.
    pub const fn from_raw(raw: u8) -> Option<Self> {
        Some(match raw {
            0x02 => Self::Tp1,
            0x04 => Self::Pl110,
            0x10 => Self::RadioFrequency,
            0x20 => Self::Ip,
            _ => return None,
        })
    }

    /// Return the medium code used on the wire.
    pub const fn to_raw(self) -> u8 {
        self as u8
    }
}

/// Error returned while parsing one or more KNXnet/IP DIBs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DibParseError {
    /// Fewer than two bytes remain for a DIB header.
    TruncatedHeader {
        /// Offset of the incomplete header within the DIB sequence.
        offset: usize,
        /// Number of bytes available at that offset.
        remaining: usize,
    },
    /// A structure length is smaller than the common two-byte header.
    InvalidStructureLength {
        /// Offset of the DIB within the sequence.
        offset: usize,
        /// Invalid length from the wire.
        length: usize,
    },
    /// A structure length is odd, contrary to the DIB wire format.
    OddStructureLength {
        /// Offset of the DIB within the sequence.
        offset: usize,
        /// Invalid length from the wire.
        length: usize,
    },
    /// The declared DIB does not fit in the remaining input.
    TruncatedStructure {
        /// Offset of the DIB within the sequence.
        offset: usize,
        /// Length declared by the DIB header.
        declared: usize,
        /// Number of bytes available at that offset.
        remaining: usize,
    },
    /// A typed parser received a DIB with another description type.
    UnexpectedDescriptionType {
        /// Required type code.
        expected: u8,
        /// Type code found on the wire.
        actual: u8,
    },
    /// A Device Information DIB does not have its required fixed size.
    InvalidDeviceInformationLength {
        /// Length found on the wire.
        actual: usize,
    },
    /// A DIB sequence contains the same description type more than once.
    DuplicateDescriptionType {
        /// Repeated description type code.
        type_code: u8,
    },
}

impl fmt::Display for DibParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { offset, remaining } => write!(
                f,
                "truncated DIB header at offset {offset}: {remaining} byte(s) remain"
            ),
            Self::InvalidStructureLength { offset, length } => write!(
                f,
                "invalid DIB structure length {length} at offset {offset}"
            ),
            Self::OddStructureLength { offset, length } => {
                write!(f, "odd DIB structure length {length} at offset {offset}")
            }
            Self::TruncatedStructure {
                offset,
                declared,
                remaining,
            } => write!(
                f,
                "truncated DIB at offset {offset}: declares {declared} bytes, {remaining} remain"
            ),
            Self::UnexpectedDescriptionType { expected, actual } => write!(
                f,
                "unexpected DIB description type {actual:#04x}, expected {expected:#04x}"
            ),
            Self::InvalidDeviceInformationLength { actual } => write!(
                f,
                "invalid Device Information DIB length {actual}, expected {}",
                DeviceInformationDib::LEN
            ),
            Self::DuplicateDescriptionType { type_code } => {
                write!(f, "duplicate DIB description type {type_code:#04x}")
            }
        }
    }
}

impl core::error::Error for DibParseError {}

/// A validated, borrowed KNXnet/IP Description Information Block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dib<'a> {
    bytes: &'a [u8],
}

impl<'a> Dib<'a> {
    /// Return the complete DIB, including its two-byte header.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Return the validated structure length.
    pub const fn structure_length(&self) -> usize {
        self.bytes.len()
    }

    /// Return the description type code exactly as received.
    pub const fn description_type_raw(&self) -> u8 {
        self.bytes[1]
    }

    /// Return the known description type, or `None` for an extension unknown
    /// to this crate version.
    pub const fn description_type(&self) -> Option<DibType> {
        DibType::from_raw(self.description_type_raw())
    }

    /// Return the information block data after the common header.
    pub const fn data(&self) -> &'a [u8] {
        self.bytes.split_at(DIB_HEADER_LEN).1
    }
}

/// Iterator over a contiguous sequence of KNXnet/IP DIBs.
///
/// The iterator stops permanently after the first malformed structure so a
/// zero or truncated length cannot cause an infinite loop.
#[derive(Debug, Clone)]
pub struct DibIterator<'a> {
    remaining: &'a [u8],
    offset: usize,
    failed: bool,
}

impl<'a> DibIterator<'a> {
    /// Create an iterator over a DIB sequence.
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            remaining: bytes,
            offset: 0,
            failed: false,
        }
    }

    const fn fail(&mut self, error: DibParseError) -> Result<Dib<'a>, DibParseError> {
        self.failed = true;
        self.remaining = &[];
        Err(error)
    }
}

impl<'a> Iterator for DibIterator<'a> {
    type Item = Result<Dib<'a>, DibParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining.is_empty() {
            return None;
        }
        if self.remaining.len() < DIB_HEADER_LEN {
            return Some(self.fail(DibParseError::TruncatedHeader {
                offset: self.offset,
                remaining: self.remaining.len(),
            }));
        }

        let length = usize::from(self.remaining[0]);
        if length < DIB_HEADER_LEN {
            return Some(self.fail(DibParseError::InvalidStructureLength {
                offset: self.offset,
                length,
            }));
        }
        if length % 2 != 0 {
            return Some(self.fail(DibParseError::OddStructureLength {
                offset: self.offset,
                length,
            }));
        }
        if length > self.remaining.len() {
            return Some(self.fail(DibParseError::TruncatedStructure {
                offset: self.offset,
                declared: length,
                remaining: self.remaining.len(),
            }));
        }

        let (bytes, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        self.offset += length;
        Some(Ok(Dib { bytes }))
    }
}

impl FusedIterator for DibIterator<'_> {}

/// A completely validated sequence of KNXnet/IP DIBs.
///
/// Parsing validates every structure and rejects duplicate description type
/// codes. Unknown types remain accessible so later protocol extensions do not
/// make otherwise well-formed sequences unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DibSequence<'a> {
    bytes: &'a [u8],
}

/// Iterator over an already validated [`DibSequence`].
#[derive(Debug, Clone)]
pub struct DibSequenceIterator<'a> {
    remaining: &'a [u8],
}

impl<'a> Iterator for DibSequenceIterator<'a> {
    type Item = Dib<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }

        let length = usize::from(self.remaining[0]);
        let (bytes, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        Some(Dib { bytes })
    }
}

impl FusedIterator for DibSequenceIterator<'_> {}

impl<'a> DibSequence<'a> {
    /// Parse and validate a complete DIB sequence.
    ///
    /// # Errors
    ///
    /// Returns [`DibParseError`] if any structure is malformed or a
    /// description type occurs more than once.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, DibParseError> {
        let mut seen_types = [0_u64; 4];
        for dib in DibIterator::new(bytes) {
            let dib = dib?;
            let type_code = dib.description_type_raw();
            let type_index = usize::from(type_code);
            let word = type_index / u64::BITS as usize;
            let mask = 1_u64 << (type_index % u64::BITS as usize);
            if seen_types[word] & mask != 0 {
                return Err(DibParseError::DuplicateDescriptionType { type_code });
            }
            seen_types[word] |= mask;
        }
        Ok(Self { bytes })
    }

    /// Return the validated wire representation.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Return whether the sequence contains no DIBs.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Iterate over the validated DIBs in wire order.
    pub const fn iter(&self) -> DibSequenceIterator<'a> {
        DibSequenceIterator {
            remaining: self.bytes,
        }
    }

    /// Find a DIB by a known description type.
    pub fn get(&self, description_type: DibType) -> Option<Dib<'a>> {
        self.get_raw(description_type.to_raw())
    }

    /// Find a DIB by its raw description type code.
    pub fn get_raw(&self, type_code: u8) -> Option<Dib<'a>> {
        self.iter()
            .find(|dib| dib.description_type_raw() == type_code)
    }
}

impl<'a> IntoIterator for &DibSequence<'a> {
    type Item = Dib<'a>;
    type IntoIter = DibSequenceIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

const KNX_MEDIUM_OFFSET: usize = DIB_HEADER_LEN;
const DEVICE_STATUS_OFFSET: usize = KNX_MEDIUM_OFFSET + 1;
const INDIVIDUAL_ADDRESS_OFFSET: usize = DEVICE_STATUS_OFFSET + 1;
const INDIVIDUAL_ADDRESS_LEN: usize = 2;
const PROJECT_INSTALLATION_ID_OFFSET: usize = INDIVIDUAL_ADDRESS_OFFSET + INDIVIDUAL_ADDRESS_LEN;
const PROJECT_INSTALLATION_ID_LEN: usize = 2;
const SERIAL_NUMBER_OFFSET: usize = PROJECT_INSTALLATION_ID_OFFSET + PROJECT_INSTALLATION_ID_LEN;
const SERIAL_NUMBER_LEN: usize = 6;
const ROUTING_MULTICAST_OFFSET: usize = SERIAL_NUMBER_OFFSET + SERIAL_NUMBER_LEN;
const ROUTING_MULTICAST_LEN: usize = 4;
const MAC_ADDRESS_OFFSET: usize = ROUTING_MULTICAST_OFFSET + ROUTING_MULTICAST_LEN;
const MAC_ADDRESS_LEN: usize = 6;
const FRIENDLY_NAME_OFFSET: usize = MAC_ADDRESS_OFFSET + MAC_ADDRESS_LEN;
const FRIENDLY_NAME_LEN: usize = 30;

/// Typed view of a fixed-layout KNXnet/IP Device Information DIB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInformationDib<'a> {
    dib: Dib<'a>,
}

impl<'a> DeviceInformationDib<'a> {
    /// Required Device Information DIB length on the wire.
    pub const LEN: usize = FRIENDLY_NAME_OFFSET + FRIENDLY_NAME_LEN;

    /// Device Information description type.
    pub const TYPE: DibType = DibType::DeviceInformation;

    /// Parse a validated generic DIB as Device Information.
    ///
    /// # Errors
    ///
    /// Returns [`DibParseError`] if the description type or fixed structure
    /// length is invalid.
    pub const fn parse(dib: Dib<'a>) -> Result<Self, DibParseError> {
        if dib.description_type_raw() != Self::TYPE.to_raw() {
            return Err(DibParseError::UnexpectedDescriptionType {
                expected: Self::TYPE.to_raw(),
                actual: dib.description_type_raw(),
            });
        }
        if dib.structure_length() != Self::LEN {
            return Err(DibParseError::InvalidDeviceInformationLength {
                actual: dib.structure_length(),
            });
        }
        Ok(Self { dib })
    }

    /// Find and parse the Device Information block in a validated DIB sequence.
    ///
    /// Unknown DIB types are retained for forward compatibility. The complete
    /// sequence is validated, including structures after Device Information.
    ///
    /// # Errors
    ///
    /// Returns [`DibParseError`] for malformed structures, an invalid Device
    /// Information block, or duplicate description types.
    pub fn find_in(bytes: &'a [u8]) -> Result<Option<Self>, DibParseError> {
        let sequence = DibSequence::parse(bytes)?;
        sequence.get(Self::TYPE).map(Self::parse).transpose()
    }

    /// Return the underlying validated generic DIB.
    pub const fn as_dib(&self) -> Dib<'a> {
        self.dib
    }

    /// Return the KNX medium code exactly as received.
    pub const fn knx_medium_raw(&self) -> u8 {
        self.dib.bytes[KNX_MEDIUM_OFFSET]
    }

    /// Return the known KNX medium, or `None` for a future extension.
    pub const fn knx_medium(&self) -> Option<KnxMedium> {
        KnxMedium::from_raw(self.knx_medium_raw())
    }

    /// Return the device status byte.
    pub const fn device_status(&self) -> u8 {
        self.dib.bytes[DEVICE_STATUS_OFFSET]
    }

    /// Return whether the programming-mode bit is set.
    pub const fn is_programming_mode(&self) -> bool {
        self.device_status() & 0x01 != 0
    }

    /// Return the KNX individual address in wire representation.
    pub const fn individual_address(&self) -> u16 {
        u16::from_be_bytes([
            self.dib.bytes[INDIVIDUAL_ADDRESS_OFFSET],
            self.dib.bytes[INDIVIDUAL_ADDRESS_OFFSET + 1],
        ])
    }

    /// Return the combined project-installation identifier.
    pub const fn project_installation_identifier(&self) -> u16 {
        u16::from_be_bytes([
            self.dib.bytes[PROJECT_INSTALLATION_ID_OFFSET],
            self.dib.bytes[PROJECT_INSTALLATION_ID_OFFSET + 1],
        ])
    }

    /// Return the 12-bit KNX project number.
    pub const fn project_number(&self) -> u16 {
        self.project_installation_identifier() >> 4
    }

    /// Return the four-bit KNX installation number.
    pub const fn installation_number(&self) -> u8 {
        self.dib.bytes[PROJECT_INSTALLATION_ID_OFFSET + 1] & 0x0F
    }

    /// Return the six-byte KNX serial number.
    pub const fn serial_number(&self) -> [u8; SERIAL_NUMBER_LEN] {
        [
            self.dib.bytes[SERIAL_NUMBER_OFFSET],
            self.dib.bytes[SERIAL_NUMBER_OFFSET + 1],
            self.dib.bytes[SERIAL_NUMBER_OFFSET + 2],
            self.dib.bytes[SERIAL_NUMBER_OFFSET + 3],
            self.dib.bytes[SERIAL_NUMBER_OFFSET + 4],
            self.dib.bytes[SERIAL_NUMBER_OFFSET + 5],
        ]
    }

    /// Return the four-byte IPv4 routing multicast address.
    pub const fn routing_multicast_address(&self) -> [u8; ROUTING_MULTICAST_LEN] {
        [
            self.dib.bytes[ROUTING_MULTICAST_OFFSET],
            self.dib.bytes[ROUTING_MULTICAST_OFFSET + 1],
            self.dib.bytes[ROUTING_MULTICAST_OFFSET + 2],
            self.dib.bytes[ROUTING_MULTICAST_OFFSET + 3],
        ]
    }

    /// Return the six-byte Ethernet MAC address.
    pub const fn mac_address(&self) -> [u8; MAC_ADDRESS_LEN] {
        [
            self.dib.bytes[MAC_ADDRESS_OFFSET],
            self.dib.bytes[MAC_ADDRESS_OFFSET + 1],
            self.dib.bytes[MAC_ADDRESS_OFFSET + 2],
            self.dib.bytes[MAC_ADDRESS_OFFSET + 3],
            self.dib.bytes[MAC_ADDRESS_OFFSET + 4],
            self.dib.bytes[MAC_ADDRESS_OFFSET + 5],
        ]
    }

    /// Return the fixed 30-byte friendly-name field without decoding it.
    pub const fn friendly_name_bytes(&self) -> &'a [u8] {
        self.dib
            .bytes
            .split_at(FRIENDLY_NAME_OFFSET)
            .1
            .split_at(FRIENDLY_NAME_LEN)
            .0
    }

    /// Decode the null-terminated ISO-8859-1 device friendly name.
    pub fn friendly_name(&self) -> String {
        self.friendly_name_bytes()
            .iter()
            .copied()
            .take_while(|byte| *byte != 0)
            .map(char::from)
            .collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::unwrap_used
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn device_information(name: &[u8]) -> Vec<u8> {
        let mut dib = vec![0; DeviceInformationDib::LEN];
        dib[0] = DeviceInformationDib::LEN as u8;
        dib[1] = DibType::DeviceInformation.to_raw();
        dib[2] = 0x02;
        dib[3] = 0x01;
        dib[4..6].copy_from_slice(&0x1100_u16.to_be_bytes());
        dib[6..8].copy_from_slice(&0x1234_u16.to_be_bytes());
        dib[8..14].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        dib[14..18].copy_from_slice(&[224, 0, 23, 12]);
        dib[18..24].copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x45, 0x46]);
        dib[24..24 + name.len()].copy_from_slice(name);
        dib
    }

    fn first_dib(bytes: &[u8]) -> Dib<'_> {
        DibIterator::new(bytes).next().unwrap().unwrap()
    }

    #[test]
    fn parses_every_device_information_field() {
        let bytes = device_information(b"Gira KNX/IP-Router");
        let device = DeviceInformationDib::parse(first_dib(&bytes)).unwrap();

        assert_eq!(device.as_dib().structure_length(), 54);
        assert_eq!(device.knx_medium_raw(), 0x02);
        assert_eq!(device.knx_medium(), Some(KnxMedium::Tp1));
        assert_eq!(device.device_status(), 0x01);
        assert!(device.is_programming_mode());
        assert_eq!(device.individual_address(), 0x1100);
        assert_eq!(device.project_installation_identifier(), 0x1234);
        assert_eq!(device.project_number(), 0x0123);
        assert_eq!(device.installation_number(), 0x04);
        assert_eq!(device.serial_number(), [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(device.routing_multicast_address(), [224, 0, 23, 12]);
        assert_eq!(device.mac_address(), [0x01, 0x02, 0x03, 0x04, 0x45, 0x46]);
        assert_eq!(device.friendly_name(), "Gira KNX/IP-Router");
    }

    #[test]
    fn decodes_latin1_and_stops_at_first_null() {
        let bytes = device_information(b"Ger\xE4t\0ignored");
        let device = DeviceInformationDib::parse(first_dib(&bytes)).unwrap();

        assert_eq!(device.friendly_name(), "Ger\u{e4}t");
    }

    #[test]
    fn decodes_full_length_name_without_null_terminator() {
        let name = b"123456789012345678901234567890";
        let bytes = device_information(name);
        let device = DeviceInformationDib::parse(first_dib(&bytes)).unwrap();

        assert_eq!(device.friendly_name_bytes(), name);
        assert_eq!(device.friendly_name(), "123456789012345678901234567890");
    }

    #[test]
    fn finds_device_information_after_other_dibs() {
        let mut bytes = vec![4, DibType::SupportedServiceFamilies.to_raw(), 0x02, 0x01];
        bytes.extend_from_slice(&device_information(b"Gateway"));
        bytes.extend_from_slice(&[2, 0xF0]);

        let device = DeviceInformationDib::find_in(&bytes).unwrap().unwrap();
        assert_eq!(device.friendly_name(), "Gateway");
    }

    #[test]
    fn accepts_well_formed_unknown_dib_types() {
        let bytes = [4, 0xF0, 0xAA, 0xBB];
        let sequence = DibSequence::parse(&bytes).unwrap();
        let dib = sequence.get_raw(0xF0).unwrap();

        assert_eq!(dib.description_type(), None);
        assert_eq!(dib.description_type_raw(), 0xF0);
        assert_eq!(dib.data(), [0xAA, 0xBB]);
        assert_eq!(sequence.as_bytes(), bytes);
        assert!(!sequence.is_empty());
    }

    #[test]
    fn rejects_truncated_header_once() {
        let mut dibs = DibIterator::new(&[0x02]);

        assert_eq!(
            dibs.next(),
            Some(Err(DibParseError::TruncatedHeader {
                offset: 0,
                remaining: 1,
            }))
        );
        assert_eq!(dibs.next(), None);
    }

    #[test]
    fn rejects_lengths_that_cannot_form_a_dib() {
        assert_eq!(
            DibIterator::new(&[0, 0x01]).next(),
            Some(Err(DibParseError::InvalidStructureLength {
                offset: 0,
                length: 0,
            }))
        );
        assert_eq!(
            DibIterator::new(&[1, 0x01]).next(),
            Some(Err(DibParseError::InvalidStructureLength {
                offset: 0,
                length: 1,
            }))
        );
        assert_eq!(
            DibIterator::new(&[3, 0x01, 0x00]).next(),
            Some(Err(DibParseError::OddStructureLength {
                offset: 0,
                length: 3,
            }))
        );
    }

    #[test]
    fn reports_truncated_structure_at_its_sequence_offset() {
        let bytes = [2, 0xF0, 6, 0xF1, 0xAA, 0xBB];
        let mut dibs = DibIterator::new(&bytes);

        assert!(dibs.next().unwrap().is_ok());
        assert_eq!(
            dibs.next(),
            Some(Err(DibParseError::TruncatedStructure {
                offset: 2,
                declared: 6,
                remaining: 4,
            }))
        );
    }

    #[test]
    fn rejects_wrong_device_information_type_and_length() {
        let mut wrong_type = device_information(b"Gateway");
        wrong_type[1] = DibType::SupportedServiceFamilies.to_raw();
        assert_eq!(
            DeviceInformationDib::parse(first_dib(&wrong_type)),
            Err(DibParseError::UnexpectedDescriptionType {
                expected: DibType::DeviceInformation.to_raw(),
                actual: DibType::SupportedServiceFamilies.to_raw(),
            })
        );

        let mut wrong_length = device_information(b"Gateway");
        wrong_length.extend_from_slice(&[0, 0]);
        wrong_length[0] = 56;
        assert_eq!(
            DeviceInformationDib::parse(first_dib(&wrong_length)),
            Err(DibParseError::InvalidDeviceInformationLength { actual: 56 })
        );
    }

    #[test]
    fn validates_the_complete_sequence_after_device_information() {
        let mut bytes = device_information(b"Gateway");
        bytes.extend_from_slice(&[4, 0xF0]);

        assert_eq!(
            DeviceInformationDib::find_in(&bytes),
            Err(DibParseError::TruncatedStructure {
                offset: DeviceInformationDib::LEN,
                declared: 4,
                remaining: 2,
            })
        );
    }

    #[test]
    fn rejects_every_truncated_device_information_prefix() {
        let bytes = device_information(b"Gateway");

        for end in 1..DeviceInformationDib::LEN {
            assert!(DeviceInformationDib::find_in(&bytes[..end]).is_err());
        }
        assert_eq!(DeviceInformationDib::find_in(&[]), Ok(None));
    }

    #[test]
    fn known_wire_enums_roundtrip() {
        for dib_type in [
            DibType::DeviceInformation,
            DibType::SupportedServiceFamilies,
            DibType::IpConfiguration,
            DibType::IpCurrentConfiguration,
            DibType::KnxAddresses,
            DibType::SecuredServiceFamilies,
            DibType::TunnelingInformation,
            DibType::ExtendedDeviceInformation,
            DibType::ManufacturerData,
        ] {
            assert_eq!(DibType::from_raw(dib_type.to_raw()), Some(dib_type));
        }
        assert_eq!(DibType::from_raw(0xF0), None);

        for medium in [
            KnxMedium::Tp1,
            KnxMedium::Pl110,
            KnxMedium::RadioFrequency,
            KnxMedium::Ip,
        ] {
            assert_eq!(KnxMedium::from_raw(medium.to_raw()), Some(medium));
        }
        assert_eq!(KnxMedium::from_raw(0xFF), None);
    }

    #[test]
    fn rejects_duplicate_description_types() {
        let mut bytes = device_information(b"First");
        bytes.extend_from_slice(&device_information(b"Second"));

        assert_eq!(
            DeviceInformationDib::find_in(&bytes),
            Err(DibParseError::DuplicateDescriptionType {
                type_code: DibType::DeviceInformation.to_raw(),
            })
        );
        assert_eq!(
            DibSequence::parse(&[2, 0xF0, 2, 0xF0]),
            Err(DibParseError::DuplicateDescriptionType { type_code: 0xF0 })
        );
    }
}
