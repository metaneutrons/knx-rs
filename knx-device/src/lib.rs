// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! `knx-device` — KNX device stack with ETS programming support.
//!
//! Provides the full device-side KNX stack:
//!
//! - [`property`] — Property system (data types, read/write)
//! - [`interface_object`] — Interface object model
//! - [`device_object`] — Device object (individual address, serial number, etc.)
//! - [`application_program`] — Application program object (ETS parameters)
//! - [`address_table`] — Address table (TSAP → group address mapping)
//! - [`association_table`] — Association table (TSAP → ASAP mapping)
//! - [`group_object_table`] — Group object table (communication object descriptors)
//!
//! # `no_std` Support
//!
//! This crate is `no_std`-compatible with `alloc`.

#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

extern crate alloc;

pub mod address_table;
pub mod application_program;
pub mod association_table;
pub mod device_object;
pub mod group_object_table;
pub mod interface_object;
pub mod property;
