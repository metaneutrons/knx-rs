// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Numeric DPT encode/decode — all main groups that map to typed [`DptValue`] variants.

use alloc::vec::Vec;

use super::super::{Dpt, DptError, DptValue};

/// Round `f64` to nearest integer (`no_std` compatible).
fn round(v: f64) -> f64 {
    libm::round(v)
}

/// Convert a clamped `f64` to `u8`.
///
/// Clamps to `[0, 255]` before conversion.
const fn f64_to_u8(v: f64) -> u8 {
    // SAFETY: value is clamped to [0, 255], truncation and sign loss are impossible.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "value clamped to u8 range"
    )]
    {
        v.clamp(0.0, 255.0) as u8
    }
}

/// Convert an `f64` to `u32`, rounding to nearest.
///
/// Returns `Err(OutOfRange)` if the value is negative or exceeds `u32::MAX`.
fn f64_to_u32(v: f64) -> Result<u32, DptError> {
    let r = round(v);
    if r < 0.0 || r > f64::from(u32::MAX) {
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
/// Returns `Err(OutOfRange)` if the value exceeds `i32` range.
fn f64_to_i32(v: f64) -> Result<i32, DptError> {
    let r = round(v);
    if r < f64::from(i32::MIN) || r > f64::from(i32::MAX) {
        return Err(DptError::out_of_range("expected -2147483648..=2147483647"));
    }
    // SAFETY: range validated above.
    #[expect(clippy::cast_possible_truncation, reason = "range validated above")]
    Ok(r as i32)
}

/// Convert a range-checked `f64` to `i64`.
///
/// Precision loss is inherent: `f64` cannot represent all `i64` values.
fn f64_to_i64(v: f64) -> i64 {
    // SAFETY: round() returns a whole number; the cast is the best approximation.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "best approximation for f64→i64"
    )]
    {
        round(v) as i64
    }
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
        DptValue::Float(v) => Ok(f64_to_i64(*v)),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── Dispatch ──────────────────────────────────────────────────

pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        1 => decode_dpt1(payload),
        2 => decode_dpt2(payload),
        3 => decode_dpt3(payload),
        4 => decode_dpt4(payload),
        5 => decode_dpt5(dpt, payload),
        6 => decode_dpt6(payload),
        7 => decode_dpt7(payload),
        8 => decode_dpt8(payload),
        9 => decode_dpt9(payload),
        10 => decode_dpt10(payload),
        11 => decode_dpt11(payload),
        12 => decode_dpt12(payload),
        13 | 27 => decode_dpt13(payload),
        14 => decode_dpt14(payload),
        15 => decode_dpt15(payload),
        17 | 26 | 238 => decode_dpt17(payload),
        18 => decode_dpt18(payload),
        19 => decode_dpt19(payload),
        29 => decode_dpt29(payload),
        217 => decode_dpt217(payload),
        219 => decode_dpt219(payload),
        221 => decode_dpt221(payload),
        225 => decode_dpt225(payload),
        231 | 234 => decode_dpt_locale(dpt, payload),
        232 => decode_dpt232(payload),
        235 => decode_dpt235(payload),
        239 => decode_dpt239(payload),
        251 => decode_dpt251(payload),
        _ => Err(DptError::UnsupportedDpt(dpt)),
    }
}

pub fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    match dpt.main {
        1 => encode_dpt1(value),
        2 => encode_dpt2(value),
        3 => encode_dpt3(value),
        4 => encode_dpt4(value),
        5 => encode_dpt5(dpt, value),
        6 => encode_dpt6(value),
        7 => encode_dpt7(value),
        8 => encode_dpt8(value),
        9 => encode_dpt9(value),
        10 => encode_dpt10(value),
        11 => encode_dpt11(value),
        12 => encode_dpt12(value),
        13 | 27 => encode_dpt13(value),
        14 => encode_dpt14(value),
        15 => encode_dpt15(value),
        17 | 26 | 238 => encode_dpt17(value),
        18 => encode_dpt18(value),
        19 => encode_dpt19(value),
        29 => encode_dpt29(value),
        217 => encode_dpt217(value),
        219 => encode_dpt219(value),
        221 => encode_dpt221(value),
        225 => encode_dpt225(value),
        231 | 234 => encode_dpt_locale(dpt, value),
        232 => encode_dpt232(value),
        235 => encode_dpt235(value),
        239 => encode_dpt239(value),
        251 => encode_dpt251(value),
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

// ── DPT 2: 1-bit controlled (2 bits) ─────────────────────────

fn decode_dpt2(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0] & 0x03)))
}

fn encode_dpt2(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    Ok(alloc::vec![low_u8(v & 0x03)])
}

