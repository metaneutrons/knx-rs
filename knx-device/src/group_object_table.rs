// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Group Object Table — defines the group objects (communication objects).
//!
//! Wire format: `[count:u16be] [go_desc_1:u16be] [go_desc_2:u16be] ...`
//!
//! Each descriptor is a 16-bit value with flag bits:
//! - Bit 15: Update on response (A-flag)
//! - Bit 14: Transmit on change (Ü-flag)
//! - Bit 13: Read on init (I-flag)
//! - Bit 12: Write enabled (S-flag)
//! - Bit 11: Read enabled (L-flag)
//! - Bit 10: Communication enabled (K-flag)
//! - Bits 9..8: Priority
//! - Bits 7..0: DPT size code

use alloc::vec::Vec;

/// Parsed descriptor for a single group object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupObjectDescriptor {
    /// Raw 16-bit descriptor value.
    pub raw: u16,
}

impl GroupObjectDescriptor {
    /// Communication enabled (K-flag, bit 10).
    pub const fn communication_enable(self) -> bool {
        self.raw & (1 << 10) != 0
    }

    /// Read enabled (L-flag, bit 11).
    pub const fn read_enable(self) -> bool {
        self.raw & (1 << 11) != 0
    }

    /// Write enabled (S-flag, bit 12).
    pub const fn write_enable(self) -> bool {
        self.raw & (1 << 12) != 0
    }

    /// Read on init (I-flag, bit 13).
    pub const fn read_on_init(self) -> bool {
        self.raw & (1 << 13) != 0
    }

    /// Transmit on change (Ü-flag, bit 14).
    pub const fn transmit_enable(self) -> bool {
        self.raw & (1 << 14) != 0
    }

    /// Update on response (A-flag, bit 15).
    pub const fn update_enable(self) -> bool {
        self.raw & (1 << 15) != 0
    }

    /// Priority (bits 9..8).
    pub const fn priority(self) -> u8 {
        ((self.raw >> 8) & 0x03) as u8
    }

    /// Value size code (bits 7..0). Encodes the DPT data length.
    pub const fn value_type(self) -> u8 {
        (self.raw & 0xFF) as u8
    }
}

/// Group object table: defines the communication objects.
pub struct GroupObjectTable {
    data: Vec<u8>,
}

impl Default for GroupObjectTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupObjectTable {
    /// Create an empty table.
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Load table data from ETS.
    pub fn load(&mut self, data: &[u8]) {
        self.data = data.to_vec();
    }

    /// Number of group objects.
    pub fn entry_count(&self) -> u16 {
        if self.data.len() < 2 {
            return 0;
        }
        u16::from_be_bytes([self.data[0], self.data[1]])
    }

    /// Get the descriptor for a group object (1-based ASAP).
    pub fn get_descriptor(&self, asap: u16) -> Option<GroupObjectDescriptor> {
        if asap == 0 || asap > self.entry_count() {
            return None;
        }
        let offset = (asap as usize) * 2;
        if offset + 2 > self.data.len() {
            return None;
        }
        let raw = u16::from_be_bytes([self.data[offset], self.data[offset + 1]]);
        Some(GroupObjectDescriptor { raw })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample_table() -> GroupObjectTable {
        let mut t = GroupObjectTable::new();
        // 2 group objects
        // GO 1: comm + read + transmit + update = bits 10,11,14,15 = 0xCC00 | size=1
        // GO 2: comm + write + read_on_init = bits 10,12,13 = 0x3400 | size=1
        let go1: u16 = (1 << 15) | (1 << 14) | (1 << 11) | (1 << 10) | 1; // 0xCC01
        let go2: u16 = (1 << 13) | (1 << 12) | (1 << 10) | 1; // 0x3401
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_be_bytes()); // count
        data.extend_from_slice(&go1.to_be_bytes());
        data.extend_from_slice(&go2.to_be_bytes());
        t.load(&data);
        t
    }

    #[test]
    fn entry_count() {
        assert_eq!(sample_table().entry_count(), 2);
    }

    #[test]
    fn get_descriptor() {
        let t = sample_table();
        let d1 = t.get_descriptor(1).unwrap();
        assert!(d1.communication_enable());
        assert!(d1.read_enable());
        assert!(d1.transmit_enable());
        assert!(d1.update_enable());
        assert!(!d1.write_enable());
        assert!(!d1.read_on_init());
        assert_eq!(d1.value_type(), 1);

        let d2 = t.get_descriptor(2).unwrap();
        assert!(d2.communication_enable());
        assert!(d2.write_enable());
        assert!(d2.read_on_init());
        assert!(!d2.read_enable());
        assert!(!d2.transmit_enable());
        assert!(!d2.update_enable());
    }

    #[test]
    fn out_of_range() {
        let t = sample_table();
        assert!(t.get_descriptor(0).is_none());
        assert!(t.get_descriptor(3).is_none());
    }
}
