// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Bus Access Unit (BAU) — the main KNX device controller.
//!
//! Ties together all device components and processes incoming CEMI frames.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use knx_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_core::cemi::CemiFrame;
use knx_core::message::MessageCode;
use knx_core::tpdu::Tpdu;
use knx_core::types::Priority;

use crate::address_table::AddressTable;
use crate::application_layer::{self, AppIndication};
use crate::association_table::AssociationTable;
use crate::device_object;
use crate::group_object::{ComFlag, GroupObjectStore};
use crate::group_object_table::GroupObjectTable;
use crate::interface_object::InterfaceObject;
use crate::property::{Property, PropertyId};
use crate::transport_layer::TransportLayer;

/// Mask version for IP devices (System B).
pub const MASK_VERSION_IP: u16 = 0x57B0;

/// The Bus Access Unit — main device controller.
pub struct Bau {
    /// Interface objects indexed by object index (0=device, 1=address table, etc.).
    objects: Vec<InterfaceObject>,
    /// Address table.
    pub address_table: AddressTable,
    /// Association table.
    pub association_table: AssociationTable,
    /// Group object table.
    pub group_object_table: GroupObjectTable,
    /// Group objects.
    pub group_objects: GroupObjectStore,
    /// Transport layer.
    transport: TransportLayer,
    /// Memory area for MemoryRead/Write (table data loaded by ETS).
    memory_area: Vec<u8>,
    /// Outgoing frame queue.
    outbox: VecDeque<CemiFrame>,
}

impl Bau {
    /// Create a new BAU.
    ///
    /// `device` is the device object (index 0). Additional interface objects
    /// can be added with `add_object()`.
    pub fn new(device: InterfaceObject, group_object_count: u16, default_go_size: usize) -> Self {
        Self {
            objects: vec![device],
            address_table: AddressTable::new(),
            association_table: AssociationTable::new(),
            group_object_table: GroupObjectTable::new(),
            group_objects: GroupObjectStore::new(group_object_count, default_go_size),
            transport: TransportLayer::new(),
            memory_area: Vec::new(),
            outbox: VecDeque::new(),
        }
    }

    /// The device object (index 0).
    pub fn device(&self) -> &InterfaceObject {
        &self.objects[0]
    }

    /// Mutable device object.
    pub fn device_mut(&mut self) -> &mut InterfaceObject {
        &mut self.objects[0]
    }

    /// Add an interface object. Returns its index.
    #[expect(clippy::cast_possible_truncation)]
    pub fn add_object(&mut self, obj: InterfaceObject) -> u8 {
        let idx = self.objects.len() as u8;
        self.objects.push(obj);
        idx
    }

    /// Get an interface object by index.
    pub fn object(&self, index: u8) -> Option<&InterfaceObject> {
        self.objects.get(index as usize)
    }

    /// Get a mutable interface object by index.
    pub fn object_mut(&mut self, index: u8) -> Option<&mut InterfaceObject> {
        self.objects.get_mut(index as usize)
    }

    /// The device's individual address.
    pub fn individual_address(&self) -> IndividualAddress {
        device_object::individual_address(self.device())
    }

    /// Process an incoming CEMI frame.
    pub fn process_frame(&mut self, frame: &CemiFrame) {
        let Some(tpdu) = frame.tpdu() else { return };

        match &tpdu {
            Tpdu::Control { tpdu_type, .. } => {
                self.process_control_tpdu(frame, *tpdu_type);
            }
            Tpdu::Data { apdu, .. } => {
                self.process_data_tpdu(frame, apdu);
            }
        }
    }

    fn process_control_tpdu(&mut self, frame: &CemiFrame, tpdu_type: knx_core::message::TpduType) {
        match tpdu_type {
            knx_core::message::TpduType::Connect => {
                self.transport.connect(frame.source_address().raw());
            }
            knx_core::message::TpduType::Disconnect => {
                self.transport.disconnect();
            }
            _ => {}
        }
    }

    fn process_data_tpdu(&mut self, frame: &CemiFrame, apdu: &knx_core::apdu::Apdu) {
        let source = frame.source_address().raw();
        let Some(indication) = application_layer::parse_indication(apdu.apdu_type, &apdu.data)
        else {
            return;
        };

        self.dispatch_indication(frame, source, indication);
    }

