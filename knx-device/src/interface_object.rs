// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX interface objects.

use alloc::vec::Vec;

use crate::property::{DataProperty, Property, PropertyDataType, PropertyDescription, PropertyId};

/// KNX interface object type. See KNX 3/7/3 §2.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
#[allow(missing_docs)] // KNX spec names are self-documenting
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
pub struct InterfaceObject {
    object_type: ObjectType,
    properties: Vec<Property>,
}

impl InterfaceObject {
    /// Create a new interface object of the given type.
    pub fn new(object_type: ObjectType) -> Self {
        let type_bytes = (object_type as u16).to_be_bytes();
        let mut obj = Self {
            object_type,
            properties: Vec::new(),
        };
        obj.add_property(Property::from(DataProperty::read_only(
            PropertyId::ObjectType,
            PropertyDataType::UnsignedInt,
            &type_bytes,
        )));
        obj
    }

    /// The object type.
    pub const fn object_type(&self) -> ObjectType {
        self.object_type
    }

    /// Add a property to this object.
    pub fn add_property(&mut self, prop: Property) {
        self.properties.push(prop);
    }

    /// Find a property by ID.
    pub fn property(&self, id: PropertyId) -> Option<&Property> {
        self.properties.iter().find(|p| p.id() == id)
    }

    /// Find a mutable property by ID.
    pub fn property_mut(&mut self, id: PropertyId) -> Option<&mut Property> {
        self.properties.iter_mut().find(|p| p.id() == id)
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
    pub fn read_property_description(
        &self,
        property_id: u8,
        property_index: u8,
    ) -> Option<(u8, PropertyDescription)> {
        if property_id != 0 {
            let pid = PropertyId::try_from(property_id).ok()?;
            let (idx, prop) = self
                .properties
                .iter()
                .enumerate()
                .find(|(_, p)| p.id() == pid)?;
            #[expect(clippy::cast_possible_truncation)]
            Some((idx as u8, prop.description()))
        } else {
            let prop = self.properties.get(property_index as usize)?;
            Some((property_index, prop.description()))
        }
    }

    /// Number of properties in this object.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }
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
    use crate::property::AccessLevel;
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicU8, Ordering};

    #[test]
    fn new_object_has_type_property() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::ObjectType, 1, 1, &mut buf);
        assert_eq!(count, 1);
        assert_eq!(buf, &[0x00, 0x00]);
    }

    #[test]
    fn add_and_read_data_property() {
        let mut obj = InterfaceObject::new(ObjectType::Device);
        obj.add_property(Property::from(DataProperty::read_only(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &[0x00, 0xFA],
        )));
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::ManufacturerId, 1, 1, &mut buf);
        assert_eq!(count, 1);
        assert_eq!(buf, &[0x00, 0xFA]);
    }

    #[test]
    fn write_data_property() {
        let mut obj = InterfaceObject::new(ObjectType::Device);
        obj.add_property(Property::from(DataProperty::read_write(
            PropertyId::ProgMode,
            PropertyDataType::UnsignedChar,
            &[0x00],
        )));
        obj.write_property(PropertyId::ProgMode, 1, 1, &[0x01]);
        let mut buf = Vec::new();
        obj.read_property(PropertyId::ProgMode, 1, 1, &mut buf);
        assert_eq!(buf, &[0x01]);
    }

    #[test]
    fn callback_property_read_write() {
        let counter = Arc::new(AtomicU8::new(0));
        let cr = counter.clone();

        let mut obj = InterfaceObject::new(ObjectType::Device);
        obj.add_property(Property::callback(
            PropertyId::ProgMode,
            true,
            PropertyDataType::UnsignedChar,
            1,
            AccessLevel::None,
            move |_start: u16, _count: u8| -> Vec<u8> { alloc::vec![cr.load(Ordering::Relaxed)] },
            Some(Box::new(
                move |_start: u16, _count: u8, data: &[u8]| -> u8 {
                    if let Some(&v) = data.first() {
                        counter.store(v, Ordering::Relaxed);
                    }
                    1
                },
            )),
        ));

        obj.write_property(PropertyId::ProgMode, 1, 1, &[42]);
        let mut buf = Vec::new();
        obj.read_property(PropertyId::ProgMode, 1, 1, &mut buf);
        assert_eq!(buf, &[42]);
    }

    #[test]
    fn read_nonexistent_property() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let mut buf = Vec::new();
        let count = obj.read_property(PropertyId::SerialNumber, 1, 1, &mut buf);
        assert_eq!(count, 0);
    }

    #[test]
    fn property_description_by_id() {
        let obj = InterfaceObject::new(ObjectType::Device);
        let (idx, desc) = obj
            .read_property_description(PropertyId::ObjectType as u8, 0)
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(desc.id, PropertyId::ObjectType);
    }
}
