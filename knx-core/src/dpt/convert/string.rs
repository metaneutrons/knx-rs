// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! String DPT encode/decode (DPT 16, 28).

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{Dpt, DptError, DptValue};

pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        16 => decode_dpt16(payload).map(DptValue::Text),
        28 => decode_dpt28(payload).map(DptValue::Text),
        _ => Err(DptError::TypeMismatch),
    }
}

pub fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    let s = value.as_str().ok_or(DptError::TypeMismatch)?;
    match dpt.main {
        16 => Ok(encode_dpt16(s)),
        28 => Ok(encode_dpt28(s)),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 16: 14-byte string (ASCII or Latin-1) ────────────────

fn decode_dpt16(payload: &[u8]) -> Result<String, DptError> {
    if payload.len() < 14 {
        return Err(DptError::PayloadTooShort);
    }
    let end = payload[..14].iter().position(|&b| b == 0).unwrap_or(14);
    let s: String = payload[..end].iter().map(|&b| char::from(b)).collect();
    Ok(s)
}

fn encode_dpt16(value: &str) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 14];
    for (i, ch) in value.chars().take(14).enumerate() {
        buf[i] = if ch as u32 <= 0xFF { ch as u8 } else { b'?' };
    }
    buf
}

// ── DPT 28: Variable-length UTF-8 string ──────────────────────

fn decode_dpt28(payload: &[u8]) -> Result<String, DptError> {
    if payload.is_empty() {
        return Ok(String::new());
    }
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    core::str::from_utf8(&payload[..end])
        .map(String::from)
        .map_err(|_| DptError::out_of_range("invalid UTF-8 in DPT 28 payload"))
}

fn encode_dpt28(value: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(value.len() + 1);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0);
    buf
}
