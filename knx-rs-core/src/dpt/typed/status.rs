// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! The status types of DPT 207: burner controller, actuator position, lighting actuator.

use alloc::vec::Vec;

use super::{TypedDpt, bit, ensure, flag, octets, percent_to_raw, raw_to_percent};
use crate::dpt::{Dpt, DptError};

/// DPT 207.100, status of a burner controller.
///
/// Two octets: the actual relative power in percent and four attribute bits. The power is
/// range-checked (0 to 100) only while `relative_power_valid` is set on decoding.
///
/// # Contradiction in the standard
///
/// KNX Standard 03.07.02 (Interworking, Datapoint Types) v02.02.01, section 4.13.1, page 148
/// (<https://support.knx.org/hc/en-us/articles/15392604906514-Interworking-Datapoint-types>)
/// draws the attribute octet as `00BBBBBB`, that is, six usable bits. The attribute table of
/// the same section defines bits 0 to 3 (`PrelBurnerValid`, `Fault`, `StatusStage1`,
/// `StatusStage2`) and calls bits 4 to 7 reserved, default 0. The two cannot both hold, and
/// bits 4 and 5 are in neither reading a defined flag.
///
/// This type follows the table: it names exactly the four attributes the table defines and
/// writes the other bits as 0. Reasons: only the table gives the attributes a name and a
/// meaning, a diagram of fewer reserved bits does not add any, and xknx implements the same
/// four bits. The ETS master data (schema 23, version 313) does not list 207.100, so there is
/// no third source. A receiver of a frame with bits 4 to 7 set ignores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct StatusBuc {
    /// Actual relative power in percent, 0 to 100 (1 % resolution).
    pub relative_power: u8,
    /// Bit 0: `relative_power` is valid.
    pub relative_power_valid: bool,
    /// Bit 1: burner failure.
    pub fault: bool,
    /// Bit 2: stage 1 or base stage active.
    pub stage1_active: bool,
    /// Bit 3: stage 2 or modulation active.
    pub stage2_active: bool,
}

impl TypedDpt for StatusBuc {
    const DPT: Dpt = Dpt::new(207, 100);
    const LEN: usize = 2;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [power, attributes] = octets::<2>(payload)?;
        // The attribute table (bits 0 to 3), not the `00BBBBBB` diagram; see the type docs.
        let value = Self {
            relative_power: power,
            relative_power_valid: flag(attributes, 0),
            fault: flag(attributes, 1),
            stage1_active: flag(attributes, 2),
            stage2_active: flag(attributes, 3),
        };
        if value.relative_power_valid {
            ensure(power <= 100, "relative power outside 0..=100")?;
        }
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        ensure(self.relative_power <= 100, "relative power outside 0..=100")?;
        let attributes = bit(self.relative_power_valid, 0)
            | bit(self.fault, 1)
            | bit(self.stage1_active, 2)
            | bit(self.stage2_active, 3);
        Ok(alloc::vec![self.relative_power, attributes])
    }
}

/// DPT 207.105, status of an actuator position.
///
/// Two octets: the actual position in percent and five attribute bits. There is no validity bit,
/// the position is always range-checked (0 to 100).
///
/// # Contradiction in the standard
///
/// KNX Standard 03.07.02 (Interworking, Datapoint Types) v02.02.01, section 4.13.5, page 151
/// (<https://support.knx.org/hc/en-us/articles/15392604906514-Interworking-Datapoint-types>)
/// draws the attribute octet as `0000BBBB`, that is, four usable bits. The attribute table of
/// the same section defines five: bits 0 to 3 (`Failure`, `ManualOverride`, `CalibrationMode`,
/// `ValveKick`) and bit 4 (`SynchronizationMode`), and calls only bits 5 to 7 reserved.
///
/// This type follows the table, so `synchronization_mode` (bit 4) is read and written. Reasons:
/// following the diagram would drop a flag the table names and explains, and a device that
/// reports a stroke-model synchronisation would then look idle. xknx also implements five bits.
/// The ETS master data (schema 23, version 313) does not list 207.105, so there is no third
/// source. A receiver of a frame with bits 5 to 7 set ignores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct StatusAct {
    /// Actual actuator position in percent, 0 to 100 (1 % resolution).
    pub position: u8,
    /// Bit 0: the actuator has a failure.
    pub failure: bool,
    /// Bit 1: the position is manually overridden.
    pub manual_override: bool,
    /// Bit 2: the actuator is in calibration mode.
    pub calibration_mode: bool,
    /// Bit 3: the actuator is executing a valve kick.
    pub valve_kick: bool,
    /// Bit 4: the actuator is synchronising its stroke model.
    pub synchronization_mode: bool,
}

