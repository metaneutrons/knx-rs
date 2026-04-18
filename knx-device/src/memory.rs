// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Memory management — persistence abstraction for KNX device state.
//!
//! The [`MemoryBackend`] trait abstracts over the actual storage (flash,
//! EEPROM, file, RAM). The [`DeviceMemory`] struct manages serialization
//! of interface objects and table data.

use alloc::vec::Vec;

/// Trait for non-volatile storage backends.
///
/// Implementations provide read/write access to a flat byte buffer.
/// On embedded targets this maps to flash or EEPROM; on servers it
/// maps to a file.
pub trait MemoryBackend {
    /// Read the entire stored state. Returns empty vec if nothing stored.
    fn read_all(&self) -> Vec<u8>;

    /// Write the entire state. Replaces any previous content.
    fn write_all(&mut self, data: &[u8]);

    /// Commit pending writes (flush to storage). No-op for RAM backends.
    fn commit(&mut self) {}
}

/// RAM-only memory backend (no persistence across restarts).
///
/// Useful for testing and for devices that don't need persistence.
pub struct RamBackend {
    data: Vec<u8>,
}

impl RamBackend {
    /// Create a new empty RAM backend.
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }
}

impl Default for RamBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend for RamBackend {
    fn read_all(&self) -> Vec<u8> {
        self.data.clone()
    }

    fn write_all(&mut self, data: &[u8]) {
        self.data = data.to_vec();
    }
}

/// Magic bytes to identify valid stored data.
const MAGIC: [u8; 4] = [b'K', b'N', b'X', 0x01];

/// Device memory manager — serializes/deserializes device state.
///
/// Format: `[MAGIC:4] [version:u16be] [data_len:u32be] [data...]`
pub struct DeviceMemory<B: MemoryBackend> {
    backend: B,
    version: u16,
}

impl<B: MemoryBackend> DeviceMemory<B> {
    /// Create a new device memory manager.
    ///
    /// `version` is used to detect firmware changes that invalidate stored data.
    pub const fn new(backend: B, version: u16) -> Self {
        Self { backend, version }
    }

    /// Save device state to the backend.
    pub fn save(&mut self, data: &[u8]) {
        let mut buf = Vec::with_capacity(10 + data.len());
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&self.version.to_be_bytes());
        #[expect(clippy::cast_possible_truncation)]
        let len = data.len() as u32;
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(data);
        self.backend.write_all(&buf);
        self.backend.commit();
    }

    /// Load device state from the backend.
    ///
    /// Returns `None` if no valid data is stored or the version doesn't match.
    pub fn load(&self) -> Option<Vec<u8>> {
        let buf = self.backend.read_all();
        if buf.len() < 10 {
            return None;
        }
        if buf[..4] != MAGIC {
            return None;
        }
        let stored_version = u16::from_be_bytes([buf[4], buf[5]]);
        if stored_version != self.version {
            return None;
        }
        let data_len = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]) as usize;
        if buf.len() < 10 + data_len {
            return None;
        }
        Some(buf[10..10 + data_len].to_vec())
    }

    /// Clear all stored data.
    pub fn clear(&mut self) {
        self.backend.write_all(&[]);
        self.backend.commit();
    }

    /// Access the underlying backend.
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Mutable access to the underlying backend.
    #[allow(clippy::missing_const_for_fn)]
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load() {
        let mut mem = DeviceMemory::new(RamBackend::new(), 1);
        let data = b"hello knx";
        mem.save(data);

        let loaded = mem.load().unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn load_empty_returns_none() {
        let mem = DeviceMemory::new(RamBackend::new(), 1);
        assert!(mem.load().is_none());
    }

    #[test]
    fn version_mismatch_returns_none() {
        let mut mem = DeviceMemory::new(RamBackend::new(), 1);
        mem.save(b"data");

        // Read with different version
        let raw = mem.backend().read_all();
        let mut mem2 = DeviceMemory::new(RamBackend::new(), 2);
        mem2.backend_mut().write_all(&raw);
        assert!(mem2.load().is_none());
    }

    #[test]
    fn clear() {
        let mut mem = DeviceMemory::new(RamBackend::new(), 1);
        mem.save(b"data");
        assert!(mem.load().is_some());
        mem.clear();
        assert!(mem.load().is_none());
    }

    #[test]
    fn corrupted_magic_returns_none() {
        let mut mem = DeviceMemory::new(RamBackend::new(), 1);
        mem.backend_mut().write_all(&[0xFF; 20]);
        assert!(mem.load().is_none());
    }
}
