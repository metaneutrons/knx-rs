// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Table Object — base for Address Table, Association Table, Application Program.
//!
//! Implements the ETS Load State Machine (KNX 3/5/1 §4.10):
//! `Unloaded → Loading → Loaded`, with `Error` as a terminal state.
//!
//! ETS programs a table object by:
//! 1. Writing `LoadStateControl = LE_START_LOADING` → state becomes `Loading`
//! 2. Sending `AdditionalLoadControls` with allocation size → memory is reserved
//! 3. Writing table data via `MemoryWrite` at the allocated offset
//! 4. Writing `LoadStateControl = LE_LOAD_COMPLETED` → state becomes `Loaded`

use alloc::vec::Vec;

use crate::property::{LoadEvent, LoadState};

// ── Load Error ────────────────────────────────────────────────

/// Load error codes specific to table object operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LoadError {
    /// No error.
    NoFault = 0,
    /// Received an undefined load command.
    UndefinedLoadCommand = 1,
    /// Invalid opcode in additional load controls.
    InvalidOpcode = 2,
    /// Requested table size exceeds maximum.
    MaxTableLengthExceeded = 3,
}

// ── Table Object ──────────────────────────────────────────────

/// Maximum allowed table size (256 KiB).
/// Maximum allowed table/memory size (256 KiB).
pub const MAX_MEMORY_SIZE: usize = 256 * 1024;

/// Minimum data length for `AdditionalLoadControls` (opcode + size + fill mode + fill byte).
const ALC_MIN_LENGTH: usize = 8;
/// Opcode for `AllocAbsDataSeg` in `AdditionalLoadControls`.
const ALC_OPCODE_ALLOC: u8 = 0x0B;
/// Fill mode value indicating memory should be filled.
const ALC_FILL_ENABLED: u8 = 0x01;

/// A table object that can be loaded by ETS via the Load State Machine.
///
/// The table data lives inside the BAU's `memory_area` at `data_offset`.
/// The offset and size are determined by ETS during the download process.
pub struct TableObject {
    state: LoadState,
    error: LoadError,
    /// Offset into BAU `memory_area` where table data starts.
    data_offset: u32,
    /// Size of the allocated table data in bytes.
    data_size: u32,
}

impl TableObject {
    /// Create a new unloaded table object.
    pub const fn new() -> Self {
        Self {
            state: LoadState::Unloaded,
            error: LoadError::NoFault,
            data_offset: 0,
            data_size: 0,
        }
    }

    /// Reset data offset and size to zero.
    const fn reset_data(&mut self) {
        self.data_offset = 0;
        self.data_size = 0;
    }

    /// Current load state.
    pub const fn load_state(&self) -> LoadState {
        self.state
    }

    /// Current error code.
    pub const fn error_code(&self) -> LoadError {
        self.error
    }

    /// Offset of table data in BAU memory area.
    pub const fn data_offset(&self) -> u32 {
        self.data_offset
    }

    /// Size of table data in bytes.
    pub const fn data_size(&self) -> u32 {
        self.data_size
    }

