// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! DPT encode/decode dispatch — delegates to per-group modules.

mod numeric;
mod string;

use alloc::string::String;
use alloc::vec::Vec;

use super::{Dpt, DptError, DptValue};

pub(super) fn decode_value(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        16 | 28 => decode_string(dpt, payload).map(DptValue::Text),
        _ => decode(dpt, payload).map(DptValue::Numeric),
    }
}

pub(super) fn encode_value(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    match value {
        DptValue::Numeric(v) => encode(dpt, *v),
        DptValue::Text(s) => encode_string(dpt, s),
    }
}

pub(super) fn decode(dpt: Dpt, payload: &[u8]) -> Result<f64, DptError> {
    numeric::decode(dpt, payload)
}

pub(super) fn encode(dpt: Dpt, value: f64) -> Result<Vec<u8>, DptError> {
    numeric::encode(dpt, value)
}

pub(super) fn decode_string(dpt: Dpt, payload: &[u8]) -> Result<String, DptError> {
    string::decode(dpt, payload)
}

pub(super) fn encode_string(dpt: Dpt, value: &str) -> Result<Vec<u8>, DptError> {
    string::encode(dpt, value)
}
