// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Datapoint Type (DPT) framework.
//!
//! The [`DptValue`] enum is the single type for all KNX datapoint values.
//! Each variant matches the natural type for its DPT group:
//!
//! | Variant | DPT groups |
//! |---------|------------|
//! | `Bool` | 1 |
//! | `UInt` | 2, 3, 4, 5 (raw), 6.020, 7, 12, 17, 18, 20 to 23, 25, 26, 27, 30, 31, 232, 236, 238 |
//! | `Int` | 6 (except 6.020), 8, 13 |
//! | `Float` | 5.001/5.003 (scaled), 9, 14 |
//! | `Int64` | 29 |
//! | `Text` | 16, 24, 28 |
//! | `Bytes` | every other multi-field group (10, 11, 15, 19, 200 to 224, 225 to 257, 265 to 284) |
//!
//! All main groups of KNX Standard `03_07_02` v02.02.01 are supported, and 275, 285 and 288 from
//! the ETS master data. A multi-field group is
//! carried as the exact octets of its format, in transmission order, so that no field and no
//! reserved bit is lost; see the `Bytes` variant.
//!
//! Encoding is strict: a value that does not fit the field (a number beyond the range, a
//! percentage above 100, a scene number above 63, a `Bytes` value of the wrong length) is an
//! [`DptError::OutOfRange`], never clamped, masked or padded. Decoding is lenient: octets beyond
//! the length of the DPT are ignored, reserved bits are ignored or carried on. DPT 16 is the one
//! lossy encoder, for display text: it cuts at 14 characters and replaces a character the
//! character set cannot represent by `?`.
//!
//! In DPT 9 the encoded value `0x7FFF` means invalid data; it decodes to `Float(NaN)`, and
//! `Float(NaN)` encodes to it.

