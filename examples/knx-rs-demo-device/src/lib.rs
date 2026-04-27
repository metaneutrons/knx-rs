// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX demo device — reusable device setup for the demo and tests.

use knx_rs_core::dpt::{DPT_SCALING, DPT_STRING_ASCII, DPT_SWITCH, DPT_VALUE_TEMP, DptValue};
use knx_rs_device::bau::Bau;
use knx_rs_device::device_object;
use knx_rs_device::group_object::ComFlag;

/// Create the demo device BAU with 4 group objects and loaded tables.
pub fn create_demo_bau(individual_address: u16) -> Bau {
    let device = device_object::new_device_object([0x00, 0xFA, 0xDE, 0xD0, 0x00, 0x01], [0x00; 6]);
    let mut bau = Bau::new(device, 4, 2);
    device_object::set_individual_address(bau.device_mut(), individual_address);

    if let Some(go) = bau.group_objects_mut().get_mut(1) {
        go.set_dpt(DPT_VALUE_TEMP);
    }
    if let Some(go) = bau.group_objects_mut().get_mut(2) {
        go.set_dpt(DPT_SWITCH);
    }
    if let Some(go) = bau.group_objects_mut().get_mut(3) {
        go.set_dpt(DPT_SCALING);
    }
    if let Some(go) = bau.group_objects_mut().get_mut(4) {
        go.set_dpt(DPT_STRING_ASCII);
    }

    bau.load_tables_from_memory(0, 0, 0, 0);

    // Load address table directly
    let addr_data = [0x00, 0x04, 0x08, 0x01, 0x08, 0x02, 0x08, 0x03, 0x08, 0x04];
    let assoc_data = [
        0x00, 0x04, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02, 0x00, 0x03, 0x00, 0x03, 0x00,
        0x04, 0x00, 0x04,
    ];
    // We need to set memory area and load tables through the BAU's public API
    let mut mem = Vec::new();
    mem.extend_from_slice(&addr_data);
    mem.extend_from_slice(&assoc_data);
    bau.set_memory_area(mem);
    bau.load_tables_from_memory(0, addr_data.len(), addr_data.len(), assoc_data.len());

    if let Some(go) = bau.group_objects_mut().get_mut(1) {
        let _ = go.set_value(&DptValue::Float(21.0));
        go.set_comm_flag(ComFlag::Ok);
    }

    bau
}
