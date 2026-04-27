// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Application-layer types — `AppIndication` and `AppLayerError`.

use alloc::vec::Vec;

use knx_rs_core::message::ApduType;

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

impl core::fmt::Display for AppLayerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedApdu(t) => write!(f, "unsupported APDU type: {t:?}"),
            Self::TruncatedPayload { expected, got } => {
                write!(f, "truncated payload: expected {expected} bytes, got {got}")
            }
            Self::MalformedData => write!(f, "malformed APDU data"),
        }
    }
}

impl core::error::Error for AppLayerError {}

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
