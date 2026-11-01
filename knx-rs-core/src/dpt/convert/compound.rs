// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Multi-field DPTs, carried as the exact octets of the wire format.
//!
//! These main groups combine several fields (numbers, enumerations, bit sets, status octets,
//! date and time, tariff registers, and so on) in one value. They decode to [`DptValue::Bytes`] holding
//! exactly the octets of the format, in transmission order (the octet with the highest number in
//! KNX Standard `03_07_02` first), and encode from the same.
//!
//! Policy:
//!
//! - The octets travel unchanged, so no field and no reserved bit is lost.
//! - Decoding takes the first N octets of the payload and ignores the rest.
//! - Encoding requires exactly N octets, never shortens or pads.
//! - Reserved bits and field ranges are not interpreted: the standard defines no receiver
//!   behaviour for them, and a stack that sits between devices must not alter them.
//!
//! Open points in the standard
//!
//! Version 02.02.01 of `03_07_02` is ambiguous or inconsistent in a few places. These groups are
//! carried as octets precisely so that none of this is interpreted here:
//!
//! - 19.001 (and every group that starts with it): the SRC bit is in the figure and the field
//!   table, but note 15 still calls the lower seven bits of the first octet reserved.
//! - 243.600: the figure orders the axes y, x; the field table and 242.600 order them x, y.
//! - 245.600: the format string is incomplete; the six octets follow from the figure.
//! - 247.600: the bit after the date and time is not named. 248.600: `RatedDur` is eight bits but
//!   refers to a 16-bit type (7.006), and no scaling is given.
//! - 207.100 and 207.105: the bit diagrams disagree with the attribute tables.
//! - 237.600: the table and the sentence below it give opposite meanings to the address bit.
//! - 229.001: the value information field is "N8, 00h to 7Fh" but its codes run from 80h.
//! - 230.1000: the medium enumeration is in another document.
//! - 206.105: the header says N8U16, the layout says U16N8 (the layout is the wire order).
//! - 273.004 refers to 9.007 (relative humidity) where 9.029 (absolute humidity) is meant.
//! - 277 to 284: the extensions for energy registers are not defined in this document (unit of
//!   reactive energy, tariff of registers without one, padding of the 12-octet string).
//! - The polarity of validity bits differs between groups: 1 means "not valid" in 235, 269 and 277
//!   to 284, and "valid" in 249, 251, 273 and 274.
//! - The chapter headings of 240.800 and 249.600 give octet counts (2 and 4) that differ from their
//!   formats (3 and 6); the formats are followed.
//! - Z8 (groups 200 to 205): the last octet means status in a response and command in a write,
//!   and the standard says these types do not apply to ordinary group objects. The octet is
//!   carried without a decision about which it is.

use alloc::vec::Vec;

use super::super::{Dpt, DptError, DptValue};

/// The number of octets of a fixed-length multi-field main group, `None` for other groups.
///
/// Grouped by length. The groups are: date (11) and date and time (19); version, alarm, serial
/// number, time period with percent, locale, language, tariff and flagged scaling (217 to 239);
/// value with a status/command octet (200 to 205, 218, 229) or with mode octets (206 to 224, 240 to
/// 246, 248, 252); colour, lighting, shutters and blinds (247 to 257); metering and the groups that
/// start with a date and time (265 to 284).
pub(super) const fn len(main: u16) -> Option<usize> {
    Some(match main {
        200..=202 | 204 | 207 | 211 | 217 | 234 | 237 | 239 | 244 | 246 => 2,
        11 | 203 | 205 | 206 | 209 | 223 | 225 | 240 | 250 | 254 => 3,
        210 | 214 | 220 | 231 | 241 | 253 | 276 => 4,
        215 | 216 | 218 | 248 | 252 => 5,
        212 | 219 | 221 | 222 | 224 | 229 | 235 | 242 | 245 | 249 | 251 | 274 => 6,
        271 | 272 => 7,
        19 | 213 | 230 | 243 | 255 | 273 => 8,
        265 | 268 => 9,
        247 | 257 | 266 => 12,
        269 => 14,
        256 => 16,
        270 => 20,
        277 => 36,
        278 => 42,
        281 => 44,
        279 => 48,
        282 => 50,
        283 => 56,
        280 => 78,
        284 => 86,
        _ => return None,
    })
}

/// Whether `main` is a multi-field group this module carries, fixed or variable length.
pub(super) const fn handles(main: u16) -> bool {
    len(main).is_some() || main == DPT267
}

/// DPT 267 is a date and time (8 octets) followed by a UTF-8 string and its terminating NUL.
const DPT267: u16 = 267;

/// Octets of the date and time header of DPT 267.
const DPT267_HEADER: usize = 8;

pub(super) fn decode(dpt: Dpt, payload: &[u8]) -> Result<DptValue, DptError> {
    if dpt.main == DPT267 {
        // Header, at least the NUL of an empty string.
        if payload.len() <= DPT267_HEADER {
            return Err(DptError::PayloadTooShort);
        }
        return Ok(DptValue::Bytes(payload.to_vec()));
    }
    let n = len(dpt.main).ok_or(DptError::UnsupportedDpt(dpt))?;
    if payload.len() < n {
        return Err(DptError::PayloadTooShort);
    }
    Ok(DptValue::Bytes(payload[..n].to_vec()))
}

pub(super) fn encode(dpt: Dpt, value: &DptValue) -> Result<Vec<u8>, DptError> {
    let DptValue::Bytes(bytes) = value else {
        return Err(DptError::TypeMismatch);
    };
    if dpt.main == DPT267 {
        if bytes.len() <= DPT267_HEADER {
            return Err(DptError::out_of_range(
                "DPT 267 needs a date and time and a terminated string",
            ));
        }
        if bytes.last() != Some(&0) {
            return Err(DptError::out_of_range(
                "DPT 267 string must end with a NUL octet",
            ));
        }
        return Ok(bytes.clone());
    }
    let n = len(dpt.main).ok_or(DptError::UnsupportedDpt(dpt))?;
    if bytes.len() != n {
        return Err(DptError::out_of_range(
            "value must have exactly the octets of the DPT",
        ));
    }
    Ok(bytes.clone())
}
