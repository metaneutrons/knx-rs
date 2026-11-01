// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Numeric DPT encode/decode — all main groups that map to typed [`DptValue`] variants.

use alloc::vec::Vec;

use super::super::{Dpt, DptError, DptValue};

/// Round `f64` to nearest integer (`no_std` compatible).
fn round(v: f64) -> f64 {
    libm::round(v)
}

/// Convert an `f64` to `u8`, rounding to nearest.
///
/// Returns `Err(OutOfRange)` for NaN, infinity and values outside `[0, 255]`.
fn f64_to_u8(v: f64) -> Result<u8, DptError> {
    let r = round(v);
    if !r.is_finite() || !(0.0..=255.0).contains(&r) {
        return Err(DptError::out_of_range("expected 0..=255"));
    }
    // SAFETY: range validated above.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "range validated above"
    )]
    Ok(r as u8)
}

/// Convert an `f64` to `u32`, rounding to nearest.
///
/// Returns `Err(OutOfRange)` for NaN, infinity, negative values and values above `u32::MAX`.
fn f64_to_u32(v: f64) -> Result<u32, DptError> {
    let r = round(v);
    if !r.is_finite() || r < 0.0 || r > f64::from(u32::MAX) {
        return Err(DptError::out_of_range("expected 0..=4294967295"));
    }
    // SAFETY: range validated above.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "range validated above"
    )]
    Ok(r as u32)
}

/// Convert an `f64` to `i32`, rounding to nearest.
///
/// Returns `Err(OutOfRange)` for NaN, infinity and values outside the `i32` range.
fn f64_to_i32(v: f64) -> Result<i32, DptError> {
    let r = round(v);
    if !r.is_finite() || r < f64::from(i32::MIN) || r > f64::from(i32::MAX) {
        return Err(DptError::out_of_range("expected -2147483648..=2147483647"));
    }
    // SAFETY: range validated above.
    #[expect(clippy::cast_possible_truncation, reason = "range validated above")]
    Ok(r as i32)
}

/// Convert an `f64` to `i64`, rounding to nearest.
///
/// Precision loss is inherent: `f64` cannot represent all `i64` values. Returns
/// `Err(OutOfRange)` for NaN, infinity and values outside the `i64` range.
fn f64_to_i64(v: f64) -> Result<i64, DptError> {
    let r = round(v);
    // 2^63 is exactly representable; i64::MAX itself is not.
    if !r.is_finite() || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&r) {
        return Err(DptError::out_of_range("expected a value in the i64 range"));
    }
    // SAFETY: range validated above.
    #[expect(clippy::cast_possible_truncation, reason = "range validated above")]
    Ok(r as i64)
}

const fn check_len(payload: &[u8], min: usize) -> Result<(), DptError> {
    if payload.len() < min {
        Err(DptError::PayloadTooShort)
    } else {
        Ok(())
    }
}

/// Truncate a `u32` to `u8`. Value must be ≤ 255 (masked or clamped by caller).
const fn low_u8(v: u32) -> u8 {
    (v & 0xFF) as u8
}

/// Truncate a `u32` to `u16`. Value must be ≤ 65535 (masked or clamped by caller).
const fn low_u16(v: u32) -> u16 {
    (v & 0xFFFF) as u16
}

/// Truncate an `i32` to `i8`. Value must be in `[-128, 127]` (clamped by caller).
#[expect(
    clippy::cast_possible_truncation,
    reason = "caller guarantees value fits in i8"
)]
const fn low_i8(v: i32) -> i8 {
    v as i8
}

/// Truncate an `i32` to `i16`. Value must be in `[-32768, 32767]` (clamped by caller).
#[expect(
    clippy::cast_possible_truncation,
    reason = "caller guarantees value fits in i16"
)]
const fn low_i16(v: i32) -> i16 {
    v as i16
}

/// Extract a `bool` from a `DptValue`, coercing numeric types.
fn val_bool(value: &DptValue) -> Result<bool, DptError> {
    match value {
        DptValue::Bool(v) => Ok(*v),
        DptValue::UInt(v) => Ok(*v != 0),
        DptValue::Int(v) => Ok(*v != 0),
        DptValue::Float(v) => Ok(*v != 0.0),
        _ => Err(DptError::TypeMismatch),
    }
}