    /// Read the table data from the BAU memory area.
    pub fn data<'a>(&self, memory_area: &'a [u8]) -> &'a [u8] {
        if self.state != LoadState::Loaded || self.data_size == 0 {
            return &[];
        }
        let start = self.data_offset as usize;
        let end = start + self.data_size as usize;
        if end <= memory_area.len() {
            &memory_area[start..end]
        } else {
            &[]
        }
    }

    /// Handle a write to `PID_LOAD_STATE_CONTROL`.
    ///
    /// `memory_area_len` is the current size of the BAU memory area.
    ///
    /// Returns `(became_loaded, fill_request)`:
    /// - `became_loaded`: true if state changed to `Loaded` (caller should parse table data)
    /// - `fill_request`: `Some((offset, size, fill_byte))` if memory should be filled
    pub fn handle_load_event(
        &mut self,
        data: &[u8],
        memory_area_len: usize,
    ) -> (bool, Option<(u32, u32, u8)>) {
        if data.is_empty() {
            return (false, None);
        }
        let Some(event) = LoadEvent::from_byte(data[0]) else {
            self.state = LoadState::Error;
            self.error = LoadError::UndefinedLoadCommand;
            return (false, None);
        };

        match self.state {
            LoadState::Unloaded => {
                self.on_unloaded(event);
                (false, None)
            }
            LoadState::Loading => self.on_loading(event, data, memory_area_len),
            LoadState::Loaded => {
                self.on_loaded(event);
                (false, None)
            }
            LoadState::Error => {
                self.on_error(event);
                (false, None)
            }
            _ => (false, None),
        }
    }

    const fn on_unloaded(&mut self, event: LoadEvent) -> bool {
        match event {
            LoadEvent::StartLoading => {
                self.state = LoadState::Loading;
                self.reset_data();
                false
            }
            LoadEvent::Noop
            | LoadEvent::LoadCompleted
            | LoadEvent::Unload
            | LoadEvent::AdditionalLoadControls => false,
        }
    }

    fn on_loading(
        &mut self,
        event: LoadEvent,
        data: &[u8],
        memory_area_len: usize,
    ) -> (bool, Option<(u32, u32, u8)>) {
        match event {
            LoadEvent::Noop | LoadEvent::StartLoading => (false, None),
            LoadEvent::LoadCompleted => {
                self.state = LoadState::Loaded;
                (true, None)
            }
            LoadEvent::Unload => {
                self.state = LoadState::Unloaded;
                self.reset_data();
                (false, None)
            }
            LoadEvent::AdditionalLoadControls => {
                let fill = self.handle_additional_load_controls(data, memory_area_len);
                (false, fill)
            }
        }
    }

    const fn on_loaded(&mut self, event: LoadEvent) -> bool {
        match event {
            LoadEvent::Noop | LoadEvent::LoadCompleted => false,
            LoadEvent::StartLoading => {
                self.state = LoadState::Loading;
                self.reset_data();
                false
            }
            LoadEvent::Unload => {
                self.state = LoadState::Unloaded;
                self.reset_data();
                false
            }
            LoadEvent::AdditionalLoadControls => {
                self.state = LoadState::Error;
                self.error = LoadError::InvalidOpcode;
                false
            }
        }
    }

    const fn on_error(&mut self, event: LoadEvent) -> bool {
        if matches!(event, LoadEvent::Unload) {
            self.state = LoadState::Unloaded;
            self.reset_data();
        }
        false
    }

    /// Handle `AdditionalLoadControls` — allocate memory for table data.
    ///
    /// Data format: `[0x03] [ALC_OPCODE_ALLOC] [size:4be] [fill_mode:1] [fill_byte:1]`
    ///
    /// Returns `Some((offset, size, fill_byte))` if memory should be filled,
    /// `None` if no fill is needed or on error.
    fn handle_additional_load_controls(
        &mut self,
        data: &[u8],
        memory_area_len: usize,
    ) -> Option<(u32, u32, u8)> {
        if data.len() < ALC_MIN_LENGTH || data[1] != ALC_OPCODE_ALLOC {
            self.state = LoadState::Error;
            self.error = LoadError::InvalidOpcode;
            return None;
        }
        let size = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        if size as usize > MAX_MEMORY_SIZE {
            self.state = LoadState::Error;
            self.error = LoadError::MaxTableLengthExceeded;
            return None;
        }
        let do_fill = data[6] == ALC_FILL_ENABLED;
        let fill_byte = data[7];
        let offset = u32::try_from(memory_area_len).unwrap_or(u32::MAX);
        self.data_offset = offset;
        self.data_size = size;
        if do_fill {
            Some((offset, size, fill_byte))
        } else {
            None
        }
    }

    /// Table reference (offset into memory area). Returns 0 if not loaded.
    pub const fn table_reference(&self) -> u32 {
        if matches!(self.state, LoadState::Loaded) {
            self.data_offset
        } else {
            0
        }
    }

    /// MCB table data: `[segment_size:4be] [crc_control:1] [access:1] [crc16:2be]`.
    ///
    /// Returns 8 bytes for `PID_MCB_TABLE` property read. Empty if not loaded.
    pub fn mcb_table(&self, memory_area: &[u8]) -> [u8; 8] {
        if !matches!(self.state, LoadState::Loaded) || self.data_size == 0 {
            return [0; 8];
        }
        let data = self.data(memory_area);
        let crc = crc16_ccitt(data);
        let size = self.data_size.to_be_bytes();
        let crc_be = crc.to_be_bytes();
        [
            size[0], size[1], size[2], size[3], 0x00, 0xFF, crc_be[0], crc_be[1],
        ]
    }

    // ── Persistence ───────────────────────────────────────────

    /// Serialized size in bytes: state(1) + offset(4) + size(4) = 9.
    pub const SAVE_SIZE: usize = 9;

    /// Serialize table object state for persistence.
    pub fn save(&self, buf: &mut Vec<u8>) {
        buf.push(self.state as u8);
        buf.extend_from_slice(&self.data_offset.to_le_bytes());
        buf.extend_from_slice(&self.data_size.to_le_bytes());
    }

    /// Restore table object state from persisted data.
    ///
    /// Returns the number of bytes consumed, or 0 on error.
    pub fn restore(&mut self, data: &[u8]) -> usize {
        if data.len() < Self::SAVE_SIZE {
            return 0;
        }
        self.state = LoadState::from(data[0]);
        self.data_offset = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
        self.data_size = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);
        Self::SAVE_SIZE
    }
}

