// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Numeric DPT encode/decode — all main groups that map to `f64`.

use alloc::vec::Vec;

use super::super::{Dpt, DptError};

/// Round `f64` to nearest integer (`no_std` compatible).
fn round(v: f64) -> f64 {
    libm::round(v)
}

// ── Cast helpers (clamped, clippy-clean) ──────────────────────

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u8(v: f64) -> u8 {
    round(v).clamp(0.0, 255.0) as u8
}

#[expect(clippy::cast_possible_truncation)]
fn to_i8(v: f64) -> i8 {
    round(v).clamp(-128.0, 127.0) as i8
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u16(v: f64) -> u16 {
    round(v).clamp(0.0, 65535.0) as u16
}

#[expect(clippy::cast_possible_truncation)]
fn to_i16(v: f64) -> i16 {
    round(v).clamp(-32768.0, 32767.0) as i16
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u32(v: f64) -> u32 {
    round(v).clamp(0.0, f64::from(u32::MAX)) as u32
}

#[expect(clippy::cast_possible_truncation)]
fn to_i32(v: f64) -> i32 {
    round(v).clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn to_i64(v: f64) -> i64 {
    round(v).clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

const fn check_len(payload: &[u8], min: usize) -> Result<(), DptError> {
    if payload.len() < min {
        Err(DptError::PayloadTooShort)
    } else {
        Ok(())
    }
}

// ── Dispatch ──────────────────────────────────────────────────

pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<f64, DptError> {
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

pub fn encode(dpt: Dpt, value: f64) -> Result<Vec<u8>, DptError> {
    match dpt.main {
        1 => Ok(encode_dpt1(value)),
        2 => Ok(encode_dpt2(value)),
        3 => Ok(encode_dpt3(value)),
        4 => Ok(encode_dpt4(value)),
        5 => Ok(encode_dpt5(dpt, value)),
        6 => Ok(encode_dpt6(value)),
        7 => Ok(encode_dpt7(value)),
        8 => Ok(encode_dpt8(value)),
        9 => encode_dpt9(value),
        10 => Ok(encode_dpt10(value)),
        11 => Ok(encode_dpt11(value)),
        12 => Ok(encode_dpt12(value)),
        13 | 27 => Ok(encode_dpt13(value)),
        14 => Ok(encode_dpt14(value)),
        15 => Ok(encode_dpt15(value)),
        17 | 26 | 238 => Ok(encode_dpt17(value)),
        18 => Ok(encode_dpt18(value)),
        19 => Ok(encode_dpt19(value)),
        29 => Ok(encode_dpt29(value)),
        217 => Ok(encode_dpt217(value)),
        219 => Ok(encode_dpt219(value)),
        221 => Ok(encode_dpt221(value)),
        225 => Ok(encode_dpt225(value)),
        231 | 234 => Ok(encode_dpt_locale(dpt, value)),
        232 => Ok(encode_dpt232(value)),
        235 => Ok(encode_dpt235(value)),
        239 => Ok(encode_dpt239(value)),
        251 => Ok(encode_dpt251(value)),
        _ => Err(DptError::UnsupportedDpt(dpt)),
    }
}

// ── DPT 1: Boolean (1 bit) ───────────────────────────────────

fn decode_dpt1(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0] & 0x01))
}

fn encode_dpt1(value: f64) -> Vec<u8> {
    alloc::vec![u8::from(value != 0.0)]
}

// ── DPT 2: 1-bit controlled (2 bits) ─────────────────────────

fn decode_dpt2(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0] & 0x03))
}

fn encode_dpt2(value: f64) -> Vec<u8> {
    alloc::vec![to_u8(value) & 0x03]
}

// ── DPT 3: 3-bit controlled (4 bits) ─────────────────────────

fn decode_dpt3(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0] & 0x0F))
}

fn encode_dpt3(value: f64) -> Vec<u8> {
    alloc::vec![to_u8(value) & 0x0F]
}

// ── DPT 4: Character (1 byte) ────────────────────────────────

fn decode_dpt4(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0]))
}

fn encode_dpt4(value: f64) -> Vec<u8> {
    alloc::vec![to_u8(value)]
}

