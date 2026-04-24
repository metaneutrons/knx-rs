// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Property type definitions matching the KNX specification.

/// Property data type (PDT). Determines the wire encoding size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PropertyDataType {
    /// Control (1 byte read, 10 bytes write).
    Control = 0x00,
    /// Signed 8-bit character.
    Char = 0x01,
    /// Unsigned 8-bit.
    UnsignedChar = 0x02,
    /// Signed 16-bit.
    Int = 0x03,
    /// Unsigned 16-bit.
    UnsignedInt = 0x04,
    /// KNX 16-bit float.
    KnxFloat = 0x05,
    /// Date (3 bytes).
    Date = 0x06,
    /// Time (3 bytes).
    Time = 0x07,
    /// Signed 32-bit.
    Long = 0x08,
    /// Unsigned 32-bit.
    UnsignedLong = 0x09,
    /// IEEE 754 32-bit float.
    Float = 0x0A,
    /// IEEE 754 64-bit float.
    Double = 0x0B,
    /// 10-byte character block.
    CharBlock = 0x0C,
    /// Poll group setting (3 bytes).
    PollGroupSetting = 0x0D,
    /// 5-byte short character block.
    ShortCharBlock = 0x0E,
    /// Date and time (8 bytes).
    DateTime = 0x0F,
    /// Variable length.
    VariableLength = 0x10,
    /// Generic 1 byte.
    Generic01 = 0x11,
    /// Generic 2 bytes.
    Generic02 = 0x12,
    /// Generic 3 bytes.
    Generic03 = 0x13,
    /// Generic 4 bytes.
    Generic04 = 0x14,
    /// Generic 5 bytes.
    Generic05 = 0x15,
    /// Generic 6 bytes.
    Generic06 = 0x16,
    /// Generic 7 bytes.
    Generic07 = 0x17,
    /// Generic 8 bytes.
    Generic08 = 0x18,
    /// Generic 9 bytes.
    Generic09 = 0x19,
    /// Generic 10 bytes.
    Generic10 = 0x1A,
    /// Function property.
    Function = 0x3E,
}

impl PropertyDataType {
    /// Size of one element in bytes for this data type.
    pub const fn size(self) -> u8 {
        match self {
            Self::Control | Self::Char | Self::UnsignedChar | Self::Generic01 => 1,
            Self::Int | Self::UnsignedInt | Self::KnxFloat | Self::Generic02 => 2,
            Self::Date | Self::Time | Self::PollGroupSetting | Self::Generic03 => 3,
            Self::Long | Self::UnsignedLong | Self::Float | Self::Generic04 => 4,
            Self::ShortCharBlock | Self::Generic05 => 5,
            Self::Generic06 => 6,
            Self::Generic07 => 7,
            Self::Double | Self::DateTime | Self::Generic08 => 8,
            Self::Generic09 => 9,
            Self::CharBlock | Self::Generic10 => 10,
            Self::VariableLength | Self::Function => 0, // variable
        }
    }
}

/// Property identifier. Values match the KNX specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PropertyId {
    /// Object type identifier.
    ObjectType = 1,
    /// Load state control.
    LoadStateControl = 5,
    /// Run state control.
    RunStateControl = 6,
    /// Table memory reference.
    TableReference = 7,
    /// Service control.
    ServiceControl = 8,
    /// Firmware revision.
    FirmwareRevision = 9,
    /// Device serial number.
    SerialNumber = 11,
    /// Manufacturer identifier.
    ManufacturerId = 12,
    /// Application program version.
    ProgramVersion = 13,
    /// Device control flags.
    DeviceControl = 14,
    /// Order information.
    OrderInfo = 15,
    /// PEI type.
    PeiType = 16,
    /// Port configuration.
    PortConfiguration = 17,
    /// Table data.
    Table = 23,
    /// Interface object version.
    Version = 25,
    /// Memory control block table.
    McbTable = 27,
    /// Error code.
    ErrorCode = 28,
    /// Object index.
    ObjectIndex = 29,
    /// Download counter.
    DownloadCounter = 30,
    /// Routing hop count.
    RoutingCount = 51,
    /// Programming mode flag.
    ProgMode = 54,
    /// Maximum APDU length.
    MaxApduLength = 56,
    /// Subnet address (high byte of individual address).
    SubnetAddr = 57,
    /// Device address (low byte of individual address).
    DeviceAddr = 58,
    /// Interface object list.
    IoList = 71,
    /// Hardware type identifier.
    HardwareType = 78,
    /// Device descriptor.
    DeviceDescriptor = 83,
}

