// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX demo device — reusable device setup for the demo and tests.

use knx_core::dpt::{DPT_SCALING, DPT_STRING_ASCII, DPT_SWITCH, DPT_VALUE_TEMP, DptValue};
use knx_device::bau::Bau;
use knx_device::device_object;
use knx_device::group_object::ComFlag;

/// Create the demo device BAU with 4 group objects and loaded tables.
pub fn create_demo_bau(individual_address: u16) -> Bau {
    let device = device_object::new_device_object([0x00, 0xFA, 0xDE, 0xD0, 0x00, 0x01], [0x00; 6]);
    let mut bau = Bau::new(device, 4, 2);
    device_object::set_individual_address(bau.device_mut(), individual_address);

    if let Some(go) = bau.group_objects.get_mut(1) {
        go.set_dpt(DPT_VALUE_TEMP);
    }
    if let Some(go) = bau.group_objects.get_mut(2) {
        go.set_dpt(DPT_SWITCH);
    }
    if let Some(go) = bau.group_objects.get_mut(3) {
        go.set_dpt(DPT_SCALING);
    }
    if let Some(go) = bau.group_objects.get_mut(4) {
        go.set_dpt(DPT_STRING_ASCII);
    }

    bau.address_table
        .load(&[0x00, 0x04, 0x08, 0x01, 0x08, 0x02, 0x08, 0x03, 0x08, 0x04]);

    bau.association_table.load(&[
        0x00, 0x04, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02, 0x00, 0x03, 0x00, 0x03, 0x00,
        0x04, 0x00, 0x04,
    ]);

    if let Some(go) = bau.group_objects.get_mut(1) {
        let _ = go.set_value(&DptValue::Float(21.0));
        go.set_comm_flag(ComFlag::Ok);
    }

    bau
}
