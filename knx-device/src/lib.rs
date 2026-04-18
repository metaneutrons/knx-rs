// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

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

pub mod address_table;
pub mod application_layer;
pub mod application_program;
pub mod association_table;
pub mod bau;
pub mod device_object;
pub mod group_object;
pub mod group_object_table;
pub mod interface_object;
pub mod memory;
pub mod property;
pub mod transport_layer;
