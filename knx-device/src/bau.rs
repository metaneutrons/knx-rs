// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Bus Access Unit (BAU) — the main KNX device controller.
//!
//! Ties together all device components: interface objects, tables,
//! group objects, transport/application layers, and memory.
//!
//! The BAU processes incoming CEMI frames and drives outgoing communication.

use alloc::vec::Vec;

use knx_core::address::{GroupAddress, IndividualAddress};
use knx_core::cemi::CemiFrame;
use knx_core::message::MessageCode;
use knx_core::types::Priority;

use crate::address_table::AddressTable;
use crate::application_layer::{self, AppIndication};
use crate::association_table::AssociationTable;
use crate::device_object;
use crate::group_object::{ComFlag, GroupObjectStore};
use crate::group_object_table::GroupObjectTable;
use crate::interface_object::InterfaceObject;
use crate::property::PropertyId;
use crate::transport_layer::TransportLayer;

/// Mask version for IP devices (System B).
pub const MASK_VERSION_IP: u16 = 0x57B0;

/// The Bus Access Unit — main device controller.
///
/// Processes incoming CEMI frames, manages group objects, and generates
/// outgoing frames. This is the central struct that application code
/// interacts with.
pub struct Bau {
    /// Device object (individual address, serial number, etc.).
    pub device: InterfaceObject,
    /// Address table (TSAP → group address).
    pub address_table: AddressTable,
    /// Association table (TSAP → ASAP).
    pub association_table: AssociationTable,
    /// Group object table (descriptors).
    pub group_object_table: GroupObjectTable,
    /// Group objects (communication objects).
    pub group_objects: GroupObjectStore,
    /// Transport layer state.
    #[allow(dead_code)] // used when processing connected-mode telegrams
    transport: TransportLayer,
    /// Outgoing frame queue.
    outbox: Vec<CemiFrame>,
}

impl Bau {
    /// Create a new BAU with the given device configuration.
    pub fn new(device: InterfaceObject, group_object_count: u16, default_go_size: usize) -> Self {
        Self {
            device,
            address_table: AddressTable::new(),
            association_table: AssociationTable::new(),
            group_object_table: GroupObjectTable::new(),
            group_objects: GroupObjectStore::new(group_object_count, default_go_size),
            transport: TransportLayer::new(),
            outbox: Vec::new(),
        }
    }

    /// The device's individual address.
    pub fn individual_address(&self) -> u16 {
        device_object::individual_address(&self.device)
    }

    /// Process an incoming CEMI frame from the bus/IP layer.
    pub fn process_frame(&mut self, frame: &CemiFrame) {
        let Some(tpdu) = frame.tpdu() else { return };
        let Some(apdu) = tpdu.apdu() else { return };

        let indication = application_layer::parse_indication(apdu.apdu_type, &apdu.data);
        let Some(indication) = indication else { return };

        match indication {
            AppIndication::GroupValueWrite { data, .. } => {
                self.handle_group_value_write(frame, &data);
            }
            AppIndication::GroupValueRead { .. } => {
                self.handle_group_value_read(frame);
            }
            AppIndication::IndividualAddressWrite { address } => {
                if device_object::prog_mode(&self.device) {
                    device_object::set_individual_address(&mut self.device, address);
                }
            }
            AppIndication::IndividualAddressRead => {
                if device_object::prog_mode(&self.device) {
                    self.queue_individual_address_response();
                }
            }
            AppIndication::PropertyValueRead {
                object_index,
                property_id,
                count,
                start_index,
            } => {
                self.handle_property_read(frame, object_index, property_id, count, start_index);
            }
            AppIndication::DeviceDescriptorRead { descriptor_type } => {
                if descriptor_type == 0 {
                    self.queue_device_descriptor_response(frame);
                }
            }
            _ => {} // Restart and others handled by application code
        }
    }

    /// Poll for outgoing frames. Call this in your main loop.
    ///
    /// Also checks group objects for pending write/read requests.
    pub fn poll(&mut self) {
        // Check for pending group object writes
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

    /// Take the next outgoing CEMI frame, if any.
    pub fn next_outgoing_frame(&mut self) -> Option<CemiFrame> {
        if self.outbox.is_empty() {
            None
        } else {
            Some(self.outbox.remove(0))
        }
    }

    // ── Internal handlers ─────────────────────────────────────

    fn handle_group_value_write(&mut self, frame: &CemiFrame, data: &[u8]) {
        let ga_raw = frame.destination_address_raw();
        let Some(tsap) = self.address_table.get_tsap(ga_raw) else {
            return;
        };
        let asaps = self.association_table.asaps_for_tsap(tsap);
        for asap in asaps {
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
        let asaps = self.association_table.asaps_for_tsap(tsap);
        for asap in &asaps {
            if let Some(go) = self.group_objects.get(*asap) {
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
        frame: &CemiFrame,
        _object_index: u8,
        property_id: u8,
        count: u8,
        start_index: u16,
    ) {
        let Ok(pid) = PropertyId::try_from(property_id) else {
            return;
        };
        let mut data = Vec::new();
        let read_count = self
            .device
            .read_property(pid, start_index, count, &mut data);
        if read_count > 0 {
            self.queue_property_response(frame, property_id, read_count, start_index, &data);
        }
    }

    // ── Frame builders ────────────────────────────────────────

    fn queue_group_value_write(&mut self, ga: u16, data: &[u8]) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Group(GroupAddress::from_raw(ga));
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(0x00); // TPCI
        if data.len() == 1 && data[0] <= 0x3F {
            payload.push(0x80 | (data[0] & 0x3F)); // short GroupValueWrite
        } else {
            payload.push(0x80); // GroupValueWrite APCI
            payload.extend_from_slice(data);
        }
        self.outbox.push(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &payload,
        ));
    }

    fn queue_group_value_read(&mut self, ga: u16) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Group(GroupAddress::from_raw(ga));
        let payload = [0x00, 0x00]; // GroupValueRead
        self.outbox.push(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &payload,
        ));
    }

