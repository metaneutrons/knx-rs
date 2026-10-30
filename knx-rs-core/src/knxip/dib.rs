// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNXnet/IP Description Information Blocks (DIBs).
//!
//! DIBs are length-prefixed structures used by KNXnet/IP discovery and
//! description services. This module validates the common envelope once and
//! exposes typed access to the fixed-layout Device Information DIB.

use alloc::string::String;
use alloc::vec::Vec;
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

// ── Building DIBs ────────────────────────────────────────────────────────────

/// KNXnet/IP service family, as listed in a Supported Service Families DIB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum ServiceFamily {
    /// KNXnet/IP Core (discovery, description, connection management).
    Core = 0x02,
    /// KNXnet/IP Device Management.
    DeviceManagement = 0x03,
    /// KNXnet/IP Tunnelling.
    Tunneling = 0x04,
    /// KNXnet/IP Routing.
    Routing = 0x05,
    /// KNXnet/IP Remote Logging.
    RemoteLogging = 0x06,
    /// KNXnet/IP Remote Configuration and Diagnosis.
    RemoteConfiguration = 0x07,
    /// KNXnet/IP Object Server.
    ObjectServer = 0x08,
}

impl ServiceFamily {
    /// Return the service family code used on the wire.
    pub const fn to_raw(self) -> u8 {
        self as u8
    }
}

/// The fields of a Device Information DIB, for building one.
///
/// [`DeviceInformation::to_bytes`] writes the fixed 54-byte layout that
/// [`DeviceInformationDib`] reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInformation<'a> {
    /// KNX medium of the device.
    pub medium: KnxMedium,
    /// Whether the device is in programming mode (bit 0 of the device status).
    pub programming_mode: bool,
    /// KNX individual address in wire representation.
    pub individual_address: u16,
    /// Combined project-installation identifier (project number and installation number).
    pub project_installation_identifier: u16,
    /// Six-byte KNX serial number.
    pub serial_number: [u8; 6],
    /// IPv4 routing multicast address, all zeros if the device does not route.
    pub routing_multicast_address: [u8; 4],
    /// Six-byte Ethernet MAC address.
    pub mac_address: [u8; 6],
    /// Friendly name. It is written as ISO-8859-1, cut after 30 bytes, and a
    /// character outside that set becomes `?`.
    pub friendly_name: &'a str,
}

impl DeviceInformation<'_> {
    /// Serialize to the 54-byte Device Information DIB.
    pub fn to_bytes(&self) -> [u8; DeviceInformationDib::LEN] {
        let mut dib = [0u8; DeviceInformationDib::LEN];
        // The structure length is the constant 54, which fits in a byte.
        dib[0] = u8::try_from(DeviceInformationDib::LEN).unwrap_or(u8::MAX);
        dib[1] = DeviceInformationDib::TYPE.to_raw();
        dib[KNX_MEDIUM_OFFSET] = self.medium.to_raw();
        dib[DEVICE_STATUS_OFFSET] = u8::from(self.programming_mode);
        dib[INDIVIDUAL_ADDRESS_OFFSET..][..INDIVIDUAL_ADDRESS_LEN]
            .copy_from_slice(&self.individual_address.to_be_bytes());
        dib[PROJECT_INSTALLATION_ID_OFFSET..][..PROJECT_INSTALLATION_ID_LEN]
            .copy_from_slice(&self.project_installation_identifier.to_be_bytes());
        dib[SERIAL_NUMBER_OFFSET..][..SERIAL_NUMBER_LEN].copy_from_slice(&self.serial_number);
        dib[ROUTING_MULTICAST_OFFSET..][..ROUTING_MULTICAST_LEN]
            .copy_from_slice(&self.routing_multicast_address);
        dib[MAC_ADDRESS_OFFSET..][..MAC_ADDRESS_LEN].copy_from_slice(&self.mac_address);
        for (slot, ch) in dib[FRIENDLY_NAME_OFFSET..][..FRIENDLY_NAME_LEN]
            .iter_mut()
            .zip(self.friendly_name.chars())
        {
            *slot = u8::try_from(u32::from(ch)).unwrap_or(b'?');
        }
        dib
    }
}

/// How many service families fit into one DIB: its length is a single byte.
const MAX_SERVICE_FAMILIES: usize = (u8::MAX as usize - DIB_HEADER_LEN) / 2;