impl TypedDpt for StatusAct {
    const DPT: Dpt = Dpt::new(207, 105);
    const LEN: usize = 2;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [position, attributes] = octets::<2>(payload)?;
        // The attribute table (bits 0 to 4), not the `0000BBBB` diagram; see the type docs.
        ensure(position <= 100, "position outside 0..=100")?;
        Ok(Self {
            position,
            failure: flag(attributes, 0),
            manual_override: flag(attributes, 1),
            calibration_mode: flag(attributes, 2),
            valve_kick: flag(attributes, 3),
            synchronization_mode: flag(attributes, 4),
        })
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        ensure(self.position <= 100, "position outside 0..=100")?;
        let attributes = bit(self.failure, 0)
            | bit(self.manual_override, 1)
            | bit(self.calibration_mode, 2)
            | bit(self.valve_kick, 3)
            | bit(self.synchronization_mode, 4);
        Ok(alloc::vec![self.position, attributes])
    }
}

/// DPT 207.600, status of a lighting actuator.
///
/// Two octets: the current lighting level and eight attribute bits. The level is a scaling value
/// like DPT 5.001, the octet 0 to 255 is 0 to 100 % (0.4 % resolution); a switching actuator
/// reports only 0 % and 100 %.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct StatusLightingActuator {
    /// Current lighting level, raw octet 0 to 255 for 0 to 100 %.
    pub actual_value: u8,
    /// Bit 0: `actual_value` is valid.
    pub valid_actual_value: bool,
    /// Bit 1: the actuator is locked.
    pub locked: bool,
    /// Bit 2: forced on/off control is active.
    pub forced: bool,
    /// Bit 3: night mode is active.
    pub night_mode_active: bool,
    /// Bit 4: the staircase lighting function is active.
    pub staircase_lighting: bool,
    /// Bit 5: the actuator is dimming (not applicable to a switching actuator).
    pub dimming: bool,
    /// Bit 6: the set value is overridden locally.
    pub local_override: bool,
    /// Bit 7: general actuator failure.
    pub failure: bool,
}

impl StatusLightingActuator {
    /// The lighting level in percent.
    #[must_use]
    pub fn actual_percent(&self) -> f64 {
        raw_to_percent(self.actual_value)
    }

    /// Set the lighting level from a percentage, rounded to the nearest step.
    ///
    /// # Errors
    ///
    /// [`DptError::OutOfRange`] for NaN, infinity and values outside 0 to 100.
    pub fn set_actual_percent(&mut self, percent: f64) -> Result<(), DptError> {
        self.actual_value = percent_to_raw(percent)?;
        Ok(())
    }
}

impl TypedDpt for StatusLightingActuator {
    const DPT: Dpt = Dpt::new(207, 600);
    const LEN: usize = 2;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [actual_value, attributes] = octets::<2>(payload)?;
        Ok(Self {
            actual_value,
            valid_actual_value: flag(attributes, 0),
            locked: flag(attributes, 1),
            forced: flag(attributes, 2),
            night_mode_active: flag(attributes, 3),
            staircase_lighting: flag(attributes, 4),
            dimming: flag(attributes, 5),
            local_override: flag(attributes, 6),
            failure: flag(attributes, 7),
        })
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        let attributes = bit(self.valid_actual_value, 0)
            | bit(self.locked, 1)
            | bit(self.forced, 2)
            | bit(self.night_mode_active, 3)
            | bit(self.staircase_lighting, 4)
            | bit(self.dimming, 5)
            | bit(self.local_override, 6)
            | bit(self.failure, 7);
        Ok(alloc::vec![self.actual_value, attributes])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::tests::round_trip;
    use super::*;

