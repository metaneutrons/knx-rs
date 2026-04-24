// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX property system.
//!
//! Properties are the fundamental data model for KNX interface objects.
//! ETS reads and writes properties to configure devices.

mod data;
mod types;

pub use data::DataProperty;
pub use types::{
    AccessLevel, LoadEvent, LoadState, PropertyDataType, PropertyDescription, PropertyId,
};

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Read callback signature: `(start, count) -> data bytes`.
type ReadFn = Box<dyn Fn(u16, u8) -> Vec<u8> + Send>;
/// Write callback: `(start_index, count, data) -> elements_written`.
pub type PropertyWriteFn = Box<dyn Fn(u16, u8, &[u8]) -> u8 + Send>;

/// A KNX property with metadata and either data storage or callbacks.
///
/// Metadata (id, type, access) is always stored inline for zero-cost access.
/// The actual data is either a `DataProperty` or a pair of callbacks.
pub struct Property {
    id: PropertyId,
    write_enable: bool,
    data_type: PropertyDataType,
    max_elements: u16,
    access: u8,
    storage: PropertyStorage,
}

enum PropertyStorage {
    Data(DataProperty),
    Callback {
        read_fn: ReadFn,
        write_fn: Option<PropertyWriteFn>,
    },
}

impl Property {
    /// Create a data-backed property.
    pub const fn data(prop: DataProperty) -> Self {
        Self {
            id: prop.id(),
            write_enable: prop.write_enable(),
            data_type: prop.data_type(),
            max_elements: prop.max_elements(),
            access: prop.access(),
            storage: PropertyStorage::Data(prop),
        }
    }

    /// Create a callback-backed property.
    pub fn callback(
        id: PropertyId,
        write_enable: bool,
        data_type: PropertyDataType,
        max_elements: u16,
        access: AccessLevel,
        read_fn: impl Fn(u16, u8) -> Vec<u8> + Send + 'static,
        write_fn: Option<PropertyWriteFn>,
    ) -> Self {
        Self {
            id,
            write_enable,
            data_type,
            max_elements,
            access: access as u8,
            storage: PropertyStorage::Callback {
                read_fn: Box::new(read_fn),
                write_fn,
            },
        }
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

    /// Read elements from the property.
    pub fn read(&self, start: u16, count: u8, buf: &mut Vec<u8>) -> u8 {
        match &self.storage {
            PropertyStorage::Data(d) => d.read(start, count, buf),
            PropertyStorage::Callback { read_fn, .. } => {
                let data = read_fn(start, count);
                if data.is_empty() {
                    return 0;
                }
                let elem_size = self.element_size() as usize;
                #[expect(clippy::cast_possible_truncation)]
                let read_count = data.len().checked_div(elem_size).unwrap_or(1) as u8;
                buf.extend_from_slice(&data);
                read_count
            }
        }
    }

    /// Write elements to the property.
    pub fn write(&mut self, start: u16, count: u8, data: &[u8]) -> u8 {
        match &mut self.storage {
            PropertyStorage::Data(d) => d.write(start, count, data),
            PropertyStorage::Callback { write_fn, .. } => {
                write_fn.as_ref().map_or(0, |wf| wf(start, count, data))
            }
        }
    }

    /// Get the underlying `DataProperty`, if this is data-backed.
    pub const fn as_data(&self) -> Option<&DataProperty> {
        match &self.storage {
            PropertyStorage::Data(d) => Some(d),
            PropertyStorage::Callback { .. } => None,
        }
    }

    /// Get the underlying `DataProperty` mutably.
    pub const fn as_data_mut(&mut self) -> Option<&mut DataProperty> {
        match &mut self.storage {
            PropertyStorage::Data(d) => Some(d),
            PropertyStorage::Callback { .. } => None,
        }
    }

    /// Property description for ETS.
    pub const fn description(&self) -> PropertyDescription {
        PropertyDescription {
            id: self.id,
            write_enable: self.write_enable,
            data_type: self.data_type,
            max_elements: self.max_elements,
            access: self.access,
        }
    }
}

/// Convenience: create a `Property` from a `DataProperty`.
impl From<DataProperty> for Property {
    fn from(d: DataProperty) -> Self {
        Self::data(d)
    }
}