// ── DPT 3: 3-bit controlled (4 bits) ─────────────────────────

fn decode_dpt3(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0] & 0x0F)))
}

fn encode_dpt3(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    Ok(alloc::vec![low_u8(v & 0x0F)])
}

// ── DPT 4: Character (1 byte) ────────────────────────────────

fn decode_dpt4(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0])))
}

fn encode_dpt4(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    Ok(alloc::vec![low_u8(v)])
}

// ── DPT 5: Unsigned 8-bit (1 byte) ───────────────────────────

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
            Ok(alloc::vec![f64_to_u8(round(v * 255.0 / 100.0))])
        }
        3 => {
            let v = val_f64(value)?;
            Ok(alloc::vec![f64_to_u8(round(v * 255.0 / 360.0))])
        }
        _ => {
            let v = val_u32(value)?;
            Ok(alloc::vec![low_u8(v.min(255))])
        }
    }
}

// ── DPT 6: Signed 8-bit (1 byte) ─────────────────────────────

fn decode_dpt6(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::Int(i32::from(i8::from_ne_bytes([payload[0]]))))
}

fn encode_dpt6(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_i32(value)?;
    let clamped = v.clamp(-128, 127);
    Ok(alloc::vec![low_i8(clamped).to_ne_bytes()[0]])
}

// ── DPT 7: Unsigned 16-bit (2 bytes) ─────────────────────────

fn decode_dpt7(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::UInt(u32::from(u16::from_be_bytes([
        payload[0], payload[1],
    ]))))
}

fn encode_dpt7(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    let clamped = low_u16(v.min(65535));
    Ok(clamped.to_be_bytes().to_vec())
}

// ── DPT 8: Signed 16-bit (2 bytes) ───────────────────────────

fn decode_dpt8(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::Int(i32::from(i16::from_be_bytes([
        payload[0], payload[1],
    ]))))
}

fn encode_dpt8(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_i32(value)?;
    let clamped = low_i16(v.clamp(-32768, 32767));
    Ok(clamped.to_be_bytes().to_vec())
}

// ── DPT 9: 16-bit float (2 bytes, KNX F16) ───────────────────
//
// Wire: `MEEEEMMM MMMMMMMM` — sign + 11-bit mantissa (two's complement), 4-bit exponent.
// Value = 0.01 × mantissa × 2^exponent

fn decode_dpt9(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    let raw = u16::from_be_bytes([payload[0], payload[1]]);
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
    // round once, then range-check without re-rounding
    let scaled = round(v * 100.0);
    if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(DptError::out_of_range("KNX F16 mantissa overflow"));
    }
    #[expect(clippy::cast_possible_truncation, reason = "range validated above")]
    let mut mantissa = scaled as i32;
    let mut exponent: u16 = 0;

    while mantissa > 2047 {
        mantissa >>= 1;
        exponent += 1;
    }
    while mantissa < -2048 {
        mantissa >>= 1;
        exponent += 1;
    }
    if exponent > 15 {
        return Err(DptError::out_of_range("KNX F16 exponent overflow (>15)"));
    }

    let m = low_u16((mantissa & 0x07FF).unsigned_abs());
    let sign: u16 = if mantissa < 0 { 0x8000 } else { 0 };
    let raw = sign | (exponent << 11) | m;
    Ok(raw.to_be_bytes().to_vec())
}

// ── DPT 10: Time of day (3 bytes) ────────────────────────────

fn decode_dpt10(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 3)?;
    Ok(DptValue::Bytes(payload[..3].to_vec()))
}

fn encode_dpt10(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 3 => Ok(b[..3].to_vec()),
        DptValue::UInt(secs) => {
            let total = (*secs).min(86399);
            let hours = low_u8(total / 3600);
            let minutes = low_u8((total % 3600) / 60);
            let seconds = low_u8(total % 60);
            Ok(alloc::vec![hours & 0x1F, minutes & 0x3F, seconds & 0x3F])
        }
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 11: Date (3 bytes) ───────────────────────────────────

fn decode_dpt11(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 3)?;
    Ok(DptValue::Bytes(payload[..3].to_vec()))
}

fn encode_dpt11(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 3 => Ok(b[..3].to_vec()),
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
    // f64→f32 narrowing: values outside f32 range become ±inf, which is acceptable
    // for IEEE 754 wire encoding.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "f64→f32 narrowing is intentional for IEEE 754 wire format"
    )]
    let f = v as f32;
    Ok(f.to_be_bytes().to_vec())
}

// ── DPT 15: Access data (4 bytes, 6-digit BCD) ───────────────

