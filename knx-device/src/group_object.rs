// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Group Objects (communication objects).
//!
//! A group object is the application-level interface to the KNX bus.
//! Each group object has a value, a state machine (`ComFlag`), and
//! configuration flags from the group object table.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use knx_core::dpt::{self, Dpt, DptError, DptValue};

use crate::group_object_table::GroupObjectTable;

/// Callback invoked when a group object is updated from the bus.
pub type GroupObjectCallback = Box<dyn Fn(&GroupObject) + Send>;

/// Communication flag — tracks the state of a group object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ComFlag {
    /// Value was updated from the bus.
    Updated = 0,
    /// A read request is pending.
    ReadRequest = 1,
    /// A write request is pending (value should be sent to bus).
    WriteRequest = 2,
    /// Currently being transmitted.
    Transmitting = 3,
    /// Last operation completed successfully.
    Ok = 4,
    /// Last operation failed.
    Error = 5,
    /// Value has never been set.
    Uninitialized = 6,
}

/// A single group object (communication object / KO).
///
/// Application code reads and writes values through group objects.
/// The device stack handles bus communication based on the `ComFlag` state.
pub struct GroupObject {
    asap: u16,
    comm_flag: ComFlag,
    data: Vec<u8>,
    dpt: Option<Dpt>,
    on_update: Option<GroupObjectCallback>,
}

impl GroupObject {
    /// Create a new group object with the given ASAP and data size.
    pub fn new(asap: u16, size: usize) -> Self {
        Self {
            asap,
            comm_flag: ComFlag::Uninitialized,
            data: vec![0u8; size],
            dpt: None,
            on_update: None,
        }
    }

    /// Create a group object with a specific DPT.
    pub fn with_dpt(asap: u16, dpt: Dpt) -> Self {
        Self {
            asap,
            comm_flag: ComFlag::Uninitialized,
            data: vec![0u8; dpt.data_length() as usize],
            dpt: Some(dpt),
            on_update: None,
        }
    }

    /// Set the DPT for this group object.
    pub fn set_dpt(&mut self, dpt: Dpt) {
        self.dpt = Some(dpt);
        let needed = dpt.data_length() as usize;
        if self.data.len() < needed {
            self.data.resize(needed, 0);
        }
    }

    /// The configured DPT, if any.
    pub const fn dpt(&self) -> Option<Dpt> {
        self.dpt
    }

    /// Register a callback invoked when the value is updated from the bus.
    pub fn on_update(&mut self, callback: impl Fn(&Self) + Send + 'static) {
        self.on_update = Some(Box::new(callback));
    }

    /// The ASAP (application service access point) — the group object number (1-based).
    pub const fn asap(&self) -> u16 {
        self.asap
    }

    /// Current communication flag.
    pub const fn comm_flag(&self) -> ComFlag {
        self.comm_flag
    }

    /// Set the communication flag. Application code should set to `Ok` after
    /// processing an `Updated` value.
    pub const fn set_comm_flag(&mut self, flag: ComFlag) {
        self.comm_flag = flag;
    }

    /// Whether the value has been initialized (set from bus or application).
    pub const fn initialized(&self) -> bool {
        !matches!(self.comm_flag, ComFlag::Uninitialized)
    }

    /// Request a read from the bus. Sets flag to `ReadRequest`.
    pub const fn request_object_read(&mut self) {
        self.comm_flag = ComFlag::ReadRequest;
    }

    /// Mark the object as written (value should be sent to bus).
    /// Sets flag to `WriteRequest`.
    pub const fn object_written(&mut self) {
        self.comm_flag = ComFlag::WriteRequest;
    }

    /// Raw value bytes.
    pub fn value_ref(&self) -> &[u8] {
        &self.data
    }

    /// Mutable raw value bytes.
    pub fn value_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Size of the value in bytes.
    pub fn value_size(&self) -> usize {
        self.data.len()
    }

    /// Resize the data buffer (called during GO table initialization).
    pub fn resize_data(&mut self, new_size: usize) {
        self.data.resize(new_size, 0);
    }

    /// Write a value and mark as `WriteRequest` (triggers bus send).
    pub fn write_value(&mut self, data: &[u8]) {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        self.comm_flag = ComFlag::WriteRequest;
    }

    /// Write a value without triggering a bus send.
    pub fn write_value_no_send(&mut self, data: &[u8]) {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        if self.comm_flag == ComFlag::Uninitialized {
            self.comm_flag = ComFlag::Ok;
        }
    }

