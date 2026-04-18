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
    AccessLevel, ErrorCode, LoadEvent, LoadState, PropertyDataType, PropertyDescription, PropertyId,
};

use alloc::vec::Vec;

/// A KNX property — the unit of configuration data in interface objects.
///
/// Properties are identified by [`PropertyId`] and have a type, access level,
/// and one or more elements.
pub trait Property {
    /// The property identifier.
    fn id(&self) -> PropertyId;

    /// Whether the property can be written.
    fn write_enable(&self) -> bool;

    /// The data type of the property.
    fn data_type(&self) -> PropertyDataType;

    /// Maximum number of elements.
    fn max_elements(&self) -> u16;

    /// Access level (read/write).
    fn access(&self) -> u8;

    /// Size of one element in bytes.
    fn element_size(&self) -> u8 {
        self.data_type().size()
    }

    /// Read elements from the property.
    ///
    /// Returns the number of elements actually read. Data is written to `buf`.
    fn read(&self, start: u16, count: u8, buf: &mut Vec<u8>) -> u8;

    /// Write elements to the property.
    ///
    /// Returns the number of elements actually written.
    fn write(&mut self, start: u16, count: u8, data: &[u8]) -> u8;

    /// Property description for ETS.
    fn description(&self) -> PropertyDescription {
        PropertyDescription {
            id: self.id(),
            write_enable: self.write_enable(),
            data_type: self.data_type(),
            max_elements: self.max_elements(),
            access: self.access(),
        }
    }
}
