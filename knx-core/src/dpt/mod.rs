// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Datapoint Type (DPT) framework.
//!
//! Provides the [`Dpt`] identifier, [`DptValue`] for type-safe values,
//! and encoding/decoding for all common KNX datapoint types.
//!
//! # Supported DPT main groups
//!
//! | Main | Name | Rust type |
//! |------|------|-----------|
//! | 1 | Boolean | `f64` (0.0/1.0) |
//! | 2 | Controlled boolean | `f64` |
//! | 3 | Controlled step | `f64` |
//! | 4 | Character | `f64` (ASCII/Latin-1 code) |
//! | 5 | Unsigned 8-bit | `f64` |
//! | 6 | Signed 8-bit | `f64` |
//! | 7 | Unsigned 16-bit | `f64` |
//! | 8 | Signed 16-bit | `f64` |
//! | 9 | 16-bit float | `f64` |
//! | 10 | Time of day | `f64` (seconds since midnight) |
//! | 11 | Date | `f64` (days: YYYYMMDD as integer) |
//! | 12 | Unsigned 32-bit | `f64` |
//! | 13 | Signed 32-bit | `f64` |
//! | 14 | IEEE 754 float | `f64` |
//! | 15 | Access data | `f64` (6-digit BCD code) |
//! | 16 | String | `String` (14 bytes ASCII/Latin-1) |
//! | 17 | Scene number | `f64` (0–63) |
//! | 18 | Scene control | `f64` |
//! | 19 | Date and time | `f64` (Unix timestamp) |
//! | 27 | 32-bit field | `f64` |
//! | 28 | Unicode string | `String` (UTF-8) |
//! | 29 | Signed 64-bit | `f64` |
//! | 232 | RGB | `f64` (0xRRGGBB) |
//! | 251 | RGBW | `f64` (0xRRGGBBWW) |

mod convert;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A KNX Datapoint Type identifier (main group / sub group / index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dpt {
    /// Main group number (e.g. 1 for boolean, 9 for 16-bit float).
    pub main: u16,
    /// Sub group number (e.g. 1 for DPT 1.001 Switch).
    pub sub: u16,
    /// Index (used by DPT 10.001 `TimeOfDay`, usually 0).
    pub index: u16,
}

impl Dpt {
    /// Create a new DPT identifier.
    pub const fn new(main: u16, sub: u16) -> Self {
        Self {
            main,
            sub,
            index: 0,
        }
    }

    /// Create a new DPT identifier with index.
    pub const fn with_index(main: u16, sub: u16, index: u16) -> Self {
        Self { main, sub, index }
    }

    /// Wire data length in bytes for this DPT's main group.
    ///
    /// Matches the C++ `Dpt::dataLength()` implementation.
    pub const fn data_length(self) -> u8 {
        match self.main {
            7 | 8 | 9 | 22 | 207 | 217 | 234 | 237 | 244 | 246 => 2,
            10 | 11 | 30 | 206 | 225 | 232 | 240 | 250 | 254 => 3,
            12 | 13 | 14 | 15 | 27 | 241 | 251 => 4,
            252 => 5,
            219 | 222 | 229 | 235 | 242 | 245 | 249 => 6,
            19 | 29 | 230 | 255 | 275 => 8,
            16 => 14,
            285 => 16,
            _ => 1,
        }
    }
}

impl fmt::Display for Dpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.index == 0 {
            write!(f, "{}.{:03}", self.main, self.sub)
        } else {
            write!(f, "{}.{:03}.{}", self.main, self.sub, self.index)
        }
    }
}

/// A decoded DPT value — either numeric or string.
#[derive(Debug, Clone, PartialEq)]
pub enum DptValue {
    /// Numeric value (covers all integer and float DPTs).
    Numeric(f64),
    /// String value (DPT 16, 28).
    Text(String),
}

impl DptValue {
    /// Get the numeric value, if this is a `Numeric` variant.
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Numeric(v) => Some(*v),
            Self::Text(_) => None,
        }
    }

    /// Get the string value, if this is a `Text` variant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            Self::Numeric(_) => None,
        }
    }
}

impl From<f64> for DptValue {
    fn from(v: f64) -> Self {
        Self::Numeric(v)
    }
}

impl From<String> for DptValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for DptValue {
    fn from(s: &str) -> Self {
        Self::Text(String::from(s))
    }
}

/// Error returned when DPT encoding or decoding fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DptError {
    /// The payload is too short for the requested DPT.
    PayloadTooShort,
    /// The DPT main group is not supported.
    UnsupportedDpt(Dpt),
    /// The value is out of range for the requested DPT.
    OutOfRange,
    /// Wrong value type (e.g. numeric for a string DPT).
    TypeMismatch,
}

impl fmt::Display for DptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooShort => f.write_str("payload too short for DPT"),
            Self::UnsupportedDpt(dpt) => write!(f, "unsupported DPT: {dpt}"),
            Self::OutOfRange => f.write_str("value out of range for DPT"),
            Self::TypeMismatch => f.write_str("wrong value type for DPT"),
        }
    }
}

impl core::error::Error for DptError {}