mod convert;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A KNX Datapoint Type identifier (main group / sub group / index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dpt {
    /// Main group number.
    pub main: u16,
    /// Sub group number.
    pub sub: u16,
    /// Index (usually 0).
    pub index: u16,
}

/// Size of a DPT value as represented on the KNX wire.
///
/// Bit-sized values are carried in the six-bit data field of a group-value
/// APDU. Byte-sized values follow the APCI bytes even when their significant
/// value would fit in that field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DptWireSize {
    /// A value shorter than one octet.
    Bits(u8),
    /// A fixed number of complete octets.
    Bytes(u8),
    /// A variable number of complete octets.
    VariableBytes,
    /// The DPT main group is not known to this crate.
    Unknown,
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

    /// Size of this DPT's value on the KNX wire.
    ///
    /// DPT 17 is intentionally byte-sized despite having only six significant
    /// value bits. DPT 31 is bit-sized metadata but is not valid for runtime
    /// group communication according to the KNX specification.
    #[must_use]
    pub const fn wire_size(self) -> DptWireSize {
        match self.main {
            1 => DptWireSize::Bits(1),
            2 | 23 => DptWireSize::Bits(2),
            3 => DptWireSize::Bits(4),
            31 => DptWireSize::Bits(3),
            4 | 5 | 6 | 17 | 18 | 20 | 21 | 25 | 26 | 236 | 238 => DptWireSize::Bytes(1),
            7 | 8 | 9 | 22 | 200 | 201 | 202 | 204 | 207 | 211 | 217 | 234 | 237 | 239 | 244
            | 246 => DptWireSize::Bytes(2),
            10 | 11 | 30 | 203 | 205 | 206 | 209 | 223 | 225 | 232 | 240 | 250 | 254 => {
                DptWireSize::Bytes(3)
            }
            12 | 13 | 14 | 15 | 27 | 210 | 214 | 220 | 231 | 241 | 253 | 276 => {
                DptWireSize::Bytes(4)
            }
            215 | 216 | 218 | 248 | 252 => DptWireSize::Bytes(5),
            212 | 219 | 221 | 222 | 224 | 229 | 235 | 242 | 245 | 249 | 251 | 274 => {
                DptWireSize::Bytes(6)
            }
            271 | 272 => DptWireSize::Bytes(7),
            19 | 29 | 213 | 230 | 243 | 255 | 273 | 275 | 288 => DptWireSize::Bytes(8),
            265 | 268 => DptWireSize::Bytes(9),
            247 | 257 | 266 => DptWireSize::Bytes(12),
            16 | 269 => DptWireSize::Bytes(14),
            256 | 285 => DptWireSize::Bytes(16),
            270 => DptWireSize::Bytes(20),
            277 => DptWireSize::Bytes(36),
            278 => DptWireSize::Bytes(42),
            281 => DptWireSize::Bytes(44),
            279 => DptWireSize::Bytes(48),
            282 => DptWireSize::Bytes(50),
            283 => DptWireSize::Bytes(56),
            280 => DptWireSize::Bytes(78),
            284 => DptWireSize::Bytes(86),
            24 | 28 | 267 => DptWireSize::VariableBytes,
            _ => DptWireSize::Unknown,
        }
    }

    /// Wire data length in bytes for this DPT's main group.
    ///
    /// Falls back to `1` for variable-length and unrecognised main groups; use
    /// [`Dpt::wire_len`] to distinguish a genuine 1-byte DPT from that fallback.
    #[must_use]
    pub const fn data_length(self) -> u8 {
        match self.wire_size() {
            DptWireSize::Bytes(bytes) => bytes,
            DptWireSize::Bits(_) | DptWireSize::VariableBytes | DptWireSize::Unknown => 1,
        }
    }

    /// Whether this DPT's main group has a variable wire length.
    ///
    /// DPT 28 (Unicode string) is null-terminated and has no fixed size.
    #[must_use]
    pub const fn is_variable_length(self) -> bool {
        matches!(self.wire_size(), DptWireSize::VariableBytes)
    }

    /// The fixed wire length in bytes, or `None` for variable-length DPTs.
    ///
    /// Unlike [`Dpt::data_length`], this is the single source of truth for
    /// "does this DPT have a fixed size", so callers (e.g. group-object buffer
    /// sizing) can treat variable-length DPTs explicitly instead of silently
    /// allocating a 1-byte buffer.
    #[must_use]
    pub const fn wire_len(self) -> Option<u8> {
        match self.wire_size() {
            DptWireSize::Bits(_) | DptWireSize::Unknown => Some(1),
            DptWireSize::Bytes(bytes) => Some(bytes),
            DptWireSize::VariableBytes => None,
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DptValue {
    /// Boolean (DPT 1).
    Bool(bool),
    /// Unsigned integer, or a bit set or enumeration of up to 32 bits (see the module table).
    UInt(u32),
    /// Signed integer (DPT 6, 8, 13).
    Int(i32),
    /// Floating point (DPT 9, 14).
    Float(f64),
    /// Signed 64-bit integer (DPT 29).
    Int64(i64),
    /// String (DPT 16, 24, 28).
    Text(String),
    /// The octets of a multi-field group in transmission order (DPT 10, 11, 15, 19 and the
    /// structured groups from 200 on).
    Bytes(Vec<u8>),
}

impl fmt::Display for DptValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::UInt(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Int64(v) => write!(f, "{v}"),
            Self::Text(s) => f.write_str(s),
            Self::Bytes(b) => {
                for (i, byte) in b.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{byte:02X}")?;
                }
                Ok(())
            }
        }
    }
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
    ///
    /// Note: `Int64` → `f64` may lose precision for values > 2^53.
    #[expect(
        clippy::cast_precision_loss,
        reason = "i64→f64 precision loss is inherent and documented"
    )]
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Self::UInt(v) => Some(*v as f64), // lossless: f64 covers all u32
            Self::Int(v) => Some(*v as f64),  // lossless: f64 covers all i32
            Self::Int64(v) => Some(*v as f64), // lossy for |v| > 2^53
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
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<u8> for DptValue {
    fn from(v: u8) -> Self {
        Self::UInt(u32::from(v))
    }
}

impl From<u16> for DptValue {
    fn from(v: u16) -> Self {
        Self::UInt(u32::from(v))
    }
}

impl From<u32> for DptValue {
    fn from(v: u32) -> Self {
        Self::UInt(v)
    }
}

impl From<i8> for DptValue {
    fn from(v: i8) -> Self {
        Self::Int(i32::from(v))
    }
}

impl From<i16> for DptValue {
    fn from(v: i16) -> Self {
        Self::Int(i32::from(v))
    }
}