/// Extract a `u32` from a `DptValue`, coercing numeric types.
fn val_u32(value: &DptValue) -> Result<u32, DptError> {
    match value {
        DptValue::UInt(v) => Ok(*v),
        DptValue::Bool(v) => Ok(u32::from(*v)),
        DptValue::Int(v) => {
            u32::try_from(*v).map_err(|_| DptError::out_of_range("negative value for unsigned DPT"))
        }
        DptValue::Float(v) => f64_to_u32(*v),
        _ => Err(DptError::TypeMismatch),
    }
}

/// Extract a `u32` that must not exceed `max` (the largest value the field can hold).
///
/// Encoders use this instead of clamping or masking: a value that does not fit the field is an
/// error, not a different value on the bus.
fn val_u32_max(value: &DptValue, max: u32) -> Result<u32, DptError> {
    let v = val_u32(value)?;
    if v > max {
        return Err(DptError::out_of_range("value exceeds the range of the DPT"));
    }
    Ok(v)
}

/// Extract an `i32` that must lie within `[min, max]`.
fn val_i32_range(value: &DptValue, min: i32, max: i32) -> Result<i32, DptError> {
    let v = val_i32(value)?;
    if !(min..=max).contains(&v) {
        return Err(DptError::out_of_range("value exceeds the range of the DPT"));
    }
    Ok(v)
}

/// Extract an `i32` from a `DptValue`, coercing numeric types.
fn val_i32(value: &DptValue) -> Result<i32, DptError> {
    match value {
        DptValue::Int(v) => Ok(*v),
        DptValue::UInt(v) => {
            i32::try_from(*v).map_err(|_| DptError::out_of_range("unsigned value exceeds i32::MAX"))
        }
        DptValue::Bool(v) => Ok(i32::from(*v)),
        DptValue::Float(v) => f64_to_i32(*v),
        _ => Err(DptError::TypeMismatch),
    }
}

/// Extract an `f64` from a `DptValue`, coercing numeric types.
fn val_f64(value: &DptValue) -> Result<f64, DptError> {
    value.as_f64().ok_or(DptError::TypeMismatch)
}

/// Extract an `i64` from a `DptValue`, coercing numeric types.
fn val_i64(value: &DptValue) -> Result<i64, DptError> {
    match value {
        DptValue::Int64(v) => Ok(*v),
        DptValue::Int(v) => Ok(i64::from(*v)),
        DptValue::UInt(v) => Ok(i64::from(*v)),
        DptValue::Bool(v) => Ok(i64::from(*v)),
        DptValue::Float(v) => f64_to_i64(*v),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── Dispatch ──────────────────────────────────────────────────

pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        1 => decode_dpt1(payload),
        2 => decode_masked(payload, DPT2_MASK),
        3 => decode_masked(payload, DPT3_MASK),
        4 => decode_dpt4(payload),
        5 => decode_dpt5(dpt, payload),
        6 => decode_dpt6(dpt, payload),
        7 => decode_dpt7(payload),
        8 => decode_dpt8(payload),
        9 => decode_dpt9(payload),
        10 => decode_dpt10(payload),
        12 => decode_dpt12(payload),
        13 => decode_dpt13(payload),
        14 => decode_dpt14(payload),
        15 => decode_dpt15(payload),
        17 => decode_masked(payload, DPT17_MASK),
        18 => decode_dpt18(payload),
        26 => decode_masked(payload, DPT26_MASK),
        27 => decode_dpt27(payload),
        29 => decode_dpt29(payload),
        232 => decode_dpt232(payload),
        // One octet, all eight bits used: enumerations and bit sets.
        20 | 21 | 25 | 236 | 238 => decode_masked(payload, 0xFF),
        22 => decode_uint_be(payload, 2),
        30 => decode_uint_be(payload, 3),
        23 => decode_masked(payload, DPT23_MASK),
        31 => decode_masked(payload, DPT31_MASK),
        _ => Err(DptError::UnsupportedDpt(dpt)),
    }
}