/// Decode a KNX bus payload into a [`DptValue`].
///
/// # Errors
///
/// Returns [`DptError`] if the payload is too short or the DPT is unsupported.
pub fn decode_value(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    convert::decode_value(dpt, payload)
}

/// Encode a [`DptValue`] into a KNX bus payload.
///
/// # Errors
///
/// Returns [`DptError`] if the value is out of range or the DPT is unsupported.
pub fn encode_value(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    convert::encode_value(dpt, value)
}

/// Decode a KNX bus payload into an `f64` value (numeric DPTs only).
///
/// # Errors
///
/// Returns [`DptError`] if the payload is too short, the DPT is unsupported,
/// or the DPT is a string type.
pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<f64, DptError> {
    convert::decode(dpt, payload)
}

/// Encode an `f64` value into a KNX bus payload (numeric DPTs only).
///
/// # Errors
///
/// Returns [`DptError`] if the value is out of range or the DPT is unsupported.
pub fn encode(dpt: Dpt, value: f64) -> Result<Vec<u8>, DptError> {
    convert::encode(dpt, value)
}

/// Decode a KNX bus payload into a `String` (string DPTs only: 16, 28).
///
/// # Errors
///
/// Returns [`DptError`] if the DPT is not a string type.
pub fn decode_string(dpt: Dpt, payload: &[u8]) -> Result<String, DptError> {
    convert::decode_string(dpt, payload)
}

/// Encode a string into a KNX bus payload (string DPTs only: 16, 28).
///
/// # Errors
///
/// Returns [`DptError`] if the DPT is not a string type.
pub fn encode_string(dpt: Dpt, value: &str) -> Result<Vec<u8>, DptError> {
    convert::encode_string(dpt, value)
}

// ── Well-known DPT constants ──────────────────────────────────

/// DPT 1.001 — Switch (bool).
pub const DPT_SWITCH: Dpt = Dpt::new(1, 1);
/// DPT 1.002 — Bool.
pub const DPT_BOOL: Dpt = Dpt::new(1, 2);
/// DPT 4.001 — ASCII character.
pub const DPT_CHAR_ASCII: Dpt = Dpt::new(4, 1);
/// DPT 4.002 — ISO 8859-1 character.
pub const DPT_CHAR_8859_1: Dpt = Dpt::new(4, 2);
/// DPT 5.001 — Scaling (0–100%).
pub const DPT_SCALING: Dpt = Dpt::new(5, 1);
/// DPT 5.003 — Angle (0–360°).
pub const DPT_ANGLE: Dpt = Dpt::new(5, 3);
/// DPT 5.010 — Unsigned count (0–255).
pub const DPT_VALUE_1_UCOUNT: Dpt = Dpt::new(5, 10);
/// DPT 7.001 — Unsigned 16-bit count.
pub const DPT_VALUE_2_UCOUNT: Dpt = Dpt::new(7, 1);
/// DPT 8.001 — Signed 16-bit count.
pub const DPT_VALUE_2_COUNT: Dpt = Dpt::new(8, 1);
/// DPT 9.001 — Temperature (°C), 16-bit float.
pub const DPT_VALUE_TEMP: Dpt = Dpt::new(9, 1);
/// DPT 9.004 — Lux, 16-bit float.
pub const DPT_VALUE_LUX: Dpt = Dpt::new(9, 4);
/// DPT 10.001 — Time of day.
pub const DPT_TIMEOFDAY: Dpt = Dpt::with_index(10, 1, 1);
/// DPT 11.001 — Date.
pub const DPT_DATE: Dpt = Dpt::new(11, 1);
/// DPT 12.001 — Unsigned 32-bit count.
pub const DPT_VALUE_4_UCOUNT: Dpt = Dpt::new(12, 1);
/// DPT 13.001 — Signed 32-bit count.
pub const DPT_VALUE_4_COUNT: Dpt = Dpt::new(13, 1);
/// DPT 14.056 — Power (W), 32-bit float.
pub const DPT_VALUE_POWER: Dpt = Dpt::new(14, 56);
/// DPT 15.000 — Access data (4 bytes).
pub const DPT_ACCESS_DATA: Dpt = Dpt::new(15, 0);
/// DPT 16.000 — ASCII string (14 bytes).
pub const DPT_STRING_ASCII: Dpt = Dpt::new(16, 0);
/// DPT 16.001 — ISO 8859-1 string (14 bytes).
pub const DPT_STRING_8859_1: Dpt = Dpt::new(16, 1);
/// DPT 17.001 — Scene number (0–63).
pub const DPT_SCENE_NUMBER: Dpt = Dpt::new(17, 1);
/// DPT 18.001 — Scene control.
pub const DPT_SCENE_CONTROL: Dpt = Dpt::new(18, 1);
/// DPT 19.001 — Date and time.
pub const DPT_DATETIME: Dpt = Dpt::new(19, 1);
/// DPT 29.010 — Active energy (Wh), signed 64-bit.
pub const DPT_ACTIVE_ENERGY_V64: Dpt = Dpt::new(29, 10);
/// DPT 232.600 — RGB colour.
pub const DPT_COLOUR_RGB: Dpt = Dpt::new(232, 600);
/// DPT 251.600 — RGBW colour.
pub const DPT_COLOUR_RGBW: Dpt = Dpt::new(251, 600);
