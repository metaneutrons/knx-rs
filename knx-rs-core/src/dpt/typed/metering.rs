// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Metering: the Z8 status octet, DPT 229.001 and DPT 235.001.

use alloc::vec::Vec;

use super::{TypedDpt, bit, ensure, flag, octets};
use crate::dpt::{Dpt, DptError};

/// The status octet Z8, coded as DPT 21.001.
///
/// In a response or an info report the octet is a status, in a write it is a command. The five
/// flags are bits 0 to 4, bits 5 to 7 are reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct Z8Status {
    /// Bit 0: out of service.
    pub out_of_service: bool,
    /// Bit 1: fault.
    pub fault: bool,
    /// Bit 2: overridden.
    pub overridden: bool,
    /// Bit 3: in alarm.
    pub in_alarm: bool,
    /// Bit 4: alarm not acknowledged.
    pub alarm_unacknowledged: bool,
}

impl Z8Status {
    /// The flags from an octet. The reserved bits are not read.
    #[must_use]
    pub const fn from_octet(octet: u8) -> Self {
        Self {
            out_of_service: flag(octet, 0),
            fault: flag(octet, 1),
            overridden: flag(octet, 2),
            in_alarm: flag(octet, 3),
            alarm_unacknowledged: flag(octet, 4),
        }
    }

    /// The octet, with the reserved bits 0.
    #[must_use]
    pub const fn to_octet(self) -> u8 {
        bit(self.out_of_service, 0)
            | bit(self.fault, 1)
            | bit(self.overridden, 2)
            | bit(self.in_alarm, 3)
            | bit(self.alarm_unacknowledged, 4)
    }
}

/// DPT 229.001, a metering value.
///
/// Six octets: a signed 32-bit count, the `ValInfField` octet that gives the unit and the
/// exponent of the count, and the status octet Z8.
///
/// `value_info_field` stays the raw octet. The ETS master data lists its meaning for the codes 0
/// to 177 (for example 3 is energy in units of 1 Wh, 19 is volume in units of 1 l); the field is
/// a whole octet, not only the codes 0 to 127 of the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeteringValue {
    /// The count.
    pub count: i32,
    /// The `ValInfField` octet.
    pub value_info_field: u8,
    /// The status (response, info report) or command (write) octet.
    pub status: Z8Status,
}

impl TypedDpt for MeteringValue {
    const DPT: Dpt = Dpt::new(229, 1);
    const LEN: usize = 6;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [c0, c1, c2, c3, value_info_field, status] = octets::<6>(payload)?;
        Ok(Self {
            count: i32::from_be_bytes([c0, c1, c2, c3]),
            value_info_field,
            status: Z8Status::from_octet(status),
        })
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&self.count.to_be_bytes());
        out.push(self.value_info_field);
        out.push(self.status.to_octet());
        Ok(out)
    }
}

/// DPT 235.001, active energy with a tariff.
///
/// Six octets: the energy in Wh as a signed 32-bit value (as DPT 13.010), the tariff (as DPT
/// 5.006) and two validity bits. The validity bits are stored the other way round on the wire (a
/// set bit means not valid); the fields here are `true` when the data is valid. The tariff is
/// range-checked (0 to 254) only while `tariff_valid` is set on decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TariffActiveEnergy {
    /// Active energy in Wh.
    pub energy: i32,
    /// Tariff, 0 to 254.
    pub tariff: u8,
    /// Bit 1 is clear: `energy` is valid.
    pub energy_valid: bool,
    /// Bit 0 is clear: `tariff` is valid.
    pub tariff_valid: bool,
}

impl TypedDpt for TariffActiveEnergy {
    const DPT: Dpt = Dpt::new(235, 1);
    const LEN: usize = 6;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [e0, e1, e2, e3, tariff, validity] = octets::<6>(payload)?;
        let value = Self {
            energy: i32::from_be_bytes([e0, e1, e2, e3]),
            tariff,
            energy_valid: !flag(validity, 1),
            tariff_valid: !flag(validity, 0),
        };
        if value.tariff_valid {
            ensure(tariff <= 254, "tariff outside 0..=254")?;
        }
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        ensure(self.tariff <= 254, "tariff outside 0..=254")?;
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&self.energy.to_be_bytes());
        out.push(self.tariff);
        out.push(bit(!self.energy_valid, 1) | bit(!self.tariff_valid, 0));
        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::tests::round_trip;
    use super::*;

    #[test]
    fn metering_value_carries_count_unit_and_status() {
        let value = MeteringValue {
            count: -1000,
            value_info_field: 3,
            status: Z8Status {
                fault: true,
                alarm_unacknowledged: true,
                ..Z8Status::default()
            },
        };
        round_trip(&[0xFF, 0xFF, 0xFC, 0x18, 3, 0b0001_0010], &value);
        // Codes above 127 are valid, the field is a whole octet.
        let high = MeteringValue {
            value_info_field: 177,
            ..value
        };
        round_trip(&[0xFF, 0xFF, 0xFC, 0x18, 177, 0b0001_0010], &high);
    }

    #[test]
    fn z8_status_has_five_flags_and_reserved_bits() {
        for n in 0..5u8 {
            let status = Z8Status::from_octet(1 << n);
            assert_eq!(status.to_octet(), 1 << n);
        }
        assert_eq!(Z8Status::from_octet(0xE0), Z8Status::default());
        assert_eq!(Z8Status::from_octet(0xFF).to_octet(), 0x1F);
        assert!(Z8Status::from_octet(0x01).out_of_service);
        assert!(Z8Status::from_octet(0x02).fault);
        assert!(Z8Status::from_octet(0x04).overridden);
        assert!(Z8Status::from_octet(0x08).in_alarm);
        assert!(Z8Status::from_octet(0x10).alarm_unacknowledged);
    }

    #[test]
    fn tariff_active_energy_stores_validity_inverted() {
        let valid = TariffActiveEnergy {
            energy: 123_456,
            tariff: 3,
            energy_valid: true,
            tariff_valid: true,
        };
        round_trip(&[0x00, 0x01, 0xE2, 0x40, 3, 0x00], &valid);
        round_trip(
            &[0x00, 0x01, 0xE2, 0x40, 3, 0x02],
            &TariffActiveEnergy {
                energy_valid: false,
                ..valid
            },
        );
        round_trip(
            &[0x00, 0x01, 0xE2, 0x40, 3, 0x01],
            &TariffActiveEnergy {
                tariff_valid: false,
                ..valid
            },
        );
        round_trip(
            &[0xFF, 0xFF, 0xFF, 0xFE, 254, 0x00],
            &TariffActiveEnergy {
                energy: -2,
                tariff: 254,
                ..valid
            },
        );
        assert!(TariffActiveEnergy::decode(&[0, 0, 0, 0, 255, 0x00]).is_err());
        // An invalid tariff carries nothing.
        assert!(TariffActiveEnergy::decode(&[0, 0, 0, 0, 255, 0x01]).is_ok());
        assert!(
            TariffActiveEnergy::encode(&TariffActiveEnergy {
                tariff: 255,
                ..valid
            })
            .is_err()
        );
        // Bits 2 to 7 are reserved.
        assert_eq!(
            TariffActiveEnergy::decode(&[0x00, 0x01, 0xE2, 0x40, 3, 0xFC]).unwrap(),
            valid
        );
    }
}
