// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Association Table — maps TSAPs to ASAPs (group object numbers).
//!
//! Wire format: `[count:u16be] [tsap_1:u16be asap_1:u16be] [tsap_2:u16be asap_2:u16be] ...`
//!
//! TSAP = Transport Service Access Point (index into address table).
//! ASAP = Application Service Access Point (group object number, 1-based).

use alloc::vec::Vec;

const ENTRY_SIZE: usize = 4;
const TSAP_OFFSET: usize = 2;

/// Association table: maps TSAPs to ASAPs.
pub struct AssociationTable {
    data: Vec<u8>,
}

impl Default for AssociationTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AssociationTable {
    /// Create an empty association table.
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Load table data from ETS.
    pub fn load(&mut self, data: &[u8]) {
        self.data = data.to_vec();
    }

    /// Number of association entries.
    pub fn entry_count(&self) -> u16 {
        if self.data.len() < 2 {
            return 0;
        }
        u16::from_be_bytes([self.data[0], self.data[1]])
    }

    /// Get the TSAP for entry at `idx` (0-based).
    fn get_tsap(&self, idx: u16) -> Option<u16> {
        let offset = TSAP_OFFSET + (idx as usize) * ENTRY_SIZE;
        if offset + 2 > self.data.len() {
            return None;
        }
        Some(u16::from_be_bytes([
            self.data[offset],
            self.data[offset + 1],
        ]))
    }

    /// Get the ASAP for entry at `idx` (0-based).
    fn get_asap(&self, idx: u16) -> Option<u16> {
        let offset = TSAP_OFFSET + (idx as usize) * ENTRY_SIZE + TSAP_OFFSET;
        if offset + 2 > self.data.len() {
            return None;
        }
        Some(u16::from_be_bytes([
            self.data[offset],
            self.data[offset + 1],
        ]))
    }

    /// Translate an ASAP to its TSAP. Returns `None` if not found.
    pub fn translate_asap(&self, asap: u16) -> Option<u16> {
        for i in 0..self.entry_count() {
            if self.get_asap(i) == Some(asap) {
                return self.get_tsap(i);
            }
        }
        None
    }

    /// Find the next ASAP for a given TSAP, starting from `start_idx`.
    ///
    /// Returns `(asap, next_start_idx)` or `None` if no more entries.
    pub fn next_asap(&self, tsap: u16, start_idx: u16) -> Option<(u16, u16)> {
        for i in start_idx..self.entry_count() {
            if self.get_tsap(i) == Some(tsap) {
                return Some((self.get_asap(i)?, i + 1));
            }
        }
        None
    }

    /// Collect all ASAPs associated with a given TSAP.
    pub fn asaps_for_tsap(&self, tsap: u16) -> Vec<u16> {
        let mut result = Vec::new();
        let mut idx = 0;
        while let Some((asap, next)) = self.next_asap(tsap, idx) {
            result.push(asap);
            idx = next;
        }
        result
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample_table() -> AssociationTable {
        let mut t = AssociationTable::new();
        // 3 entries: TSAP 1→ASAP 1, TSAP 1→ASAP 2, TSAP 2→ASAP 3
        t.load(&[
            0x00, 0x03, // count = 3
            0x00, 0x01, 0x00, 0x01, // TSAP 1 → ASAP 1
            0x00, 0x01, 0x00, 0x02, // TSAP 1 → ASAP 2
            0x00, 0x02, 0x00, 0x03, // TSAP 2 → ASAP 3
        ]);
        t
    }

    #[test]
    fn entry_count() {
        assert_eq!(sample_table().entry_count(), 3);
    }

    #[test]
    fn translate_asap() {
        let t = sample_table();
        assert_eq!(t.translate_asap(1), Some(1)); // ASAP 1 → TSAP 1
        assert_eq!(t.translate_asap(3), Some(2)); // ASAP 3 → TSAP 2
        assert_eq!(t.translate_asap(99), None);
    }

    #[test]
    fn asaps_for_tsap() {
        let t = sample_table();
        assert_eq!(t.asaps_for_tsap(1), &[1, 2]); // TSAP 1 has ASAP 1 and 2
        assert_eq!(t.asaps_for_tsap(2), &[3]);
        assert!(t.asaps_for_tsap(99).is_empty());
    }
}