impl From<i32> for DptValue {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl From<i64> for DptValue {
    fn from(v: i64) -> Self {
        Self::Int64(v)
    }
}

impl From<f32> for DptValue {
    fn from(v: f32) -> Self {
        Self::Float(f64::from(v))
    }
}

impl From<f64> for DptValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
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

impl From<Vec<u8>> for DptValue {
    fn from(b: Vec<u8>) -> Self {
        Self::Bytes(b)
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
    OutOfRange {
        /// Human-readable context (e.g. "expected 0..=255, got 300").
        context: &'static str,
    },
    /// Wrong value type for the DPT (e.g. Bool for a float DPT).
    TypeMismatch,
    /// No DPT configured on the group object.
    NoDpt,
}

impl DptError {
    /// Create an `OutOfRange` error with context.
    pub const fn out_of_range(context: &'static str) -> Self {
        Self::OutOfRange { context }
    }
}

impl fmt::Display for DptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooShort => f.write_str("payload too short for DPT"),
            Self::UnsupportedDpt(dpt) => write!(f, "unsupported DPT: {dpt}"),
            Self::OutOfRange { context } => write!(f, "value out of range: {context}"),
            Self::TypeMismatch => f.write_str("wrong value type for DPT"),
            Self::NoDpt => f.write_str("no DPT configured on group object"),
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn encode_length_matches_wire_len() {
        // Guards against drift between Dpt::wire_len and the codec dispatch:
        // for every fixed-length DPT, encode() must produce wire_len() bytes.
        let cases: &[(Dpt, DptValue)] = &[
            (DPT_SWITCH, DptValue::Bool(true)),
            (DPT_SCALING, DptValue::Float(50.0)),
            (DPT_VALUE_1_UCOUNT, DptValue::UInt(42)),
            (DPT_VALUE_2_UCOUNT, DptValue::UInt(1000)),
            (DPT_VALUE_2_COUNT, DptValue::Int(-1000)),
            (DPT_VALUE_TEMP, DptValue::Float(21.5)),
            (DPT_VALUE_4_UCOUNT, DptValue::UInt(100_000)),
            (DPT_VALUE_4_COUNT, DptValue::Int(-5)),
            (DPT_VALUE_POWER, DptValue::Float(1234.0)),
            (DPT_SCENE_NUMBER, DptValue::UInt(5)),
            (DPT_STRING_ASCII, DptValue::Text("hi".into())),
        ];
        for (dpt, value) in cases {
            let expected = dpt.wire_len().expect("fixed-length DPT");
            let encoded = encode(*dpt, value).expect("encode");
            assert_eq!(
                encoded.len(),
                usize::from(expected),
                "wire_len/codec length mismatch for DPT {dpt}"
            );
        }
    }

    /// Every main group of KNX Standard `03_07_02` v02.02.01 and of the ETS 6.3 master data.
    const SPEC_MAIN_GROUPS: &[u16] = &[
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 200, 201, 202, 203, 204, 205, 206, 207, 209, 210, 211, 212, 213,
        214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 229, 230, 231, 232, 234, 235,
        236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253,
        254, 255, 256, 257, 265, 266, 267, 268, 269, 270, 271, 272, 273, 274, 276, 277, 278, 279,
        280, 281, 282, 283, 284, // Not in v02.02.01, but in the ETS 6.3 master data.
        275, 285, 288,
    ];

    /// A value of the shape the codec of `dpt` uses, with every field zero (valid for all).
    fn zero_value(dpt: Dpt) -> DptValue {
        let n = usize::from(dpt.data_length());
        match dpt.main {
            // 6.020 needs a one-hot mode, 16/24/28 are text.
            6 if dpt.sub == 20 => DptValue::UInt(1),
            16 | 24 | 28 => DptValue::Text(String::new()),
            // A date and time header and an empty string: just the terminating NUL.
            267 => DptValue::Bytes(alloc::vec![0; 9]),
            1 => DptValue::Bool(false),
            9 | 14 => DptValue::Float(0.0),
            29 => DptValue::Int64(0),
            6 | 8 | 13 => DptValue::Int(0),
            2..=5 | 7 | 12 | 17 | 18 | 20..=23 | 25..=27 | 30 | 31 | 232 | 236 | 238 => {
                DptValue::UInt(0)
            }
            10 => DptValue::UInt(0),
            _ => DptValue::Bytes(alloc::vec![0; n]),
        }
    }