// ── DPT 5: Unsigned 8-bit (1 byte) ───────────────────────────

fn decode_dpt5(dpt: Dpt, payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    let raw = f64::from(payload[0]);
    Ok(match dpt.sub {
        1 => raw * 100.0 / 255.0,
        3 => raw * 360.0 / 255.0,
        _ => raw,
    })
}

fn encode_dpt5(dpt: Dpt, value: f64) -> Vec<u8> {
    let raw = match dpt.sub {
        1 => value * 255.0 / 100.0,
        3 => value * 255.0 / 360.0,
        _ => value,
    };
    alloc::vec![to_u8(raw)]
}

// ── DPT 6: Signed 8-bit (1 byte) ─────────────────────────────

#[expect(clippy::cast_possible_wrap)]
fn decode_dpt6(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0] as i8))
}

#[expect(clippy::cast_sign_loss)]
fn encode_dpt6(value: f64) -> Vec<u8> {
    alloc::vec![to_i8(value) as u8]
}

// ── DPT 7: Unsigned 16-bit (2 bytes) ─────────────────────────

fn decode_dpt7(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 2)?;
    Ok(f64::from(u16::from_be_bytes([payload[0], payload[1]])))
}

fn encode_dpt7(value: f64) -> Vec<u8> {
    to_u16(value).to_be_bytes().to_vec()
}

// ── DPT 8: Signed 16-bit (2 bytes) ───────────────────────────

fn decode_dpt8(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 2)?;
    Ok(f64::from(i16::from_be_bytes([payload[0], payload[1]])))
}

fn encode_dpt8(value: f64) -> Vec<u8> {
    to_i16(value).to_be_bytes().to_vec()
}

// ── DPT 9: 16-bit float (2 bytes, KNX F16) ───────────────────
//
// Wire: `MEEEEMMM MMMMMMMM` — sign + 11-bit mantissa (two's complement), 4-bit exponent.
// Value = 0.01 × mantissa × 2^exponent

fn decode_dpt9(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 2)?;
    let raw = u16::from_be_bytes([payload[0], payload[1]]);
    let exponent = i32::from((raw >> 11) & 0x0F);
    let mantissa = {
        let m = i32::from(raw & 0x07FF);
        if raw & 0x8000 != 0 { m - 0x0800 } else { m }
    };
    Ok(0.01 * f64::from(mantissa) * f64::from(1 << exponent))
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt9(value: f64) -> Result<Vec<u8>, DptError> {
    let mut mantissa = round(value * 100.0) as i32;
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
        return Err(DptError::OutOfRange);
    }

    let m = (mantissa & 0x07FF) as u16;
    let sign: u16 = if mantissa < 0 { 0x8000 } else { 0 };
    let raw = sign | (exponent << 11) | m;
    Ok(raw.to_be_bytes().to_vec())
}

// ── DPT 10: Time of day (3 bytes) ────────────────────────────
//
// Byte 0: [weekday:3][hours:5], Byte 1: [00][minutes:6], Byte 2: [00][seconds:6]
// Decoded as seconds since midnight (weekday × 86400 not included in f64).

fn decode_dpt10(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 3)?;
    let hours = f64::from(payload[0] & 0x1F);
    let minutes = f64::from(payload[1] & 0x3F);
    let seconds = f64::from(payload[2] & 0x3F);
    Ok(hours * 3600.0 + minutes * 60.0 + seconds)
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt10(value: f64) -> Vec<u8> {
    let total_secs = round(value).clamp(0.0, 86399.0) as u32;
    let hours = (total_secs / 3600) as u8;
    let minutes = ((total_secs % 3600) / 60) as u8;
    let seconds = (total_secs % 60) as u8;
    alloc::vec![hours & 0x1F, minutes & 0x3F, seconds & 0x3F]
}

// ── DPT 11: Date (3 bytes) ───────────────────────────────────
//
// Byte 0: [000][day:5], Byte 1: [0000][month:4], Byte 2: [0][year:7]
// Year: 0–99, where ≥90 → 1900+year, <90 → 2000+year.
// Decoded as YYYYMMDD integer.

