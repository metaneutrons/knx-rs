// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Concrete property implementation that stores data in memory.

use alloc::vec::Vec;

use super::Property;
use super::types::{AccessLevel, PropertyDataType, PropertyId};

/// A property that stores its data in a `Vec<u8>`.
///
/// This is the most common property type — used for device configuration
/// data that ETS reads and writes.
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

    /// Direct access to the underlying data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Direct mutable access to the underlying data.
    #[allow(clippy::missing_const_for_fn)] // &mut Vec can't be const
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }
}

impl Property for DataProperty {
    fn id(&self) -> PropertyId {
        self.id
    }

    fn write_enable(&self) -> bool {
        self.write_enable
    }

    fn data_type(&self) -> PropertyDataType {
        self.data_type
    }

    fn max_elements(&self) -> u16 {
        self.max_elements
    }

    fn access(&self) -> u8 {
        self.access
    }

    fn read(&self, start: u16, count: u8, buf: &mut Vec<u8>) -> u8 {
        let elem_size = self.element_size() as usize;
        if elem_size == 0 {
            // Variable length: return all data
            buf.extend_from_slice(&self.data);
            return 1;
        }

        let total_elements = if elem_size > 0 {
            self.data.len() / elem_size
        } else {
            0
        };

        let start_idx = start.saturating_sub(1) as usize; // KNX uses 1-based indexing
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

    fn write(&mut self, start: u16, count: u8, data: &[u8]) -> u8 {
        if !self.write_enable {
            return 0;
        }

        let elem_size = self.element_size() as usize;
        if elem_size == 0 {
            // Variable length: replace all data
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

            // Grow data if needed (up to max_elements)
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
    fn read_multi_element() {
        let prop = DataProperty::new(
            PropertyId::Table,
            false,
            PropertyDataType::UnsignedInt,
            10,
            AccessLevel::None,
            &[0x00, 0x01, 0x00, 0x02, 0x00, 0x03],
        );
        let mut buf = Vec::new();
        let count = prop.read(1, 3, &mut buf);
        assert_eq!(count, 3);
        assert_eq!(buf, &[0x00, 0x01, 0x00, 0x02, 0x00, 0x03]);
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

    #[test]
    fn element_size_matches_type() {
        let prop = DataProperty::read_only(
            PropertyId::ObjectType,
            PropertyDataType::UnsignedInt,
            &[0, 0],
        );
        assert_eq!(prop.element_size(), 2);
    }

    #[test]
    fn description() {
        let prop = DataProperty::read_only(
            PropertyId::ManufacturerId,
            PropertyDataType::UnsignedInt,
            &[0, 0],
        );
        let desc = prop.description();
        assert_eq!(desc.id, PropertyId::ManufacturerId);
        assert!(!desc.write_enable);
        assert_eq!(desc.data_type, PropertyDataType::UnsignedInt);
    }
}