    fn queue_group_value_response(&mut self, ga: u16, data: &[u8]) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Group(GroupAddress::from_raw(ga));
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(0x00);
        if data.len() == 1 && data[0] <= 0x3F {
            payload.push(0x40 | (data[0] & 0x3F)); // short GroupValueResponse
        } else {
            payload.push(0x40);
            payload.extend_from_slice(data);
        }
        self.outbox.push(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::Low,
            &payload,
        ));
    }

    fn queue_individual_address_response(&mut self) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Group(GroupAddress::from_raw(0));
        let payload = [0x01, 0x40]; // IndividualAddressResponse
        self.outbox.push(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &payload,
        ));
    }

    fn queue_device_descriptor_response(&mut self, _request: &CemiFrame) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Individual(src);
        let mask = MASK_VERSION_IP.to_be_bytes();
        let payload = [0x03, 0x40, mask[0], mask[1]]; // DeviceDescriptorResponse type 0
        self.outbox.push(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &payload,
        ));
    }

    fn queue_property_response(
        &mut self,
        _request: &CemiFrame,
        property_id: u8,
        count: u8,
        start_index: u16,
        data: &[u8],
    ) {
        let src = IndividualAddress::from_raw(self.individual_address());
        let dst = knx_core::address::DestinationAddress::Individual(src);
        let mut payload = Vec::with_capacity(6 + data.len());
        payload.push(0x03); // TPCI
        payload.push(0xD6); // PropertyValueResponse APCI
        payload.push(0x00); // object index
        payload.push(property_id);
        #[allow(clippy::cast_possible_truncation)] // u16 shift can't truncate
        let count_start = (u16::from(count) << 12) | (start_index & 0x0FFF);
        payload.extend_from_slice(&count_start.to_be_bytes());
        payload.extend_from_slice(data);
        self.outbox.push(CemiFrame::new_l_data(
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
        device_object::set_individual_address(&mut bau.device, 0x1101);

        // Load tables: 2 GAs, 2 associations
        bau.address_table.load(&[
            0x00, 0x02, // 2 entries
            0x08, 0x01, // TSAP 1 → GA 1/0/1
            0x08, 0x02, // TSAP 2 → GA 1/0/2
        ]);
        bau.association_table.load(&[
            0x00, 0x02, // 2 entries
            0x00, 0x01, 0x00, 0x01, // TSAP 1 → ASAP 1
            0x00, 0x02, 0x00, 0x02, // TSAP 2 → ASAP 2
        ]);
        bau
    }

    #[test]
    fn individual_address() {
        let bau = test_bau();
        assert_eq!(bau.individual_address(), 0x1101);
    }

    #[test]
    fn group_value_write_updates_go() {
        let mut bau = test_bau();

        // Simulate incoming GroupValueWrite to 1/0/1 with value=1
        let frame_bytes = [
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, // source 1.1.2
            0x08, 0x01, // dest 1/0/1
            0x01, // npdu len
            0x00, 0x81, // GroupValueWrite, value=1
        ];
        let frame = CemiFrame::parse(&frame_bytes).unwrap();
        bau.process_frame(&frame);

        let go = bau.group_objects.get(1).unwrap();
        assert_eq!(go.comm_flag(), ComFlag::Updated);
        assert_eq!(go.value_ref(), &[0x01]);
    }

    #[test]
    fn group_value_read_sends_response() {
        let mut bau = test_bau();

        // Pre-set GO 1 with a value
        bau.group_objects
            .get_mut(1)
            .unwrap()
            .write_value_no_send(&[42]);

        // Simulate incoming GroupValueRead for 1/0/1
        let frame_bytes = [
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, 0x08, 0x01, 0x01, 0x00, 0x00, // GroupValueRead
        ];
        let frame = CemiFrame::parse(&frame_bytes).unwrap();
        bau.process_frame(&frame);

        let response = bau.next_outgoing_frame().unwrap();
        assert_eq!(response.message_code_raw(), MessageCode::LDataReq as u8);
    }

    #[test]
    fn poll_sends_pending_writes() {
        let mut bau = test_bau();

        // App writes a value to GO 1
        bau.group_objects.get_mut(1).unwrap().write_value(&[1]);
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::WriteRequest
        );

        bau.poll();

        // Should have generated an outgoing frame
        let frame = bau.next_outgoing_frame().unwrap();
        assert_eq!(frame.destination_address_raw(), 0x0801); // GA 1/0/1

        // GO should be in Transmitting state
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::Transmitting
        );
    }

    #[test]
    fn prog_mode_individual_address_write() {
        let mut bau = test_bau();
        device_object::set_prog_mode(&mut bau.device, true);

        let frame_bytes = [
            0x29, 0x00, 0xB0, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00,
            0xC0, // IndividualAddressWrite
            0x11, 0x05, // new address 1.1.5
        ];
        let frame = CemiFrame::parse(&frame_bytes).unwrap();
        bau.process_frame(&frame);

        assert_eq!(bau.individual_address(), 0x1105);
    }

    #[test]
    fn no_address_write_without_prog_mode() {
        let mut bau = test_bau();
        // prog mode is off by default

        let frame_bytes = [
            0x29, 0x00, 0xB0, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0xC0, 0x11, 0x05,
        ];
        let frame = CemiFrame::parse(&frame_bytes).unwrap();
        bau.process_frame(&frame);

        assert_eq!(bau.individual_address(), 0x1101); // unchanged
    }
}