pub fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    match dpt.main {
        1 => encode_dpt1(value),
        2 => encode_masked(value, DPT2_MASK),
        3 => encode_masked(value, DPT3_MASK),
        4 => encode_dpt4(dpt, value),
        5 => encode_dpt5(dpt, value),
        6 => encode_dpt6(dpt, value),
        7 => encode_dpt7(value),
        8 => encode_dpt8(value),
        9 => encode_dpt9(value),
        10 => encode_dpt10(value),
        12 => encode_dpt12(value),
        13 => encode_dpt13(value),
        14 => encode_dpt14(value),
        15 => encode_dpt15(value),
        17 => encode_masked(value, DPT17_MASK),
        18 => encode_dpt18(value),
        26 => encode_masked(value, DPT26_MASK),
        27 => encode_dpt27(value),
        29 => encode_dpt29(value),
        232 => encode_dpt232(value),
        20 | 21 | 25 | 236 | 238 => encode_masked(value, 0xFF),
        22 => encode_uint_be(value, 2),
        30 => encode_uint_be(value, 3),
        23 => encode_masked(value, DPT23_MASK),
        31 => encode_masked(value, DPT31_MASK),
        _ => Err(DptError::UnsupportedDpt(dpt)),
    }
}

// ── DPT 1: Boolean (1 bit) ───────────────────────────────────

fn decode_dpt1(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::Bool(payload[0] & 0x01 != 0))
}

fn encode_dpt1(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(alloc::vec![u8::from(val_bool(value)?)])
}

// ── 1-byte unsigned DPTs with a mask (2, 3, 17, 20-23, 25, 26, 31, 236, 238) ──

/// DPT 2 (1-bit controlled): 2 significant bits.
const DPT2_MASK: u8 = 0x03;
/// DPT 3 (3-bit controlled): 4 significant bits.
const DPT3_MASK: u8 = 0x0F;
/// DPT 17 (scene number): 6 significant bits (0–63).
const DPT17_MASK: u8 = 0x3F;
/// DPT 23 (N2, 2 bits): carried in the lower bits of the data field.
const DPT23_MASK: u8 = 0x03;
/// DPT 26 (scene info, `r1B1U6`): bit 7 reserved, bit 6 = inactive flag, bits 5..0 = scene.
const DPT26_MASK: u8 = 0x7F;
/// DPT 31 (N3, 3 bits): carried in the lower bits of the data field.
const DPT31_MASK: u8 = 0x07;

/// Decode a single byte masked to `mask` significant bits as a `UInt`.
///
/// Bits outside the mask are reserved; a receiver ignores them.
fn decode_masked(payload: &[u8], mask: u8) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0] & mask)))
}

/// Encode a `UInt` into a single byte. A value with bits outside `mask` is an error.
fn encode_masked(value: &DptValue, mask: u8) -> Result<Vec<u8>, DptError> {
    let v = val_u32_max(value, u32::from(mask))?;
    // Only the bits of `mask` can be set; only the contiguous low masks above are used.
    Ok(alloc::vec![low_u8(v)])
}

/// Decode `len` octets as one big-endian unsigned number (DPT 22, 30: bit sets).
fn decode_uint_be(payload: &[u8], len: usize) -> Result<DptValue, DptError> {
    check_len(payload, len)?;
    Ok(DptValue::UInt(
        payload[..len]
            .iter()
            .fold(0u32, |acc, &b| (acc << 8) | u32::from(b)),
    ))
}

/// Encode a `UInt` as `len` big-endian octets. A value that needs more is an error.
fn encode_uint_be(value: &DptValue, len: usize) -> Result<Vec<u8>, DptError> {
    let max = if len >= 4 {
        u32::MAX
    } else {
        (1u32 << (8 * len)) - 1
    };
    let v = val_u32_max(value, max)?;
    Ok(v.to_be_bytes()[4 - len..].to_vec())
}

// ── DPT 4: Character (1 byte) ────────────────────────────────

fn decode_dpt4(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0])))
}

fn encode_dpt4(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    // 4.001 is ASCII: the most significant bit is always 0. 4.002 is ISO 8859-1.
    let max = if dpt.sub == 1 { 0x7F } else { 0xFF };
    let v = val_u32_max(value, max)?;
    Ok(alloc::vec![low_u8(v)])
}

// ── DPT 5: Unsigned 8-bit (1 byte) ───────────────────────────
//
// The decoded variant depends on the sub-type: 5.001 (scaling, 0–100%) and
// 5.003 (angle, 0–360°) are physically continuous and decode to `Float`; all
// other sub-types (e.g. 5.010 raw count) decode to `UInt`. The scaled
// sub-types map u8 0..=255 onto the range, so round-trips are quantized to
// 1/255 of full scale.