impl Default for TableObject {
    fn default() -> Self {
        Self::new()
    }
}

/// CRC-16 CCITT (polynomial 0x1021, init 0xFFFF).
/// Used for `PID_MCB_TABLE` integrity check.
fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn initial_state_is_unloaded() {
        let to = TableObject::new();
        assert_eq!(to.load_state(), LoadState::Unloaded);
        assert_eq!(to.data_offset(), 0);
        assert_eq!(to.data_size(), 0);
    }

    #[test]
    fn load_state_machine_happy_path() {
        let mut to = TableObject::new();

        // Start loading
        let (became_loaded, _fill) = to.handle_load_event(&[1], 0); // LE_START_LOADING
        assert!(!became_loaded);
        assert_eq!(to.load_state(), LoadState::Loading);

        // Additional load controls: allocate 100 bytes
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x64, 0x01, 0x00];
        let (became_loaded, _fill) = to.handle_load_event(&alc, 200);
        assert!(!became_loaded);
        assert_eq!(to.data_offset(), 200);
        assert_eq!(to.data_size(), 100);

        // Load completed
        let (became_loaded, _fill) = to.handle_load_event(&[2], 300); // LE_LOAD_COMPLETED
        assert!(became_loaded);
        assert_eq!(to.load_state(), LoadState::Loaded);
    }

    #[test]
    fn unload_from_loaded() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        to.handle_load_event(&[2], 0);
        assert_eq!(to.load_state(), LoadState::Loaded);

        to.handle_load_event(&[4], 0); // LE_UNLOAD
        assert_eq!(to.load_state(), LoadState::Unloaded);
        assert_eq!(to.data_offset(), 0);
        assert_eq!(to.data_size(), 0);
    }

    #[test]
    fn invalid_event_causes_error() {
        let mut to = TableObject::new();
        to.handle_load_event(&[0xFF], 0);
        assert_eq!(to.load_state(), LoadState::Error);
    }

    #[test]
    fn unload_from_error() {
        let mut to = TableObject::new();
        to.handle_load_event(&[0xFF], 0);
        assert_eq!(to.load_state(), LoadState::Error);

        to.handle_load_event(&[4], 0); // LE_UNLOAD
        assert_eq!(to.load_state(), LoadState::Unloaded);
    }

    #[test]
    fn save_restore_roundtrip() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x10, 0x01, 0x00];
        to.handle_load_event(&alc, 50);
        to.handle_load_event(&[2], 66);

        let mut buf = Vec::new();
        to.save(&mut buf);
        assert_eq!(buf.len(), TableObject::SAVE_SIZE);

        let mut restored = TableObject::new();
        let consumed = restored.restore(&buf);
        assert_eq!(consumed, TableObject::SAVE_SIZE);
        assert_eq!(restored.load_state(), LoadState::Loaded);
        assert_eq!(restored.data_offset(), 50);
        assert_eq!(restored.data_size(), 16);
    }

    #[test]
    fn data_returns_slice_when_loaded() {
        let mut to = TableObject::new();
        to.state = LoadState::Loaded;
        to.data_offset = 2;
        to.data_size = 3;

        let mem = vec![0xAA, 0xBB, 0x01, 0x02, 0x03, 0xCC];
        assert_eq!(to.data(&mem), &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn data_returns_empty_when_unloaded() {
        let to = TableObject::new();
        let mem = vec![0x01, 0x02, 0x03];
        assert_eq!(to.data(&mem), &[] as &[u8]);
    }

    #[test]
    fn mcb_table_returns_correct_format() {
        let mut to = TableObject::new();
        to.state = LoadState::Loaded;
        to.data_offset = 0;
        to.data_size = 4;

        let mem = vec![0x01, 0x02, 0x03, 0x04];
        let crc = crc16_ccitt(&mem);
        let crc_be = crc.to_be_bytes();
        let expected = [0x00, 0x00, 0x00, 0x04, 0x00, 0xFF, crc_be[0], crc_be[1]];
        assert_eq!(to.mcb_table(&mem), expected);
    }

    #[test]
    fn mcb_table_returns_zeros_when_unloaded() {
        let to = TableObject::new();
        assert_eq!(to.mcb_table(&[]), [0; 8]);
    }

    #[test]
    fn crc16_ccitt_known_vector() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn table_reference_returns_offset_when_loaded() {
        let mut to = TableObject::new();
        to.state = LoadState::Loaded;
        to.data_offset = 42;
        assert_eq!(to.table_reference(), 42);
    }

    #[test]
    fn table_reference_returns_zero_when_unloaded() {
        let to = TableObject::new();
        assert_eq!(to.table_reference(), 0);
    }

    #[test]
    fn additional_load_controls_with_fill() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x20, 0x01, 0xAA];
        let (_loaded, fill) = to.handle_load_event(&alc, 100);
        assert_eq!(fill, Some((100, 32, 0xAA)));
    }

    #[test]
    fn additional_load_controls_invalid_opcode() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        let alc = [0x03, 0xFF, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00];
        to.handle_load_event(&alc, 0);
        assert_eq!(to.load_state(), LoadState::Error);
        assert_eq!(to.error_code(), LoadError::InvalidOpcode);
    }

    #[test]
    fn handle_load_event_empty_data() {
        let mut to = TableObject::new();
        let (became_loaded, fill) = to.handle_load_event(&[], 0);
        assert!(!became_loaded);
        assert_eq!(fill, None);
    }

    #[test]
    fn additional_load_controls_short_data() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        // Only 7 bytes — less than required 8
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x10, 0x00];
        to.handle_load_event(&alc, 0);
        assert_eq!(to.load_state(), LoadState::Error);
    }

    #[test]
    fn additional_load_controls_no_fill() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        // do_fill byte = 0x00 (not 0x01)
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00];
        let (_loaded, fill) = to.handle_load_event(&alc, 0);
        assert_eq!(fill, None);
    }

    #[test]
    fn data_out_of_bounds_returns_empty() {
        let mut to = TableObject::new();
        to.state = LoadState::Loaded;
        to.data_offset = 10;
        to.data_size = 20;
        let mem = vec![0u8; 15]; // offset(10) + size(20) = 30 > 15
        assert_eq!(to.data(&mem), &[] as &[u8]);
    }

    #[test]
    fn unloading_and_load_completing_states_ignored() {
        for state in [LoadState::Unloading, LoadState::LoadCompleting] {
            let mut to = TableObject::new();
            to.state = state;
            let (became_loaded, fill) = to.handle_load_event(&[1], 0); // StartLoading
            assert!(!became_loaded);
            assert_eq!(fill, None);
            assert_eq!(to.load_state(), state);
        }
    }

    #[test]
    fn additional_load_controls_exceeds_max_size() {
        let mut to = TableObject::new();
        to.handle_load_event(&[1], 0);
        // Size = 0x00100000 (1 MiB) > MAX_TABLE_SIZE (256 KiB)
        let alc = [0x03, 0x0B, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00];
        to.handle_load_event(&alc, 0);
        assert_eq!(to.load_state(), LoadState::Error);
        assert_eq!(to.error_code(), LoadError::MaxTableLengthExceeded);
    }
}