    fn dispatch_indication(&mut self, frame: &CemiFrame, source: u16, indication: AppIndication) {
        match indication {
            AppIndication::GroupValueWrite { data, .. } => {
                self.handle_group_value_write(frame, &data);
            }
            AppIndication::GroupValueResponse { data, .. } => {
                self.handle_group_value_response(frame, &data);
            }
            AppIndication::GroupValueRead { .. } => {
                self.handle_group_value_read(frame);
            }
            AppIndication::IndividualAddressWrite { address }
                if device_object::prog_mode(self.device()) =>
            {
                device_object::set_individual_address(self.device_mut(), address);
            }
            AppIndication::IndividualAddressRead if device_object::prog_mode(self.device()) => {
                self.queue_individual_address_response();
            }
            AppIndication::PropertyValueRead {
                object_index,
                property_id,
                count,
                start_index,
            } => {
                self.handle_property_read(source, object_index, property_id, count, start_index);
            }
            AppIndication::PropertyValueWrite {
                object_index,
                property_id,
                count,
                start_index,
                data,
            } => {
                self.handle_property_write(
                    source,
                    object_index,
                    property_id,
                    count,
                    start_index,
                    &data,
                );
            }
            AppIndication::DeviceDescriptorRead { descriptor_type: 0 } => {
                self.queue_device_descriptor_response(source);
            }
            AppIndication::DeviceDescriptorRead { .. } => {
                // Unsupported descriptor type — respond with type 0x3F (C++ ref behavior)
                self.queue_device_descriptor_unsupported(source);
            }
            AppIndication::MemoryRead { count, address } => {
                self.handle_memory_read(source, count, address);
            }
            AppIndication::MemoryWrite {
                count: _,
                address,
                data,
            } => {
                self.handle_memory_write(address, &data);
            }
            _ => {}
        }
    }

    /// Poll for outgoing frames. Drives pending group object writes/reads.
    pub fn poll(&mut self) {
        while let Some(asap) = self.group_objects.next_pending() {
            let Some(go) = self.group_objects.get(asap) else {
                break;
            };
            let flag = go.comm_flag();
            match flag {
                ComFlag::WriteRequest => {
                    let data = go.value_ref().to_vec();
                    if let Some(tsap) = self.association_table.translate_asap(asap) {
                        if let Some(ga) = self.address_table.get_group_address(tsap) {
                            self.queue_group_value_write(ga, &data);
                            if let Some(go) = self.group_objects.get_mut(asap) {
                                go.set_comm_flag(ComFlag::Transmitting);
                            }
                        }
                    }
                }
                ComFlag::ReadRequest => {
                    if let Some(tsap) = self.association_table.translate_asap(asap) {
                        if let Some(ga) = self.address_table.get_group_address(tsap) {
                            self.queue_group_value_read(ga);
                            if let Some(go) = self.group_objects.get_mut(asap) {
                                go.set_comm_flag(ComFlag::Transmitting);
                            }
                        }
                    }
                }
                _ => break,
            }
        }
    }

    /// Take the next outgoing CEMI frame.
    pub fn next_outgoing_frame(&mut self) -> Option<CemiFrame> {
        self.outbox.pop_front()
    }

    /// Set the memory area (for MemoryRead/Write from ETS).
    pub fn set_memory_area(&mut self, data: Vec<u8>) {
        self.memory_area = data;
    }

    /// Load tables from the memory area at the given offsets.
    pub fn load_tables_from_memory(
        &mut self,
        addr_table_offset: usize,
        addr_table_len: usize,
        assoc_table_offset: usize,
        assoc_table_len: usize,
    ) {
        if addr_table_offset + addr_table_len <= self.memory_area.len() {
            self.address_table
                .load(&self.memory_area[addr_table_offset..addr_table_offset + addr_table_len]);
        }
        if assoc_table_offset + assoc_table_len <= self.memory_area.len() {
            self.association_table
                .load(&self.memory_area[assoc_table_offset..assoc_table_offset + assoc_table_len]);
        }
    }

    // ── Handlers ──────────────────────────────────────────────

