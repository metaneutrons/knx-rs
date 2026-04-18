// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX interface objects.
//!
//! An interface object is a container of properties that ETS can read, write,
//! and query. Every KNX device exposes a set of interface objects (device object,
//! address table, group object table, etc.).

use alloc::vec::Vec;

use crate::property::{DataProperty, Property, PropertyDataType, PropertyDescription, PropertyId};

/// KNX interface object type. See KNX 3/7/3 §2.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum ObjectType {
    Device = 0,
    AddressTable = 1,
    AssociationTable = 2,
    ApplicationProgram = 3,
    InterfaceProgram = 4,
    ObjectAssociationTable = 5,
    Router = 6,
    CemiServer = 8,
    GroupObjectTable = 9,
    IpParameter = 11,
    Security = 17,
    RfMedium = 19,
}

/// An interface object — a named collection of properties.
///
/// This is the concrete type (not a trait) because KNX interface objects
/// all share the same property-bag structure. Specialization happens
/// through which properties are added and how they're initialized.
pub struct InterfaceObject {
    object_type: ObjectType,
    properties: Vec<DataProperty>,
}

impl InterfaceObject {
    /// Create a new interface object of the given type.
    ///
    /// Automatically adds the mandatory `PID_OBJECT_TYPE` property.
    pub fn new(object_type: ObjectType) -> Self {
        let type_bytes = (object_type as u16).to_be_bytes();
        let mut obj = Self {
            object_type,
            properties: Vec::new(),
        };
        obj.add_property(DataProperty::read_only(
            PropertyId::ObjectType,
            PropertyDataType::UnsignedInt,
            &type_bytes,
        ));
        obj
    }

    /// The object type.
    pub const fn object_type(&self) -> ObjectType {
        self.object_type
    }

    /// Add a property to this object.
    pub fn add_property(&mut self, prop: DataProperty) {
        self.properties.push(prop);
    }

    /// Find a property by ID.
    pub fn property(&self, id: PropertyId) -> Option<&DataProperty> {
        self.properties.iter().find(|p| Property::id(*p) == id)
    }

    /// Find a mutable property by ID.
    pub fn property_mut(&mut self, id: PropertyId) -> Option<&mut DataProperty> {
        self.properties.iter_mut().find(|p| Property::id(*p) == id)
    }

    /// Read a property value. Returns the number of elements read.
    pub fn read_property(&self, id: PropertyId, start: u16, count: u8, buf: &mut Vec<u8>) -> u8 {
        self.property(id).map_or(0, |p| p.read(start, count, buf))
    }

    /// Write a property value. Returns the number of elements written.
    pub fn write_property(&mut self, id: PropertyId, start: u16, count: u8, data: &[u8]) -> u8 {
        self.property_mut(id)
            .map_or(0, |p| p.write(start, count, data))
    }

    /// Get the description of a property by ID or index.
    ///
    /// If `property_id` is non-zero, looks up by ID and returns the index.
    /// If `property_id` is zero, looks up by `property_index` and returns the ID.
    pub fn read_property_description(
        &self,
        property_id: u8,
        property_index: u8,
    ) -> Option<(u8, PropertyDescription)> {
        if property_id != 0 {
            // Look up by ID
            let pid = PropertyId::try_from(property_id).ok()?;
            let (idx, prop) = self
                .properties
                .iter()
                .enumerate()
                .find(|(_, p)| Property::id(*p) == pid)?;
            #[expect(clippy::cast_possible_truncation)]
            Some((idx as u8, prop.description()))
        } else {
            // Look up by index
            let prop = self.properties.get(property_index as usize)?;
            Some((property_index, prop.description()))
        }
    }

    /// Number of properties in this object.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// Serialize all property data for persistence.
    pub fn save(&self, buf: &mut Vec<u8>) {
        for prop in &self.properties {
            let data = prop.data();
            // Write length (u16 BE) + data
            #[expect(clippy::cast_possible_truncation)]
            let len = data.len() as u16;
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(data);
        }
    }

    /// Restore property data from a persistence buffer.
    ///
    /// Returns the number of bytes consumed.
    pub fn restore(&mut self, buf: &[u8]) -> usize {
        let mut offset = 0;
        for prop in &mut self.properties {
            if offset + 2 > buf.len() {
                break;
            }
            let len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + len > buf.len() {
                break;
            }
            if prop.write_enable() {
                prop.write(1, 1, &buf[offset..offset + len]);
            }
            offset += len;
        }
        offset
    }
}

impl TryFrom<u8> for PropertyId {
    type Error = u8;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        // Only the common PIDs — extend as needed
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn new_object_has_type_property() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::ObjectType, 1, 1, &mut buf);
        assert_eq!(count, 1);
        assert_eq!(buf, &[0x00, 0x00]); // ObjectType::Device = 0
    }

    #[test]
    fn add_and_read_property() {
        let mut obj = InterfaceObject::new(ObjectType::Device);
        obj.add_property(DataProperty::read_only(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &[0x00, 0xFA],
        ));
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::ManufacturerId, 1, 1, &mut buf);
        assert_eq!(count, 1);
        assert_eq!(buf, &[0x00, 0xFA]);
    }

    #[test]
    fn write_property() {
        let mut obj = InterfaceObject::new(ObjectType::Device);
        obj.add_property(DataProperty::read_write(
            PropertyId::ProgMode,
            PropertyDataType::UnsignedChar,
            &[0x00],
        ));
        let count = obj.write_property(PropertyId::ProgMode, 1, 1, &[0x01]);
        assert_eq!(count, 1);

        let mut buf = Vec::new();
        obj.read_property(PropertyId::ProgMode, 1, 1, &mut buf);
        assert_eq!(buf, &[0x01]);
    }

    #[test]
    fn read_nonexistent_property() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::SerialNumber, 1, 1, &mut buf);
        assert_eq!(count, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn property_description_by_id() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let (idx, desc) = obj
            .read_property_description(PropertyId::ObjectType as u8, 0)
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(desc.id, PropertyId::ObjectType);
        assert!(!desc.write_enable);
    }

    #[test]
    fn property_description_by_index() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let (idx, desc) = obj.read_property_description(0, 0).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(desc.id, PropertyId::ObjectType);
    }
}
