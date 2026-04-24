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
use crate::group_object_table::{GroupObjectDescriptor, GroupObjectTable};
use crate::interface_object::InterfaceObject;
use crate::property::{Property, PropertyId};
use crate::table_object::TableObject;
use crate::transport_layer::TransportLayer;

/// Mask version for IP devices (System B).
pub const MASK_VERSION_IP: u16 = 0x57B0;

// ── Standard interface object indices (KNX spec) ─────────────

/// Object index for the address table object.
const OBJ_ADDR_TABLE: u8 = 1;
/// Object index for the association table object.
const OBJ_ASSOC_TABLE: u8 = 2;
/// Object index for the application program object (first user object).
const OBJ_APP_PROGRAM: u8 = 3;

// ── KNX restart erase codes (KNX 3/5/2) ─────────────────────

/// Erase code: confirmed restart (reset table objects).
const ERASE_CONFIRMED_RESTART: u8 = 1;
/// Erase code range: factory reset upper bound.
const ERASE_FACTORY_RESET_MAX: u8 = 4;

// ── KNX system network parameter constants ───────────────────

/// Property ID for serial number (KNX system network parameter read).
const PID_SERIAL_NUMBER: u16 = 11;
/// Object type for the device object.
const OBJECT_TYPE_DEVICE: u16 = 0;
/// Domain address for IP devices (always 0).
const DOMAIN_ADDRESS_IP: u16 = 0;
/// Restart response: no error.
const RESTART_ERROR_CODE_OK: u8 = 0;
/// Restart response: zero process time.
const RESTART_PROCESS_TIME_ZERO: u16 = 0;
/// Default ADC value (no ADC hardware).
const ADC_VALUE_DEFAULT: u16 = 0;
/// Maximum allowed memory area size (256 KiB).
const MAX_MEMORY_SIZE: usize = 256 * 1024;

/// Authorization level: full access (no restrictions).
const AUTH_LEVEL_FULL: u8 = 0;
/// Memory extended read/write return code: success.
const MEM_EXT_RETURN_OK: u8 = 0;
/// Memory extended read/write return code: error (out of range).
const MEM_EXT_RETURN_ERROR: u8 = 1;
/// Broadcast group address (raw value 0).
const BROADCAST_GA: u16 = 0;

/// Parameters for extended property value services (read/write).
struct ExtPropertyParams {
    object_type: u16,
    object_instance: u16,
    property_id: u16,
    count: u8,
    start_index: u16,
}

/// The Bus Access Unit — main device controller.
pub struct Bau {
    /// Interface objects indexed by object index (0=device, 1=address table, etc.).
    objects: Vec<InterfaceObject>,
    /// Address table.
    pub address_table: AddressTable,
    /// Association table.
    pub association_table: AssociationTable,
    /// Address table object (Load State Machine for ETS programming).
    pub addr_table_object: TableObject,
    /// Association table object (Load State Machine for ETS programming).
    pub assoc_table_object: TableObject,
    /// Application program table object (Load State Machine for ETS programming).
    pub app_program_object: TableObject,
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
        use crate::application_program::new_application_program_object;
        use crate::interface_object::ObjectType;

        // Standard object layout: 0=Device, 1=AddrTable, 2=AssocTable, 3=AppProgram
        let addr_table_obj = InterfaceObject::new(ObjectType::AddressTable);
        let assoc_table_obj = InterfaceObject::new(ObjectType::AssociationTable);
        let app_program_obj = new_application_program_object();

