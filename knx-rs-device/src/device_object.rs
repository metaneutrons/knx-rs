// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Device Object — the mandatory root interface object.

use alloc::vec::Vec;

use knx_rs_core::address::IndividualAddress;

use crate::interface_object::{InterfaceObject, ObjectType};
use crate::property::{AccessLevel, DataProperty, PropertyDataType, PropertyId};

const DEVICE_VERSION: [u8; 2] = [0x00, 0x03];
const FIRMWARE_REVISION: u8 = 0x01;
/// Default routing hop count (6 hops, encoded in upper nibble per KNX 3/3/7).
const DEFAULT_ROUTING_COUNT: u8 = 6 << 4;
const MAX_APDU_LENGTH: [u8; 2] = [0x00, 0xFE];
const DEFAULT_SUBNET_ADDR: u8 = 0xFF;
const DEFAULT_DEVICE_ADDR: u8 = 0xFF;

/// Create a new device object with standard KNX properties.
pub fn new_device_object(serial_number: [u8; 6], hardware_type: [u8; 6]) -> InterfaceObject {
    let mut obj = InterfaceObject::new(ObjectType::Device);
    add_identity_properties(&mut obj, serial_number);
    add_config_properties(&mut obj, hardware_type);
    obj
}

fn add_identity_properties(obj: &mut InterfaceObject, serial: [u8; 6]) {
    obj.add_property(
        DataProperty::new(
            PropertyId::SerialNumber,
            false,
            PropertyDataType::Generic06,
            1,
            AccessLevel::None,
            &serial,
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_only(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &serial[..2],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_write(
            PropertyId::DeviceControl,
            PropertyDataType::UnsignedChar,
            &[0x00],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::new(
            PropertyId::OrderInfo,
            false,
            PropertyDataType::Generic10,
            1,
            AccessLevel::None,
            &[0u8; 10],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_only(
            PropertyId::Version,
            PropertyDataType::UnsignedInt,
            &DEVICE_VERSION,
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_only(
            PropertyId::FirmwareRevision,
            PropertyDataType::UnsignedChar,
            &[FIRMWARE_REVISION],
        )
        .into(),
    );
}

fn add_config_properties(obj: &mut InterfaceObject, hw_type: [u8; 6]) {
    obj.add_property(
        DataProperty::read_write(
            PropertyId::RoutingCount,
            PropertyDataType::UnsignedChar,
            &[DEFAULT_ROUTING_COUNT],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_write(
            PropertyId::ProgMode,
            PropertyDataType::UnsignedChar,
            &[0x00],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_only(
            PropertyId::MaxApduLength,
            PropertyDataType::UnsignedInt,
            &MAX_APDU_LENGTH,
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_write(
            PropertyId::SubnetAddr,
            PropertyDataType::UnsignedChar,
            &[DEFAULT_SUBNET_ADDR],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::read_write(
            PropertyId::DeviceAddr,
            PropertyDataType::UnsignedChar,
            &[DEFAULT_DEVICE_ADDR],
        )
        .into(),
    );
    obj.add_property(
        DataProperty::new(
            PropertyId::HardwareType,
            false,
            PropertyDataType::Generic06,
            1,
            AccessLevel::None,
            &hw_type,
        )
        .into(),
    );
    // Device descriptor / mask version (PID 83)
    obj.add_property(
        DataProperty::read_only(
            PropertyId::DeviceDescriptor,
            PropertyDataType::UnsignedInt,
            &crate::bau::MASK_VERSION_IP.to_be_bytes(),
        )
        .into(),
    );
}

/// Read the individual address from a device object.
///
/// Only valid for device-type interface objects (index 0).
pub fn individual_address(obj: &InterfaceObject) -> IndividualAddress {
    let mut subnet = Vec::new();
    let mut device = Vec::new();
    obj.read_property(PropertyId::SubnetAddr, 1, 1, &mut subnet);
    obj.read_property(PropertyId::DeviceAddr, 1, 1, &mut device);
    let s = subnet.first().copied().unwrap_or(DEFAULT_SUBNET_ADDR);
    let d = device.first().copied().unwrap_or(DEFAULT_DEVICE_ADDR);
    IndividualAddress::from_raw(u16::from(s) << 8 | u16::from(d))
}

/// Set the individual address on a device object.
///
/// Only valid for device-type interface objects (index 0).
pub fn set_individual_address(obj: &mut InterfaceObject, addr: u16) {
    let subnet = (addr >> 8) as u8;
    let device = (addr & 0xFF) as u8;
    obj.write_property(PropertyId::SubnetAddr, 1, 1, &[subnet]);
    obj.write_property(PropertyId::DeviceAddr, 1, 1, &[device]);
}

/// Check if programming mode is active.
///
/// Only valid for device-type interface objects (index 0).
pub fn prog_mode(obj: &InterfaceObject) -> bool {
    let mut buf = Vec::new();
    obj.read_property(PropertyId::ProgMode, 1, 1, &mut buf);
    buf.first().copied().unwrap_or(0) != 0
}

/// Set programming mode.
///
/// Only valid for device-type interface objects (index 0).
pub fn set_prog_mode(obj: &mut InterfaceObject, enabled: bool) {
    obj.write_property(PropertyId::ProgMode, 1, 1, &[u8::from(enabled)]);
}

/// Read the device serial number (6 bytes).
///
/// Only valid for device-type interface objects (index 0).
pub fn serial_number(obj: &InterfaceObject) -> [u8; 6] {
    let mut buf = Vec::new();
    obj.read_property(PropertyId::SerialNumber, 1, 1, &mut buf);
    let mut serial = [0u8; 6];
    let len = buf.len().min(6);
    serial[..len].copy_from_slice(&buf[..len]);
    serial
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_device() -> InterfaceObject {
        new_device_object([0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], [0x00; 6])
    }

    #[test]
    fn has_serial_number() {
        let obj = test_device();
        let mut buf = Vec::new();
        obj.read_property(PropertyId::SerialNumber, 1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0xFA, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn individual_address_default() {
        assert_eq!(individual_address(&test_device()).raw(), 0xFFFF);
    }

    #[test]
    fn set_and_get_individual_address() {
        let mut obj = test_device();
        set_individual_address(&mut obj, 0x1101);
        assert_eq!(individual_address(&obj).raw(), 0x1101);
    }

    #[test]
    fn prog_mode_toggle() {
        let mut obj = test_device();
        assert!(!prog_mode(&obj));
        set_prog_mode(&mut obj, true);
        assert!(prog_mode(&obj));
    }
}