    fn handle_group_value_write(&mut self, frame: &CemiFrame, data: &[u8]) {
        let ga_raw = frame.destination_address_raw();
        let Some(tsap) = self.address_table.get_tsap(ga_raw) else {
            return;
        };
        for asap in self.association_table.asaps_for_tsap(tsap) {
            // Check communication and write flags (C++ ref: groupValueWriteIndication)
            if let Some(desc) = self.group_object_table.get_descriptor(asap) {
                if !desc.communication_enable() || !desc.write_enable() {
                    continue;
                }
            }
            if let Some(go) = self.group_objects.get_mut(asap) {
                go.value_from_bus(data);
            }
        }
    }

    /// Handle `GroupValueResponse` — checks `update_enable` (A-flag) instead of `write_enable`.
    /// C++ ref: `groupValueReadAppLayerConfirm` checks `responseUpdateEnable()`.
    fn handle_group_value_response(&mut self, frame: &CemiFrame, data: &[u8]) {
        let ga_raw = frame.destination_address_raw();
        let Some(tsap) = self.address_table.get_tsap(ga_raw) else {
            return;
        };
        for asap in self.association_table.asaps_for_tsap(tsap) {
            if let Some(desc) = self.group_object_table.get_descriptor(asap) {
                if !desc.communication_enable() || !desc.update_enable() {
                    continue;
                }
            }
            if let Some(go) = self.group_objects.get_mut(asap) {
                go.value_from_bus(data);
            }
        }
    }

    fn handle_group_value_read(&mut self, frame: &CemiFrame) {
        let ga_raw = frame.destination_address_raw();
        let Some(tsap) = self.address_table.get_tsap(ga_raw) else {
            return;
        };
        for asap in self.association_table.asaps_for_tsap(tsap) {
            // Check communication and read flags (C++ ref: groupValueReadIndication)
            if let Some(desc) = self.group_object_table.get_descriptor(asap) {
                if !desc.communication_enable() || !desc.read_enable() {
                    continue;
                }
            }
            if let Some(go) = self.group_objects.get(asap) {
                if go.initialized() {
                    let data = go.value_ref().to_vec();
                    self.queue_group_value_response(ga_raw, &data);
                    return;
                }
            }
        }
    }

    fn handle_property_read(
        &mut self,
        source: u16,
        object_index: u8,
        property_id: u8,
        count: u8,
        start_index: u16,
    ) {
        let Ok(pid) = PropertyId::try_from(property_id) else {
            // Unknown property — send error response (count=0)
            self.queue_property_response(source, object_index, property_id, 0, start_index, &[]);
            return;
        };
        let Some(obj) = self.objects.get(object_index as usize) else {
            // Unknown object — send error response (count=0)
            self.queue_property_response(source, object_index, property_id, 0, start_index, &[]);
            return;
        };

        // BAU-4: startIndex=0 returns current element count (C++ ref convention)
        if start_index == 0 {
            let elem_count = obj.property(pid).map_or(0u16, Property::max_elements);
            self.queue_property_response(
                source,
                object_index,
                property_id,
                1,
                0,
                &elem_count.to_be_bytes(),
            );
            return;
        }

        let mut data = Vec::new();
        let read_count = obj.read_property(pid, start_index, count, &mut data);
        // Always send response — count=0 signals error (C++ ref behavior)
        self.queue_property_response(
            source,
            object_index,
            property_id,
            read_count,
            start_index,
            &data,
        );
    }

    fn handle_property_write(
        &mut self,
        source: u16,
        object_index: u8,
        property_id: u8,
        count: u8,
        start_index: u16,
        data: &[u8],
    ) {
        let Ok(pid) = PropertyId::try_from(property_id) else {
            return;
        };
        if let Some(obj) = self.objects.get_mut(object_index as usize) {
            obj.write_property(pid, start_index, count, data);
        }
        // C++ ref: always send read-back response after write (ETS expects confirmation)
        self.handle_property_read(source, object_index, property_id, count, start_index);
    }

    fn handle_memory_read(&mut self, source: u16, count: u8, address: u16) {
        let addr = address as usize;
        let len = count as usize;
        // Always send response — empty data on out-of-bounds (C++ ref behavior)
        let data = if addr + len <= self.memory_area.len() {
            self.memory_area[addr..addr + len].to_vec()
        } else {
            Vec::new()
        };
        self.queue_memory_response(source, address, &data);
    }

