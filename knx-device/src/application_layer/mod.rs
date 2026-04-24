// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Application Layer — APDU service dispatch.
//!
//! Processes incoming APDUs and generates outgoing ones. This is the
//! bridge between the transport layer and the device's interface objects
//! and group objects.
//!
//! # Module structure
//!
//! - [`types`] — `AppIndication` and `AppLayerError`
//! - [`encode`] — outgoing APDU encoding functions
//! - [`parse`] — incoming APDU parsing functions

/// Outgoing APDU encoding functions for all KNX application-layer services.
pub mod encode;
/// Incoming APDU parsing — converts raw bytes into [`AppIndication`] variants.
pub mod parse;
mod types;

// Re-export public API at module level for backward compatibility.
pub use encode::{
    encode_adc_response, encode_authorize_response, encode_device_descriptor_response,
    encode_device_descriptor_unsupported, encode_function_property_state_response,
    encode_group_value_read, encode_group_value_response, encode_group_value_write,
    encode_individual_address_response, encode_individual_address_serial_number_response,
    encode_key_response, encode_memory_ext_read_response, encode_memory_ext_write_response,
    encode_memory_response, encode_property_description_response,
    encode_property_ext_description_response, encode_property_response,
    encode_property_value_ext_response, encode_raw_apdu, encode_restart_response,
    encode_system_network_parameter_response,
};
pub use parse::{parse_indication, parse_raw_apdu};
pub use types::{AppIndication, AppLayerError};

use knx_core::message::ApduType;

// ── APCI byte helpers (SSOT: derived from ApduType enum) ─────

/// Split an `ApduType` into its two APCI wire bytes `[high, low]`.
///
/// The 10-bit APCI value is encoded as:
/// - `high`: bits 9..8 (masked into the lower 2 bits)
/// - `low`: bits 7..0
#[expect(
    clippy::cast_possible_truncation,
    reason = "APCI is 10-bit, both halves fit in u8"
)]
pub(crate) const fn apci_bytes(t: ApduType) -> [u8; 2] {
    let v = t as u16;
    [(v >> 8) as u8, v as u8]
}

// ── Shared bit-mask constants ────────────────────────────────

/// 6-bit mask for short APDU values and descriptor types.
pub(crate) const MASK_6BIT: u8 = 0x3F;
/// 4-bit mask for count fields and nibble extraction.
pub(crate) const MASK_4BIT: u8 = 0x0F;
/// 12-bit mask for `start_index` fields.
pub(crate) const MASK_12BIT: u16 = 0x0FFF;
/// Write-enable flag in property description type byte.
pub(crate) const WRITE_ENABLE_FLAG: u8 = 0x80;
/// Unsupported device descriptor type (0x3F = all bits set in 6-bit field).
pub(crate) const DESCRIPTOR_TYPE_UNSUPPORTED: u8 = 0x3F;