fn decode_dpt11(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 3)?;
    let day = f64::from(payload[0] & 0x1F);
    let month = f64::from(payload[1] & 0x0F);
    let year_raw = payload[2] & 0x7F;
    let year = if year_raw >= 90 {
        f64::from(1900 + u16::from(year_raw))
    } else {
        f64::from(2000 + u16::from(year_raw))
    };
    Ok(year * 10000.0 + month * 100.0 + day)
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt11(value: f64) -> Vec<u8> {
    let v = round(value) as u32;
    let year = v / 10000;
    let month = ((v % 10000) / 100) as u8;
    let day = (v % 100) as u8;
    let year_byte = if year >= 2000 {
        (year - 2000) as u8
    } else {
        (year - 1900) as u8
    };
    alloc::vec![day & 0x1F, month & 0x0F, year_byte & 0x7F]
}

// ── DPT 12: Unsigned 32-bit (4 bytes) ────────────────────────

fn decode_dpt12(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 4)?;
    Ok(f64::from(u32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt12(value: f64) -> Vec<u8> {
    to_u32(value).to_be_bytes().to_vec()
}

// ── DPT 13: Signed 32-bit (4 bytes) ──────────────────────────

fn decode_dpt13(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 4)?;
    Ok(f64::from(i32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt13(value: f64) -> Vec<u8> {
    to_i32(value).to_be_bytes().to_vec()
}

// ── DPT 14: IEEE 754 32-bit float (4 bytes) ──────────────────

fn decode_dpt14(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 4)?;
    Ok(f64::from(f32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

#[expect(clippy::cast_possible_truncation)]
fn encode_dpt14(value: f64) -> Vec<u8> {
    (value as f32).to_be_bytes().to_vec()
}

// ── DPT 15: Access data (4 bytes, 6-digit BCD) ───────────────

fn decode_dpt15(payload: &[u8]) -> Result<f64, DptError> {
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
    Ok(f64::from(digits))
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt15(value: f64) -> Vec<u8> {
    let mut v = round(value).clamp(0.0, 999_999.0) as u32;
    let mut buf = [0u8; 4];
    for i in (0..6).rev() {
        let digit = (v % 10) as u8;
        v /= 10;
        if i % 2 == 0 {
            buf[i / 2] |= digit << 4;
        } else {
            buf[i / 2] |= digit;
        }
    }
    buf.to_vec()
}

// ── DPT 17: Scene number (1 byte, 0–63) ──────────────────────

fn decode_dpt17(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0] & 0x3F))
}

fn encode_dpt17(value: f64) -> Vec<u8> {
    alloc::vec![to_u8(value) & 0x3F]
}

// ── DPT 18: Scene control (1 byte) ───────────────────────────
//
// Bit 7: learn (1) / activate (0), Bits 5..0: scene number.
// Encoded as: learn_flag * 128 + scene_number.

fn decode_dpt18(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 1)?;
    Ok(f64::from(payload[0]))
}

fn encode_dpt18(value: f64) -> Vec<u8> {
    alloc::vec![to_u8(value)]
}

// ── DPT 19: Date and time (8 bytes) ──────────────────────────
//
// Bytes: year(1) month(1) day(1) weekday+hour(1) min(1) sec(1) status(1) quality(1)
// Year = value + 1900. Decoded as YYYYMMDDHHMMSS as f64.

fn decode_dpt19(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 8)?;
    let year = f64::from(u16::from(payload[0]) + 1900);
    let month = f64::from(payload[1] & 0x0F);
    let day = f64::from(payload[2] & 0x1F);
    let hour = f64::from(payload[3] & 0x1F);
    let min = f64::from(payload[4] & 0x3F);
    let sec = f64::from(payload[5] & 0x3F);
    // Encode as YYYYMMDDHHmmss
    Ok(year * 1e10 + month * 1e8 + day * 1e6 + hour * 1e4 + min * 1e2 + sec)
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt19(value: f64) -> Vec<u8> {
    let v = round(value) as u64;
    let sec = (v % 100) as u8;
    let min = ((v / 100) % 100) as u8;
    let hour = ((v / 10000) % 100) as u8;
    let day = ((v / 1_000_000) % 100) as u8;
    let month = ((v / 100_000_000) % 100) as u8;
    let year = ((v / 10_000_000_000) as u16).saturating_sub(1900) as u8;
    alloc::vec![year, month, day, hour, min, sec, 0, 0]
}

// ── DPT 29: Signed 64-bit (8 bytes) ──────────────────────────

fn decode_dpt29(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 8)?;
    let v = i64::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6],
        payload[7],
    ]);
    // Note: f64 can represent i64 values up to 2^53 exactly
    #[expect(clippy::cast_precision_loss)]
    Ok(v as f64)
}