/// Build a Supported Service Families DIB from `(family, version)` pairs.
///
/// More than 126 pairs do not fit into one DIB and are left out.
pub fn supported_service_families_dib(families: &[(ServiceFamily, u8)]) -> Vec<u8> {
    let families = &families[..families.len().min(MAX_SERVICE_FAMILIES)];
    let mut dib = Vec::with_capacity(DIB_HEADER_LEN + 2 * families.len());
    // At most 2 + 2 * 126 = 254 bytes, so the length fits in a byte.
    dib.push(u8::try_from(DIB_HEADER_LEN + 2 * families.len()).unwrap_or(u8::MAX));
    dib.push(DibType::SupportedServiceFamilies.to_raw());
    for (family, version) in families {
        dib.push(family.to_raw());
        dib.push(*version);
    }
    dib
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

    // ── Building ────────────────────────────────────────────────

    // Cross-implementation vector from XKNX's SearchResponse test fixture (the same
    // one the discovery client is tested against).
    const XKNX_SEARCH_RESPONSE: [u8; 80] = [
        0x06, 0x10, 0x02, 0x02, 0x00, 0x50, 0x08, 0x01, 0xC0, 0xA8, 0x2A, 0x0A, 0x0E, 0x57, 0x36,
        0x01, 0x02, 0x00, 0x11, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0xE0, 0x00,
        0x17, 0x0C, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x47, 0x69, 0x72, 0x61, 0x20, 0x4B, 0x4E,
        0x58, 0x2F, 0x49, 0x50, 0x2D, 0x52, 0x6F, 0x75, 0x74, 0x65, 0x72, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x02, 0x02, 0x01, 0x03, 0x02, 0x04,
        0x01, 0x05, 0x01, 0x07, 0x01,
    ];

    fn router_information(name: &str) -> DeviceInformation<'_> {
        DeviceInformation {
            medium: KnxMedium::Tp1,
            programming_mode: false,
            individual_address: 0x1100,
            project_installation_identifier: 0,
            serial_number: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            routing_multicast_address: [224, 0, 23, 12],
            mac_address: [1, 2, 3, 4, 5, 6],
            friendly_name: name,
        }
    }

    #[test]
    fn the_builders_reproduce_a_reference_search_response_byte_for_byte() {
        let mut dibs = router_information("Gira KNX/IP-Router").to_bytes().to_vec();
        dibs.extend(supported_service_families_dib(&[
            (ServiceFamily::Core, 1),
            (ServiceFamily::DeviceManagement, 2),
            (ServiceFamily::Tunneling, 1),
            (ServiceFamily::Routing, 1),
            (ServiceFamily::RemoteConfiguration, 1),
        ]));
        let control = crate::knxip::Hpai {
            protocol: crate::knxip::HostProtocol::Ipv4Udp,
            ip: [192, 168, 42, 10],
            port: 3671,
        };
        let frame = crate::knxip::KnxIpFrame::search_response(control, &dibs);
        assert_eq!(frame.to_bytes(), XKNX_SEARCH_RESPONSE);
    }

    #[test]
    fn a_built_device_information_dib_reads_back_through_the_parser() {
        let information = DeviceInformation {
            medium: KnxMedium::Ip,
            programming_mode: true,
            individual_address: 0x1164,
            project_installation_identifier: 0x0123,
            serial_number: [0, 1, 2, 3, b'S', b'D'],
            routing_multicast_address: [0; 4],
            mac_address: [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10],
            friendly_name: "SnapDog",
        };
        let bytes = information.to_bytes();
        let dib = DeviceInformationDib::find_in(&bytes).unwrap().unwrap();
        assert_eq!(dib.knx_medium(), Some(KnxMedium::Ip));
        assert!(dib.is_programming_mode());
        assert_eq!(dib.individual_address(), 0x1164);
        assert_eq!(dib.project_installation_identifier(), 0x0123);
        assert_eq!(dib.serial_number(), [0, 1, 2, 3, b'S', b'D']);
        assert_eq!(dib.routing_multicast_address(), [0; 4]);
        assert_eq!(dib.mac_address(), [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10]);
        assert_eq!(dib.friendly_name(), "SnapDog");
        assert_eq!(
            DeviceInformation {
                programming_mode: false,
                ..information
            }
            .to_bytes()[DEVICE_STATUS_OFFSET],
            0
        );
    }

    #[test]
    fn the_friendly_name_is_latin1_padded_and_cut_after_30_bytes() {
        let name = |text: &str| router_information(text).to_bytes();
        // ISO-8859-1: "ü" is one byte, 0xFC, and decodes back.
        let bytes = name("Küche");
        assert_eq!(
            &bytes[FRIENDLY_NAME_OFFSET..FRIENDLY_NAME_OFFSET + 6],
            b"K\xFCche\0"
        );
        assert_eq!(
            DeviceInformationDib::find_in(&bytes)
                .unwrap()
                .unwrap()
                .friendly_name(),
            "Küche"
        );
        // What ISO-8859-1 cannot express becomes a question mark.
        let bytes = name("Zone \u{20AC}");
        assert_eq!(
            DeviceInformationDib::find_in(&bytes)
                .unwrap()
                .unwrap()
                .friendly_name(),
            "Zone ?"
        );
        // A longer name is cut; the field has no terminator then, and still parses.
        let long = "abcdefghijklmnopqrstuvwxyz0123456789";
        let bytes = name(long);
        assert_eq!(
            DeviceInformationDib::find_in(&bytes)
                .unwrap()
                .unwrap()
                .friendly_name(),
            &long[..FRIENDLY_NAME_LEN]
        );
        assert_eq!(bytes.len(), DeviceInformationDib::LEN);
    }

    #[test]
    fn a_built_service_families_dib_has_the_documented_layout() {
        let dib = supported_service_families_dib(&[
            (ServiceFamily::Core, 1),
            (ServiceFamily::Tunneling, 1),
        ]);
        assert_eq!(dib, [6, 0x02, 0x02, 1, 0x04, 1]);
        let sequence = DibSequence::parse(&dib).unwrap();
        assert!(sequence.get(DibType::SupportedServiceFamilies).is_some());
        assert_eq!(supported_service_families_dib(&[]), [2, 0x02]);
    }

    #[test]
    fn more_service_families_than_fit_are_left_out() {
        let many = vec![(ServiceFamily::Core, 1); 200];
        let dib = supported_service_families_dib(&many);
        assert_eq!(dib.len(), 254);
        assert_eq!(usize::from(dib[0]), dib.len());
        assert!(DibSequence::parse(&dib).is_ok());
    }

    #[test]
    fn the_discovery_frames_have_the_documented_bodies() {
        use crate::knxip::{Hpai, KnxIpFrame, ServiceType};
        let endpoint = Hpai::nat_udp(3671);
        for (frame, service) in [
            (
                KnxIpFrame::search_request(endpoint),
                ServiceType::SearchRequest,
            ),
            (
                KnxIpFrame::description_request(endpoint),
                ServiceType::DescriptionRequest,
            ),
        ] {
            assert_eq!(frame.service_type, service);
            assert_eq!(frame.body, endpoint.to_bytes());
            assert_eq!(KnxIpFrame::parse(&frame.to_bytes()).unwrap(), frame);
        }
        let dibs = router_information("x").to_bytes();
        let response = KnxIpFrame::description_response(&dibs);
        assert_eq!(response.service_type, ServiceType::DescriptionResponse);
        assert_eq!(response.body, dibs);
        let search = KnxIpFrame::search_response(endpoint, &dibs);
        assert_eq!(&search.body[..8], &endpoint.to_bytes());
        assert_eq!(&search.body[8..], &dibs);
    }

    // Extended search vectors from XKNX's search_request_extended_test.py and
    // search_response_extended_test.py.
    #[test]
    fn the_extended_search_frames_reproduce_the_reference_vectors() {
        use crate::knxip::srp::{mac_address_srp, programming_mode_srp};
        use crate::knxip::{HostProtocol, Hpai, KnxIpFrame, ServiceType};

        let multicast = Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [224, 0, 23, 12],
            port: 3671,
        };
        // No SRP.
        let request = KnxIpFrame::search_request_extended(multicast, &[]);
        assert_eq!(
            request.to_bytes(),
            [
                0x06, 0x10, 0x02, 0x0B, 0x00, 0x0E, 0x08, 0x01, 0xE0, 0x00, 0x17, 0x0C, 0x0E, 0x57
            ]
        );
        // The programming mode SRP.
        let request = KnxIpFrame::search_request_extended(multicast, &programming_mode_srp());
        assert_eq!(
            request.to_bytes(),
            [
                0x06, 0x10, 0x02, 0x0B, 0x00, 0x10, 0x08, 0x01, 0xE0, 0x00, 0x17, 0x0C, 0x0E, 0x57,
                0x02, 0x81
            ]
        );
        // Two SRPs, parsed back.
        let mut srps = programming_mode_srp();
        srps.extend(mac_address_srp([1, 2, 3, 4, 5, 6]));
        let request = KnxIpFrame::search_request_extended(multicast, &srps);
        assert_eq!(
            request.to_bytes(),
            [
                0x06, 0x10, 0x02, 0x0B, 0x00, 0x18, 0x08, 0x01, 0xE0, 0x00, 0x17, 0x0C, 0x0E, 0x57,
                0x02, 0x81, 0x08, 0x82, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06
            ]
        );
        let parsed = KnxIpFrame::parse(&request.to_bytes()).unwrap();
        assert_eq!(parsed.service_type, ServiceType::SearchRequestExtended);
        assert_eq!(parsed, request);

        // The extended response is the search response with another service type.
        let mut dibs = router_information("Gira KNX/IP-Router").to_bytes().to_vec();
        dibs.extend(supported_service_families_dib(&[
            (ServiceFamily::Core, 1),
            (ServiceFamily::DeviceManagement, 2),
            (ServiceFamily::Tunneling, 1),
            (ServiceFamily::Routing, 1),
            (ServiceFamily::RemoteConfiguration, 1),
        ]));
        let control = Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [192, 168, 42, 10],
            port: 3671,
        };
        let response = KnxIpFrame::search_response_extended(control, &dibs);
        let mut reference = XKNX_SEARCH_RESPONSE;
        reference[2..4].copy_from_slice(&0x020C_u16.to_be_bytes());
        assert_eq!(response.to_bytes(), reference);
        assert_eq!(response.service_type, ServiceType::SearchResponseExtended);
    }
}
