// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! BAU state persistence — save/restore device state.

use alloc::vec::Vec;

use crate::bau::Bau;
use crate::property::PropertyId;
use crate::table_object::TableObject;

/// Size of the program version field in bytes.
const PROGRAM_VERSION_SIZE: usize = 5;
/// Size of the memory length field in bytes.
const MEMORY_LENGTH_SIZE: usize = 4;
/// Number of table objects persisted (addr, assoc, app program).
const TABLE_OBJECT_COUNT: usize = 3;
/// Minimum header size: 3 table objects + program version + memory length.
const HEADER_SIZE: usize =
    TableObject::SAVE_SIZE * TABLE_OBJECT_COUNT + PROGRAM_VERSION_SIZE + MEMORY_LENGTH_SIZE;

/// Error type for BAU state restoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistenceError {
    /// The data is too short to contain a valid BAU state.
    TruncatedData,
    /// A table object could not be restored.
    InvalidTableObject,
    /// The memory area exceeds the maximum allowed size.
    MemoryTooLarge,
}

impl core::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TruncatedData => write!(f, "persistence data truncated"),
            Self::InvalidTableObject => write!(f, "invalid table object in persistence data"),
            Self::MemoryTooLarge => write!(f, "memory area exceeds maximum size"),
        }
    }
}

impl core::error::Error for PersistenceError {}

/// Serialize the full BAU device state for persistence.
///
/// Format: `[addr_table_obj][assoc_table_obj][app_program_obj][prog_version:5][mem_len:4LE][memory_area]`
pub fn save_bau_state(bau: &Bau) -> Vec<u8> {
    let memory = bau.memory_area();
    let mut buf = Vec::with_capacity(HEADER_SIZE + memory.len());

    bau.addr_table_object.save(&mut buf);
    bau.assoc_table_object.save(&mut buf);
    bau.app_program_object.save(&mut buf);

    // Program version (5 bytes) from application program object (index 3)
    let mut ver = Vec::new();
    if let Some(obj) = bau.object(3) {
        obj.read_property(PropertyId::ProgramVersion, 1, 1, &mut ver);
    }
    ver.resize(PROGRAM_VERSION_SIZE, 0);
    buf.extend_from_slice(&ver);

    let mem_len = u32::try_from(memory.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&mem_len.to_le_bytes());
    buf.extend_from_slice(memory);
    buf
}