fn encode_dpt29(value: f64) -> Vec<u8> {
    to_i64(value).to_be_bytes().to_vec()
}

// ── DPT 232: RGB (3 bytes) ───────────────────────────────────
//
// Encoded as 0xRRGGBB in f64.

fn decode_dpt232(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 3)?;
    let rgb = (u32::from(payload[0]) << 16) | (u32::from(payload[1]) << 8) | u32::from(payload[2]);
    Ok(f64::from(rgb))
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt232(value: f64) -> Vec<u8> {
    let rgb = round(value).clamp(0.0, f64::from(0x00FF_FFFFu32)) as u32;
    alloc::vec![
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    ]
}

// ── DPT 251: RGBW (4 bytes) ──────────────────────────────────
//
// Encoded as 0xRRGGBBWW in f64.

fn decode_dpt251(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 4)?;
    let rgbw = (u32::from(payload[0]) << 24)
        | (u32::from(payload[1]) << 16)
        | (u32::from(payload[2]) << 8)
        | u32::from(payload[3]);
    Ok(f64::from(rgbw))
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt251(value: f64) -> Vec<u8> {
    let rgbw = round(value).clamp(0.0, f64::from(u32::MAX)) as u32;
    rgbw.to_be_bytes().to_vec()
}

// ── DPT 217: Version (2 bytes) ────────────────────────────────
//
// Byte 0: [magic:3][major:5], Byte 1: [minor:5][patch:6] — but actually:
// magic(3) major(5) minor(5) patch(6) packed into 16 bits.
// Decoded as major * 10000 + minor * 100 + patch.

fn decode_dpt217(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 2)?;
    let major = f64::from((payload[0] >> 3) & 0x1F);
    let minor = f64::from(((u16::from(payload[0]) << 2) | (u16::from(payload[1]) >> 6)) & 0x1F);
    let patch = f64::from(payload[1] & 0x3F);
    Ok(major * 10000.0 + minor * 100.0 + patch)
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_dpt217(value: f64) -> Vec<u8> {
    let v = round(value) as u32;
    let major = ((v / 10000) & 0x1F) as u8;
    let minor = (((v / 100) % 100) & 0x1F) as u8;
    let patch = ((v % 100) & 0x3F) as u8;
    alloc::vec![(major << 3) | (minor >> 2), ((minor & 0x03) << 6) | patch]
}

// ── DPT 219: Alarm info (6 bytes) ────────────────────────────
//
// Primary value (index 0): log number (byte 0).

fn decode_dpt219(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 6)?;
    Ok(f64::from(payload[0]))
}

fn encode_dpt219(value: f64) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 6];
    buf[0] = to_u8(value);
    buf
}

// ── DPT 221: Serial number (6 bytes) ─────────────────────────
//
// Bytes 0-1: manufacturer code (u16), Bytes 2-5: serial (u32).
// Primary value (index 0): manufacturer code.

fn decode_dpt221(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 6)?;
    Ok(f64::from(u16::from_be_bytes([payload[0], payload[1]])))
}

fn encode_dpt221(value: f64) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 6];
    let mfr = to_u16(value).to_be_bytes();
    buf[0] = mfr[0];
    buf[1] = mfr[1];
    buf
}

// ── DPT 225: Scaling speed / step time (3 bytes) ─────────────
//
// Bytes 0-1: time period (u16), Byte 2: scaling (0-255 → 0-100%).
// Primary value (index 0): time period.