fn decode_dpt5(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    let raw = f64::from(payload[0]);
    Ok(DptValue::Float(match dpt.sub {
        1 => raw * 100.0 / 255.0,
        3 => raw * 360.0 / 255.0,
        _ => return Ok(DptValue::UInt(u32::from(payload[0]))),
    }))
}

fn encode_dpt5(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    match dpt.sub {
        1 => {
            let v = val_f64(value)?;
            if !(0.0..=100.0).contains(&v) {
                return Err(DptError::out_of_range("expected 0..=100 %"));
            }
            Ok(alloc::vec![f64_to_u8(round(v * 255.0 / 100.0))?])
        }
        3 => {
            let v = val_f64(value)?;
            if !(0.0..=360.0).contains(&v) {
                return Err(DptError::out_of_range("expected 0..=360 degrees"));
            }
            Ok(alloc::vec![f64_to_u8(round(v * 255.0 / 360.0))?])
        }
        6 => {
            // 5.006 (tariff): 255 shall not be transmitted.
            let v = val_u32_max(value, 254)?;
            Ok(alloc::vec![low_u8(v)])
        }
        _ => {
            let v = val_u32_max(value, 255)?;
            Ok(alloc::vec![low_u8(v)])
        }
    }
}

// ── DPT 6: Signed 8-bit (1 byte) ─────────────────────────────

/// 6.020 (`DPT_Status_Mode3`, `B5N3`): five status flags and a one-hot mode in bits 2..0.
const DPT6_STATUS_MODE: u16 = 20;

fn decode_dpt6(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    if dpt.sub == DPT6_STATUS_MODE {
        // A bit field, not a number: bit 7 is flag a, not a sign.
        return Ok(DptValue::UInt(u32::from(payload[0])));
    }
    Ok(DptValue::Int(i32::from(i8::from_ne_bytes([payload[0]]))))
}

fn encode_dpt6(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    if dpt.sub == DPT6_STATUS_MODE {
        let v = val_u32_max(value, 0xFF)?;
        // The mode is exactly one of 001b, 010b and 100b.
        if !matches!(v & 0x07, 0b001 | 0b010 | 0b100) {
            return Err(DptError::out_of_range(
                "6.020 mode (bits 2..0) must be 001, 010 or 100",
            ));
        }
        return Ok(alloc::vec![low_u8(v)]);
    }
    let v = val_i32_range(value, -128, 127)?;
    Ok(alloc::vec![low_i8(v).to_ne_bytes()[0]])
}

// ── DPT 7: Unsigned 16-bit (2 bytes) ─────────────────────────

fn decode_dpt7(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::UInt(u32::from(u16::from_be_bytes([
        payload[0], payload[1],
    ]))))
}

fn encode_dpt7(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32_max(value, 65535)?;
    Ok(low_u16(v).to_be_bytes().to_vec())
}

// ── DPT 8: Signed 16-bit (2 bytes) ───────────────────────────

fn decode_dpt8(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::Int(i32::from(i16::from_be_bytes([
        payload[0], payload[1],
    ]))))
}

fn encode_dpt8(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_i32_range(value, -32768, 32767)?;
    Ok(low_i16(v).to_be_bytes().to_vec())
}

// ── DPT 9: 16-bit float (2 bytes, KNX F16) ───────────────────
//
// Wire: `MEEEEMMM MMMMMMMM` — sign + 11-bit mantissa (two's complement), 4-bit exponent.
// Value = 0.01 × mantissa × 2^exponent.
//
// The encoded value 0x7FFF (mantissa 2047, exponent 15) denotes invalid data for every 9.xxx.
// It decodes to `Float(NaN)`, and `Float(NaN)` encodes to it. The largest encodable value is
// therefore 670 433,28 (mantissa 2046, exponent 15), the smallest -671 088,64.

/// The wire value for "invalid data" in every DPT 9.xxx.
const DPT9_INVALID: u16 = 0x7FFF;

fn decode_dpt9(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    let raw = u16::from_be_bytes([payload[0], payload[1]]);
    if raw == DPT9_INVALID {
        return Ok(DptValue::Float(f64::NAN));
    }
    let exponent = i32::from((raw >> 11) & 0x0F);
    let mantissa = {
        let m = i32::from(raw & 0x07FF);
        if raw & 0x8000 != 0 { m - 0x0800 } else { m }
    };
    Ok(DptValue::Float(
        0.01 * f64::from(mantissa) * f64::from(1 << exponent),
    ))
}