    /// Called by the transport layer when a value is received from the bus.
    pub fn value_from_bus(&mut self, data: &[u8]) {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        self.comm_flag = ComFlag::Updated;
        if let Some(cb) = &self.on_update {
            cb(self);
        }
    }

    /// Read the value as a decoded [`DptValue`] using the configured DPT.
    ///
    /// # Errors
    ///
    /// Returns [`DptError`] if no DPT is configured or decoding fails.
    pub fn value(&self) -> Result<DptValue, DptError> {
        let dpt = self.dpt.ok_or(DptError::NoDpt)?;
        dpt::decode(dpt, &self.data)
    }

    /// Write a [`DptValue`] using the configured DPT, and mark as `WriteRequest`.
    ///
    /// # Errors
    ///
    /// Returns [`DptError`] if no DPT is configured or encoding fails.
    pub fn set_value(&mut self, value: &DptValue) -> Result<(), DptError> {
        let dpt = self.dpt.ok_or(DptError::NoDpt)?;
        let encoded = dpt::encode(dpt, value)?;
        let len = encoded.len().min(self.data.len());
        self.data[..len].copy_from_slice(&encoded[..len]);
        self.comm_flag = ComFlag::WriteRequest;
        Ok(())
    }

    /// Write a value only if it differs from the current value (avoids bus traffic).
    /// Returns `true` if the value changed and a write was queued.
    ///
    /// # Errors
    ///
    /// Returns [`DptError`] if no DPT is configured or encoding fails.
    pub fn set_value_if_changed(&mut self, value: &DptValue) -> Result<bool, DptError> {
        let dpt = self.dpt.ok_or(DptError::NoDpt)?;
        let encoded = dpt::encode(dpt, value)?;
        let len = encoded.len().min(self.data.len());
        if self.data[..len] == encoded[..len] {
            return Ok(false);
        }
        self.data[..len].copy_from_slice(&encoded[..len]);
        self.comm_flag = ComFlag::WriteRequest;
        Ok(true)
    }

    /// Called by the transport layer when a transmit completes.
    pub const fn transmit_done(&mut self, success: bool) {
        self.comm_flag = if success { ComFlag::Ok } else { ComFlag::Error };
    }
}

/// A collection of group objects managed by the device stack.
pub struct GroupObjectStore {
    objects: Vec<GroupObject>,
}

impl GroupObjectStore {
    /// Create a store with the given number of group objects, each with `size` bytes.
    pub fn new(count: u16, default_size: usize) -> Self {
        let mut objects = Vec::with_capacity(count as usize);
        for i in 0..count {
            objects.push(GroupObject::new(i + 1, default_size));
        }
        Self { objects }
    }

    /// Get a group object by ASAP (1-based).
    pub fn get(&self, asap: u16) -> Option<&GroupObject> {
        if asap == 0 {
            return None;
        }
        self.objects.get((asap - 1) as usize)
    }

    /// Get a mutable group object by ASAP (1-based).
    pub fn get_mut(&mut self, asap: u16) -> Option<&mut GroupObject> {
        if asap == 0 {
            return None;
        }
        self.objects.get_mut((asap - 1) as usize)
    }

