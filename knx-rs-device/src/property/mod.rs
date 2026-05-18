// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

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
    access: AccessLevel,
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
            access: prop.access_level(),
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
            access,
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
    pub const fn access(&self) -> AccessLevel {
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
                let read_count =
                    u8::try_from(data.len().checked_div(elem_size).unwrap_or(1)).unwrap_or(u8::MAX);
                buf.extend_from_slice(&data);
                read_count
            }
        }
    }

    /// Write elements to the property.
    pub fn write(&mut self, start: u16, count: u8, data: &[u8]) -> u8 {
        if !self.write_enable {
            return 0;
        }

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn callback_property_respects_write_enable() {
        let called = Arc::new(AtomicBool::new(false));
        let called_by_write = Arc::clone(&called);
        let write_fn: PropertyWriteFn = Box::new(move |_, _, _| {
            called_by_write.store(true, Ordering::SeqCst);
            1
        });
        let mut prop = Property::callback(
            PropertyId::ManufacturerId,
            false,
            PropertyDataType::UnsignedInt,
            1,
            AccessLevel::None,
            |_, _| Vec::new(),
            Some(write_fn),
        );

        assert_eq!(prop.write(1, 1, &[0x00, 0xFA]), 0);
        assert!(!called.load(Ordering::SeqCst));
    }
}