fn decode_dpt225(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 3)?;
    Ok(f64::from(u16::from_be_bytes([payload[0], payload[1]])))
}

fn encode_dpt225(value: f64) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 3];
    let period = to_u16(value).to_be_bytes();
    buf[0] = period[0];
    buf[1] = period[1];
    buf
}

// ── DPT 231/234: Locale / Language code (2 or 4 bytes) ───────
//
// Two ASCII characters. Decoded as u16 (char1 << 8 | char2).

fn decode_dpt_locale(dpt: Dpt, payload: &[u8]) -> Result<f64, DptError> {
    let min_len = if dpt.main == 231 { 4 } else { 2 };
    check_len(payload, min_len)?;
    Ok(f64::from(u16::from_be_bytes([payload[0], payload[1]])))
}

fn encode_dpt_locale(dpt: Dpt, value: f64) -> Vec<u8> {
    let code = to_u16(value).to_be_bytes();
    if dpt.main == 231 {
        alloc::vec![code[0], code[1], 0, 0]
    } else {
        code.to_vec()
    }
}

// ── DPT 235: Active energy (6 bytes) ─────────────────────────
//
// Bytes 0-3: energy (i32), Byte 4: tariff, Byte 5: flags.
// Primary value (index 0): energy.

fn decode_dpt235(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 6)?;
    Ok(f64::from(i32::from_be_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ])))
}

fn encode_dpt235(value: f64) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 6];
    let energy = to_i32(value).to_be_bytes();
    buf[..4].copy_from_slice(&energy);
    buf
}

// ── DPT 239: Flagged scaling (2 bytes) ───────────────────────
//
// Byte 0: scaling (0-255 → 0-100%), Byte 1: flags.
// Primary value (index 0): scaling percentage.

fn decode_dpt239(payload: &[u8]) -> Result<f64, DptError> {
    check_len(payload, 2)?;
    Ok(f64::from(payload[0]) * 100.0 / 255.0)
}

fn encode_dpt239(value: f64) -> Vec<u8> {
    let scaled = to_u8(value * 255.0 / 100.0);
    alloc::vec![scaled, 0]
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::unreadable_literal
)]
mod tests {
    use super::super::super::*;

    #[test]
    fn dpt1_roundtrip() {
        assert_eq!(encode(DPT_SWITCH, 1.0).unwrap(), &[1]);
        assert_eq!(decode(DPT_SWITCH, &[1]).unwrap(), 1.0);
        assert_eq!(encode(DPT_SWITCH, 0.0).unwrap(), &[0]);
        assert_eq!(decode(DPT_SWITCH, &[0]).unwrap(), 0.0);
    }

    #[test]
    fn dpt4_char() {
        assert_eq!(encode(DPT_CHAR_ASCII, 65.0).unwrap(), &[65]); // 'A'
        assert_eq!(decode(DPT_CHAR_ASCII, &[65]).unwrap(), 65.0);
    }

    #[test]
    fn dpt5_scaling() {
        let bytes = encode(DPT_SCALING, 100.0).unwrap();
        assert_eq!(bytes, &[255]);
        assert!((decode(DPT_SCALING, &bytes).unwrap() - 100.0).abs() < 0.5);
    }

    #[test]
    fn dpt9_temperature() {
        let bytes = encode(DPT_VALUE_TEMP, 21.5).unwrap();
        let val = decode(DPT_VALUE_TEMP, &bytes).unwrap();
        assert!((val - 21.5).abs() < 0.1, "got {val}");
    }

    #[test]
    fn dpt9_negative() {
        let bytes = encode(DPT_VALUE_TEMP, -10.0).unwrap();
        let val = decode(DPT_VALUE_TEMP, &bytes).unwrap();
        assert!((val + 10.0).abs() < 0.1, "got {val}");
    }

    #[test]
    fn dpt10_time() {
        // 14:30:00 = 52200 seconds
        let bytes = encode(DPT_TIMEOFDAY, 52200.0).unwrap();
        assert_eq!(bytes, &[14, 30, 0]);
        assert_eq!(decode(DPT_TIMEOFDAY, &bytes).unwrap(), 52200.0);
    }

