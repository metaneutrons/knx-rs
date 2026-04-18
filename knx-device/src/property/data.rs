// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Concrete property implementation that stores data in memory.

use alloc::vec::Vec;

use super::types::{AccessLevel, PropertyDataType, PropertyId};

/// A property that stores its data in a `Vec<u8>`.
pub struct DataProperty {
    id: PropertyId,
    write_enable: bool,
    data_type: PropertyDataType,
    max_elements: u16,
    access: u8,
    data: Vec<u8>,
}

impl DataProperty {
    /// Create a new data property with the given initial value.
    pub fn new(
        id: PropertyId,
        write_enable: bool,
        data_type: PropertyDataType,
        max_elements: u16,
        access: AccessLevel,
        initial_data: &[u8],
    ) -> Self {
        Self {
            id,
            write_enable,
            data_type,
            max_elements,
            access: access as u8,
            data: initial_data.to_vec(),
        }
    }

    /// Create a read-only property with a single-element value.
    pub fn read_only(id: PropertyId, data_type: PropertyDataType, value: &[u8]) -> Self {
        Self::new(id, false, data_type, 1, AccessLevel::None, value)
    }

    /// Create a read-write property with a single-element value.
    pub fn read_write(id: PropertyId, data_type: PropertyDataType, value: &[u8]) -> Self {
        Self::new(id, true, data_type, 1, AccessLevel::None, value)
    }

    /// The property identifier.
    pub const fn id(&self) -> PropertyId {
        self.id
    }

    /// Whether the property can be written.
    pub const fn write_enable(&self) -> bool {
        self.write_enable
    }

    /// The data type.
    pub const fn data_type(&self) -> PropertyDataType {
        self.data_type
    }

    /// Maximum number of elements.
    pub const fn max_elements(&self) -> u16 {
        self.max_elements
    }

    /// Access level.
    pub const fn access(&self) -> u8 {
        self.access
    }

    /// Size of one element in bytes.
    pub const fn element_size(&self) -> u8 {
        self.data_type.size()
    }

    /// Direct access to the underlying data.
    #[allow(clippy::missing_const_for_fn)]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Direct mutable access to the underlying data.
    #[allow(clippy::missing_const_for_fn)]
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    /// Read elements from the property.
    pub fn read(&self, start: u16, count: u8, buf: &mut Vec<u8>) -> u8 {
        let elem_size = self.element_size() as usize;
        if elem_size == 0 {
            buf.extend_from_slice(&self.data);
            return 1;
        }

        let total_elements = self.data.len() / elem_size;
        let start_idx = start.saturating_sub(1) as usize;
        let mut read_count = 0u8;

        for i in 0..count as usize {
            let elem_idx = start_idx + i;
            if elem_idx >= total_elements {
                break;
            }
            let offset = elem_idx * elem_size;
            let end = offset + elem_size;
            if end <= self.data.len() {
                buf.extend_from_slice(&self.data[offset..end]);
                read_count += 1;
            }
        }

        read_count
    }

    /// Write elements to the property.
    pub fn write(&mut self, start: u16, count: u8, data: &[u8]) -> u8 {
        if !self.write_enable {
            return 0;
        }

        let elem_size = self.element_size() as usize;
        if elem_size == 0 {
            self.data = data.to_vec();
            return 1;
        }

        let start_idx = start.saturating_sub(1) as usize;
        let mut written = 0u8;

        for i in 0..count as usize {
            let elem_idx = start_idx + i;
            let data_offset = i * elem_size;
            let prop_offset = elem_idx * elem_size;

            if data_offset + elem_size > data.len() {
                break;
            }

            let needed = prop_offset + elem_size;
            if needed > self.data.len() {
                if elem_idx >= self.max_elements as usize {
                    break;
                }
                self.data.resize(needed, 0);
            }

            self.data[prop_offset..prop_offset + elem_size]
                .copy_from_slice(&data[data_offset..data_offset + elem_size]);
            written += 1;
        }

        written
    }

    /// Property description for ETS.
    pub const fn description(&self) -> super::PropertyDescription {
        super::PropertyDescription {
            id: self.id,
            write_enable: self.write_enable,
            data_type: self.data_type,
            max_elements: self.max_elements,
            access: self.access,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn read_single_element() {
        let prop = DataProperty::read_only(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &[0x00, 0xFA],
        );
        let mut buf = Vec::new();
        let count = prop.read(1, 1, &mut buf);
        assert_eq!(count, 1);
        assert_eq!(buf, &[0x00, 0xFA]);
    }

    #[test]
    fn write_single_element() {
        let mut prop = DataProperty::read_write(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &[0x00, 0x00],
        );
        let count = prop.write(1, 1, &[0x00, 0xFA]);
        assert_eq!(count, 1);
        let mut buf = Vec::new();
        prop.read(1, 1, &mut buf);
        assert_eq!(buf, &[0x00, 0xFA]);
    }

    #[test]
    fn read_only_rejects_write() {
        let mut prop = DataProperty::read_only(
            PropertyId::ObjectType,
            PropertyDataType::UnsignedInt,
            &[0x00, 0x00],
        );
        let count = prop.write(1, 1, &[0xFF, 0xFF]);
        assert_eq!(count, 0);
    }

    #[test]
    fn write_grows_data() {
        let mut prop = DataProperty::new(
            PropertyId::Table,
            true,
            PropertyDataType::UnsignedInt,
            10,
            AccessLevel::None,
            &[],
        );
        let count = prop.write(1, 2, &[0x00, 0x01, 0x00, 0x02]);
        assert_eq!(count, 2);
        assert_eq!(prop.data(), &[0x00, 0x01, 0x00, 0x02]);
    }
}