        Self {
            objects: vec![device, addr_table_obj, assoc_table_obj, app_program_obj],
            address_table: AddressTable::new(),
            association_table: AssociationTable::new(),
            addr_table_object: TableObject::new(),
            assoc_table_object: TableObject::new(),
            app_program_object: TableObject::new(),
            group_object_table: GroupObjectTable::new(),
            group_objects: GroupObjectStore::new(group_object_count, default_go_size),
            transport: TransportLayer::new(),
            memory_area: Vec::new(),
            outbox: VecDeque::new(),
        }
    }

    /// The device object (index 0).
    ///
    /// # Panics
    ///
    /// Panics if the object list is empty (violates constructor invariant).
    #[expect(clippy::expect_used, reason = "constructor guarantees index 0 exists")]
    pub fn device(&self) -> &InterfaceObject {
        self.objects
            .first()
            .expect("BAU invariant: device object at index 0")
    }

    /// Mutable device object.
    ///
    /// # Panics
    ///
    /// Panics if the object list is empty (violates constructor invariant).
    #[expect(clippy::expect_used, reason = "constructor guarantees index 0 exists")]
    pub fn device_mut(&mut self) -> &mut InterfaceObject {
        self.objects
            .first_mut()
            .expect("BAU invariant: device object at index 0")
    }

    /// Add an interface object. Returns its index, or `None` if full.
    pub fn add_object(&mut self, obj: InterfaceObject) -> Option<u8> {
        if self.objects.len() >= 256 {
            return None;
        }
        #[expect(clippy::cast_possible_truncation)]
        let idx = self.objects.len() as u8;
        self.objects.push(obj);
        Some(idx)
    }

    /// Get an interface object by index.
    pub fn object(&self, index: u8) -> Option<&InterfaceObject> {
        self.objects.get(index as usize)
    }

    /// Get a mutable interface object by index.
    pub fn object_mut(&mut self, index: u8) -> Option<&mut InterfaceObject> {
        self.objects.get_mut(index as usize)
    }

    /// Look up the `TableObject` for a given interface object index.
    ///
    /// Returns `None` for object index 0 (device object) which has no table.
    const fn table_object(&self, object_index: u8) -> Option<&TableObject> {
        match object_index {
            OBJ_ADDR_TABLE => Some(&self.addr_table_object),
            OBJ_ASSOC_TABLE => Some(&self.assoc_table_object),
            _ if object_index >= OBJ_APP_PROGRAM => Some(&self.app_program_object),
            _ => None,
        }
    }

    /// Mutable version of [`table_object`](Self::table_object).
    const fn table_object_mut(&mut self, object_index: u8) -> Option<&mut TableObject> {
        match object_index {
            OBJ_ADDR_TABLE => Some(&mut self.addr_table_object),
            OBJ_ASSOC_TABLE => Some(&mut self.assoc_table_object),
            _ if object_index >= OBJ_APP_PROGRAM => Some(&mut self.app_program_object),
            _ => None,
        }
    }

    /// The device's individual address.
    pub fn individual_address(&self) -> IndividualAddress {
        device_object::individual_address(self.device())
    }

    /// Process an incoming CEMI frame.
    ///
    /// `now_ms` is the current monotonic time in milliseconds, used for
    /// transport layer timeouts and retry logic.
    pub fn process_frame(&mut self, frame: &CemiFrame, now_ms: u64) {
        let Some(tpdu) = frame.tpdu() else { return };

        match &tpdu {
            Tpdu::Control { tpdu_type, .. } => {
                self.process_control_tpdu(frame, *tpdu_type, now_ms);
            }
            Tpdu::Data {
                tpdu_type,
                sequence_number,
                apdu,
            } => {
                self.process_data_tpdu(frame, *tpdu_type, *sequence_number, apdu, now_ms);
            }
        }

        // Process transport layer actions (ACK/NACK/Disconnect frames + connected data)
        self.drain_transport_actions();
    }

    fn process_control_tpdu(
        &mut self,
        frame: &CemiFrame,
        tpdu_type: knx_core::message::TpduType,
        now_ms: u64,
    ) {
        use knx_core::message::TpduType;
        let seq_no = frame.tpdu().map_or(0, |t| match t {
            Tpdu::Control {
                sequence_number, ..
            } => sequence_number,
            Tpdu::Data { .. } => 0,
        });
        match tpdu_type {
            TpduType::Connect => {
                self.transport
                    .connect_indication(frame.source_address().raw(), now_ms);
            }
            TpduType::Disconnect => {
                self.transport
                    .disconnect_indication(frame.source_address().raw());
            }
            TpduType::Ack => {
                self.transport
                    .ack_indication(frame.source_address().raw(), seq_no, now_ms);
            }
            TpduType::Nack => {
                self.transport
                    .nack_indication(frame.source_address().raw(), seq_no, now_ms);
            }
            _ => {}
        }
    }

    fn process_data_tpdu(
        &mut self,
        frame: &CemiFrame,
        tpdu_type: knx_core::message::TpduType,
        sequence_number: u8,
        apdu: &knx_core::apdu::Apdu,
        now_ms: u64,
    ) {
        use knx_core::message::TpduType;
        let source = frame.source_address().raw();

        if tpdu_type == TpduType::DataConnected {
            // Route through transport layer — it handles ACK/NACK and sequence validation.
            let apdu_data = application_layer::encode_raw_apdu(apdu);
            self.transport.data_connected_indication(
                source,
                sequence_number,
                frame.priority(),
                apdu_data,
                now_ms,
            );
        } else {
            // Connectionless: DataGroup, DataBroadcast, DataIndividual
            let Ok(indication) = application_layer::parse_indication(apdu.apdu_type, &apdu.data)
            else {
                return;
            };
            self.dispatch_indication(frame, source, indication);
        }
    }

    fn dispatch_indication(&mut self, frame: &CemiFrame, source: u16, indication: AppIndication) {
        match &indication {
            AppIndication::GroupValueWrite { .. }
            | AppIndication::GroupValueResponse { .. }
            | AppIndication::GroupValueRead { .. } => {
                self.dispatch_group_services(frame, source, indication);
            }
            AppIndication::DeviceDescriptorRead { .. }
            | AppIndication::Restart
            | AppIndication::RestartMasterReset { .. }
            | AppIndication::IndividualAddressWrite { .. }
            | AppIndication::IndividualAddressRead
            | AppIndication::AuthorizeRequest { .. }
            | AppIndication::KeyWrite { .. } => {
                self.dispatch_device_management(source, &indication);
            }
            AppIndication::PropertyValueRead { .. }
            | AppIndication::PropertyValueWrite { .. }
            | AppIndication::PropertyDescriptionRead { .. } => {
                self.dispatch_property_services(source, indication);
            }
            AppIndication::MemoryRead { .. }
            | AppIndication::MemoryWrite { .. }
            | AppIndication::MemoryExtRead { .. }
            | AppIndication::MemoryExtWrite { .. } => {
                self.dispatch_memory_services(source, indication);
            }
            AppIndication::PropertyValueExtRead { .. }
            | AppIndication::PropertyValueExtWriteCon { .. }
            | AppIndication::PropertyValueExtWriteUnCon { .. }
            | AppIndication::FunctionPropertyCommand { .. }
            | AppIndication::FunctionPropertyState { .. }
            | AppIndication::SystemNetworkParameterRead { .. }
            | AppIndication::AdcRead { .. }
            | AppIndication::IndividualAddressSerialNumberRead { .. }
            | AppIndication::IndividualAddressSerialNumberWrite { .. } => {
                self.dispatch_ext_property_services(source, indication);
            }
            AppIndication::PropertyExtDescriptionRead { .. } => {
                // TODO: not yet implemented — requires extended property description support.
            }
        }
    }

    fn dispatch_group_services(
        &mut self,
        frame: &CemiFrame,
        _source: u16,
        indication: AppIndication,
    ) {
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
            _ => {}
        }
    }

    fn dispatch_device_management(&mut self, source: u16, indication: &AppIndication) {
        match *indication {
            AppIndication::DeviceDescriptorRead { descriptor_type: 0 } => {
                self.queue_device_descriptor_response(source);
            }
            AppIndication::DeviceDescriptorRead { .. } => {
                self.queue_device_descriptor_unsupported(source);
            }
            AppIndication::RestartMasterReset {
                erase_code,
                channel: _,
            } => {
                self.handle_restart_master_reset(source, erase_code);
            }
            AppIndication::IndividualAddressWrite { address }
                if device_object::prog_mode(self.device()) =>
            {
                device_object::set_individual_address(self.device_mut(), address);
            }
            AppIndication::IndividualAddressRead if device_object::prog_mode(self.device()) => {
                self.queue_individual_address_response();
            }
            AppIndication::AuthorizeRequest { key: _ } => {
                self.queue_authorize_response(source, AUTH_LEVEL_FULL);
            }
            AppIndication::KeyWrite { level, key: _ } => {
                self.queue_key_response(source, level);
            }
            // Restart (non-master-reset) is handled at the transport level, not here.
            _ => {}
        }
    }

    fn dispatch_property_services(&mut self, source: u16, indication: AppIndication) {
        match indication {
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
            AppIndication::PropertyDescriptionRead {
                object_index,
                property_id,
                property_index,
            } => {
                self.handle_property_description_read(
                    source,
                    object_index,
                    property_id,
                    property_index,
                );
            }
            _ => {}
        }
    }

    fn dispatch_memory_services(&mut self, source: u16, indication: AppIndication) {
        match indication {
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
            AppIndication::MemoryExtRead { count, address } => {
                self.handle_memory_ext_read(source, count, address);
            }
            AppIndication::MemoryExtWrite {
                count: _,
                address,
                data,
            } => {
                self.handle_memory_ext_write(source, address, &data);
            }
            _ => {}
        }
    }

    fn dispatch_ext_property_services(&mut self, source: u16, indication: AppIndication) {
        match indication {
            AppIndication::PropertyValueExtRead {
                object_type,
                object_instance,
                property_id,
                count,
                start_index,
            } => {
                self.handle_property_value_ext_read(
                    source,
                    object_type,
                    object_instance,
                    property_id,
                    count,
                    start_index,
                );
            }
            AppIndication::PropertyValueExtWriteCon {
                object_type,
                object_instance,
                property_id,
                count,
                start_index,
                data,
            } => {
                let params = ExtPropertyParams {
                    object_type,
                    object_instance,
                    property_id,
                    count,
                    start_index,
                };
                self.handle_property_value_ext_write(source, &params, &data, true);
            }
            AppIndication::PropertyValueExtWriteUnCon {
                object_type,
                object_instance,
                property_id,
                count,
                start_index,
                data,
            } => {
                let params = ExtPropertyParams {
                    object_type,
                    object_instance,
                    property_id,
                    count,
                    start_index,
                };
                self.handle_property_value_ext_write(source, &params, &data, false);
            }
            AppIndication::FunctionPropertyCommand {
                object_index,
                property_id,
                data: _,
            }
            | AppIndication::FunctionPropertyState {
                object_index,
                property_id,
                data: _,
            } => {
                self.queue_function_property_state_response(source, object_index, property_id, &[]);
            }
            AppIndication::SystemNetworkParameterRead {
                object_type,
                property_id,
                test_info,
            } => {
                self.handle_system_network_parameter_read(object_type, property_id, &test_info);
            }
            AppIndication::AdcRead { channel, count } => {
                self.queue_adc_response(source, channel, count);
            }
            AppIndication::IndividualAddressSerialNumberRead { serial } => {
                self.handle_individual_address_serial_number_read(serial);
            }
            AppIndication::IndividualAddressSerialNumberWrite { serial, address } => {
                self.handle_individual_address_serial_number_write(serial, address);
            }
            _ => {}
        }
    }

    /// Poll for outgoing frames. Drives pending group object writes/reads.
    ///
    /// `now_ms` is the current monotonic time in milliseconds.
    pub fn poll(&mut self, now_ms: u64) {
        // Transport layer timeouts and buffered request retry
        self.transport.poll(now_ms);
        self.drain_transport_actions();

        // Don't send group telegrams until tables are loaded
        if !self.configured() {
            return;
        }

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

    /// Queue read requests for all group objects with the I-flag (read on init) set.
    /// Should be called after tables are loaded (ETS programming or restore).
    pub fn init_read_requests(&mut self) {
        let count = self.group_object_table.entry_count();
        for asap in 1..=count {
            if let Some(desc) = self.group_object_table.get_descriptor(asap) {
                if desc.communication_enable() && desc.read_on_init() {
                    if let Some(go) = self.group_objects.get_mut(asap) {
                        go.request_object_read();
                    }
                }
            }
        }
    }

    /// Take the next outgoing CEMI frame.
    pub fn next_outgoing_frame(&mut self) -> Option<CemiFrame> {
        self.outbox.pop_front()
    }

    /// Check if the device is fully configured (all tables loaded).
    pub fn configured(&self) -> bool {
        use crate::property::LoadState;
        self.addr_table_object.load_state() == LoadState::Loaded
            && self.assoc_table_object.load_state() == LoadState::Loaded
    }

    /// Consume transport layer actions and convert them to outgoing frames.
    fn drain_transport_actions(&mut self) {
        use crate::transport_layer::Action;
        for action in self.transport.take_actions() {
            match action {
                Action::SendControl {
                    destination,
                    tpdu_type,
                    seq_no,
                } => {
                    self.queue_control_frame(destination, tpdu_type, seq_no);
                }
                Action::SendDataConnected {
                    destination,
                    seq_no,
                    priority,
                    apdu,
                } => {
                    self.queue_data_connected_frame(destination, seq_no, priority, &apdu);
                }
                Action::ConnectIndication { .. }
                | Action::ConnectConfirm { .. }
                | Action::DisconnectIndication { .. }
                | Action::DataConnectedConfirm => {}
                Action::DataConnectedIndication {
                    source,
                    priority: _,
                    apdu,
                } => {
                    // Connected data received — parse and dispatch
                    if let Ok(parsed) = application_layer::parse_raw_apdu(&apdu) {
                        // Create a minimal frame for dispatch_indication (source needed)
                        self.dispatch_connected_indication(source, parsed);
                    }
                }
            }
        }
    }

    /// Dispatch a connected-mode indication (from transport layer).
    fn dispatch_connected_indication(&mut self, source: u16, indication: AppIndication) {
        // Connected indications don't have a CemiFrame, so we create a dummy
        // for handlers that need the source address.
        let src = IndividualAddress::from_raw(source);
        let dst = DestinationAddress::Individual(self.individual_address());
        let dummy_frame =
            CemiFrame::new_l_data(MessageCode::LDataInd, src, dst, Priority::System, &[]);
        self.dispatch_indication(&dummy_frame, source, indication);
    }

    /// Set the memory area (for `MemoryRead`/`MemoryWrite` from ETS).
    pub fn set_memory_area(&mut self, data: Vec<u8>) {
        self.memory_area = data;
    }

    /// Get the memory area (for persistence after ETS programming).
    pub fn memory_area(&self) -> &[u8] {
        &self.memory_area
    }

    /// Serialize the full device state for persistence.
    ///
    /// Format: `[addr_table_obj:9][assoc_table_obj:9][app_program_obj:9][prog_version:5][mem_len:4LE][memory_area]`
    pub fn save(&self) -> Vec<u8> {
        crate::bau_persistence::save_bau_state(self)
    }

    /// Restore device state from persisted data.
    ///
    /// After restoring, the address and association tables are automatically
    /// parsed from the memory area if their table objects are in `Loaded` state.
    ///
    /// Returns `true` if restore succeeded.
    pub fn restore(&mut self, data: &[u8]) -> bool {
        crate::bau_persistence::restore_bau_state(self, data).is_ok()
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
        self.update_group_object_from_bus(frame, data, GroupObjectDescriptor::write_enable);
    }

    /// Handle `GroupValueResponse` — checks `update_enable` (A-flag) instead of `write_enable`.
    /// C++ ref: `groupValueReadAppLayerConfirm` checks `responseUpdateEnable()`.
    fn handle_group_value_response(&mut self, frame: &CemiFrame, data: &[u8]) {
        self.update_group_object_from_bus(frame, data, GroupObjectDescriptor::update_enable);
    }

    /// Shared logic for `GroupValueWrite` and `GroupValueResponse`.
    ///
    /// `check_flag` selects the descriptor flag to test (`write_enable` vs `update_enable`).
    fn update_group_object_from_bus(
        &mut self,
        frame: &CemiFrame,
        data: &[u8],
        check_flag: impl Fn(GroupObjectDescriptor) -> bool,
    ) {
        let ga_raw = frame.destination_address_raw();
        let Some(tsap) = self.address_table.get_tsap(ga_raw) else {
            return;
        };
        for asap in self.association_table.asaps_for_tsap(tsap) {
            if let Some(desc) = self.group_object_table.get_descriptor(asap) {
                if !desc.communication_enable() || !check_flag(desc) {
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
            self.queue_property_response(source, object_index, property_id, 0, start_index, &[]);
            return;
        };

        // Intercept LoadStateControl reads for table objects
        if pid == PropertyId::LoadStateControl && start_index == 1 {
            if let Some(to) = self.table_object(object_index) {
                let state = to.load_state() as u8;
                self.queue_property_response(
                    source,
                    object_index,
                    property_id,
                    1,
                    start_index,
                    &[state],
                );
                return;
            }
        }

        // Intercept TableReference reads for table objects
        if pid == PropertyId::TableReference && start_index == 1 {
            if let Some(to) = self.table_object(object_index) {
                let table_ref = to.table_reference();
                self.queue_property_response(
                    source,
                    object_index,
                    property_id,
                    1,
                    start_index,
                    &table_ref.to_be_bytes(),
                );
                return;
            }
        }

        // Intercept McbTable reads for table objects
        if pid == PropertyId::McbTable && start_index == 1 {
            if let Some(to) = self.table_object(object_index) {
                let mcb = to.mcb_table(&self.memory_area);
                self.queue_property_response(
                    source,
                    object_index,
                    property_id,
                    1,
                    start_index,
                    &mcb,
                );
                return;
            }
        }

        let Some(obj) = self.objects.get(object_index as usize) else {
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

        // Intercept LoadStateControl writes for table objects
        if pid == PropertyId::LoadStateControl {
            let mem_len = self.memory_area.len();
            if let Some(to) = self.table_object_mut(object_index) {
                let (loaded, fill) = to.handle_load_event(data, mem_len);
                self.apply_fill(fill);
                if loaded {
                    self.reload_runtime_table(object_index);
                }
            }
            // Send read-back response with current load state
            self.handle_property_read(source, object_index, property_id, count, start_index);
            return;
        }

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
        self.write_to_memory(address as usize, data);
    }

    fn handle_restart_master_reset(&mut self, source: u16, erase_code: u8) {
        if let ERASE_CONFIRMED_RESTART..=ERASE_FACTORY_RESET_MAX = erase_code {
            self.addr_table_object = TableObject::new();
            self.assoc_table_object = TableObject::new();
            self.app_program_object = TableObject::new();
            self.address_table = AddressTable::new();
            self.association_table = AssociationTable::new();
            self.memory_area.clear();
        }
        let payload = application_layer::encode_restart_response(
            RESTART_ERROR_CODE_OK,
            RESTART_PROCESS_TIME_ZERO,
        );
        self.queue_individual_frame(source, Priority::System, &payload);
    }

    fn handle_property_description_read(
        &mut self,
        source: u16,
        object_index: u8,
        property_id: u8,
        property_index: u8,
    ) {
        let Some(obj) = self.objects.get(object_index as usize) else {
            // Unknown object — send error (property_id=0)
            let payload = application_layer::encode_property_description_response(
                object_index,
                0,
                0,
                false,
                0,
                0,
                0,
            );
            self.queue_individual_frame(source, Priority::System, &payload);
            return;
        };

        if let Some((idx, desc)) = obj.read_property_description(property_id, property_index) {
            let payload = application_layer::encode_property_description_response(
                object_index,
                desc.id as u8,
                idx,
                desc.write_enable,
                desc.data_type as u8,
                desc.max_elements,
                desc.access,
            );
            self.queue_individual_frame(source, Priority::System, &payload);
        } else {
            // Property not found
            let payload = application_layer::encode_property_description_response(
                object_index,
                0,
                0,
                false,
                0,
                0,
                0,
            );
            self.queue_individual_frame(source, Priority::System, &payload);
        }
    }

    fn handle_memory_ext_read(&mut self, source: u16, count: u8, address: u32) {
        let addr = address as usize;
        let len = count as usize;
        let (return_code, data) = if addr + len <= self.memory_area.len() {
            (MEM_EXT_RETURN_OK, self.memory_area[addr..addr + len].to_vec())
        } else {
            (MEM_EXT_RETURN_ERROR, Vec::new()) // out of range
        };
        let payload =
            application_layer::encode_memory_ext_read_response(return_code, address, &data);
        self.queue_individual_frame(source, Priority::System, &payload);
    }

    fn handle_memory_ext_write(&mut self, source: u16, address: u32, data: &[u8]) {
        if !self.write_to_memory(address as usize, data) {
            return;
        }
        let payload = application_layer::encode_memory_ext_write_response(MEM_EXT_RETURN_OK, address);
        self.queue_individual_frame(source, Priority::System, &payload);
    }

    fn handle_individual_address_serial_number_read(&mut self, serial: [u8; 6]) {
        let device_serial = device_object::serial_number(self.device());
        if serial == device_serial {
            let payload = application_layer::encode_individual_address_serial_number_response(
                serial,
                DOMAIN_ADDRESS_IP,
            );
            // Respond as broadcast
            self.queue_broadcast_frame(&payload);
        }
    }

    fn handle_individual_address_serial_number_write(&mut self, serial: [u8; 6], address: u16) {
        let device_serial = device_object::serial_number(self.device());
        if serial == device_serial {
            device_object::set_individual_address(self.device_mut(), address);
        }
    }

    fn handle_system_network_parameter_read(
        &mut self,
        object_type: u16,
        property_id: u16,
        test_info: &[u8],
    ) {
        // Only respond to PID_SERIAL_NUMBER on device object
        if object_type == OBJECT_TYPE_DEVICE && property_id == PID_SERIAL_NUMBER {
            let serial = device_object::serial_number(self.device());
            let payload = application_layer::encode_system_network_parameter_response(
                object_type,
                property_id,
                test_info,
                &serial,
            );
            self.queue_broadcast_frame(&payload);
        }
    }

    fn queue_authorize_response(&mut self, destination: u16, level: u8) {
        let payload = application_layer::encode_authorize_response(level);
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    fn queue_key_response(&mut self, destination: u16, level: u8) {
        let payload = application_layer::encode_key_response(level);
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    fn queue_function_property_state_response(
        &mut self,
        destination: u16,
        object_index: u8,
        property_id: u8,
        result: &[u8],
    ) {
        let payload = application_layer::encode_function_property_state_response(
            object_index,
            property_id,
            result,
        );
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    fn queue_adc_response(&mut self, destination: u16, channel: u8, count: u8) {
        let payload = application_layer::encode_adc_response(channel, count, ADC_VALUE_DEFAULT);
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    /// Find an interface object index by object type and instance number.
    fn find_object_by_type(&self, object_type: u16, instance: u16) -> Option<u8> {
        let target = crate::interface_object::ObjectType::try_from(object_type).ok()?;
        let mut instance_count = 0u16;
        #[expect(clippy::cast_possible_truncation)]
        for (i, obj) in self.objects.iter().enumerate() {
            if obj.object_type() == target {
                if instance_count == instance {
                    return Some(i as u8);
                }
                instance_count += 1;
            }
        }
        None
    }

    fn handle_property_value_ext_read(
        &mut self,
        source: u16,
        object_type: u16,
        object_instance: u16,
        property_id: u16,
        count: u8,
        start_index: u16,
    ) {
        let obj_idx = self.find_object_by_type(object_type, object_instance);
        if let Some(idx) = obj_idx {
            // Delegate to standard property read logic
            let Ok(pid) = u8::try_from(property_id) else {
                // Property ID too large for standard services — send error response
                let payload = application_layer::encode_property_value_ext_response(
                    object_type,
                    object_instance,
                    property_id,
                    0,
                    start_index,
                    &[],
                );
                self.queue_individual_frame(source, Priority::System, &payload);
                return;
            };
            let Ok(pid_enum) = PropertyId::try_from(pid) else {
                let payload = application_layer::encode_property_value_ext_response(
                    object_type,
                    object_instance,
                    property_id,
                    0,
                    start_index,
                    &[],
                );
                self.queue_individual_frame(source, Priority::System, &payload);
                return;
            };
            if let Some(obj) = self.objects.get(idx as usize) {
                let mut buf = Vec::new();
                let read_count = obj.read_property(pid_enum, start_index, count, &mut buf);
                let payload = application_layer::encode_property_value_ext_response(
                    object_type,
                    object_instance,
                    property_id,
                    read_count,
                    start_index,
                    &buf,
                );
                self.queue_individual_frame(source, Priority::System, &payload);
                return;
            }
        }
        // Object not found — respond with count=0
        let payload = application_layer::encode_property_value_ext_response(
            object_type,
            object_instance,
            property_id,
            0,
            start_index,
            &[],
        );
        self.queue_individual_frame(source, Priority::System, &payload);
    }

    fn handle_property_value_ext_write(
        &mut self,
        source: u16,
        params: &ExtPropertyParams,
        data: &[u8],
        confirmed: bool,
    ) {
        let obj_idx = self.find_object_by_type(params.object_type, params.object_instance);
        if let Some(idx) = obj_idx {
            let Ok(pid) = u8::try_from(params.property_id) else {
                // Property ID too large for standard services — send error response
                if confirmed {
                    let payload = application_layer::encode_property_value_ext_response(
                        params.object_type,
                        params.object_instance,
                        params.property_id,
                        0,
                        params.start_index,
                        &[],
                    );
                    self.queue_individual_frame(source, Priority::System, &payload);
                }
                return;
            };
            if let Ok(pid_enum) = PropertyId::try_from(pid) {
                if let Some(obj) = self.objects.get_mut(idx as usize) {
                    obj.write_property(pid_enum, params.start_index, params.count, data);
                }
            }
        }
        if confirmed {
            // Send write confirmation response
            let payload = application_layer::encode_property_value_ext_response(
                params.object_type,
                params.object_instance,
                params.property_id,
                params.count,
                params.start_index,
                &[],
            );
            self.queue_individual_frame(source, Priority::System, &payload);
        }
    }

    /// Apply a fill request from `AdditionalLoadControls` to the memory area.
    fn apply_fill(&mut self, fill: Option<(u32, u32, u8)>) {
        if let Some((offset, size, fill_byte)) = fill {
            let start = offset as usize;
            let end = start + size as usize;
            if end > MAX_MEMORY_SIZE {
                return;
            }
            if end > self.memory_area.len() {
                self.memory_area.resize(end, 0);
            }
            self.memory_area[start..end].fill(fill_byte);
        }
    }

    /// Reload the runtime table (address/association) from memory after ETS load completes.
    fn reload_runtime_table(&mut self, object_index: u8) {
        match object_index {
            OBJ_ADDR_TABLE => {
                let tbl_data = self.addr_table_object.data(&self.memory_area).to_vec();
                self.address_table.load(&tbl_data);
            }
            OBJ_ASSOC_TABLE => {
                let tbl_data = self.assoc_table_object.data(&self.memory_area).to_vec();
                self.association_table.load(&tbl_data);
            }
            _ => {} // Application program has no runtime table to reload
        }
    }

    // ── Frame builders ────────────────────────────────────────

    fn queue_group_value_write(&mut self, ga: u16, data: &[u8]) {
        let payload = application_layer::encode_group_value_write(data);
        self.queue_group_frame(ga, Priority::Low, &payload);
    }

    fn queue_group_value_read(&mut self, ga: u16) {
        let payload = application_layer::encode_group_value_read();
        self.queue_group_frame(ga, Priority::Low, &payload);
    }

    fn queue_group_value_response(&mut self, ga: u16, data: &[u8]) {
        let payload = application_layer::encode_group_value_response(data);
        self.queue_group_frame(ga, Priority::Low, &payload);
    }

    fn queue_individual_address_response(&mut self) {
        let payload = application_layer::encode_individual_address_response();
        self.queue_group_frame(BROADCAST_GA, Priority::System, &payload);
    }

    fn queue_device_descriptor_response(&mut self, destination: u16) {
        let payload = application_layer::encode_device_descriptor_response(MASK_VERSION_IP);
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    /// Respond to unsupported `DeviceDescriptorRead` with type 0x3F (C++ ref behavior).
    fn queue_device_descriptor_unsupported(&mut self, destination: u16) {
        let payload = application_layer::encode_device_descriptor_unsupported();
        self.queue_individual_frame(destination, Priority::System, &payload);
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
        let payload = application_layer::encode_property_response(
            object_index,
            property_id,
            count,
            start_index,
            data,
        );
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    fn queue_memory_response(&mut self, destination: u16, address: u16, data: &[u8]) {
        let payload = application_layer::encode_memory_response(address, data);
        self.queue_individual_frame(destination, Priority::System, &payload);
    }

    // ── Shared helpers ─────────────────────────────────────────

    /// Grow-and-copy into the memory area. Returns `false` if the write
    /// would exceed `MAX_MEMORY_SIZE`.
    fn write_to_memory(&mut self, addr: usize, data: &[u8]) -> bool {
        let needed = addr + data.len();
        if needed > MAX_MEMORY_SIZE {
            return false;
        }
        if needed > self.memory_area.len() {
            self.memory_area.resize(needed, 0);
        }
        self.memory_area[addr..addr + data.len()].copy_from_slice(data);
        true
    }

    // ── Shared frame helpers ──────────────────────────────────

    fn queue_group_frame(&mut self, ga: u16, priority: Priority, payload: &[u8]) {
        let src = self.individual_address();
        let dst = DestinationAddress::Group(GroupAddress::from_raw(ga));
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            priority,
            payload,
        ));
    }

    fn queue_broadcast_frame(&mut self, payload: &[u8]) {
        self.queue_group_frame(BROADCAST_GA, Priority::System, payload);
    }

    fn queue_individual_frame(&mut self, destination: u16, priority: Priority, payload: &[u8]) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            priority,
            payload,
        ));
    }

    fn queue_control_frame(
        &mut self,
        destination: u16,
        tpdu_type: knx_core::message::TpduType,
        seq_no: u8,
    ) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        let payload = knx_core::tpdu::encode_control(tpdu_type, seq_no);
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            Priority::System,
            &payload,
        ));
    }

    fn queue_data_connected_frame(
        &mut self,
        destination: u16,
        seq_no: u8,
        priority: Priority,
        apdu: &[u8],
    ) {
        let src = self.individual_address();
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(destination));
        let payload = knx_core::tpdu::encode_data_connected(seq_no, apdu);
        self.outbox.push_back(CemiFrame::new_l_data(
            MessageCode::LDataReq,
            src,
            dst,
            priority,
            &payload,
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::device_object;
    use crate::property::LoadState;

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
        bau.process_frame(&frame, 0);
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
        bau.process_frame(&frame, 0);
        assert!(bau.next_outgoing_frame().is_some());
    }

    #[test]
    fn poll_sends_pending_writes() {
        let mut bau = test_bau();
        // Mark tables as loaded so configured() returns true
        bau.addr_table_object.handle_load_event(&[1], 0);
        bau.addr_table_object.handle_load_event(&[2], 0);
        bau.assoc_table_object.handle_load_event(&[1], 0);
        bau.assoc_table_object.handle_load_event(&[2], 0);

        bau.group_objects.get_mut(1).unwrap().write_value(&[1]);
        bau.poll(0);
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
        bau.process_frame(&frame, 0);
        assert_eq!(bau.individual_address().raw(), 0x1105);
    }

    #[test]
    fn no_address_write_without_prog_mode() {
        let mut bau = test_bau();
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0xC0, 0x11, 0x05,
        ])
        .unwrap();
        bau.process_frame(&frame, 0);
        assert_eq!(bau.individual_address().raw(), 0x1101);
    }

    #[test]
    fn property_read_by_object_index() {
        let mut bau = test_bau();
        // Object 0 is device, should have ObjectType = 0x0000
        // Simulate a PropertyValueRead for object 0, PID 1 (ObjectType)
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0x60, 0x11, 0x02, 0x11, 0x01, 0x05, 0x03, 0xD5, 0x00, 0x01, 0x10,
            0x01,
        ])
        .unwrap();
        bau.process_frame(&frame, 0);
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
        bau.process_frame(&connect, 0);
        assert!(
            bau.transport.state() == crate::transport_layer::State::OpenIdle
                && bau.transport.connection_address() == 0x1102
        );

        // T_Disconnect
        let disconnect = CemiFrame::parse(&[
            0x29, 0x00, 0xB0, 0x60, 0x11, 0x02, 0x11, 0x01, 0x00, 0x81, 0x00,
        ])
        .unwrap();
        bau.process_frame(&disconnect, 0);
        assert!(
            bau.transport.state() == crate::transport_layer::State::Closed
                && bau.transport.connection_address() == 0x1102
        );
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
        let app_idx = bau.add_object(new_application_program_object()).unwrap();
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

    #[test]
    fn save_restore_roundtrip() {
        let mut bau = test_bau();

        // Simulate ETS programming: write some memory and mark tables as loaded
        bau.handle_memory_write(0x0000, &[0xAA, 0xBB, 0xCC]);

        // Manually set up address table object as loaded at offset 3, size 6
        bau.addr_table_object.handle_load_event(&[1], 0); // START_LOADING
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x01, 0x00]; // alloc 6 bytes
        bau.addr_table_object.handle_load_event(&alc, 3);
        // Write address table data at offset 3
        bau.handle_memory_write(0x0003, &[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]); // 2 entries: 1/0/1, 1/0/2
        let (became_loaded, _fill) = bau.addr_table_object.handle_load_event(&[2], 9); // LOAD_COMPLETED
        assert!(became_loaded);
        let tbl_data = bau.addr_table_object.data(&bau.memory_area).to_vec();
        bau.address_table.load(&tbl_data);

        // Verify table works
        assert_eq!(bau.address_table.get_tsap(0x0801), Some(1));
        assert_eq!(bau.address_table.get_tsap(0x0802), Some(2));

        // Save
        let saved = bau.save();

        // Restore into a fresh BAU
        let mut bau2 = test_bau();
        assert!(bau2.restore(&saved));

        // Verify tables are restored
        assert_eq!(bau2.address_table.get_tsap(0x0801), Some(1));
        assert_eq!(bau2.address_table.get_tsap(0x0802), Some(2));
        assert_eq!(
            bau2.memory_area(),
            &[0xAA, 0xBB, 0xCC, 0x00, 0x02, 0x08, 0x01, 0x08, 0x02]
        );
    }

    // ── BAU handler tests ─────────────────────────────────────

    fn handler_test_bau() -> Bau {
        let device =
            device_object::new_device_object([0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], [0x00; 6]);
        let mut bau = Bau::new(device, 2, 2);
        device_object::set_individual_address(bau.device_mut(), 0x1101);
        bau
    }

    #[test]
    fn restart_master_reset_clears_memory() {
        let mut bau = handler_test_bau();
        bau.handle_memory_write(0x0000, &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(bau.memory_area.len(), 4);

        bau.handle_restart_master_reset(0x1102, 1);
        assert!(bau.memory_area.is_empty());
    }

    #[test]
    fn restart_master_reset_resets_table_objects() {
        let mut bau = handler_test_bau();
        // Transition tables to Loaded state
        bau.addr_table_object.handle_load_event(&[1], 0);
        bau.addr_table_object.handle_load_event(&[2], 0);
        bau.assoc_table_object.handle_load_event(&[1], 0);
        bau.assoc_table_object.handle_load_event(&[2], 0);
        assert_eq!(bau.addr_table_object.load_state(), LoadState::Loaded);
        assert_eq!(bau.assoc_table_object.load_state(), LoadState::Loaded);

        bau.handle_restart_master_reset(0x1102, 1);
        assert_eq!(bau.addr_table_object.load_state(), LoadState::Unloaded);
        assert_eq!(bau.assoc_table_object.load_state(), LoadState::Unloaded);
        assert_eq!(bau.app_program_object.load_state(), LoadState::Unloaded);
    }

    #[test]
    fn restart_master_reset_sends_response() {
        let mut bau = handler_test_bau();
        bau.handle_restart_master_reset(0x1102, 1);
        let resp = bau
            .next_outgoing_frame()
            .expect("expected restart response");
        assert_eq!(resp.destination_address_raw(), 0x1102);
    }

    #[test]
    fn memory_write_and_read_roundtrip() {
        let mut bau = handler_test_bau();
        let data = [0x01, 0x02, 0x03, 0x04];
        bau.handle_memory_write(0x0010, &data);
        bau.handle_memory_read(0x1102, 4, 0x0010);

        let resp = bau
            .next_outgoing_frame()
            .expect("expected memory read response");
        // Payload: [apci_hi, apci_lo|count, addr_hi, addr_lo, data...]
        let payload = resp.payload();
        assert_eq!(&payload[4..8], &data);
    }

    #[test]
    fn memory_write_extends_memory() {
        let mut bau = handler_test_bau();
        assert!(bau.memory_area.is_empty());
        bau.handle_memory_write(0x0020, &[0xFF]);
        assert!(bau.memory_area.len() >= 0x21);
        assert_eq!(bau.memory_area[0x20], 0xFF);
    }

    #[test]
    fn memory_read_out_of_bounds() {
        let mut bau = handler_test_bau();
        // Memory is empty, reading should return empty data
        bau.handle_memory_read(0x1102, 4, 0x0000);
        let resp = bau
            .next_outgoing_frame()
            .expect("expected memory read response");
        // MemoryResponse with empty data: [apci_hi, apci_lo|0, addr_hi, addr_lo]
        let payload = resp.payload();
        assert_eq!(payload[1] & 0x0F, 0); // count nibble = 0
        assert_eq!(payload.len(), 4); // no data bytes appended
    }

    #[test]
    fn memory_write_rejects_oversized() {
        let mut bau = handler_test_bau();
        // ext write beyond MAX_MEMORY_SIZE must be rejected
        bau.handle_memory_ext_write(0x1102, MAX_MEMORY_SIZE as u32, &[0xFF]);
        assert!(bau.memory_area.is_empty());
    }

    #[test]
    fn add_object_overflow() {
        let mut bau = handler_test_bau();
        // BAU starts with 4 objects; fill up to 256
        while bau.objects.len() < 256 {
            let obj = InterfaceObject::new(crate::interface_object::ObjectType::Device);
            assert!(bau.add_object(obj).is_some());
        }
        // 257th must fail
        let obj = InterfaceObject::new(crate::interface_object::ObjectType::Device);
        assert!(bau.add_object(obj).is_none());
    }

    #[test]
    fn handle_group_value_response_checks_update_enable() {
        let mut bau = test_bau();
        // Load GO table: 2 GOs, GO1 has update_enable (A-flag) + comm_enable (K-flag)
        let go1: u16 = (1 << 15) | (1 << 10) | 1; // A + K + size=1
        let go2: u16 = (1 << 10) | 1; // K only (no A-flag)
        let mut tbl = Vec::new();
        tbl.extend_from_slice(&2u16.to_be_bytes());
        tbl.extend_from_slice(&go1.to_be_bytes());
        tbl.extend_from_slice(&go2.to_be_bytes());
        bau.group_object_table.load(&tbl);

        // GroupValueResponse to GA 0x0801 (tsap=1 → asap=1): APDU 0x00,0x41 = response with data=1
        let frame = CemiFrame::parse(&[
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, 0x08, 0x01, 0x01, 0x00, 0x41,
        ])
        .unwrap();
        bau.process_frame(&frame, 0);
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::Updated
        );
        assert_eq!(bau.group_objects.get(1).unwrap().value_ref(), &[0x01]);

        // GroupValueResponse to GA 0x0802 (tsap=2 → asap=2): GO2 has no A-flag → should NOT update
        let frame2 = CemiFrame::parse(&[
            0x29, 0x00, 0xBC, 0xE0, 0x11, 0x02, 0x08, 0x02, 0x01, 0x00, 0x41,
        ])
        .unwrap();
        bau.process_frame(&frame2, 0);
        assert_eq!(
            bau.group_objects.get(2).unwrap().comm_flag(),
            ComFlag::Uninitialized
        );
    }

    #[test]
    fn handle_property_description_read_known_property() {
        let mut bau = test_bau();
        // Object 0 (device) has PID_OBJECT_TYPE (1) — read its description
        bau.handle_property_description_read(0x1102, 0, 1, 0);
        let resp = bau.next_outgoing_frame().expect("expected response");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        // Payload should contain property_id=1 (not 0, which signals error)
        let payload = resp.payload();
        // PropertyDescriptionResponse APDU: [apci_hi, apci_lo, obj_idx, pid, pidx, ...]
        assert_ne!(payload[3], 0, "property_id=0 means error");
    }

    #[test]
    fn handle_property_description_read_unknown_object() {
        let mut bau = test_bau();
        // Object index 99 doesn't exist
        bau.handle_property_description_read(0x1102, 99, 1, 0);
        let resp = bau.next_outgoing_frame().expect("expected error response");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        // Error response has property_id=0
        let payload = resp.payload();
        assert_eq!(payload[3], 0, "expected property_id=0 for unknown object");
    }

    #[test]
    fn handle_memory_ext_read_roundtrip() {
        let mut bau = test_bau();
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        bau.handle_memory_ext_write(0x1102, 0x0010, &data);
        // Drain the write response
        bau.next_outgoing_frame().unwrap();

        bau.handle_memory_ext_read(0x1102, 4, 0x0010);
        let resp = bau
            .next_outgoing_frame()
            .expect("expected ext read response");
        let payload = resp.payload();
        // MemoryExtReadResponse: [apci_hi, apci_lo, return_code, addr(3 bytes), data...]
        assert_eq!(payload[2], 0, "return_code should be 0 (success)");
        assert_eq!(&payload[6..10], &data);
    }

    #[test]
    fn handle_memory_ext_write_roundtrip() {
        let mut bau = test_bau();
        bau.handle_memory_ext_write(0x1102, 0x0020, &[0xCA, 0xFE]);
        assert_eq!(bau.memory_area[0x20], 0xCA);
        assert_eq!(bau.memory_area[0x21], 0xFE);
    }

    #[test]
    fn init_read_requests_queues_for_i_flag() {
        let mut bau = test_bau();
        // Load GO table: GO1 has I-flag (read_on_init) + K-flag (comm_enable)
        let go1: u16 = (1 << 13) | (1 << 10) | 1; // I + K + size=1
        let go2: u16 = (1 << 10) | 1; // K only (no I-flag)
        let mut tbl = Vec::new();
        tbl.extend_from_slice(&2u16.to_be_bytes());
        tbl.extend_from_slice(&go1.to_be_bytes());
        tbl.extend_from_slice(&go2.to_be_bytes());
        bau.group_object_table.load(&tbl);

        bau.init_read_requests();

        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::ReadRequest
        );
        // GO2 has no I-flag → should remain Uninitialized
        assert_eq!(
            bau.group_objects.get(2).unwrap().comm_flag(),
            ComFlag::Uninitialized
        );
    }

    #[test]
    fn save_restore_full_roundtrip() {
        let mut bau = test_bau();

        // Set up loaded tables with memory data
        bau.addr_table_object.handle_load_event(&[1], 0);
        let alc = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x01, 0x00];
        bau.addr_table_object.handle_load_event(&alc, 0);
        bau.handle_memory_write(0x0000, &[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
        bau.addr_table_object.handle_load_event(&[2], 6);
        let tbl_data = bau.addr_table_object.data(&bau.memory_area).to_vec();
        bau.address_table.load(&tbl_data);

        bau.assoc_table_object.handle_load_event(&[1], 0);
        let alc2 = [0x03, 0x0B, 0x00, 0x00, 0x00, 0x0A, 0x01, 0x06];
        bau.assoc_table_object.handle_load_event(&alc2, 0);
        bau.handle_memory_write(
            0x0006,
            &[0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02],
        );
        bau.assoc_table_object.handle_load_event(&[2], 16);
        let tbl_data2 = bau.assoc_table_object.data(&bau.memory_area).to_vec();
        bau.association_table.load(&tbl_data2);

        // Write some extra memory
        bau.handle_memory_write(0x0010, &[0xCA, 0xFE]);

        // Verify pre-save state
        assert!(bau.configured());
        assert_eq!(bau.address_table.get_tsap(0x0801), Some(1));

        let saved = bau.save();

        // Restore into fresh BAU
        let mut bau2 = test_bau();
        assert!(!bau2.configured());
        assert!(bau2.restore(&saved));

        assert!(bau2.configured());
        assert_eq!(bau2.address_table.get_tsap(0x0801), Some(1));
        assert_eq!(bau2.address_table.get_tsap(0x0802), Some(2));
        assert_eq!(bau2.memory_area[0x10], 0xCA);
        assert_eq!(bau2.memory_area[0x11], 0xFE);
    }

    #[test]
    fn authorize_request_responds_with_level() {
        let mut bau = test_bau();
        // Build AuthorizeRequest APDU: APCI 0x3D1, data=[reserved, key(4)]
        let apdu_payload = &[0x03, 0xD1, 0x00, 0x00, 0x00, 0x00, 0x00];
        let src = IndividualAddress::from_raw(0x1102);
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(0x1101));
        let frame = CemiFrame::new_l_data(MessageCode::LDataInd, src, dst, Priority::System, apdu_payload);
        bau.process_frame(&frame, 0);
        let resp = bau.next_outgoing_frame().expect("expected AuthorizeResponse");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        // AuthorizeResponse APCI = 0x3D2, payload[2] = level
        let p = resp.payload();
        assert_eq!(p[0] & 0x03, 0x03);
        assert_eq!(p[1], 0xD2);
        assert_eq!(p[2], 0); // AUTH_LEVEL_FULL
    }

    #[test]
    fn key_write_responds_with_level() {
        let mut bau = test_bau();
        // KeyWrite APCI 0x3D3, data=[level, key(4)]
        let apdu_payload = &[0x03, 0xD3, 0x02, 0x00, 0x00, 0x00, 0xFF];
        let src = IndividualAddress::from_raw(0x1102);
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(0x1101));
        let frame = CemiFrame::new_l_data(MessageCode::LDataInd, src, dst, Priority::System, apdu_payload);
        bau.process_frame(&frame, 0);
        let resp = bau.next_outgoing_frame().expect("expected KeyResponse");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        let p = resp.payload();
        assert_eq!(p[0] & 0x03, 0x03);
        assert_eq!(p[1], 0xD4); // KeyResponse APCI low
        assert_eq!(p[2], 0x02); // echoed level
    }

    #[test]
    fn function_property_command_responds() {
        let mut bau = test_bau();
        // FunctionPropertyCommand APCI 0x2C7, data=[obj_idx, pid, ...]
        let apdu_payload = &[0x02, 0xC7, 0x00, 0x01];
        let src = IndividualAddress::from_raw(0x1102);
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(0x1101));
        let frame = CemiFrame::new_l_data(MessageCode::LDataInd, src, dst, Priority::System, apdu_payload);
        bau.process_frame(&frame, 0);
        let resp = bau.next_outgoing_frame().expect("expected FunctionPropertyStateResponse");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        let p = resp.payload();
        assert_eq!(p[0] & 0x03, 0x02);
        assert_eq!(p[1], 0xC9); // FunctionPropertyStateResponse APCI low
        assert_eq!(p[2], 0x00); // object_index
        assert_eq!(p[3], 0x01); // property_id
    }

    #[test]
    fn adc_read_responds_with_zero() {
        let mut bau = test_bau();
        // AdcRead APCI 0x180 (short APCI, but needs long encoding for channel+count)
        let apdu_payload = &[0x01, 0x80, 0x03, 0x01]; // APCI=0x180, channel=3, count=1
        let src = IndividualAddress::from_raw(0x1102);
        let dst = DestinationAddress::Individual(IndividualAddress::from_raw(0x1101));
        let frame = CemiFrame::new_l_data(MessageCode::LDataInd, src, dst, Priority::System, apdu_payload);
        bau.process_frame(&frame, 0);
        let resp = bau.next_outgoing_frame().expect("expected AdcResponse");
        assert_eq!(resp.destination_address_raw(), 0x1102);
        let p = resp.payload();
        // AdcResponse APCI 0x1C0, value should be 0
        assert_eq!(p[0] & 0x03, 0x01);
        assert_eq!(p[1] & 0xC0, 0xC0); // AdcResponse APCI low high bits
        assert_eq!(p[3], 0x00); // value high
        assert_eq!(p[4], 0x00); // value low
    }

    #[test]
    fn poll_processes_read_requests() {
        let mut bau = test_bau();
        // Mark tables as loaded
        bau.addr_table_object.handle_load_event(&[1], 0);
        bau.addr_table_object.handle_load_event(&[2], 0);
        bau.assoc_table_object.handle_load_event(&[1], 0);
        bau.assoc_table_object.handle_load_event(&[2], 0);

        bau.group_objects.get_mut(1).unwrap().request_object_read();
        assert_eq!(bau.group_objects.get(1).unwrap().comm_flag(), ComFlag::ReadRequest);

        bau.poll(0);
        let frame = bau.next_outgoing_frame().expect("expected GroupValueRead");
        assert_eq!(frame.destination_address_raw(), 0x0801);
        // GroupValueRead APCI = 0x000
        let p = frame.payload();
        assert_eq!(p[0], 0x00);
        assert_eq!(p[1], 0x00);
        assert_eq!(
            bau.group_objects.get(1).unwrap().comm_flag(),
            ComFlag::Transmitting
        );
    }

    #[test]
    fn load_tables_from_memory_loads_addr_and_assoc() {
        let mut bau = test_bau();
        // Clear pre-loaded tables
        bau.address_table = AddressTable::new();
        bau.association_table = AssociationTable::new();
        assert_eq!(bau.address_table.entry_count(), 0);
        assert_eq!(bau.association_table.entry_count(), 0);

        // Set up memory: addr table at offset 0 (6 bytes), assoc table at offset 6 (10 bytes)
        let mut mem = Vec::new();
        mem.extend_from_slice(&[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]); // addr: 2 entries
        mem.extend_from_slice(&[0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02]); // assoc: 2 entries
        bau.set_memory_area(mem);

        bau.load_tables_from_memory(0, 6, 6, 10);

        assert_eq!(bau.address_table.entry_count(), 2);
        assert_eq!(bau.address_table.get_tsap(0x0801), Some(1));
        assert_eq!(bau.address_table.get_tsap(0x0802), Some(2));
        assert_eq!(bau.association_table.entry_count(), 2);
        assert_eq!(bau.association_table.translate_asap(1), Some(1));
        assert_eq!(bau.association_table.translate_asap(2), Some(2));
    }
}
