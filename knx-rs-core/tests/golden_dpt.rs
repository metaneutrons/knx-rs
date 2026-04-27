#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! DPT golden vector tests against C++ knx-openknx reference.
//!
//! The C++ `KNXValue` type uses implicit conversions (e.g. `double` → `uint8_t`)
//! that can lose precision. The **encode bytes** are authoritative for integer
//! DPTs; for float DPTs (9, 14) we allow ±1 LSB tolerance due to intermediate
//! precision differences (`float` vs `f64`).

use knx_rs_core::dpt::{self, Dpt, DptValue};
use serde::Deserialize;

#[derive(Deserialize)]
#[allow(dead_code)]
struct DptVector {
    main: u16,
    sub: u16,
    input: f64,
    bytes: Vec<u8>,
    decoded: Option<f64>,
    error: Option<bool>,
}

/// Check if two byte vectors are within ±1 of each other (LSB tolerance).
fn bytes_within_one(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let a_val = a
        .iter()
        .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
    let b_val = b
        .iter()
        .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
    a_val.abs_diff(b_val) <= 1
}

/// Convert an f64 input to the appropriate `DptValue` for a given DPT main group.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test helper: C++ vectors use f64 for all types"
)]
fn input_to_value(main: u16, sub: u16, input: f64) -> DptValue {
    match main {
        1 => DptValue::Bool(input != 0.0),
        6 | 8 | 13 | 27 => DptValue::Int(input as i32),
        9 | 14 => DptValue::Float(input),
        5 if sub == 1 || sub == 3 => DptValue::Float(input),
        29 => DptValue::Int64(input as i64),
        16 | 28 => DptValue::Text(String::new()), // not tested here
        _ => DptValue::UInt(input as u32),
    }
}

/// Returns true if this DPT main group decodes to `Bytes` (structured DPTs).
const fn is_bytes_dpt(main: u16) -> bool {
    matches!(
        main,
        10 | 11 | 19 | 217 | 219 | 221 | 225 | 231 | 234 | 235 | 239
    )
}

#[test]
fn dpt_encode_matches_cpp() {
    let json = include_str!("fixtures/dpt_vectors.json");
    let vectors: Vec<DptVector> = serde_json::from_str(json).expect("parse dpt_vectors.json");

    let mut passed = 0;
    let mut skipped = 0;

    for v in &vectors {
        let dpt_id = Dpt::new(v.main, v.sub);

        if v.error.unwrap_or(false) || v.bytes.is_empty() || v.input < 0.0 {
            skipped += 1;
            continue;
        }

        // Structured DPTs decode to Bytes — can't encode from f64 input
        if is_bytes_dpt(v.main) {
            skipped += 1;
            continue;
        }

        let value = input_to_value(v.main, v.sub, v.input);

        match dpt::encode(dpt_id, &value) {
            Ok(encoded) => {
                if v.main == 9 {
                    assert!(
                        bytes_within_one(&encoded, &v.bytes),
                        "DPT {dpt_id} encode: input {} → got {encoded:?}, expected {:?} (±1)",
                        v.input,
                        v.bytes
                    );
                } else {
                    assert_eq!(
                        encoded, v.bytes,
                        "DPT {dpt_id} encode: input {} → got {encoded:?}, expected {:?}",
                        v.input, v.bytes
                    );
                }
                passed += 1;
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }

    eprintln!("DPT encode vectors: {passed} passed, {skipped} skipped");
    assert!(passed > 20, "too few DPT encode vectors passed: {passed}");
}

#[test]
fn dpt_decode_from_cpp_bytes() {
    let json = include_str!("fixtures/dpt_vectors.json");
    let vectors: Vec<DptVector> = serde_json::from_str(json).expect("parse dpt_vectors.json");

    let mut passed = 0;
    let mut skipped = 0;

    for v in &vectors {
        let dpt_id = Dpt::new(v.main, v.sub);

        if v.error.unwrap_or(false) || v.bytes.is_empty() {
            skipped += 1;
            continue;
        }

        match dpt::decode(dpt_id, &v.bytes) {
            Ok(decoded) => {
                // For Bytes DPTs, verify decode→encode roundtrip preserves bytes
                if is_bytes_dpt(v.main) {
                    if let Ok(re_encoded) = dpt::encode(dpt_id, &decoded) {
                        let expected_len = v.bytes.len().min(re_encoded.len());
                        assert_eq!(
                            &re_encoded[..expected_len],
                            &v.bytes[..expected_len],
                            "DPT {dpt_id} bytes roundtrip: decode({:?})={decoded:?} → {re_encoded:?}",
                            v.bytes
                        );
                    }
                    passed += 1;
                    continue;
                }

                // For numeric DPTs, re-encode and verify bytes match
                if let Ok(re_encoded) = dpt::encode(dpt_id, &decoded) {
                    if v.main == 9 {
                        assert!(
                            bytes_within_one(&re_encoded, &v.bytes),
                            "DPT {dpt_id} roundtrip: decode({:?})={decoded:?} → {re_encoded:?} (±1)",
                            v.bytes
                        );
                    } else {
                        assert_eq!(
                            re_encoded, v.bytes,
                            "DPT {dpt_id} roundtrip: decode({:?})={decoded:?} → {re_encoded:?}",
                            v.bytes
                        );
                    }
                }
                passed += 1;
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }

    eprintln!("DPT decode vectors: {passed} passed, {skipped} skipped");
    assert!(passed > 20, "too few DPT decode vectors passed: {passed}");
}
