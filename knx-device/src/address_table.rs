// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Address Table — maps TSAPs to group addresses.
//!
//! The address table is loaded by ETS. Entry 0 is the entry count,
//! entries 1..N are group addresses (big-endian u16).

use alloc::vec::Vec;

/// Address table: maps TSAP indices to group addresses.
///
/// Table wire format: `[count:u16be] [entries...]`
pub struct AddressTable {
    data: Vec<u8>,
}

impl Default for AddressTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AddressTable {
    /// Create an empty (unloaded) address table.
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Load table data (as received from ETS via `MemoryWrite`).
    pub fn load(&mut self, data: &[u8]) {
        self.data = data.to_vec();
    }

    /// Number of group address entries.
    pub fn entry_count(&self) -> u16 {
        if self.data.len() < 2 {
            return 0;
        }
        u16::from_be_bytes([self.data[0], self.data[1]])
    }

    /// Get the group address for a TSAP (1-based index).
    pub fn get_group_address(&self, tsap: u16) -> Option<u16> {
        if tsap == 0 || tsap > self.entry_count() {
            return None;
        }
        let offset = (tsap as usize) * 2;
        if offset + 2 > self.data.len() {
            return None;
        }
        Some(u16::from_be_bytes([
            self.data[offset],
            self.data[offset + 1],
        ]))
    }

    /// Find the TSAP for a group address (linear search).
    pub fn get_tsap(&self, group_address: u16) -> Option<u16> {
        (1..=self.entry_count()).find(|&i| self.get_group_address(i) == Some(group_address))
    }

    /// Check if the table contains a group address.
    pub fn contains(&self, group_address: u16) -> bool {
        self.get_tsap(group_address).is_some()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample_table() -> AddressTable {
        let mut t = AddressTable::new();
        // 3 entries: GA 0x0801 (1/0/1), 0x0802 (1/0/2), 0x0901 (1/1/1)
        t.load(&[
            0x00, 0x03, // count = 3
            0x08, 0x01, // TSAP 1 → 1/0/1
            0x08, 0x02, // TSAP 2 → 1/0/2
            0x09, 0x01, // TSAP 3 → 1/1/1
        ]);
        t
    }

    #[test]
    fn entry_count() {
        assert_eq!(sample_table().entry_count(), 3);
        assert_eq!(AddressTable::new().entry_count(), 0);
    }

    #[test]
    fn get_group_address() {
        let t = sample_table();
        assert_eq!(t.get_group_address(1), Some(0x0801));
        assert_eq!(t.get_group_address(3), Some(0x0901));
        assert_eq!(t.get_group_address(0), None);
        assert_eq!(t.get_group_address(4), None);
    }

    #[test]
    fn get_tsap() {
        let t = sample_table();
        assert_eq!(t.get_tsap(0x0801), Some(1));
        assert_eq!(t.get_tsap(0x0901), Some(3));
        assert_eq!(t.get_tsap(0xFFFF), None);
    }

    #[test]
    fn contains() {
        let t = sample_table();
        assert!(t.contains(0x0801));
        assert!(!t.contains(0xFFFF));
    }
}