    /// Number of group objects.
    pub fn count(&self) -> u16 {
        if self.objects.len() > u16::MAX as usize {
            return 0;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "guarded by bounds check above"
        )]
        let count = self.objects.len() as u16;
        count
    }

    /// Find the next group object with `WriteRequest` or `ReadRequest` flag.
    /// Returns the ASAP of the pending object, if any.
    pub fn next_pending(&self) -> Option<u16> {
        self.objects.iter().find_map(|go| {
            if go.comm_flag == ComFlag::WriteRequest || go.comm_flag == ComFlag::ReadRequest {
                Some(go.asap)
            } else {
                None
            }
        })
    }

    /// Find the next group object with `Updated` flag.
    pub fn next_updated(&self) -> Option<u16> {
        self.objects
            .iter()
            .find(|go| go.comm_flag == ComFlag::Updated)
            .map(|go| go.asap)
    }

    /// Re-initialize group objects from the loaded GO table.
    /// Resizes each GO's data buffer to match the `value_type` from the descriptor.
    pub fn reinitialize_from_table(&mut self, table: &GroupObjectTable) {
        let count = table.entry_count();
        while self.objects.len() < count as usize {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "GO count bounded by u16 entry_count"
            )]
            let asap = self.objects.len() as u16 + 1;
            self.objects.push(GroupObject::new(asap, 1));
        }
        self.objects.truncate(count as usize);
        for asap in 1..=count {
            if let Some(desc) = table.get_descriptor(asap) {
                let size = desc.go_size();
                if let Some(go) = self.get_mut(asap) {
                    if go.value_size() != size {
                        go.resize_data(size);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_core::dpt::{DPT_SWITCH, DPT_VALUE_TEMP, DptValue};

    #[test]
    fn new_group_object_is_uninitialized() {
        let go = GroupObject::new(1, 1);
        assert_eq!(go.comm_flag(), ComFlag::Uninitialized);
        assert!(!go.initialized());
        assert_eq!(go.asap(), 1);
        assert_eq!(go.value_size(), 1);
    }

    #[test]
    fn write_value_sets_write_request() {
        let mut go = GroupObject::new(1, 2);
        go.write_value(&[0x0C, 0x34]);
        assert_eq!(go.comm_flag(), ComFlag::WriteRequest);
        assert_eq!(go.value_ref(), &[0x0C, 0x34]);
    }

    #[test]
    fn write_no_send_initializes() {
        let mut go = GroupObject::new(1, 1);
        assert!(!go.initialized());
        go.write_value_no_send(&[42]);
        assert!(go.initialized());
        assert_eq!(go.comm_flag(), ComFlag::Ok);
        assert_eq!(go.value_ref(), &[42]);
    }

    #[test]
    fn value_from_bus_sets_updated() {
        let mut go = GroupObject::new(1, 1);
        go.value_from_bus(&[1]);
        assert_eq!(go.comm_flag(), ComFlag::Updated);
        assert!(go.initialized());
    }

    #[test]
    fn request_read() {
        let mut go = GroupObject::new(1, 1);
        go.request_object_read();
        assert_eq!(go.comm_flag(), ComFlag::ReadRequest);
    }

    #[test]
    fn transmit_done_success() {
        let mut go = GroupObject::new(1, 1);
        go.write_value(&[1]);
        go.set_comm_flag(ComFlag::Transmitting);
        go.transmit_done(true);
        assert_eq!(go.comm_flag(), ComFlag::Ok);
    }

    #[test]
    fn transmit_done_error() {
        let mut go = GroupObject::new(1, 1);
        go.set_comm_flag(ComFlag::Transmitting);
        go.transmit_done(false);
        assert_eq!(go.comm_flag(), ComFlag::Error);
    }

    #[test]
    fn store_get_by_asap() {
        let store = GroupObjectStore::new(3, 1);
        assert_eq!(store.count(), 3);
        assert!(store.get(0).is_none());
        assert_eq!(store.get(1).unwrap().asap(), 1);
        assert_eq!(store.get(3).unwrap().asap(), 3);
        assert!(store.get(4).is_none());
    }

    #[test]
    fn store_next_pending() {
        let mut store = GroupObjectStore::new(3, 1);
        assert!(store.next_pending().is_none());

        store.get_mut(2).unwrap().write_value(&[1]);
        assert_eq!(store.next_pending(), Some(2));
    }

    #[test]
    fn store_next_updated() {
        let mut store = GroupObjectStore::new(3, 1);
        assert!(store.next_updated().is_none());

        store.get_mut(3).unwrap().value_from_bus(&[1]);
        assert_eq!(store.next_updated(), Some(3));
    }

    #[test]
    fn dpt_aware_value() {
        let mut go = GroupObject::with_dpt(1, DPT_VALUE_TEMP);
        go.set_value(&DptValue::Float(21.5)).unwrap();
        assert_eq!(go.comm_flag(), ComFlag::WriteRequest);

        let val = go.value().unwrap().as_f64().unwrap();
        assert!((val - 21.5).abs() < 0.1, "got {val}");
    }

    #[test]
    fn set_value_if_changed_no_change() {
        let mut go = GroupObject::with_dpt(1, DPT_SWITCH);
        go.set_value(&DptValue::Bool(true)).unwrap();
        go.set_comm_flag(ComFlag::Ok);

        let changed = go.set_value_if_changed(&DptValue::Bool(true)).unwrap();
        assert!(!changed);
        assert_eq!(go.comm_flag(), ComFlag::Ok); // not changed to WriteRequest
    }

    #[test]
    fn set_value_if_changed_with_change() {
        let mut go = GroupObject::with_dpt(1, DPT_SWITCH);
        go.set_value(&DptValue::Bool(false)).unwrap();
        go.set_comm_flag(ComFlag::Ok);

        let changed = go.set_value_if_changed(&DptValue::Bool(true)).unwrap();
        assert!(changed);
        assert_eq!(go.comm_flag(), ComFlag::WriteRequest);
    }

    #[test]
    fn callback_on_bus_update() {
        use alloc::sync::Arc;
        use core::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let mut go = GroupObject::new(1, 1);
        go.on_update(move |_| {
            called_clone.store(true, Ordering::Relaxed);
        });

        go.value_from_bus(&[1]);
        assert!(called.load(Ordering::Relaxed));
    }

    #[test]
    fn state_machine_full_cycle() {
        let mut go = GroupObject::new(1, 1);

        // Uninitialized → WriteRequest (app writes value)
        assert_eq!(go.comm_flag(), ComFlag::Uninitialized);
        go.write_value(&[1]);
        assert_eq!(go.comm_flag(), ComFlag::WriteRequest);

        // WriteRequest → Transmitting (stack picks it up)
        go.set_comm_flag(ComFlag::Transmitting);
        assert_eq!(go.comm_flag(), ComFlag::Transmitting);

        // Transmitting → Ok (ack received)
        go.transmit_done(true);
        assert_eq!(go.comm_flag(), ComFlag::Ok);

        // Ok → Updated (value received from bus)
        go.value_from_bus(&[2]);
        assert_eq!(go.comm_flag(), ComFlag::Updated);

        // Updated → Ok (app acknowledges)
        go.set_comm_flag(ComFlag::Ok);
        assert_eq!(go.comm_flag(), ComFlag::Ok);
    }

    #[test]
    fn resize_data() {
        let mut go = GroupObject::new(1, 1);
        assert_eq!(go.value_size(), 1);
        go.resize_data(4);
        assert_eq!(go.value_size(), 4);
        assert_eq!(go.value_ref(), &[0, 0, 0, 0]);
    }

    #[test]
    fn reinitialize_from_table_resizes_gos() {
        use crate::group_object_table::GroupObjectTable;

        let mut store = GroupObjectStore::new(2, 1);
        assert_eq!(store.get(1).unwrap().value_size(), 1);
        assert_eq!(store.get(2).unwrap().value_size(), 1);

        // Build GO table: GO1 value_type=8 (2 bytes), GO2 value_type=14 (14 bytes)
        let go1: u16 = (1 << 10) | 8; // comm + value_type=8
        let go2: u16 = (1 << 10) | 14; // comm + value_type=14
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&go1.to_be_bytes());
        data.extend_from_slice(&go2.to_be_bytes());
        let mut table = GroupObjectTable::new();
        table.load(&data);

        store.reinitialize_from_table(&table);
        assert_eq!(store.get(1).unwrap().value_size(), 2);
        assert_eq!(store.get(2).unwrap().value_size(), 14);
    }

    #[test]
    fn reinitialize_from_table_grows_store() {
        use crate::group_object_table::GroupObjectTable;

        let mut store = GroupObjectStore::new(1, 1);
        assert_eq!(store.count(), 1);

        // Table has 3 GOs
        let go: u16 = (1 << 10) | 7; // comm + value_type=7 (1 byte)
        let mut data = Vec::new();
        data.extend_from_slice(&3u16.to_be_bytes());
        for _ in 0..3 {
            data.extend_from_slice(&go.to_be_bytes());
        }
        let mut table = GroupObjectTable::new();
        table.load(&data);

        store.reinitialize_from_table(&table);
        assert_eq!(store.count(), 3);
        assert_eq!(store.get(3).unwrap().asap(), 3);
    }

    #[test]
    fn reinitialize_from_table_truncates_store() {
        use crate::group_object_table::GroupObjectTable;

        let mut store = GroupObjectStore::new(5, 1);
        assert_eq!(store.count(), 5);

        // Table has only 2 GOs
        let go: u16 = (1 << 10) | 7;
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&go.to_be_bytes());
        data.extend_from_slice(&go.to_be_bytes());
        let mut table = GroupObjectTable::new();
        table.load(&data);

        store.reinitialize_from_table(&table);
        assert_eq!(store.count(), 2);
    }
}
