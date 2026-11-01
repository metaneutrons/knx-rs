// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! String DPT encode/decode (DPT 16, 24, 28).

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{Dpt, DptError, DptValue};

pub fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        16 => decode_dpt16(dpt, payload).map(DptValue::Text),
        24 => Ok(DptValue::Text(decode_dpt24(payload))),
        28 => decode_dpt28(payload).map(DptValue::Text),
        _ => Err(DptError::TypeMismatch),
    }
}

pub fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    let s = value.as_str().ok_or(DptError::TypeMismatch)?;
    match dpt.main {
        16 => Ok(encode_dpt16(dpt, s)),
        24 => encode_dpt24(s),
        28 => encode_dpt28(s),
        _ => Err(DptError::TypeMismatch),
    }
}

// ── DPT 16: 14-byte string (ASCII or Latin-1) ────────────────
//
// 16.000 is ASCII (the most significant bit of every octet is 0), 16.001 is ISO 8859-1. Unused
// trailing octets are 0. Decoding stops at the first 0 octet.
//
// Encoding is lossy by design, for display text of any length (track titles, names): a text longer
// than 14 characters is cut, and a character the character set cannot represent (above U+007F in
// 16.000, above U+00FF in 16.001) becomes `?`. A NUL character in the text ends it.

fn decode_dpt16(dpt: Dpt, payload: &[u8]) -> Result<String, DptError> {
    if payload.len() < 14 {
        return Err(DptError::PayloadTooShort);
    }
    let end = payload[..14].iter().position(|&b| b == 0).unwrap_or(14);
    if dpt.sub == 0 && payload[..end].iter().any(|&b| b > 0x7F) {
        return Err(DptError::out_of_range("DPT 16.000 is ASCII (0..=127)"));
    }
    let s: String = payload[..end].iter().map(|&b| char::from(b)).collect();
    Ok(s)
}

fn encode_dpt16(dpt: Dpt, value: &str) -> Vec<u8> {
    let limit = if dpt.sub == 0 { 0x7F } else { 0xFF };
    let mut buf = alloc::vec![0u8; 14];
    for (i, ch) in value
        .chars()
        .take_while(|&c| c != '\0')
        .take(14)
        .enumerate()
    {
        buf[i] = if ch as u32 <= limit { ch as u8 } else { b'?' };
    }
    buf
}

// ── DPT 24: Variable-length ISO 8859-1 string ─────────────────
//
// `A[n]`: the characters followed by one 0 octet; the length comes from the frame. Characters
// above U+00FF cannot be represented and are an error, as is a NUL in the text (the receiver
// would cut the text there).

fn decode_dpt24(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    payload[..end].iter().map(|&b| char::from(b)).collect()
}

fn encode_dpt24(value: &str) -> Result<Vec<u8>, DptError> {
    let mut buf = Vec::with_capacity(value.len() + 1);
    for ch in value.chars() {
        if ch == '\0' {
            return Err(DptError::out_of_range("DPT 24 text contains a NUL"));
        }
        let code = u32::from(ch);
        if code > 0xFF {
            return Err(DptError::out_of_range(
                "DPT 24 is ISO 8859-1; character above U+00FF",
            ));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "checked to be at most 0xFF"
        )]
        buf.push(code as u8);
    }
    buf.push(0);
    Ok(buf)
}

// ── DPT 28: Variable-length UTF-8 string ──────────────────────
//
// `A[n]`: UTF-8 followed by one 0 octet; the length comes from the frame. Decoding is lenient
// about the terminator (a payload without one is taken whole, an empty payload is the empty
// string). Encoding rejects a NUL in the text, because the receiver would cut it there. The
// maximum length is the caller's concern: a longer text has to be cut at a character boundary
// before it is encoded.

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

fn encode_dpt28(value: &str) -> Result<Vec<u8>, DptError> {
    if value.contains('\0') {
        return Err(DptError::out_of_range("DPT 28 text contains a NUL"));
    }
    let mut buf = Vec::with_capacity(value.len() + 1);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0);
    Ok(buf)
}