    #[test]
    fn every_main_group_of_the_standard_has_a_size_and_a_codec() {
        for &main in SPEC_MAIN_GROUPS {
            let dpt = Dpt::new(main, 1);
            assert_ne!(
                dpt.wire_size(),
                DptWireSize::Unknown,
                "main group {main} has no wire size"
            );
            let value = zero_value(dpt);
            let bytes = encode(dpt, &value)
                .unwrap_or_else(|e| panic!("main group {main}: encode failed: {e}"));
            if let Some(len) = dpt.wire_len() {
                // Bit-sized groups travel in one octet of the group value telegram.
                let expected = match dpt.wire_size() {
                    DptWireSize::Bytes(n) => usize::from(n),
                    _ => 1,
                };
                assert_eq!(bytes.len(), expected, "main group {main}, wire_len {len}");
            }
            let decoded = decode(dpt, &bytes)
                .unwrap_or_else(|e| panic!("main group {main}: decode failed: {e}"));
            let again = encode(dpt, &decoded)
                .unwrap_or_else(|e| panic!("main group {main}: re-encode failed: {e}"));
            assert_eq!(again, bytes, "main group {main} does not round-trip");
        }
    }

    #[test]
    fn fixed_length_octet_groups_reject_a_wrong_length_on_encode_and_a_short_payload_on_decode() {
        for &main in SPEC_MAIN_GROUPS {
            let dpt = Dpt::new(main, 1);
            let DptWireSize::Bytes(n) = dpt.wire_size() else {
                continue;
            };
            // Multi-field groups only: they carry the octets as `Bytes`.
            if !matches!(zero_value(dpt), DptValue::Bytes(_)) {
                continue;
            }
            let n = usize::from(n);
            assert!(
                encode(dpt, &DptValue::Bytes(alloc::vec![0; n + 1])).is_err(),
                "main group {main}: a longer value must not be cut"
            );
            assert!(
                encode(dpt, &DptValue::Bytes(alloc::vec![0; n - 1])).is_err(),
                "main group {main}: a shorter value must not be padded"
            );
            assert_eq!(
                decode(dpt, &alloc::vec![0; n - 1]),
                Err(DptError::PayloadTooShort),
                "main group {main}"
            );
        }
    }

    #[test]
    fn scene_info_keeps_the_inactive_flag() {
        // 26.001 is `r1B1U6`: bit 6 says the scene is inactive.
        let dpt = Dpt::new(26, 1);
        assert_eq!(decode(dpt, &[0x45]).unwrap(), DptValue::UInt(0x45));
        assert_eq!(encode(dpt, &DptValue::UInt(0x45)).unwrap(), [0x45]);
        // Bit 7 is reserved: ignored on receipt, an error to send.
        assert_eq!(decode(dpt, &[0xC5]).unwrap(), DptValue::UInt(0x45));
        assert!(encode(dpt, &DptValue::UInt(0x80)).is_err());
    }

    #[test]
    fn scene_config_keeps_both_flag_bits() {
        // 238.001 is `B2U6`: storage and activation flags above the scene number.
        let dpt = Dpt::new(238, 1);
        assert_eq!(decode(dpt, &[0xC5]).unwrap(), DptValue::UInt(0xC5));
        assert_eq!(encode(dpt, &DptValue::UInt(0xC5)).unwrap(), [0xC5]);
        assert!(encode(dpt, &DptValue::UInt(256)).is_err());
    }

    #[test]
    fn b32_set_is_unsigned() {
        // 27.001: the mask bit m15 is bit 31, not a sign.
        let dpt = Dpt::new(27, 1);
        assert_eq!(
            decode(dpt, &[0x80, 0, 0, 1]).unwrap(),
            DptValue::UInt(0x8000_0001)
        );
        assert_eq!(
            encode(dpt, &DptValue::UInt(0x8000_0001)).unwrap(),
            [0x80, 0, 0, 1]
        );
    }