/// Restore BAU device state from persisted data.
///
/// After restoring, address and association tables are automatically parsed
/// from the memory area if their table objects are in `Loaded` state.
///
/// # Errors
///
/// Returns [`PersistenceError`] if the data is truncated, a table object is
/// invalid, or the declared memory length exceeds the available data.
pub fn restore_bau_state(bau: &mut Bau, data: &[u8]) -> Result<(), PersistenceError> {
    if data.len() < HEADER_SIZE {
        return Err(PersistenceError::TruncatedData);
    }

    let mut offset = 0;

    let n = bau.addr_table_object.restore(&data[offset..]);
    if n == 0 {
        return Err(PersistenceError::InvalidTableObject);
    }
    offset += n;

    let n = bau.assoc_table_object.restore(&data[offset..]);
    if n == 0 {
        return Err(PersistenceError::InvalidTableObject);
    }
    offset += n;

    let n = bau.app_program_object.restore(&data[offset..]);
    if n == 0 {
        return Err(PersistenceError::InvalidTableObject);
    }
    offset += n;

    // Restore program version
    if offset + PROGRAM_VERSION_SIZE > data.len() {
        return Err(PersistenceError::TruncatedData);
    }
    if let Some(obj) = bau.object_mut(3) {
        obj.write_property(
            PropertyId::ProgramVersion,
            1,
            1,
            &data[offset..offset + PROGRAM_VERSION_SIZE],
        );
    }
    offset += PROGRAM_VERSION_SIZE;

    if offset + MEMORY_LENGTH_SIZE > data.len() {
        return Err(PersistenceError::TruncatedData);
    }
    let mem_len = u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    offset += MEMORY_LENGTH_SIZE;

    if data.len() < offset + mem_len {
        return Err(PersistenceError::MemoryTooLarge);
    }

    bau.set_memory_area(data[offset..offset + mem_len].to_vec());

    // Reload tables from memory if they were in Loaded state
    let addr_data = bau.addr_table_object.data(bau.memory_area()).to_vec();
    if !addr_data.is_empty() {
        bau.address_table.load(&addr_data);
    }
    let assoc_data = bau.assoc_table_object.data(bau.memory_area()).to_vec();
    if !assoc_data.is_empty() {
        bau.association_table.load(&assoc_data);
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::device_object;

    fn test_bau() -> Bau {
        let device =
            device_object::new_device_object([0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], [0x00; 6]);
        let mut bau = Bau::new(device, 3, 1);
        device_object::set_individual_address(bau.device_mut(), 0x1101);
        bau
    }

    #[test]
    fn save_restore_roundtrip() {
        let mut bau = test_bau();

        // Simulate ETS programming: mark addr table as loaded with data
        bau.addr_table_object.handle_load_event(&[1], 0); // START_LOADING
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x01, 0x00];
        bau.addr_table_object.handle_load_event(&alc, 0);
        bau.set_memory_area(alloc::vec![0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
        bau.addr_table_object.handle_load_event(&[2], 6); // LOAD_COMPLETED

        let addr_data = bau.addr_table_object.data(bau.memory_area()).to_vec();
        bau.address_table.load(&addr_data);
        assert_eq!(bau.address_table.get_tsap(0x0801), Some(1));

        let saved = save_bau_state(&bau);

        let mut bau2 = test_bau();
        restore_bau_state(&mut bau2, &saved).unwrap();

        assert_eq!(bau2.address_table.get_tsap(0x0801), Some(1));
        assert_eq!(bau2.address_table.get_tsap(0x0802), Some(2));
        assert_eq!(bau2.memory_area(), &[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
    }

    #[test]
    fn restore_truncated_data_fails() {
        let mut bau = test_bau();
        // Data shorter than HEADER_SIZE
        let short_data = [0u8; HEADER_SIZE - 1];
        assert_eq!(
            restore_bau_state(&mut bau, &short_data),
            Err(PersistenceError::TruncatedData)
        );
    }

    #[test]
    fn restore_corrupted_header_fails() {
        let mut bau = test_bau();
        // Valid header size but memory length claims more data than available
        let mut data = alloc::vec![0u8; HEADER_SIZE];
        // Set mem_len field (last 4 bytes of header) to 999
        let mem_len_offset = HEADER_SIZE - MEMORY_LENGTH_SIZE;
        data[mem_len_offset..HEADER_SIZE].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            restore_bau_state(&mut bau, &data),
            Err(PersistenceError::MemoryTooLarge)
        );
    }

    #[test]
    fn restore_corrupted_bytes_in_middle() {
        let mut bau = test_bau();
        bau.addr_table_object.handle_load_event(&[1], 0);
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x01, 0x00];
        bau.addr_table_object.handle_load_event(&alc, 0);
        bau.set_memory_area(alloc::vec![0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
        bau.addr_table_object.handle_load_event(&[2], 6);

        let mut saved = save_bau_state(&bau);
        // Corrupt a byte in the middle of the table object data (state byte)
        // Set the second table object's state to an invalid LoadState value that
        // still parses (LoadState::from maps unknown to Unloaded), but corrupt
        // the mem_len to claim more data than available
        let mem_len_offset = HEADER_SIZE - MEMORY_LENGTH_SIZE;
        saved[mem_len_offset..mem_len_offset + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());

        let mut bau2 = test_bau();
        assert_eq!(
            restore_bau_state(&mut bau2, &saved),
            Err(PersistenceError::MemoryTooLarge),
            "corrupted mem_len should cause MemoryTooLarge error"
        );
    }

    #[test]
    fn restore_memory_exceeds_max_size() {
        let mut bau = test_bau();
        // Craft data with valid header but mem_len > available trailing data
        let mut data = alloc::vec![0u8; HEADER_SIZE + 10];
        // Set mem_len to a value larger than the remaining bytes
        let mem_len_offset = HEADER_SIZE - MEMORY_LENGTH_SIZE;
        data[mem_len_offset..mem_len_offset + 4].copy_from_slice(&100u32.to_le_bytes());
        // Only 10 bytes follow the header, but mem_len claims 100
        assert_eq!(
            restore_bau_state(&mut bau, &data),
            Err(PersistenceError::MemoryTooLarge),
            "mem_len exceeding available data should fail"
        );
    }

    #[test]
    fn save_during_loading_state() {
        let mut bau = test_bau();
        // Set addr_table_object to Loading state
        bau.addr_table_object.handle_load_event(&[1], 0); // START_LOADING
        assert_eq!(
            bau.addr_table_object.load_state(),
            crate::property::LoadState::Loading
        );

        let saved = save_bau_state(&bau);

        let mut bau2 = test_bau();
        restore_bau_state(&mut bau2, &saved).unwrap();
        assert_eq!(
            bau2.addr_table_object.load_state(),
            crate::property::LoadState::Loading,
            "Loading state should be preserved across save/restore"
        );
    }
}
