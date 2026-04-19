// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Datapoint Type (DPT) framework.
//!
//! The [`DptValue`] enum is the single type for all KNX datapoint values.
//! Each variant matches the natural type for its DPT group:
//!
//! | Variant | DPT groups |
//! |---------|------------|
//! | `Bool` | 1 |
//! | `UInt` | 2, 3, 4, 5, 7, 12, 15, 17, 18, 26, 232, 238 |
//! | `Int` | 6, 8, 13, 27 |
//! | `Float` | 9, 14 |
//! | `Int64` | 29 |
//! | `Text` | 16, 28 |
//! | `Bytes` | 10, 11, 19, 217, 219, 221, 225, 231, 234, 235, 239, 251 |

mod convert;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A KNX Datapoint Type identifier (main group / sub group / index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dpt {
    /// Main group number.
    pub main: u16,
    /// Sub group number.
    pub sub: u16,
    /// Index (usually 0).
    pub index: u16,
}

impl Dpt {
    /// Create a new DPT identifier.
    pub const fn new(main: u16, sub: u16) -> Self {
        Self { main, sub, index: 0 }
    }

    /// Create a new DPT identifier with index.
    pub const fn with_index(main: u16, sub: u16, index: u16) -> Self {
        Self { main, sub, index }
    }

    /// Wire data length in bytes for this DPT's main group.
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

/// A typed KNX datapoint value.
///
/// Each variant matches the natural type for its DPT group.
/// Use [`From`] impls for ergonomic construction.
#[derive(Debug, Clone, PartialEq)]
pub enum DptValue {
    /// Boolean (DPT 1).
    Bool(bool),
    /// Unsigned integer (DPT 2, 3, 4, 5, 7, 12, 15, 17, 18, 26, 232, 238).
    UInt(u32),
    /// Signed integer (DPT 6, 8, 13, 27).
    Int(i32),
    /// Floating point (DPT 9, 14).
    Float(f64),
    /// Signed 64-bit integer (DPT 29).
    Int64(i64),
    /// String (DPT 16, 28).
    Text(String),
    /// Raw bytes (DPT 10, 11, 19, 217, 219, 221, 225, 231, 234, 235, 239, 251).
    Bytes(Vec<u8>),
}

impl DptValue {
    /// Get as bool. Returns `None` if not `Bool`.
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as u32. Returns `None` if not `UInt`.
    pub const fn as_u32(&self) -> Option<u32> {
        match self {
            Self::UInt(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as i32. Returns `None` if not `Int`.
    pub const fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as f64. Converts from any numeric variant.
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Self::UInt(v) => Some(*v as f64),
            Self::Int(v) => Some(*v as f64),
            Self::Int64(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Get as i64. Returns `None` if not `Int64`.
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int64(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as string slice. Returns `None` if not `Text`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get as byte slice. Returns `None` if not `Bytes`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

// ── From impls ────────────────────────────────────────────────

impl From<bool> for DptValue {
    fn from(v: bool) -> Self { Self::Bool(v) }
}

impl From<u8> for DptValue {
    fn from(v: u8) -> Self { Self::UInt(u32::from(v)) }
}

impl From<u16> for DptValue {
    fn from(v: u16) -> Self { Self::UInt(u32::from(v)) }
}

impl From<u32> for DptValue {
    fn from(v: u32) -> Self { Self::UInt(v) }
}

impl From<i8> for DptValue {
    fn from(v: i8) -> Self { Self::Int(i32::from(v)) }
}

impl From<i16> for DptValue {
    fn from(v: i16) -> Self { Self::Int(i32::from(v)) }
}

impl From<i32> for DptValue {
    fn from(v: i32) -> Self { Self::Int(v) }
}

impl From<i64> for DptValue {
    fn from(v: i64) -> Self { Self::Int64(v) }
}

impl From<f32> for DptValue {
    fn from(v: f32) -> Self { Self::Float(f64::from(v)) }
}

impl From<f64> for DptValue {
    fn from(v: f64) -> Self { Self::Float(v) }
}

impl From<String> for DptValue {
    fn from(s: String) -> Self { Self::Text(s) }
}

impl From<&str> for DptValue {
    fn from(s: &str) -> Self { Self::Text(String::from(s)) }
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
    /// Wrong value type for the DPT (e.g. Bool for a float DPT).
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
pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    convert::decode(dpt, payload)
}

/// Encode a [`DptValue`] into a KNX bus payload.
///
/// # Errors
///
/// Returns [`DptError`] if the value type doesn't match the DPT or is out of range.
pub fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    convert::encode(dpt, value)
}

// ── Well-known DPT constants ──────────────────────────────────

/// DPT 1.001 — Switch (bool).
pub const DPT_SWITCH: Dpt = Dpt::new(1, 1);
/// DPT 1.002 — Bool.
pub const DPT_BOOL: Dpt = Dpt::new(1, 2);
/// DPT 4.001 — ASCII character.
pub const DPT_CHAR_ASCII: Dpt = Dpt::new(4, 1);
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
/// DPT 9.001 — Temperature (°C).
pub const DPT_VALUE_TEMP: Dpt = Dpt::new(9, 1);
/// DPT 9.004 — Lux.
pub const DPT_VALUE_LUX: Dpt = Dpt::new(9, 4);
/// DPT 10.001 — Time of day.
pub const DPT_TIMEOFDAY: Dpt = Dpt::with_index(10, 1, 1);
/// DPT 11.001 — Date.
pub const DPT_DATE: Dpt = Dpt::new(11, 1);
/// DPT 12.001 — Unsigned 32-bit count.
pub const DPT_VALUE_4_UCOUNT: Dpt = Dpt::new(12, 1);
/// DPT 13.001 — Signed 32-bit count.
pub const DPT_VALUE_4_COUNT: Dpt = Dpt::new(13, 1);
/// DPT 14.056 — Power (W).
pub const DPT_VALUE_POWER: Dpt = Dpt::new(14, 56);
/// DPT 15.000 — Access data.
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
/// DPT 29.010 — Active energy (Wh).
pub const DPT_ACTIVE_ENERGY_V64: Dpt = Dpt::new(29, 10);
/// DPT 232.600 — RGB colour.
pub const DPT_COLOUR_RGB: Dpt = Dpt::new(232, 600);
/// DPT 251.600 — RGBW colour.
pub const DPT_COLOUR_RGBW: Dpt = Dpt::new(251, 600);