    #[test]
    fn status_with_mode_is_a_bit_field() {
        let dpt = Dpt::new(6, 20);
        // Flag a (bit 7) set, mode 1: not a negative number.
        assert_eq!(decode(dpt, &[0x82]).unwrap(), DptValue::UInt(0x82));
        assert_eq!(encode(dpt, &DptValue::UInt(0x82)).unwrap(), [0x82]);
        // The mode is exactly one of 001, 010, 100.
        assert!(encode(dpt, &DptValue::UInt(0x80)).is_err());
        assert!(encode(dpt, &DptValue::UInt(0x83)).is_err());
        // The other 6.xxx stay signed.
        assert_eq!(
            decode(Dpt::new(6, 1), &[0x80]).unwrap(),
            DptValue::Int(-128)
        );
    }

    #[test]
    fn date_and_time_carries_the_source_reliability_bit() {
        // 19.001 as xknx and calimero read it: the last octet is CLQ (bit 7), SRC (bit 6) and six
        // reserved bits. Whatever the octets hold survives a round trip.
        let dpt = DPT_DATETIME;
        for last in [0x00u8, 0x40, 0x80, 0xC0] {
            let octets = alloc::vec![126, 11, 1, 0, 0, 0, 0x04, last];
            let value = DptValue::Bytes(octets.clone());
            assert_eq!(encode(dpt, &value).unwrap(), octets);
            assert_eq!(decode(dpt, &octets).unwrap(), value);
        }
    }

    #[test]
    fn status_with_mode_reads_a_set_flag_as_one() {
        // 6.020 as the ETS master data and calimero read it: flags A to E in bits 7 to 3 are 1 when
        // set. All five set with mode 2 is 0xFC, none set with mode 0 is 0x01.
        let dpt = Dpt::new(6, 20);
        assert_eq!(decode(dpt, &[0xFC]).unwrap(), DptValue::UInt(0xFC));
        assert_eq!(encode(dpt, &DptValue::UInt(0xFC)).unwrap(), [0xFC]);
        assert_eq!(decode(dpt, &[0x01]).unwrap(), DptValue::UInt(0x01));
        assert_eq!(encode(dpt, &DptValue::UInt(0x01)).unwrap(), [0x01]);
    }

    #[test]
    fn channel_activation_has_channel_one_in_the_most_significant_bit() {
        // 21.1010, 22.1010 and 30.1010 as ETS lists the fields: channel 1 first, so bit 7, 15 or
        // 23 of the value, not bit 0 as the text of the standard numbers it.
        let cases: &[(Dpt, usize, u32)] = &[
            (Dpt::new(21, 1010), 1, 0x80),
            (Dpt::new(22, 1010), 2, 0x8000),
            (Dpt::new(30, 1010), 3, 0x80_0000),
        ];
        for &(dpt, octets, channel_one) in cases {
            let wire = channel_one.to_be_bytes();
            let wire = &wire[4 - octets..];
            assert_eq!(decode(dpt, wire).unwrap(), DptValue::UInt(channel_one));
            assert_eq!(encode(dpt, &DptValue::UInt(channel_one)).unwrap(), wire);
        }
    }

    #[test]
    fn float16_marks_invalid_data_with_7fff() {
        let dpt = DPT_VALUE_TEMP;
        let decoded = decode(dpt, &[0x7F, 0xFF]).unwrap();
        assert!(decoded.as_f64().unwrap().is_nan());
        // NaN is the invalid marker in both directions.
        assert_eq!(
            encode(dpt, &DptValue::Float(f64::NAN)).unwrap(),
            [0x7F, 0xFF]
        );
        // The largest value is 670433.28 (mantissa 2046); 7FFF is never produced for a number.
        assert_eq!(
            encode(dpt, &DptValue::Float(670_433.28)).unwrap(),
            [0x7F, 0xFE]
        );
        assert!(encode(dpt, &DptValue::Float(670_760.96)).is_err());
        assert!(encode(dpt, &DptValue::Float(1.0e9)).is_err());
        assert!(encode(dpt, &DptValue::Float(f64::INFINITY)).is_err());
        assert_eq!(
            encode(dpt, &DptValue::Float(-671_088.64)).unwrap(),
            [0xF8, 0x00]
        );
    }