    #[test]
    fn dpt11_date() {
        // 2025-04-18 = 20_250_418.0
        let bytes = encode(DPT_DATE, 20_250_418.0).unwrap();
        assert_eq!(bytes, &[18, 4, 25]); // day, month, year-2000
        assert_eq!(decode(DPT_DATE, &bytes).unwrap(), 20_250_418.0);
    }

    #[test]
    fn dpt14_float32() {
        let bytes = encode(DPT_VALUE_POWER, 1234.5).unwrap();
        let val = decode(DPT_VALUE_POWER, &bytes).unwrap();
        assert!((val - 1234.5).abs() < 0.1);
    }

    #[test]
    fn dpt15_access() {
        let bytes = encode(DPT_ACCESS_DATA, 123_456.0).unwrap();
        assert_eq!(decode(DPT_ACCESS_DATA, &bytes).unwrap(), 123_456.0);
    }

    #[test]
    fn dpt16_string() {
        let bytes = encode_string(DPT_STRING_ASCII, "Hello").unwrap();
        assert_eq!(bytes.len(), 14);
        assert_eq!(decode_string(DPT_STRING_ASCII, &bytes).unwrap(), "Hello");
    }

    #[test]
    fn dpt16_string_truncation() {
        let long = "This is longer than fourteen chars";
        let bytes = encode_string(DPT_STRING_ASCII, long).unwrap();
        assert_eq!(bytes.len(), 14);
        assert_eq!(
            decode_string(DPT_STRING_ASCII, &bytes).unwrap(),
            "This is longer"
        );
    }

    #[test]
    fn dpt17_scene() {
        assert_eq!(encode(DPT_SCENE_NUMBER, 5.0).unwrap(), &[5]);
        assert_eq!(decode(DPT_SCENE_NUMBER, &[5]).unwrap(), 5.0);
        assert_eq!(decode(DPT_SCENE_NUMBER, &[0x7F]).unwrap(), 63.0); // masked to 6 bits
    }

    #[test]
    fn dpt18_scene_control() {
        // Activate scene 5 = 0x05, Learn scene 5 = 0x85
        assert_eq!(encode(DPT_SCENE_CONTROL, 5.0).unwrap(), &[5]);
        assert_eq!(encode(DPT_SCENE_CONTROL, 133.0).unwrap(), &[133]); // 0x80 | 5
    }

    #[test]
    fn dpt29_signed64() {
        let bytes = encode(DPT_ACTIVE_ENERGY_V64, -1_000_000.0).unwrap();
        assert_eq!(bytes.len(), 8);
        assert_eq!(decode(DPT_ACTIVE_ENERGY_V64, &bytes).unwrap(), -1_000_000.0);
    }

    #[test]
    fn dpt232_rgb() {
        // Red = 0xFF0000
        let bytes = encode(DPT_COLOUR_RGB, f64::from(0x00FF_0000u32)).unwrap();
        assert_eq!(bytes, &[0xFF, 0x00, 0x00]);
        assert_eq!(
            decode(DPT_COLOUR_RGB, &bytes).unwrap(),
            f64::from(0x00FF_0000u32)
        );
    }

    #[test]
    fn dpt251_rgbw() {
        let rgbw = f64::from(0xFF80_4020u32);
        let bytes = encode(DPT_COLOUR_RGBW, rgbw).unwrap();
        assert_eq!(bytes, &[0xFF, 0x80, 0x40, 0x20]);
        assert_eq!(decode(DPT_COLOUR_RGBW, &bytes).unwrap(), rgbw);
    }

    #[test]
    fn dpt28_unicode() {
        let bytes = encode_string(Dpt::new(28, 1), "Héllo 🌍").unwrap();
        assert_eq!(decode_string(Dpt::new(28, 1), &bytes).unwrap(), "Héllo 🌍");
    }

    #[test]
    fn dptvalue_roundtrip_numeric() {
        let val = DptValue::Numeric(21.5);
        let bytes = encode_value(DPT_VALUE_TEMP, &val).unwrap();
        let decoded = decode_value(DPT_VALUE_TEMP, &bytes).unwrap();
        assert!((decoded.as_f64().unwrap() - 21.5).abs() < 0.1);
    }