fn decode_dpt15(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    let mut digits: u32 = 0;
    let mut factor: u32 = 100_000;
    for i in 0..6 {
        let nibble = if i % 2 == 0 {
            (payload[i / 2] >> 4) & 0x0F
        } else {
            payload[i / 2] & 0x0F
        };
        digits += u32::from(nibble) * factor;
        factor /= 10;
    }
    Ok(DptValue::UInt(digits))
}

fn encode_dpt15(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let mut v = val_u32(value)?.min(999_999);
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

// ── DPT 17: Scene number (1 byte, 0–63) ──────────────────────

fn decode_dpt17(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0] & 0x3F)))
}

fn encode_dpt17(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    Ok(alloc::vec![low_u8(v & 0x3F)])
}

// ── DPT 18: Scene control (1 byte) ───────────────────────────

fn decode_dpt18(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 1)?;
    Ok(DptValue::UInt(u32::from(payload[0])))
}

fn encode_dpt18(value: &DptValue) -> Result<Vec<u8>, DptError> {
    let v = val_u32(value)?;
    Ok(alloc::vec![low_u8(v)])
}

// ── DPT 19: Date and time (8 bytes) ──────────────────────────

fn decode_dpt19(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 8)?;
    Ok(DptValue::Bytes(payload[..8].to_vec()))
}

fn encode_dpt19(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 8 => Ok(b[..8].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
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
    let rgb = val_u32(value)?.min(0x00FF_FFFF);
    Ok(alloc::vec![
        low_u8((rgb >> 16) & 0xFF),
        low_u8((rgb >> 8) & 0xFF),
        low_u8(rgb & 0xFF),
    ])
}

// ── DPT 251: RGBW (4 bytes) ──────────────────────────────────

fn decode_dpt251(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 4)?;
    let rgbw = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    Ok(DptValue::UInt(rgbw))
}

fn encode_dpt251(value: &DptValue) -> Result<Vec<u8>, DptError> {
    Ok(val_u32(value)?.to_be_bytes().to_vec())
}

// ── DPT 217: Version (2 bytes) ────────────────────────────────

fn decode_dpt217(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::Bytes(payload[..2].to_vec()))
}

fn encode_dpt217(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 2 => Ok(b[..2].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 219: Alarm info (6 bytes) ────────────────────────────

fn decode_dpt219(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 6)?;
    Ok(DptValue::Bytes(payload[..6].to_vec()))
}

fn encode_dpt219(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 6 => Ok(b[..6].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 221: Serial number (6 bytes) ─────────────────────────

fn decode_dpt221(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 6)?;
    Ok(DptValue::Bytes(payload[..6].to_vec()))
}

fn encode_dpt221(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 6 => Ok(b[..6].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 225: Scaling speed / step time (3 bytes) ─────────────

fn decode_dpt225(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 3)?;
    Ok(DptValue::Bytes(payload[..3].to_vec()))
}

fn encode_dpt225(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 3 => Ok(b[..3].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 231/234: Locale / Language code (2 or 4 bytes) ───────

fn decode_dpt_locale(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    let min_len = if dpt.main == 231 { 4 } else { 2 };
    check_len(payload, min_len)?;
    Ok(DptValue::Bytes(payload[..min_len].to_vec()))
}

fn encode_dpt_locale(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    let min_len = if dpt.main == 231 { 4 } else { 2 };
    match value {
        DptValue::Bytes(b) if b.len() >= min_len => Ok(b[..min_len].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 235: Active energy (6 bytes) ─────────────────────────

fn decode_dpt235(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 6)?;
    Ok(DptValue::Bytes(payload[..6].to_vec()))
}

fn encode_dpt235(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 6 => Ok(b[..6].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 239: Flagged scaling (2 bytes) ───────────────────────

fn decode_dpt239(payload: &[u8]) -> Result<DptValue, DptError> {
    check_len(payload, 2)?;
    Ok(DptValue::Bytes(payload[..2].to_vec()))
}

fn encode_dpt239(value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Bytes(b) if b.len() >= 2 => Ok(b[..2].to_vec()),
        _ => Err(DptError::TypeMismatch),
    }
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
        let bytes = encode(DPT_ACCESS_DATA, &DptValue::UInt(123_456)).unwrap();
        assert_eq!(
            decode(DPT_ACCESS_DATA, &bytes).unwrap(),
            DptValue::UInt(123_456)
        );
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
    fn dpt251_rgbw() {
        let bytes = encode(DPT_COLOUR_RGBW, &DptValue::UInt(0xFF80_4020)).unwrap();
        assert_eq!(bytes, &[0xFF, 0x80, 0x40, 0x20]);
        assert_eq!(
            decode(DPT_COLOUR_RGBW, &bytes).unwrap(),
            DptValue::UInt(0xFF80_4020)
        );
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