    #[test]
    fn float16_rounds_once_to_the_nearest_step() {
        // 20.49 is 2049 hundredths: at exponent 1 the step is 0.02, so 20.50 is the nearest value
        // (not 20.48, which truncating the mantissa would give).
        let bytes = encode(DPT_VALUE_TEMP, &DptValue::Float(20.49)).unwrap();
        let back = decode(DPT_VALUE_TEMP, &bytes).unwrap().as_f64().unwrap();
        assert!((back - 20.50).abs() < 1e-9, "got {back}");
    }

    #[test]
    fn encoders_reject_what_does_not_fit_instead_of_clamping() {
        let bad: &[(Dpt, DptValue)] = &[
            (Dpt::new(2, 1), DptValue::UInt(4)),
            (Dpt::new(3, 7), DptValue::UInt(16)),
            (Dpt::new(4, 1), DptValue::UInt(128)),
            (Dpt::new(4, 2), DptValue::UInt(256)),
            (DPT_SCALING, DptValue::Float(100.5)),
            (DPT_SCALING, DptValue::Float(-1.0)),
            (DPT_SCALING, DptValue::Float(f64::NAN)),
            (DPT_ANGLE, DptValue::Float(361.0)),
            (DPT_VALUE_1_UCOUNT, DptValue::UInt(256)),
            (Dpt::new(5, 6), DptValue::UInt(255)),
            (Dpt::new(6, 1), DptValue::Int(128)),
            (Dpt::new(6, 1), DptValue::Int(-129)),
            (DPT_VALUE_2_UCOUNT, DptValue::UInt(65536)),
            (DPT_VALUE_2_COUNT, DptValue::Int(32768)),
            (DPT_VALUE_2_COUNT, DptValue::Int(-32769)),
            (DPT_TIMEOFDAY, DptValue::UInt(86400)),
            (DPT_VALUE_4_UCOUNT, DptValue::Float(f64::NAN)),
            (DPT_VALUE_POWER, DptValue::Float(1.0e300)),
            (DPT_SCENE_NUMBER, DptValue::UInt(64)),
            (DPT_SCENE_CONTROL, DptValue::UInt(0x40)),
            (DPT_SCENE_CONTROL, DptValue::UInt(256)),
            (DPT_COLOUR_RGB, DptValue::UInt(0x0100_0000)),
            (DPT_ACCESS_DATA, DptValue::UInt(1_000_000)),
            (Dpt::new(23, 1), DptValue::UInt(4)),
            (Dpt::new(31, 1), DptValue::UInt(8)),
        ];
        for (dpt, value) in bad {
            assert!(
                encode(*dpt, value).is_err(),
                "DPT {dpt} must reject {value:?}"
            );
        }
        // The limits themselves are fine.
        assert_eq!(encode(DPT_SCALING, &DptValue::Float(100.0)).unwrap(), [255]);
        assert_eq!(encode(Dpt::new(4, 1), &DptValue::UInt(127)).unwrap(), [127]);
        assert_eq!(encode(Dpt::new(5, 6), &DptValue::UInt(254)).unwrap(), [254]);
    }

    #[test]
    fn small_enumerations_and_bit_sets() {
        // 1-octet groups with all eight bits used.
        for main in [20u16, 21, 25, 236, 238] {
            let dpt = Dpt::new(main, 1);
            assert_eq!(encode(dpt, &DptValue::UInt(0xA5)).unwrap(), [0xA5]);
            assert_eq!(decode(dpt, &[0xA5]).unwrap(), DptValue::UInt(0xA5));
            assert!(encode(dpt, &DptValue::UInt(256)).is_err());
        }
        // DPT 22 (B16) and DPT 30 (B24) are big-endian bit sets.
        assert_eq!(
            encode(Dpt::new(22, 100), &DptValue::UInt(0x1234)).unwrap(),
            [0x12, 0x34]
        );
        assert_eq!(
            decode(Dpt::new(30, 1), &[0x12, 0x34, 0x56]).unwrap(),
            DptValue::UInt(0x12_3456)
        );
        assert!(encode(Dpt::new(30, 1), &DptValue::UInt(0x0100_0000)).is_err());
        // DPT 23 (N2) and DPT 31 (N3) sit in the lower bits; the upper bits are ignored.
        assert_eq!(decode(Dpt::new(23, 1), &[0xFE]).unwrap(), DptValue::UInt(2));
        assert_eq!(decode(Dpt::new(31, 1), &[0xFD]).unwrap(), DptValue::UInt(5));
    }

