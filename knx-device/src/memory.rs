// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Memory management — persistence abstraction for KNX device state.
//!
//! The persistence format matches the C++ `knx-openknx` reference:
//! `[manufacturer_id:2][hardware_type:6][api_version:2][data...]`

use alloc::vec::Vec;

/// Trait for non-volatile storage backends.
pub trait MemoryBackend {
    /// Read the entire stored state.
    fn read_all(&self) -> Vec<u8>;

    /// Write the entire state.
    fn write_all(&mut self, data: &[u8]);

    /// Commit pending writes to storage.
    fn commit(&mut self);
}

/// RAM-only memory backend (no persistence across restarts).
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

    fn commit(&mut self) {}
}

/// Header size: `manufacturer_id`(2) + `hardware_type`(6) + `api_version`(2) + `firmware_version`(2).
const HEADER_SIZE: usize = 12;

/// Device memory manager — serializes/deserializes device state.
///
/// Format matches C++ `knx-openknx`:
/// `[manufacturer_id:2be][hardware_type:6][api_version:2be][firmware_version:2be][data...]`
pub struct DeviceMemory<B: MemoryBackend> {
    backend: B,
    manufacturer_id: u16,
    hardware_type: [u8; 6],
    api_version: u16,
    firmware_version: u16,
}

impl<B: MemoryBackend> DeviceMemory<B> {
    /// Create a new device memory manager.
    pub const fn new(
        backend: B,
        manufacturer_id: u16,
        hardware_type: [u8; 6],
        api_version: u16,
        firmware_version: u16,
    ) -> Self {
        Self {
            backend,
            manufacturer_id,
            hardware_type,
            api_version,
            firmware_version,
        }
    }

    /// Save device state to the backend.
    pub fn save(&mut self, data: &[u8]) {
        let mut buf = Vec::with_capacity(HEADER_SIZE + data.len());
        buf.extend_from_slice(&self.manufacturer_id.to_be_bytes());
        buf.extend_from_slice(&self.hardware_type);
        buf.extend_from_slice(&self.api_version.to_be_bytes());
        buf.extend_from_slice(&self.firmware_version.to_be_bytes());
        buf.extend_from_slice(data);
        self.backend.write_all(&buf);
        self.backend.commit();
    }

    /// Load device state from the backend.
    ///
    /// Returns `None` if no valid data is stored or the header doesn't match.
    pub fn load(&self) -> Option<Vec<u8>> {
        let buf = self.backend.read_all();
        if buf.len() < HEADER_SIZE {
            return None;
        }

        let stored_mfr = u16::from_be_bytes([buf[0], buf[1]]);
        if stored_mfr != self.manufacturer_id {
            return None;
        }

        if buf[2..8] != self.hardware_type {
            return None;
        }

        let stored_version = u16::from_be_bytes([buf[8], buf[9]]);
        if stored_version != self.api_version {
            return None;
        }

        let stored_fw = u16::from_be_bytes([buf[10], buf[11]]);
        if stored_fw != self.firmware_version {
            return None;
        }

        Some(buf[HEADER_SIZE..].to_vec())
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_mem() -> DeviceMemory<RamBackend> {
        DeviceMemory::new(RamBackend::new(), 0x00FA, [0x01; 6], 2, 1)
    }

    #[test]
    fn save_and_load() {
        let mut mem = test_mem();
        mem.save(b"hello knx");
        let loaded = mem.load().unwrap();
        assert_eq!(loaded, b"hello knx");
    }

    #[test]
    fn load_empty_returns_none() {
        assert!(test_mem().load().is_none());
    }

    #[test]
    fn manufacturer_mismatch_returns_none() {
        let mut mem = test_mem();
        mem.save(b"data");
        let raw = mem.backend().read_all();

        let mut mem2 = DeviceMemory::new(RamBackend::new(), 0x00FB, [0x01; 6], 2, 1);
        mem2.backend.write_all(&raw);
        assert!(mem2.load().is_none());
    }

    #[test]
    fn hardware_type_mismatch_returns_none() {
        let mut mem = test_mem();
        mem.save(b"data");
        let raw = mem.backend().read_all();

        let mut mem2 = DeviceMemory::new(RamBackend::new(), 0x00FA, [0x02; 6], 2, 1);
        mem2.backend.write_all(&raw);
        assert!(mem2.load().is_none());
    }

    #[test]
    fn version_mismatch_returns_none() {
        let mut mem = test_mem();
        mem.save(b"data");
        let raw = mem.backend().read_all();

        let mut mem2 = DeviceMemory::new(RamBackend::new(), 0x00FA, [0x01; 6], 3, 1);
        mem2.backend.write_all(&raw);
        assert!(mem2.load().is_none());
    }

    #[test]
    fn clear() {
        let mut mem = test_mem();
        mem.save(b"data");
        assert!(mem.load().is_some());
        mem.clear();
        assert!(mem.load().is_none());
    }
}
