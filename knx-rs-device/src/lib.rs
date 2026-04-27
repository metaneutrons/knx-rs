// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! `knx-device` — KNX device stack with ETS programming support.
//!
//! # Architecture
//!
//! ```text
//! Application code ←→ GroupObjects ←→ ApplicationLayer ←→ TransportLayer ←→ Bus
//!                                          ↕
//!                                   InterfaceObjects (properties, tables)
//!                                          ↕
//!                                      DeviceMemory (persistence)
//! ```
//!
//! # `no_std` Support
//!
//! This crate is `no_std`-compatible with `alloc`.

#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

extern crate alloc;

/// Address table — maps TSAPs to group addresses.
pub mod address_table;
/// Application layer — APDU encoding, parsing, and service dispatch.
pub mod application_layer;
/// Application program object — ETS-downloadable program data.
pub mod application_program;
/// Association table — maps ASAPs to TSAPs.
pub mod association_table;
/// Bus Access Unit — main device controller.
pub mod bau;
/// BAU state persistence — save/restore device state.
pub mod bau_persistence;
/// Device object — identity and configuration properties.
pub mod device_object;
/// Group objects — communication objects with DPT-aware values.
pub mod group_object;
/// Group object table — descriptors for group object configuration.
pub mod group_object_table;
/// Interface objects — property containers for device configuration.
pub mod interface_object;
/// Device memory — persistence backend for embedded targets.
pub mod memory;
/// Property system — data model for KNX interface objects.
pub mod property;
/// Table objects — ETS Load State Machine for address/association/program tables.
pub mod table_object;
/// Transport layer — connection-oriented point-to-point communication.
pub mod transport_layer;