    #[test]
    fn strings() {
        // 16.000 is ASCII: a byte above 0x7F is invalid, a wider character becomes '?'.
        let mut raw = [0u8; 14];
        raw[0] = 0xE4;
        assert!(decode(DPT_STRING_ASCII, &raw).is_err());
        assert_eq!(
            decode(DPT_STRING_8859_1, &raw).unwrap(),
            DptValue::Text("ä".into())
        );
        let ascii = encode(DPT_STRING_ASCII, &DptValue::Text("ä".into())).unwrap();
        assert_eq!(ascii[0], b'?');
        // 16.xxx cuts to 14 characters.
        let long = encode(
            DPT_STRING_8859_1,
            &DptValue::Text("0123456789ABCDEFG".into()),
        )
        .unwrap();
        assert_eq!(&long[..], b"0123456789ABCD");
        // 24.001 is ISO 8859-1 and NUL-terminated, 28.001 UTF-8.
        let dpt24 = Dpt::new(24, 1);
        assert_eq!(
            encode(dpt24, &DptValue::Text("Grüße".into())).unwrap(),
            [b'G', b'r', 0xFC, 0xDF, b'e', 0]
        );
        assert_eq!(
            decode(dpt24, &[b'G', b'r', 0xFC, 0xDF, b'e', 0]).unwrap(),
            DptValue::Text("Grüße".into())
        );
        assert!(encode(dpt24, &DptValue::Text("€".into())).is_err());
        assert!(encode(dpt24, &DptValue::Text("a\0b".into())).is_err());
        let dpt28 = Dpt::new(28, 1);
        assert_eq!(
            encode(dpt28, &DptValue::Text("€".into())).unwrap(),
            [0xE2, 0x82, 0xAC, 0]
        );
        assert!(encode(dpt28, &DptValue::Text("a\0b".into())).is_err());
        assert_eq!(dpt24.wire_size(), DptWireSize::VariableBytes);
    }

    #[test]
    fn date_and_time_with_a_string_is_a_header_and_a_terminated_text() {
        let dpt = Dpt::new(267, 1);
        let mut v = alloc::vec![0u8; 8];
        v.extend_from_slice(b"hi\0");
        assert_eq!(encode(dpt, &DptValue::Bytes(v.clone())).unwrap(), v);
        assert_eq!(decode(dpt, &v).unwrap(), DptValue::Bytes(v));
        // The header alone, or a text without the NUL, is not a value of this DPT.
        assert!(encode(dpt, &DptValue::Bytes(alloc::vec![0; 8])).is_err());
        assert!(encode(dpt, &DptValue::Bytes(alloc::vec![1; 10])).is_err());
        assert_eq!(dpt.wire_size(), DptWireSize::VariableBytes);
    }

    #[test]
    fn variable_length_dpt_has_no_wire_len() {
        let dpt28 = Dpt::new(28, 1);
        assert!(dpt28.is_variable_length());
        assert_eq!(dpt28.wire_len(), None);
    }

    #[test]
    fn wire_size_distinguishes_inline_bits_from_complete_octets() {
        assert_eq!(Dpt::new(1, 1).wire_size(), DptWireSize::Bits(1));
        assert_eq!(Dpt::new(2, 1).wire_size(), DptWireSize::Bits(2));
        assert_eq!(Dpt::new(3, 7).wire_size(), DptWireSize::Bits(4));
        assert_eq!(Dpt::new(23, 1).wire_size(), DptWireSize::Bits(2));
        assert_eq!(Dpt::new(31, 101).wire_size(), DptWireSize::Bits(3));
        assert_eq!(DPT_SCALING.wire_size(), DptWireSize::Bytes(1));
        assert_eq!(DPT_SCENE_NUMBER.wire_size(), DptWireSize::Bytes(1));
        assert_eq!(DPT_VALUE_TEMP.wire_size(), DptWireSize::Bytes(2));
        assert_eq!(Dpt::new(28, 1).wire_size(), DptWireSize::VariableBytes);
        assert_eq!(Dpt::new(999, 1).wire_size(), DptWireSize::Unknown);
    }
}