fn encode_dpt9(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_f64(value)?;
    if v.is_nan() {
        return Ok(DPT9_INVALID.to_be_bytes().to_vec());
    }
    if !v.is_finite() {
        return Err(DptError::out_of_range("KNX F16 needs a finite value"));
    }
    let hundredths = v * 100.0;
    // The smallest exponent whose mantissa fits; the value is rounded once, at that exponent.
    for exponent in 0u16..=15 {
        let mantissa = round(hundredths / f64::from(1u32 << exponent));
        if !(-2048.0..=2047.0).contains(&mantissa) {
            continue;
        }
        #[expect(clippy::cast_possible_truncation, reason = "range validated above")]
        let m = mantissa as i32;
        if exponent == 15 && m == 2047 {
            // Would be 0x7FFF, the marker for invalid data.
            break;
        }
        let sign: u16 = if m < 0 { 0x8000 } else { 0 };
        let raw = sign | (exponent << 11) | low_u16((m & 0x07FF).unsigned_abs());
        return Ok(raw.to_be_bytes().to_vec());
    }
    Err(DptError::out_of_range(
        "KNX F16 range is -671088.64 ..= 670433.28",
    ))
}

// ── DPT 10: Time of day (3 bytes) ────────────────────────────

fn decode_dpt10(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 3)?;
    Ok(DptValue::Bytes(payload[..3].to_vec()))
}

fn encode_dpt10(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() == 3 => Ok(b.clone()),
        DptValue::UInt(secs) => {
            if *secs > 86399 {
                return Err(DptError::out_of_range("expected 0..=86399 seconds"));
            }
            let total = *secs;
            let hours = low_u8(total / 3600);
            let minutes = low_u8((total % 3600) / 60);
            let seconds = low_u8(total % 60);
            Ok(alloc::vec![hours & 0x1F, minutes & 0x3F, seconds & 0x3F])
        }
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 12: Unsigned 32-bit (4 bytes) ────────────────────────

fn decode_dpt12(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    Ok(DptValue::UInt(u32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt12(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(val_u32(value)?.to_be_bytes().to_vec())
}

// ── DPT 13: Signed 32-bit (4 bytes) ──────────────────────────

fn decode_dpt13(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    Ok(DptValue::Int(i32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt13(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(val_i32(value)?.to_be_bytes().to_vec())
}

// ── DPT 14: IEEE 754 32-bit float (4 bytes) ──────────────────

fn decode_dpt14(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    Ok(DptValue::Float(f64::from(f32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))))
}

fn encode_dpt14(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_f64(value)?;
    // f64→f32 narrowing is intentional for the IEEE 754 wire format. NaN and ±infinity are
    // valid single-precision values and pass through; a finite value that is too large for
    // single precision is an error rather than a silent ±infinity.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "f64→f32 narrowing is intentional for IEEE 754 wire format"
    )]
    let f = v as f32;
    if v.is_finite() && !f.is_finite() {
        return Err(DptError::out_of_range("value exceeds the f32 range"));
    }
    Ok(f.to_be_bytes().to_vec())
}

// ── DPT 15: Access data (4 bytes) ────────────────────────────
//
// Octets 1..3 on the wire hold six BCD digits (D6..D1); the fourth octet carries the flags
// E (detection error), P (permission), D (read direction), C (encryption) in bits 7..4 and the
// index in bits 3..0. Decoding keeps all four octets (`Bytes`), so the permission flag survives.
// Encoding accepts the same four octets, or a `UInt` of up to six decimal digits for a value
// with all flags and the index 0.

fn bcd_digits_valid(octets: &[u8]) -> bool {
    octets.iter().all(|b| (b >> 4) <= 9 && (b & 0x0F) <= 9)
}

fn decode_dpt15(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    if !bcd_digits_valid(&payload[..3]) {
        return Err(DptError::out_of_range("DPT 15 digit above 9"));
    }
    Ok(DptValue::Bytes(payload[..4].to_vec()))
}

fn encode_dpt15(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() == 4 => {
            if !bcd_digits_valid(&b[..3]) {
                return Err(DptError::out_of_range("DPT 15 digit above 9"));
            }
            Ok(b.clone())
        }
        DptValue::Bytes(_) => Err(DptError::out_of_range("DPT 15 has four octets")),
        _ => {
            let mut v = val_u32_max(value, 999_999)?;
            let mut buf = [0u8; 4];
            for i in (0..6).rev() {
                let digit = low_u8(v % 10);
                v /= 10;
                if i % 2 == 0 {
                    buf[i / 2] |= digit << 4;
                } else {
                    buf[i / 2] |= digit;
                }
            }
            Ok(buf.to_vec())
        }
    }
}

// ── DPT 18: Scene control (1 byte) ───────────────────────────
//
// `B1r1U6`: bit 7 = control (0 = activate, 1 = learn), bit 6 reserved (0), bits 5..0 = scene.

fn decode_dpt18(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0])))
}