    fn handle_memory_write(&mut self, address: u16, data: &[u8]) {
        let addr = address as usize;
        let needed = addr + data.len();
        if needed > self.memory_area.len() {
            self.memory_area.resize(needed, 0);
        }
        self.memory_area[addr..addr + data.len()].copy_from_slice(data);
    }

    // ── Frame builders ────────────────────────────────────────

    fn queue_group_value_write(&mut self, ga: u16, data: &[u8]) {
        let src = self.individual_address();
        let dst = DestinationAddress::Group(GroupAddress::from_raw(ga));
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(0x00);
        if data.len() == 1 && data[0] <= 0x3F {
            payload.push(0x80 | (data[0] & 0x3F));
        } else {
            payload.push(0x80);
            payload.extend_from_slice(data);
        }
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &payload,
        ));
    }

    fn queue_group_value_read(&mut self, ga: u16) {
        let src = self.individual_address();
        let dst = DestinationAddress::Group(GroupAddress::from_raw(ga));
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &[0x00, 0x00],
        ));
    }

    fn queue_group_value_response(&mut self, ga: u16, data: &[u8]) {
        let src = self.individual_address();
        let dst = DestinationAddress::Group(GroupAddress::from_raw(ga));
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(0x00);
        if data.len() == 1 && data[0] <= 0x3F {
            payload.push(0x40 | (data[0] & 0x3F));
        } else {
            payload.push(0x40);
            payload.extend_from_slice(data);
        }
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &payload,
        ));
    }

    fn queue_individual_address_response(&mut self) {
        let src = self.individual_address();
        let dst = DestinationAddress::Group(GroupAddress::from_raw(0));
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &[0x01, 0x40],
        ));
    }

    fn queue_device_descriptor_response(&mut self, destination: u16) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        let mask = MASK_VERSION_IP.to_be_bytes();
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &[0x03, 0x40, mask[0], mask[1]],
        ));
    }

    /// Respond to unsupported `DeviceDescriptorRead` with type 0x3F (C++ ref behavior).
    fn queue_device_descriptor_unsupported(&mut self, destination: u16) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &[0x03, 0x7F], // DeviceDescriptorResponse with type=0x3F
        ));
    }

    fn queue_property_response(
        &mut self,
        destination: u16,
        object_index: u8,
        property_id: u8,
        count: u8,
        start_index: u16,
        data: &[u8],
    ) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        let mut payload = Vec::with_capacity(6 + data.len());
        payload.push(0x03);
        payload.push(0xD6); // PropertyValueResponse
        payload.push(object_index);
        payload.push(property_id);
        let count_start = (u16::from(count) << 12) | (start_index & 0x0FFF);
        payload.extend_from_slice(&count_start.to_be_bytes());
        payload.extend_from_slice(data);
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &payload,
        ));
    }

    fn queue_memory_response(&mut self, destination: u16, address: u16, data: &[u8]) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        let mut payload = Vec::with_capacity(5 + data.len());
        payload.push(0x02);
        #[expect(clippy::cast_possible_truncation)]
        let count_byte = 0x40 | (data.len() as u8 & 0x0F); // MemoryResponse
        payload.push(count_byte);
        payload.extend_from_slice(&address.to_be_bytes());
        payload.extend_from_slice(data);
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &payload,
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::device_object;

    fn test_bau() -> Bau {
        let device =
            device_object::new_device_object([0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], [0x00; 6]);
        let mut bau = Bau::new(device, 3, 1);
        device_object::set_individual_address(bau.device_mut(), 0x1101);

        bau.address_table
            .load(&[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
        bau.association_table
            .load(&[0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02]);
        bau
    }

    #[test]
    fn group_value_write_updates_go() {
        let mut bau = test_bau();
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, 0x08, 0x01, 0x01, 0x00, 0x81,
        ])
        .unwrap();
        bau.process_frame(&frame);
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::Updated
        );
        assert_eq!(bau.group_objects.get(1).unwrap().value_ref(), &[0x01]);
    }

    #[test]
    fn group_value_read_sends_response() {
        let mut bau = test_bau();
        bau.group_objects
            .get_mut(1)
            .unwrap()
            .write_value_no_send(&[42]);
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, 0x08, 0x01, 0x01, 0x00, 0x00,
        ])
        .unwrap();
        bau.process_frame(&frame);
        assert!(bau.next_outgoing_frame().is_some());
    }

    #[test]
    fn poll_sends_pending_writes() {
        let mut bau = test_bau();
        bau.group_objects.get_mut(1).unwrap().write_value(&[1]);
        bau.poll();
        let frame = bau.next_outgoing_frame().unwrap();
        assert_eq!(frame.destination_address_raw(), 0x0801);
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::Transmitting
        );
    }

    #[test]
    fn prog_mode_address_write() {
        let mut bau = test_bau();
        device_object::set_prog_mode(bau.device_mut(), true);
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0xC0, 0x11, 0x05,
        ])
        .unwrap();
        bau.process_frame(&frame);
        assert_eq!(bau.individual_address().raw(), 0x1105);
    }

    #[test]
    fn no_address_write_without_prog_mode() {
        let mut bau = test_bau();
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0xC0, 0x11, 0x05,
        ])
        .unwrap();
        bau.process_frame(&frame);
        assert_eq!(bau.individual_address().raw(), 0x1101);
    }

    #[test]
    fn property_read_by_object_index() {
        let mut bau = test_bau();
        // Object 0 is device, should have ObjectType = 0x0000
        // Simulate a PropertyValueRead for object 0, PID 1 (ObjectType)
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0x60, 0x11, 0x02, 0x11, 0x01, 0x04, 0x03, 0xD5, 0x00, 0x01, 0x10,
            0x01,
        ])
        .unwrap();
        bau.process_frame(&frame);
        let resp = bau.next_outgoing_frame().unwrap();
        // Response should be sent to 1.1.2 (the source of the request)
        assert_eq!(resp.destination_address_raw(), 0x1102);
    }

    #[test]
    fn memory_write_and_read() {
        let mut bau = test_bau();
        // MemoryWrite: 3 bytes at address 0x0000
        // APDU: [count|0x80=write] [addr_hi] [addr_lo] [data...]
        bau.handle_memory_write(0x0000, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(&bau.memory_area[0..3], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn connect_disconnect() {
        let mut bau = test_bau();
        // T_Connect from 1.1.2
        let connect = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0x60, 0x11, 0x02, 0x11, 0x01, 0x00, 0x80, 0x00,
        ])
        .unwrap();
        bau.process_frame(&connect);
        assert!(bau.transport.is_connected_to(0x1102));

        // T_Disconnect
        let disconnect = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0x60, 0x11, 0x02, 0x11, 0x01, 0x00, 0x81, 0x00,
        ])
        .unwrap();
        bau.process_frame(&disconnect);
        assert!(!bau.transport.is_connected_to(0x1102));
    }

    #[test]
    fn memory_write_grows_area() {
        let mut bau = test_bau();
        bau.handle_memory_write(0x0010, &[0x01, 0x02]);
        assert!(bau.memory_area.len() >= 0x12);
        assert_eq!(bau.memory_area[0x10], 0x01);
    }

    #[test]
    fn property_write_via_bau() {
        let mut bau = test_bau();
        bau.handle_property_write(0x1101, 0, 54, 1, 1, &[0x01]); // PID_PROG_MODE
        assert!(device_object::prog_mode(bau.device()));
        // BAU-2: write should produce a read-back response
        assert!(bau.next_outgoing_frame().is_some());
    }

    #[test]
    fn property_read_multi_object() {
        use crate::application_program::new_application_program_object;
        let mut bau = test_bau();
        let app_idx = bau.add_object(new_application_program_object());
        bau.handle_property_read(0x1102, app_idx, 1, 1, 1); // PID_OBJECT_TYPE
        assert!(bau.next_outgoing_frame().is_some());
    }

    #[test]
    fn device_descriptor_response_to_source() {
        let mut bau = test_bau();
        bau.queue_device_descriptor_response(0x1102);
        let resp = bau.next_outgoing_frame().unwrap();
        assert_eq!(resp.destination_address_raw(), 0x1102);
    }
}
