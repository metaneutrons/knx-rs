// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Application Program Object.
//!
//! Holds the application program data downloaded by ETS, including
//! parameters and group object configuration. Object type 3.

use alloc::vec::Vec;

use crate::interface_object::{InterfaceObject, ObjectType};
use crate::property::{AccessLevel, DataProperty, LoadState, PropertyDataType, PropertyId};

/// Create a new application program object.
pub fn new_application_program_object() -> InterfaceObject {
    let mut obj = InterfaceObject::new(ObjectType::ApplicationProgram);

    // Program version (5 bytes, written by ETS)
    obj.add_property(DataProperty::new(
        PropertyId::ProgramVersion,
        true,
        PropertyDataType::Generic05,
        1,
        AccessLevel::WriteHigh,
        &[0u8; 5],
    ));

    // PEI type (always 0 for IP devices)
    obj.add_property(DataProperty::read_only(
        PropertyId::PeiType,
        PropertyDataType::UnsignedChar,
        &[0x00],
    ));

    // Load state control
    obj.add_property(DataProperty::read_write(
        PropertyId::LoadStateControl,
        PropertyDataType::UnsignedChar,
        &[LoadState::Unloaded as u8],
    ));

    // Table reference (pointer to application data in memory)
    obj.add_property(DataProperty::read_write(
        PropertyId::TableReference,
        PropertyDataType::UnsignedLong,
        &[0u8; 4],
    ));

    // MCB table (memory control block)
    obj.add_property(DataProperty::read_write(
        PropertyId::McbTable,
        PropertyDataType::Generic08,
        &[0u8; 8],
    ));

    // Error code
    obj.add_property(DataProperty::read_only(
        PropertyId::ErrorCode,
        PropertyDataType::UnsignedChar,
        &[0x00],
    ));

    obj
}

/// Read the load state of an application program object.
pub fn load_state(obj: &InterfaceObject) -> LoadState {
    let mut buf = Vec::new();
    obj.read_property(PropertyId::LoadStateControl, 1, 1, &mut buf);
    match buf.first().copied().unwrap_or(0) {
        1 => LoadState::Loaded,
        2 => LoadState::Loading,
        3 => LoadState::Error,
        4 => LoadState::Unloading,
        5 => LoadState::LoadCompleting,
        _ => LoadState::Unloaded,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn has_object_type() {
        let obj = new_application_program_object();
        let mut buf = Vec::new();
        obj.read_property(PropertyId::ObjectType, 1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0x03]); // OT_APPLICATION_PROG = 3
    }

    #[test]
    fn default_load_state_unloaded() {
        let obj = new_application_program_object();
        assert_eq!(load_state(&obj), LoadState::Unloaded);
    }

    #[test]
    fn write_program_version() {
        let mut obj = new_application_program_object();
        let version = [0x01, 0x02, 0x03, 0x04, 0x05];
        obj.write_property(PropertyId::ProgramVersion, 1, 1, &version);

        let mut buf = Vec::new();
        obj.read_property(PropertyId::ProgramVersion, 1, 1, &mut buf);
        assert_eq!(buf, version);
    }
}