fn encode_dpt18(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32_max(value, 0xFF)?;
    if v & 0x40 != 0 {
        return Err(DptError::out_of_range("DPT 18 bit 6 is reserved (0)"));
    }
    Ok(alloc::vec![low_u8(v)])
}

// ── DPT 27: 32-bit set (4 bytes) ─────────────────────────────
//
// Bits 0..15 hold the states s0..s15 and bits 16..31 the masks m0..m15: a bit set, not a number.

fn decode_dpt27(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    Ok(DptValue::UInt(u32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt27(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(val_u32(value)?.to_be_bytes().to_vec())
}

// ── DPT 29: Signed 64-bit (8 bytes) ──────────────────────────

fn decode_dpt29(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 8)?;
    Ok(DptValue::Int64(i64::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6],
        payload[7],
    ])))
}

fn encode_dpt29(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(val_i64(value)?.to_be_bytes().to_vec())
}

// ── DPT 232: RGB (3 bytes) ───────────────────────────────────

fn decode_dpt232(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 3)?;
    let rgb = (u32::from(payload[0]) << 16) | (u32::from(payload[1]) << 8) | u32::from(payload[2]);
    Ok(DptValue::UInt(rgb))
}

fn encode_dpt232(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let rgb = val_u32_max(value, 0x00FF_FFFF)?;
    Ok(alloc::vec![
        low_u8((rgb >> 16) & 0xFF),
        low_u8((rgb >> 8) & 0xFF),
        low_u8(rgb & 0xFF),
    ])
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use alloc::vec;

    use super::super::super::*;

    #[test]
    fn dpt1_roundtrip() {
        let bytes = encode(DPT_SWITCH, &DptValue::Bool(true)).unwrap();
        assert_eq!(bytes, &[1]);
        assert_eq!(decode(DPT_SWITCH, &[1]).unwrap(), DptValue::Bool(true));
        assert_eq!(decode(DPT_SWITCH, &[0]).unwrap(), DptValue::Bool(false));
    }

    #[test]
    fn dpt4_char() {
        let bytes = encode(DPT_CHAR_ASCII, &DptValue::UInt(65)).unwrap();
        assert_eq!(bytes, &[65]);
        assert_eq!(decode(DPT_CHAR_ASCII, &[65]).unwrap(), DptValue::UInt(65));
    }

    #[test]
    fn dpt5_scaling() {
        let bytes = encode(DPT_SCALING, &DptValue::Float(100.0)).unwrap();
        assert_eq!(bytes, &[255]);
        let val = decode(DPT_SCALING, &bytes).unwrap();
        let f = val.as_f64().unwrap();
        assert!((f - 100.0).abs() < 0.5);
    }

    #[test]
    fn dpt5_raw() {
        let bytes = encode(DPT_VALUE_1_UCOUNT, &DptValue::UInt(42)).unwrap();
        assert_eq!(bytes, &[42]);
        assert_eq!(
            decode(DPT_VALUE_1_UCOUNT, &[42]).unwrap(),
            DptValue::UInt(42)
        );
    }

    #[test]
    fn dpt6_signed() {
        let bytes = encode(Dpt::new(6, 1), &DptValue::Int(-10)).unwrap();
        assert_eq!(decode(Dpt::new(6, 1), &bytes).unwrap(), DptValue::Int(-10));
    }

    #[test]
    fn dpt7_unsigned16() {
        let bytes = encode(DPT_VALUE_2_UCOUNT, &DptValue::UInt(1000)).unwrap();
        assert_eq!(
            decode(DPT_VALUE_2_UCOUNT, &bytes).unwrap(),
            DptValue::UInt(1000)
        );
    }

    #[test]
    fn dpt8_signed16() {
        let bytes = encode(DPT_VALUE_2_COUNT, &DptValue::Int(-500)).unwrap();
        assert_eq!(
            decode(DPT_VALUE_2_COUNT, &bytes).unwrap(),
            DptValue::Int(-500)
        );
    }

    #[test]
    fn dpt9_temperature() {
        let bytes = encode(DPT_VALUE_TEMP, &DptValue::Float(21.5)).unwrap();
        let val = decode(DPT_VALUE_TEMP, &bytes).unwrap().as_f64().unwrap();
        assert!((val - 21.5).abs() < 0.1, "got {val}");
    }

    #[test]
    fn dpt9_negative() {
        let bytes = encode(DPT_VALUE_TEMP, &DptValue::Float(-10.0)).unwrap();
        let val = decode(DPT_VALUE_TEMP, &bytes).unwrap().as_f64().unwrap();
        assert!((val + 10.0).abs() < 0.1, "got {val}");
    }

    #[test]
    fn dpt10_time_bytes() {
        let bytes = encode(DPT_TIMEOFDAY, &DptValue::Bytes(vec![14, 30, 0])).unwrap();
        assert_eq!(bytes, &[14, 30, 0]);
        let decoded = decode(DPT_TIMEOFDAY, &bytes).unwrap();
        assert_eq!(decoded, DptValue::Bytes(vec![14, 30, 0]));
    }

    #[test]
    fn dpt10_time_from_seconds() {
        // 14:30:00 = 52200 seconds
        let bytes = encode(DPT_TIMEOFDAY, &DptValue::UInt(52200)).unwrap();
        assert_eq!(bytes, &[14, 30, 0]);
    }

    #[test]
    fn dpt11_date_bytes() {
        let bytes = encode(DPT_DATE, &DptValue::Bytes(vec![18, 4, 25])).unwrap();
        assert_eq!(bytes, &[18, 4, 25]);
    }

    #[test]
    fn dpt12_unsigned32() {
        let bytes = encode(DPT_VALUE_4_UCOUNT, &DptValue::UInt(100_000)).unwrap();
        assert_eq!(
            decode(DPT_VALUE_4_UCOUNT, &bytes).unwrap(),
            DptValue::UInt(100_000)
        );
    }

    #[test]
    fn dpt13_signed32() {
        let bytes = encode(DPT_VALUE_4_COUNT, &DptValue::Int(-100_000)).unwrap();
        assert_eq!(
            decode(DPT_VALUE_4_COUNT, &bytes).unwrap(),
            DptValue::Int(-100_000)
        );
    }

    #[test]
    fn dpt14_float32() {
        let bytes = encode(DPT_VALUE_POWER, &DptValue::Float(1234.5)).unwrap();
        let val = decode(DPT_VALUE_POWER, &bytes).unwrap().as_f64().unwrap();
        assert!((val - 1234.5).abs() < 0.1);
    }

    #[test]
    fn dpt15_access() {
        // Six BCD digits and the flags/index octet; the permission flag (bit 7 of the last octet
        // is E, bit 6 P) survives a round trip.
        let bytes = encode(DPT_ACCESS_DATA, &DptValue::UInt(123_456)).unwrap();
        assert_eq!(bytes, &[0x12, 0x34, 0x56, 0x00]);
        assert_eq!(
            decode(DPT_ACCESS_DATA, &bytes).unwrap(),
            DptValue::Bytes(vec![0x12, 0x34, 0x56, 0x00])
        );
        let with_flags = [0x00, 0x00, 0x07, 0x53];
        let decoded = decode(DPT_ACCESS_DATA, &with_flags).unwrap();
        assert_eq!(decoded, DptValue::Bytes(with_flags.to_vec()));
        assert_eq!(encode(DPT_ACCESS_DATA, &decoded).unwrap(), with_flags);
        // A digit above 9 and a value above six digits are errors, not other numbers.
        assert!(decode(DPT_ACCESS_DATA, &[0x1A, 0, 0, 0]).is_err());
        assert!(encode(DPT_ACCESS_DATA, &DptValue::UInt(1_000_000)).is_err());
    }

    #[test]
    fn dpt16_string() {
        let bytes = encode(DPT_STRING_ASCII, &DptValue::Text("Hello".into())).unwrap();
        assert_eq!(bytes.len(), 14);
        assert_eq!(
            decode(DPT_STRING_ASCII, &bytes).unwrap(),
            DptValue::Text("Hello".into())
        );
    }

    #[test]
    fn dpt17_scene() {
        let bytes = encode(DPT_SCENE_NUMBER, &DptValue::UInt(5)).unwrap();
        assert_eq!(bytes, &[5]);
        assert_eq!(decode(DPT_SCENE_NUMBER, &[5]).unwrap(), DptValue::UInt(5));
        assert_eq!(
            decode(DPT_SCENE_NUMBER, &[0x7F]).unwrap(),
            DptValue::UInt(63)
        );
    }

    #[test]
    fn dpt18_scene_control() {
        let bytes = encode(DPT_SCENE_CONTROL, &DptValue::UInt(5)).unwrap();
        assert_eq!(bytes, &[5]);
        let bytes = encode(DPT_SCENE_CONTROL, &DptValue::UInt(133)).unwrap();
        assert_eq!(bytes, &[133]);
    }

    #[test]
    fn dpt29_signed64() {
        let bytes = encode(DPT_ACTIVE_ENERGY_V64, &DptValue::Int64(-1_000_000)).unwrap();
        assert_eq!(bytes.len(), 8);
        assert_eq!(
            decode(DPT_ACTIVE_ENERGY_V64, &bytes).unwrap(),
            DptValue::Int64(-1_000_000)
        );
    }

    #[test]
    fn dpt232_rgb() {
        let bytes = encode(DPT_COLOUR_RGB, &DptValue::UInt(0x00FF_0000)).unwrap();
        assert_eq!(bytes, &[0xFF, 0x00, 0x00]);
        assert_eq!(
            decode(DPT_COLOUR_RGB, &bytes).unwrap(),
            DptValue::UInt(0x00FF_0000)
        );
    }

    #[test]
    fn dpt251_rgbw_is_six_octets_with_validity_bits() {
        // R G B W, a reserved octet, and four reserved bits plus mR mG mB mW (1 = valid).
        let value = DptValue::Bytes(vec![0xFF, 0x80, 0x40, 0x20, 0x00, 0x0F]);
        let bytes = encode(DPT_COLOUR_RGBW, &value).unwrap();
        assert_eq!(bytes, &[0xFF, 0x80, 0x40, 0x20, 0x00, 0x0F]);
        assert_eq!(decode(DPT_COLOUR_RGBW, &bytes).unwrap(), value);
        // The old four-octet form is neither produced nor accepted.
        assert!(encode(DPT_COLOUR_RGBW, &DptValue::Bytes(vec![1, 2, 3, 4])).is_err());
        assert!(decode(DPT_COLOUR_RGBW, &[1, 2, 3, 4]).is_err());
    }

    #[test]
    fn dpt28_unicode() {
        let bytes = encode(Dpt::new(28, 1), &DptValue::Text("Héllo 🌍".into())).unwrap();
        assert_eq!(
            decode(Dpt::new(28, 1), &bytes).unwrap(),
            DptValue::Text("Héllo 🌍".into())
        );
    }

    #[test]
    fn unsupported_dpt() {
        assert!(matches!(
            decode(Dpt::new(999, 1), &[0]),
            Err(DptError::UnsupportedDpt(_))
        ));
    }

    #[test]
    fn type_mismatch() {
        assert!(matches!(
            encode(DPT_SWITCH, &DptValue::Text("hello".into())),
            Err(DptError::TypeMismatch)
        ));
        assert!(matches!(
            encode(DPT_STRING_ASCII, &DptValue::Bool(true)),
            Err(DptError::TypeMismatch)
        ));
    }

    #[test]
    fn coercion_bool_to_uint() {
        let bytes = encode(DPT_VALUE_1_UCOUNT, &DptValue::Bool(true)).unwrap();
        assert_eq!(bytes, &[1]);
    }

    #[test]
    fn coercion_float_to_uint() {
        let bytes = encode(DPT_VALUE_1_UCOUNT, &DptValue::Float(42.7)).unwrap();
        assert_eq!(bytes, &[43]); // rounded
    }

    #[test]
    fn payload_too_short() {
        assert!(decode(DPT_VALUE_TEMP, &[0x0C]).is_err());
        assert!(decode(DPT_VALUE_POWER, &[0, 0]).is_err());
    }
}