    #[test]
    fn status_buc_follows_the_attribute_table() {
        let status = StatusBuc {
            relative_power: 50,
            relative_power_valid: true,
            fault: false,
            stage1_active: true,
            stage2_active: true,
        };
        round_trip(&[50, 0b0000_1101], &status);
        round_trip(
            &[0, 0b0000_0010],
            &StatusBuc {
                relative_power: 0,
                relative_power_valid: false,
                fault: true,
                stage1_active: false,
                stage2_active: false,
            },
        );
        assert!(StatusBuc::decode(&[101, 0x01]).is_err());
        // The power carries nothing while it is flagged invalid.
        assert_eq!(StatusBuc::decode(&[255, 0x00]).unwrap().relative_power, 255);
        assert!(
            StatusBuc::encode(&StatusBuc {
                relative_power: 101,
                ..status
            })
            .is_err()
        );
        // Bits 4 to 7 are reserved.
        assert_eq!(StatusBuc::decode(&[50, 0xFD]).unwrap(), status);
    }

    #[test]
    fn status_act_has_five_attribute_bits() {
        let status = StatusAct {
            position: 75,
            failure: false,
            manual_override: true,
            calibration_mode: false,
            valve_kick: false,
            synchronization_mode: true,
        };
        round_trip(&[75, 0b0001_0010], &status);
        round_trip(
            &[100, 0b0000_1101],
            &StatusAct {
                position: 100,
                failure: true,
                manual_override: false,
                calibration_mode: true,
                valve_kick: true,
                synchronization_mode: false,
            },
        );
        assert!(StatusAct::decode(&[101, 0]).is_err());
        assert!(
            StatusAct::encode(&StatusAct {
                position: 101,
                ..status
            })
            .is_err()
        );
        // Bits 5 to 7 are reserved.
        assert_eq!(StatusAct::decode(&[75, 0xF2]).unwrap(), status);
    }

    #[test]
    fn status_lighting_actuator_has_the_bit_order_of_the_master_data() {
        let all = StatusLightingActuator {
            actual_value: 255,
            valid_actual_value: true,
            locked: true,
            forced: true,
            night_mode_active: true,
            staircase_lighting: true,
            dimming: true,
            local_override: true,
            failure: true,
        };
        round_trip(&[255, 0xFF], &all);
        // Valid actual value is bit 0, failure bit 7.
        let one = |octet| StatusLightingActuator::decode(&[0, octet]).unwrap();
        assert!(one(0x01).valid_actual_value);
        assert!(one(0x02).locked);
        assert!(one(0x04).forced);
        assert!(one(0x08).night_mode_active);
        assert!(one(0x10).staircase_lighting);
        assert!(one(0x20).dimming);
        assert!(one(0x40).local_override);
        assert!(one(0x80).failure);
    }

    #[test]
    fn status_lighting_actuator_scales_the_level_like_dpt5() {
        let mut status = StatusLightingActuator::decode(&[255, 0x01]).unwrap();
        assert!((status.actual_percent() - 100.0).abs() < 1e-9);
        status.set_actual_percent(50.0).unwrap();
        assert_eq!(status.actual_value, 128);
        status.set_actual_percent(0.0).unwrap();
        assert_eq!(status.actual_value, 0);
        status.set_actual_percent(100.0).unwrap();
        assert_eq!(status.actual_value, 255);
        assert!(status.set_actual_percent(100.5).is_err());
        assert!(status.set_actual_percent(-0.1).is_err());
        assert!(status.set_actual_percent(f64::NAN).is_err());
        assert_eq!(status.actual_value, 255);
    }
}
