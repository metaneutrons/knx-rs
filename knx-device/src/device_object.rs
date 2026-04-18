// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Device Object — the mandatory root interface object.
//!
//! Every KNX device has exactly one device object (object type 0).
//! It holds the individual address, serial number, manufacturer ID,
//! programming mode flag, and other device-level configuration.

use crate::interface_object::{InterfaceObject, ObjectType};
use crate::property::{AccessLevel, DataProperty, PropertyDataType, PropertyId};

/// Create a new device object with standard KNX properties.
///
/// `serial_number` is 6 bytes, `hardware_type` is 6 bytes.
pub fn new_device_object(serial_number: [u8; 6], hardware_type: [u8; 6]) -> InterfaceObject {
    let mut obj = InterfaceObject::new(ObjectType::Device);

    obj.add_property(DataProperty::new(
        PropertyId::SerialNumber,
        false,
        PropertyDataType::Generic06,
        1,
        AccessLevel::None,
        &serial_number,
    ));

    // Manufacturer ID: derived from serial number bytes 0-1
    obj.add_property(DataProperty::read_only(
        PropertyId::ManufacturerId,
        PropertyDataType::UnsignedInt,
        &serial_number[..2],
    ));

    // Device control (bitset)
    obj.add_property(DataProperty::read_write(
        PropertyId::DeviceControl,
        PropertyDataType::UnsignedChar,
        &[0x00],
    ));

    // Order info (10 bytes, initially empty)
    obj.add_property(DataProperty::new(
        PropertyId::OrderInfo,
        false,
        PropertyDataType::Generic10,
        1,
        AccessLevel::None,
        &[0u8; 10],
    ));

    // Version
    obj.add_property(DataProperty::read_only(
        PropertyId::Version,
        PropertyDataType::UnsignedInt,
        &[0x00, 0x03], // version 3
    ));

    // Routing count (default hop count = 6)
    obj.add_property(DataProperty::read_write(
        PropertyId::RoutingCount,
        PropertyDataType::UnsignedChar,
        &[6 << 4],
    ));

    // Programming mode
    obj.add_property(DataProperty::read_write(
        PropertyId::ProgMode,
        PropertyDataType::UnsignedChar,
        &[0x00],
    ));

    // Max APDU length
    obj.add_property(DataProperty::read_only(
        PropertyId::MaxApduLength,
        PropertyDataType::UnsignedInt,
        &[0x00, 0xFE], // 254 bytes
    ));

    // Subnet address (high byte of individual address)
    obj.add_property(DataProperty::read_write(
        PropertyId::SubnetAddr,
        PropertyDataType::UnsignedChar,
        &[0xFF],
    ));

    // Device address (low byte of individual address)
    obj.add_property(DataProperty::read_write(
        PropertyId::DeviceAddr,
        PropertyDataType::UnsignedChar,
        &[0xFF],
    ));

    // Hardware type
    obj.add_property(DataProperty::new(
        PropertyId::HardwareType,
        false,
        PropertyDataType::Generic06,
        1,
        AccessLevel::None,
        &hardware_type,
    ));

    // Firmware revision
    obj.add_property(DataProperty::read_only(
        PropertyId::FirmwareRevision,
        PropertyDataType::UnsignedChar,
        &[0x01],
    ));

    obj
}

/// Helper: read the individual address from a device object.
pub fn individual_address(obj: &InterfaceObject) -> u16 {
    let mut subnet = alloc::vec::Vec::new();
    let mut device = alloc::vec::Vec::new();
    obj.read_property(PropertyId::SubnetAddr, 1, 1, &mut subnet);
    obj.read_property(PropertyId::DeviceAddr, 1, 1, &mut device);
    let s = subnet.first().copied().unwrap_or(0xFF);
    let d = device.first().copied().unwrap_or(0xFF);
    u16::from(s) << 8 | u16::from(d)
}

/// Helper: set the individual address on a device object.
pub fn set_individual_address(obj: &mut InterfaceObject, addr: u16) {
    let subnet = (addr >> 8) as u8;
    let device = (addr & 0xFF) as u8;
    obj.write_property(PropertyId::SubnetAddr, 1, 1, &[subnet]);
    obj.write_property(PropertyId::DeviceAddr, 1, 1, &[device]);
}

/// Helper: check if programming mode is active.
pub fn prog_mode(obj: &InterfaceObject) -> bool {
    let mut buf = alloc::vec::Vec::new();
    obj.read_property(PropertyId::ProgMode, 1, 1, &mut buf);
    buf.first().copied().unwrap_or(0) != 0
}

/// Helper: set programming mode.
pub fn set_prog_mode(obj: &mut InterfaceObject, enabled: bool) {
    obj.write_property(PropertyId::ProgMode, 1, 1, &[u8::from(enabled)]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn test_device() -> InterfaceObject {
        let serial = [0x00, 0xFA, 0x01, 0x02, 0x03, 0x04];
        let hw_type = [0x00; 6];
        new_device_object(serial, hw_type)
    }

    #[test]
    fn has_object_type() {
        let obj = test_device();
        let mut buf = alloc::vec::Vec::new();
        obj.read_property(PropertyId::ObjectType, 1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0x00]); // OT_DEVICE = 0
    }

    #[test]
    fn has_serial_number() {
        let obj = test_device();
        let mut buf = alloc::vec::Vec::new();
        obj.read_property(PropertyId::SerialNumber, 1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0xFA, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn individual_address_default() {
        let obj = test_device();
        assert_eq!(individual_address(&obj), 0xFFFF);
    }

    #[test]
    fn set_and_get_individual_address() {
        let mut obj = test_device();
        set_individual_address(&mut obj, 0x1101); // 1.1.1
        assert_eq!(individual_address(&obj), 0x1101);
    }

    #[test]
    fn prog_mode_default_off() {
        let obj = test_device();
        assert!(!prog_mode(&obj));
    }

    #[test]
    fn toggle_prog_mode() {
        let mut obj = test_device();
        set_prog_mode(&mut obj, true);
        assert!(prog_mode(&obj));
        set_prog_mode(&mut obj, false);
        assert!(!prog_mode(&obj));
    }

    #[test]
    fn max_apdu_length() {
        let obj = test_device();
        let mut buf = alloc::vec::Vec::new();
        obj.read_property(PropertyId::MaxApduLength, 1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0xFE]); // 254
    }
}
