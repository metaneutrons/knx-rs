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

const FLAG_COMM_ENABLE: u16 = 1 << 10;
const FLAG_READ_ENABLE: u16 = 1 << 11;
const FLAG_WRITE_ENABLE: u16 = 1 << 12;
const FLAG_READ_ON_INIT: u16 = 1 << 13;
const FLAG_TRANSMIT_ENABLE: u16 = 1 << 14;
const FLAG_UPDATE_ENABLE: u16 = 1 << 15;
const PRIORITY_SHIFT: u32 = 8;
const PRIORITY_MASK: u16 = 0x03;
const VALUE_TYPE_MASK: u16 = 0xFF;

/// Parsed descriptor for a single group object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupObjectDescriptor {
    /// Raw 16-bit descriptor value.
    pub raw: u16,
}

impl GroupObjectDescriptor {
    /// Communication enabled (K-flag, bit 10).
    pub const fn communication_enable(self) -> bool {
        self.raw & FLAG_COMM_ENABLE != 0
    }

    /// Read enabled (L-flag, bit 11).
    pub const fn read_enable(self) -> bool {
        self.raw & FLAG_READ_ENABLE != 0
    }

    /// Write enabled (S-flag, bit 12).
    pub const fn write_enable(self) -> bool {
        self.raw & FLAG_WRITE_ENABLE != 0
    }

    /// Read on init (I-flag, bit 13).
    pub const fn read_on_init(self) -> bool {
        self.raw & FLAG_READ_ON_INIT != 0
    }

    /// Transmit on change (Ü-flag, bit 14).
    pub const fn transmit_enable(self) -> bool {
        self.raw & FLAG_TRANSMIT_ENABLE != 0
    }

    /// Update on response (A-flag, bit 15).
    pub const fn update_enable(self) -> bool {
        self.raw & FLAG_UPDATE_ENABLE != 0
    }

    /// Priority (bits 9..8).
    pub const fn priority(self) -> u8 {
        ((self.raw >> PRIORITY_SHIFT) & PRIORITY_MASK) as u8
    }

    /// Value size code (bits 7..0). Encodes the DPT data length.
    pub const fn value_type(self) -> u8 {
        (self.raw & VALUE_TYPE_MASK) as u8
    }

    /// Compute the data length in bytes for this GO's value type code.
    /// Returns 0 for codes < 7 (sub-byte types like DPT 1-6).
    /// Port of C++ `GroupObjectTableObject::asapValueSize()`.
    pub const fn value_size_bytes(self) -> usize {
        let code = self.value_type();
        match code {
            0..=6 => 0,
            7 => 1,
            8..=10 | 21..=254 => (code - 6) as usize,
            11 => 6,
            12 => 8,
            13 => 10,
            14 => 14,
            15 => 5,
            16 => 7,
            17 => 9,
            18 => 11,
            19 => 12,
            20 => 13,
            255 => 252,
        }
    }

    /// The GO data size for memory allocation (minimum 1 byte).
    pub const fn go_size(self) -> usize {
        let s = self.value_size_bytes();
        if s == 0 { 1 } else { s }
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

    #[test]
    fn value_size_bytes_known_codes() {
        let desc = |code: u8| GroupObjectDescriptor {
            raw: u16::from(code),
        };
        // Sub-byte types return 0
        assert_eq!(desc(0).value_size_bytes(), 0);
        assert_eq!(desc(6).value_size_bytes(), 0);
        // 1 byte
        assert_eq!(desc(7).value_size_bytes(), 1);
        // 2, 3, 4 bytes
        assert_eq!(desc(8).value_size_bytes(), 2);
        assert_eq!(desc(9).value_size_bytes(), 3);
        assert_eq!(desc(10).value_size_bytes(), 4);
        // Special mappings
        assert_eq!(desc(11).value_size_bytes(), 6);
        assert_eq!(desc(12).value_size_bytes(), 8);
        assert_eq!(desc(13).value_size_bytes(), 10);
        assert_eq!(desc(14).value_size_bytes(), 14);
        assert_eq!(desc(15).value_size_bytes(), 5);
        assert_eq!(desc(16).value_size_bytes(), 7);
        assert_eq!(desc(17).value_size_bytes(), 9);
        assert_eq!(desc(18).value_size_bytes(), 11);
        assert_eq!(desc(19).value_size_bytes(), 12);
        assert_eq!(desc(20).value_size_bytes(), 13);
        // General formula for 21..=254
        assert_eq!(desc(21).value_size_bytes(), 15);
        assert_eq!(desc(100).value_size_bytes(), 94);
        // Max
        assert_eq!(desc(255).value_size_bytes(), 252);
    }

    #[test]
    fn go_size_minimum_one() {
        let desc = |code: u8| GroupObjectDescriptor {
            raw: u16::from(code),
        };
        // Sub-byte types get minimum 1 byte
        assert_eq!(desc(0).go_size(), 1);
        assert_eq!(desc(6).go_size(), 1);
        // Larger types use actual size
        assert_eq!(desc(7).go_size(), 1);
        assert_eq!(desc(14).go_size(), 14);
    }
}
