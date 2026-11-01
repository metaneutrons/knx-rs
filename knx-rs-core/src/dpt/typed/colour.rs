// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! DPT 251.600, a colour with a white channel.

use alloc::vec::Vec;

use super::{TypedDpt, bit, flag, octets};
use crate::dpt::{Dpt, DptError};

/// DPT 251.600, a colour with a white channel.
///
/// Six octets: red, green, blue and white as scaling values (octet 0 to 255 for 0 to 100 %), a
/// reserved octet, and four bits that say which of the four values are valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct Rgbw {
    /// Red level, raw octet.
    pub red: u8,
    /// Green level, raw octet.
    pub green: u8,
    /// Blue level, raw octet.
    pub blue: u8,
    /// White level, raw octet.
    pub white: u8,
    /// Bit 3: `red` is valid.
    pub red_valid: bool,
    /// Bit 2: `green` is valid.
    pub green_valid: bool,
    /// Bit 1: `blue` is valid.
    pub blue_valid: bool,
    /// Bit 0: `white` is valid.
    pub white_valid: bool,
}

impl TypedDpt for Rgbw {
    const DPT: Dpt = Dpt::new(251, 600);
    const LEN: usize = 6;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [red, green, blue, white, _reserved, valid] = octets::<6>(payload)?;
        Ok(Self {
            red,
            green,
            blue,
            white,
            red_valid: flag(valid, 3),
            green_valid: flag(valid, 2),
            blue_valid: flag(valid, 1),
            white_valid: flag(valid, 0),
        })
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        let valid = bit(self.red_valid, 3)
            | bit(self.green_valid, 2)
            | bit(self.blue_valid, 1)
            | bit(self.white_valid, 0);
        Ok(alloc::vec![
            self.red, self.green, self.blue, self.white, 0, valid
        ])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::tests::round_trip;
    use super::*;

    #[test]
    fn rgbw_has_one_validity_bit_per_colour() {
        let rgbw = Rgbw {
            red: 0xFF,
            green: 0x80,
            blue: 0x40,
            white: 0x20,
            red_valid: true,
            green_valid: true,
            blue_valid: true,
            white_valid: true,
        };
        round_trip(&[0xFF, 0x80, 0x40, 0x20, 0x00, 0x0F], &rgbw);
        round_trip(
            &[0xFF, 0x80, 0x40, 0x20, 0x00, 0x08],
            &Rgbw {
                green_valid: false,
                blue_valid: false,
                white_valid: false,
                ..rgbw
            },
        );
        round_trip(
            &[0xFF, 0x80, 0x40, 0x20, 0x00, 0x01],
            &Rgbw {
                red_valid: false,
                green_valid: false,
                blue_valid: false,
                ..rgbw
            },
        );
        // The reserved octet and the reserved high nibble are not read.
        assert_eq!(
            Rgbw::decode(&[0xFF, 0x80, 0x40, 0x20, 0xAA, 0xFF]).unwrap(),
            rgbw
        );
    }
}