impl TryFrom<u8> for PropertyId {
    type Error = u8;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            1 => Ok(Self::ObjectType),
            5 => Ok(Self::LoadStateControl),
            6 => Ok(Self::RunStateControl),
            7 => Ok(Self::TableReference),
            8 => Ok(Self::ServiceControl),
            9 => Ok(Self::FirmwareRevision),
            11 => Ok(Self::SerialNumber),
            12 => Ok(Self::ManufacturerId),
            13 => Ok(Self::ProgramVersion),
            14 => Ok(Self::DeviceControl),
            15 => Ok(Self::OrderInfo),
            16 => Ok(Self::PeiType),
            17 => Ok(Self::PortConfiguration),
            23 => Ok(Self::Table),
            25 => Ok(Self::Version),
            27 => Ok(Self::McbTable),
            28 => Ok(Self::ErrorCode),
            29 => Ok(Self::ObjectIndex),
            30 => Ok(Self::DownloadCounter),
            51 => Ok(Self::RoutingCount),
            54 => Ok(Self::ProgMode),
            56 => Ok(Self::MaxApduLength),
            57 => Ok(Self::SubnetAddr),
            58 => Ok(Self::DeviceAddr),
            71 => Ok(Self::IoList),
            78 => Ok(Self::HardwareType),
            83 => Ok(Self::DeviceDescriptor),
            _ => Err(v),
        }
    }
}

/// Load state of a table object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LoadState {
    /// Not loaded.
    Unloaded = 0,
    /// Successfully loaded.
    Loaded = 1,
    /// Loading in progress.
    Loading = 2,
    /// Error during loading.
    Error = 3,
    /// Unloading in progress.
    Unloading = 4,
    /// Load completing.
    LoadCompleting = 5,
}

impl From<u8> for LoadState {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Loaded,
            2 => Self::Loading,
            3 => Self::Error,
            4 => Self::Unloading,
            5 => Self::LoadCompleting,
            _ => Self::Unloaded,
        }
    }
}

/// Load events that trigger state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LoadEvent {
    /// No operation.
    Noop = 0,
    /// Start loading.
    StartLoading = 1,
    /// Loading completed.
    LoadCompleted = 2,
    /// Additional load controls.
    AdditionalLoadControls = 3,
    /// Unload.
    Unload = 4,
}

impl LoadEvent {
    /// Parse a load event from a raw byte value.
    pub const fn from_byte(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Noop),
            1 => Some(Self::StartLoading),
            2 => Some(Self::LoadCompleted),
            3 => Some(Self::AdditionalLoadControls),
            4 => Some(Self::Unload),
            _ => None,
        }
    }
}

/// Access level for property read/write.
///
/// Encodes both read and write access in a single byte:
/// high nibble = read level, low nibble = write level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AccessLevel {
    /// No restriction on read or write.
    None = 0x00,
    /// Free read, low write restriction.
    WriteLow = 0x01,
    /// Free read, medium write restriction.
    WriteMedium = 0x02,
    /// Free read, high write restriction.
    WriteHigh = 0x03,
}

/// Description of a property, returned to ETS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyDescription {
    /// Property identifier.
    pub id: PropertyId,
    /// Whether the property can be written.
    pub write_enable: bool,
    /// Data type.
    pub data_type: PropertyDataType,
    /// Maximum number of elements.
    pub max_elements: u16,
    /// Access level.
    pub access: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_data_type_sizes() {
        assert_eq!(PropertyDataType::UnsignedChar.size(), 1);
        assert_eq!(PropertyDataType::UnsignedInt.size(), 2);
        assert_eq!(PropertyDataType::UnsignedLong.size(), 4);
        assert_eq!(PropertyDataType::Generic05.size(), 5);
        assert_eq!(PropertyDataType::Generic10.size(), 10);
    }

    #[test]
    fn load_state_from_u8_roundtrip() {
        assert_eq!(LoadState::from(0), LoadState::Unloaded);
        assert_eq!(LoadState::from(1), LoadState::Loaded);
        assert_eq!(LoadState::from(2), LoadState::Loading);
        assert_eq!(LoadState::from(3), LoadState::Error);
        assert_eq!(LoadState::from(4), LoadState::Unloading);
        assert_eq!(LoadState::from(5), LoadState::LoadCompleting);
        // Unknown values map to Unloaded
        assert_eq!(LoadState::from(6), LoadState::Unloaded);
        assert_eq!(LoadState::from(255), LoadState::Unloaded);
    }

    #[test]
    fn load_event_from_byte() {
        assert_eq!(LoadEvent::from_byte(0), Some(LoadEvent::Noop));
        assert_eq!(LoadEvent::from_byte(1), Some(LoadEvent::StartLoading));
        assert_eq!(LoadEvent::from_byte(2), Some(LoadEvent::LoadCompleted));
        assert_eq!(
            LoadEvent::from_byte(3),
            Some(LoadEvent::AdditionalLoadControls)
        );
        assert_eq!(LoadEvent::from_byte(4), Some(LoadEvent::Unload));
        assert_eq!(LoadEvent::from_byte(5), None);
        assert_eq!(LoadEvent::from_byte(255), None);
    }
}