    #[test]
    fn dptvalue_roundtrip_string() {
        let val = DptValue::Text("Test".into());
        let bytes = encode_value(DPT_STRING_ASCII, &val).unwrap();
        let decoded = decode_value(DPT_STRING_ASCII, &bytes).unwrap();
        assert_eq!(decoded.as_str().unwrap(), "Test");
    }

    #[test]
    fn unsupported_dpt() {
        assert!(matches!(
            decode(Dpt::new(999, 1), &[0]),
            Err(DptError::UnsupportedDpt(_))
        ));
    }

    #[test]
    fn dpt26_scene_info() {
        let dpt = Dpt::new(26, 1);
        assert_eq!(encode(dpt, 5.0).unwrap(), &[5]);
        assert_eq!(decode(dpt, &[0x45]).unwrap(), 5.0); // masked to 6 bits
    }

    #[test]
    fn dpt217_version() {
        let dpt = Dpt::new(217, 1);
        // Version 5.3.12 = 50312
        let bytes = encode(dpt, 50312.0).unwrap();
        assert_eq!(bytes.len(), 2);
        let val = decode(dpt, &bytes).unwrap();
        assert_eq!(val, 50312.0);
    }

    #[test]
    fn dpt219_alarm_info() {
        let dpt = Dpt::new(219, 1);
        let bytes = encode(dpt, 42.0).unwrap();
        assert_eq!(bytes.len(), 6);
        assert_eq!(decode(dpt, &bytes).unwrap(), 42.0);
    }

    #[test]
    fn dpt221_serial_number() {
        let dpt = Dpt::new(221, 1);
        let bytes = encode(dpt, 1234.0).unwrap();
        assert_eq!(bytes.len(), 6);
        assert_eq!(decode(dpt, &bytes).unwrap(), 1234.0);
    }

    #[test]
    fn dpt225_scaling_speed() {
        let dpt = Dpt::new(225, 1);
        let bytes = encode(dpt, 5000.0).unwrap();
        assert_eq!(bytes.len(), 3);
        assert_eq!(decode(dpt, &bytes).unwrap(), 5000.0);
    }

    #[test]
    fn dpt231_locale() {
        let dpt = Dpt::new(231, 1);
        // "DE" = 0x4445
        let bytes = encode(dpt, f64::from(0x4445u16)).unwrap();
        assert_eq!(bytes.len(), 4);
        assert_eq!(decode(dpt, &bytes).unwrap(), f64::from(0x4445u16));
    }

    #[test]
    fn dpt234_language() {
        let dpt = Dpt::new(234, 1);
        let bytes = encode(dpt, f64::from(0x4445u16)).unwrap();
        assert_eq!(bytes.len(), 2);
        assert_eq!(decode(dpt, &bytes).unwrap(), f64::from(0x4445u16));
    }

    #[test]
    fn dpt235_active_energy() {
        let dpt = Dpt::new(235, 1);
        let bytes = encode(dpt, -50000.0).unwrap();
        assert_eq!(bytes.len(), 6);
        assert_eq!(decode(dpt, &bytes).unwrap(), -50000.0);
    }

    #[test]
    fn dpt238_scene_config() {
        let dpt = Dpt::new(238, 1);
        assert_eq!(encode(dpt, 10.0).unwrap(), &[10]);
        assert_eq!(decode(dpt, &[10]).unwrap(), 10.0);
    }

    #[test]
    fn dpt239_flagged_scaling() {
        let dpt = Dpt::new(239, 1);
        let bytes = encode(dpt, 50.0).unwrap();
        assert_eq!(bytes.len(), 2);
        let val = decode(dpt, &bytes).unwrap();
        assert!((val - 50.0).abs() < 1.0, "got {val}");
    }

    #[test]
    fn payload_too_short() {
        assert!(decode(DPT_VALUE_TEMP, &[0x0C]).is_err());
        assert!(decode(DPT_VALUE_POWER, &[0, 0]).is_err());
    }
}
