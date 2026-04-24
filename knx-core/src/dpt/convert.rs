// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! DPT encode/decode dispatch — delegates to per-group modules.

mod numeric;
mod string;

use alloc::vec::Vec;

use super::{Dpt, DptError, DptValue};

pub(super) fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    match dpt.main {
        16 | 28 => string::decode(dpt, payload),
        _ => numeric::decode(dpt, payload),
    }
}

pub(super) fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    match dpt.main {
        16 | 28 => string::encode(dpt, value),
        _ => numeric::encode(dpt, value),
    }
}
