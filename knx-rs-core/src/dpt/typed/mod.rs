// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Typed access to multi-field datapoint types.
//!
//! [`decode`](super::decode) and [`encode`](super::encode) carry a multi-field DPT as the exact
//! octets of its format ([`DptValue::Bytes`]), which loses nothing but leaves the fields to the
//! caller. The structs of this module name the fields of the types whose layout is settled, and
//! convert to and from those octets through [`TypedDpt`].
//!
//! | Type | DPT |
//! |------|-----|
//! | [`Date`] | 11.001 |
//! | [`DateTime`] | 19.001 |
//! | [`StatusBuc`] | 207.100 |
//! | [`StatusAct`] | 207.105 |
//! | [`StatusLightingActuator`] | 207.600 |
//! | [`MeteringValue`] with [`Z8Status`] | 229.001 |
//! | [`TariffActiveEnergy`] | 235.001 |
//! | [`Rgbw`] | 251.600 |
//!
//! Policy, in line with the codec:
//!
//! - Decoding is lenient about the payload: octets beyond the length of the DPT are ignored and
//!   reserved bits are not read. It is strict about a field that holds a value outside its
//!   range, because the standard has the receiver ignore such a message (the range of a field is
//!   not checked when a validity flag says the field carries nothing).
//! - Encoding is strict: a field outside its range is an [`DptError::OutOfRange`], never clamped.
//!   Reserved bits are written as 0.
//! - A struct does not keep reserved bits. A caller that must forward a value unchanged uses the
//!   [`DptValue::Bytes`] of the codec.
//!
//! Sources: KNX Standard `03_07_02` v02.02.01 and the ETS master data (`knx_master.xml`, schema
//! 23, version 313). Where they differ, the choice is the one of the codec (see its module
//! documentation). For 207.100 and 207.105 the bit diagrams of the standard disagree with the
//! attribute tables; the tables are followed, see [`StatusBuc`] and [`StatusAct`] for the
//! details and the reasons. The ETS master data does not list these types.
//!
//! Not covered: types whose layout the standard leaves open (the Z8 groups 200 to 205 and the
//! energy registers 277 to 284), and the other subtypes of the groups above, which would need
//! their own field lists.

use alloc::vec::Vec;

use super::{Dpt, DptError, DptValue};

mod colour;
mod datetime;
mod metering;
mod status;

pub use colour::Rgbw;
pub use datetime::{Date, DateTime};
pub use metering::{MeteringValue, TariffActiveEnergy, Z8Status};
pub use status::{StatusAct, StatusBuc, StatusLightingActuator};

/// A datapoint type with named fields, convertible to and from the octets of its format.
pub trait TypedDpt: Sized {
    /// The datapoint type the struct models.
    const DPT: Dpt;

    /// The number of octets of the format.
    const LEN: usize;

    /// Read the fields from the first [`LEN`](Self::LEN) octets of `payload`.
    ///
    /// # Errors
    ///
    /// [`DptError::PayloadTooShort`] if there are fewer octets, [`DptError::OutOfRange`] if a
    /// field holds a value outside its range.
    fn decode(payload: &[u8]) -> Result<Self, DptError>;

    /// Write the fields as exactly [`LEN`](Self::LEN) octets.
    ///
    /// # Errors
    ///
    /// [`DptError::OutOfRange`] if a field is outside its range.
    fn encode(&self) -> Result<Vec<u8>, DptError>;

    /// Read the fields from the [`DptValue::Bytes`] the codec returns for the DPT.
    ///
    /// # Errors
    ///
    /// [`DptError::TypeMismatch`] for any other variant, [`DptError::OutOfRange`] if the length
    /// is not [`LEN`](Self::LEN) or a field is outside its range.
    fn from_value(value: &DptValue) -> Result<Self, DptError> {
        let DptValue::Bytes(bytes) = value else {
            return Err(DptError::TypeMismatch);
        };
        if bytes.len() != Self::LEN {
            return Err(DptError::out_of_range(
                "value must have exactly the octets of the DPT",
            ));
        }
        Self::decode(bytes)
    }

    /// Write the fields as the [`DptValue::Bytes`] the codec accepts for the DPT.
    ///
    /// # Errors
    ///
    /// [`DptError::OutOfRange`] if a field is outside its range.
    fn to_value(&self) -> Result<DptValue, DptError> {
        self.encode().map(DptValue::Bytes)
    }
}

/// The first `N` octets of `payload`.
fn octets<const N: usize>(payload: &[u8]) -> Result<[u8; N], DptError> {
    payload
        .get(..N)
        .and_then(|s| <[u8; N]>::try_from(s).ok())
        .ok_or(DptError::PayloadTooShort)
}

/// Whether bit `n` of `octet` is set.
const fn flag(octet: u8, n: u8) -> bool {
    (octet >> n) & 1 == 1
}

/// Bit `n` as a mask, or 0.
const fn bit(set: bool, n: u8) -> u8 {
    if set { 1 << n } else { 0 }
}

/// An error unless `ok`.
const fn ensure(ok: bool, context: &'static str) -> Result<(), DptError> {
    if ok {
        Ok(())
    } else {
        Err(DptError::out_of_range(context))
    }
}

/// A percentage as the raw 0 to 255 of a scaling value (0.4 % resolution).
fn percent_to_raw(percent: f64) -> Result<u8, DptError> {
    ensure(
        percent.is_finite() && (0.0..=100.0).contains(&percent),
        "percentage outside 0..=100",
    )?;
    let raw = libm::round(percent * 255.0 / 100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(raw as u8)
}

/// The percentage of a raw 0 to 255 scaling value.
fn raw_to_percent(raw: u8) -> f64 {
    f64::from(raw) * 100.0 / 255.0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use core::fmt::Debug;

    use super::super::{DptWireSize, decode, encode};
    use super::*;

    /// The octets decode to `expected`, which encodes back to them, through the codec as well.
    pub(super) fn round_trip<T: TypedDpt + PartialEq + Debug>(octets: &[u8], expected: &T) {
        assert_eq!(&T::decode(octets).unwrap(), expected);
        assert_eq!(T::encode(expected).unwrap(), octets);
        assert_eq!(
            T::DPT.wire_size(),
            DptWireSize::Bytes(u8::try_from(T::LEN).unwrap())
        );
        let value = decode(T::DPT, octets).unwrap();
        assert_eq!(&T::from_value(&value).unwrap(), expected);
        assert_eq!(
            encode(T::DPT, &T::to_value(expected).unwrap()).unwrap(),
            octets
        );
    }

    #[test]
    fn from_value_wants_the_octets_of_the_format() {
        assert_eq!(
            StatusAct::from_value(&DptValue::UInt(1)),
            Err(DptError::TypeMismatch)
        );
        assert!(StatusAct::from_value(&DptValue::Bytes(alloc::vec![1, 2, 3])).is_err());
        assert!(StatusAct::from_value(&DptValue::Bytes(alloc::vec![1])).is_err());
        assert_eq!(StatusAct::decode(&[1]), Err(DptError::PayloadTooShort));
    }
}
