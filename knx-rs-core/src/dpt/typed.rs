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

/// DPT 11.001, a date.
///
/// Three octets: the day, the month and a two-digit year. The year covers 1990 to 2089: values
/// from 90 on are the 20th century, values below 90 the 21st.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Date {
    /// Day of month, 1 to 31.
    pub day: u8,
    /// Month, 1 to 12.
    pub month: u8,
    /// Year, 1990 to 2089.
    pub year: u16,
}

impl Date {
    fn validate(self) -> Result<(), DptError> {
        ensure((1..=31).contains(&self.day), "day of month outside 1..=31")?;
        ensure((1..=12).contains(&self.month), "month outside 1..=12")?;
        ensure(
            (1990..=2089).contains(&self.year),
            "year outside 1990..=2089",
        )
    }
}

impl TypedDpt for Date {
    const DPT: Dpt = Dpt::new(11, 1);
    const LEN: usize = 3;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [day, month, year] = octets::<3>(payload)?;
        let year = u16::from(year & 0x7F);
        let date = Self {
            day: day & 0x1F,
            month: month & 0x0F,
            year: if year >= 90 { 1900 + year } else { 2000 + year },
        };
        date.validate()?;
        Ok(date)
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        self.validate()?;
        #[allow(clippy::cast_possible_truncation)]
        let year = (self.year % 100) as u8;
        Ok(alloc::vec![self.day, self.month, year])
    }
}

/// DPT 19.001, date and time.
///
/// Eight octets: year (offset 1900), month, day of month, day of week with hour, minutes,
/// seconds, and two octets of flags. A flag that marks a field invalid (`year_invalid`,
/// `date_invalid`, `day_of_week_invalid`, `time_invalid`, `working_day_invalid`) means the
/// field carries nothing; the value of that field is then not range-checked beyond its width.
///
/// The last octet is read as `xknx` and the figure of the standard have it: bit 7 is the clock
/// quality (`clock_synchronised`), bit 6 the reliability of the synchronisation source
/// (`source_reliable`), the other six bits are reserved. The ETS master data lists only the
/// clock quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct DateTime {
    /// Year, 1900 to 2155.
    pub year: u16,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day of month, 1 to 31.
    pub day: u8,
    /// Day of week, 0 (any day) or 1 (Monday) to 7 (Sunday).
    pub day_of_week: u8,
    /// Hour of day, 0 to 24. 24 is only valid with minutes and seconds 0.
    pub hour: u8,
    /// Minutes, 0 to 59.
    pub minute: u8,
    /// Seconds, 0 to 59.
    pub second: u8,
    /// The clock data is corrupted.
    pub fault: bool,
    /// Working day (set) or bank holiday (clear). Meaningful unless `working_day_invalid`.
    pub working_day: bool,
    /// The working day flag is not valid.
    pub working_day_invalid: bool,
    /// The year is not valid.
    pub year_invalid: bool,
    /// Month and day of month are not valid.
    pub date_invalid: bool,
    /// The day of week is not valid.
    pub day_of_week_invalid: bool,
    /// Hour, minutes and seconds are not valid.
    pub time_invalid: bool,
    /// Summer time: the hour is already UT+X+1, no offset is to be added.
    pub summer_time: bool,
    /// The clock has an external synchronisation signal (CLQ).
    pub clock_synchronised: bool,
    /// The synchronisation source is reliable, such as radio or the Internet (SRC).
    pub source_reliable: bool,
}

impl DateTime {
    fn validate(&self) -> Result<(), DptError> {
        ensure(
            (1900..=2155).contains(&self.year),
            "year outside 1900..=2155",
        )?;
        ensure(self.day_of_week <= 7, "day of week outside 0..=7")?;
        if self.date_invalid {
            ensure(
                self.month <= 15 && self.day <= 31,
                "month or day does not fit its field",
            )?;
        } else {
            ensure((1..=12).contains(&self.month), "month outside 1..=12")?;
            ensure((1..=31).contains(&self.day), "day of month outside 1..=31")?;
        }
        if self.time_invalid {
            ensure(
                self.hour <= 31 && self.minute <= 63 && self.second <= 63,
                "hour, minutes or seconds do not fit their field",
            )
        } else {
            ensure(self.hour <= 24, "hour outside 0..=24")?;
            ensure(self.minute <= 59, "minutes outside 0..=59")?;
            ensure(self.second <= 59, "seconds outside 0..=59")?;
            ensure(
                self.hour < 24 || (self.minute == 0 && self.second == 0),
                "hour 24 needs minutes and seconds 0",
            )
        }
    }
}

impl TypedDpt for DateTime {
    const DPT: Dpt = Dpt::new(19, 1);
    const LEN: usize = 8;

    fn decode(payload: &[u8]) -> Result<Self, DptError> {
        let [year, month, day, dow_hour, minute, second, flags, clock] = octets::<8>(payload)?;
        let value = Self {
            year: 1900 + u16::from(year),
            month: month & 0x0F,
            day: day & 0x1F,
            day_of_week: dow_hour >> 5,
            hour: dow_hour & 0x1F,
            minute: minute & 0x3F,
            second: second & 0x3F,
            fault: flag(flags, 7),
            working_day: flag(flags, 6),
            working_day_invalid: flag(flags, 5),
            year_invalid: flag(flags, 4),
            date_invalid: flag(flags, 3),
            day_of_week_invalid: flag(flags, 2),
            time_invalid: flag(flags, 1),
            summer_time: flag(flags, 0),
            clock_synchronised: flag(clock, 7),
            source_reliable: flag(clock, 6),
        };
        value.validate()?;
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>, DptError> {
        self.validate()?;
        #[allow(clippy::cast_possible_truncation)]
        let year = (self.year - 1900) as u8;
        let flags = bit(self.fault, 7)
            | bit(self.working_day, 6)
            | bit(self.working_day_invalid, 5)
            | bit(self.year_invalid, 4)
            | bit(self.date_invalid, 3)
            | bit(self.day_of_week_invalid, 2)
            | bit(self.time_invalid, 1)
            | bit(self.summer_time, 0);
        let clock = bit(self.clock_synchronised, 7) | bit(self.source_reliable, 6);
        Ok(alloc::vec![
            year,
            self.month,
            self.day,
            (self.day_of_week << 5) | self.hour,
            self.minute,
            self.second,
            flags,
            clock,
        ])
    }
}

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
    use core::fmt::Debug;

    use super::super::{DptWireSize, decode, encode};
    use super::*;

    /// The octets decode to `expected`, which encodes back to them, through the codec as well.
    fn round_trip<T: TypedDpt + PartialEq + Debug>(octets: &[u8], expected: &T) {
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
    fn date_reads_the_century_from_the_year() {
        let date = |year| Date {
            day: 1,
            month: 11,
            year,
        };
        round_trip(&[1, 11, 26], &date(2026));
        round_trip(&[1, 11, 99], &date(1999));
        round_trip(&[1, 11, 90], &date(1990));
        round_trip(&[1, 11, 89], &date(2089));
        round_trip(&[1, 11, 0], &date(2000));
        assert!(Date::decode(&[0, 11, 26]).is_err());
        assert!(Date::decode(&[1, 13, 26]).is_err());
        assert!(Date::encode(&date(2090)).is_err());
        assert!(Date::encode(&date(1989)).is_err());
        assert_eq!(Date::decode(&[1, 11]), Err(DptError::PayloadTooShort));
        // Reserved bits are not read, octets beyond the format are ignored.
        assert_eq!(Date::decode(&[0xE1, 0xFB, 0x9A, 0x55]).unwrap(), date(2026));
    }

    fn sunday_evening() -> DateTime {
        DateTime {
            year: 2026,
            month: 11,
            day: 1,
            day_of_week: 7,
            hour: 18,
            minute: 24,
            second: 5,
            fault: false,
            working_day: false,
            working_day_invalid: false,
            year_invalid: false,
            date_invalid: false,
            day_of_week_invalid: false,
            time_invalid: false,
            summer_time: false,
            clock_synchronised: true,
            source_reliable: false,
        }
    }

    #[test]
    fn date_time_has_the_flags_of_the_standard() {
        round_trip(&[126, 11, 1, 0xF2, 24, 5, 0x00, 0x80], &sunday_evening());

        let flags = DateTime {
            fault: true,
            working_day: true,
            working_day_invalid: true,
            year_invalid: true,
            date_invalid: true,
            day_of_week_invalid: true,
            time_invalid: true,
            summer_time: true,
            clock_synchronised: true,
            source_reliable: true,
            ..sunday_evening()
        };
        round_trip(&[126, 11, 1, 0xF2, 24, 5, 0xFF, 0xC0], &flags);

        let reliable = DateTime {
            source_reliable: true,
            clock_synchronised: false,
            ..sunday_evening()
        };
        round_trip(&[126, 11, 1, 0xF2, 24, 5, 0x00, 0x40], &reliable);
    }

    #[test]
    fn date_time_ignores_reserved_bits_and_extra_octets() {
        let payload = [
            126,
            0xF0 | 0x0B,
            0xE0 | 0x01,
            0xF2,
            0xC0 | 0x18,
            0xC0 | 0x05,
            0x00,
            0xBF,
            9,
        ];
        assert_eq!(DateTime::decode(&payload).unwrap(), sunday_evening());
    }

    #[test]
    fn date_time_checks_fields_unless_they_are_flagged_invalid() {
        // Hour 24 is the end of a day, only with minutes and seconds 0.
        let midnight = DateTime {
            hour: 24,
            minute: 0,
            second: 0,
            ..sunday_evening()
        };
        round_trip(&[126, 11, 1, 0xF8, 0, 0, 0x00, 0x80], &midnight);
        assert!(DateTime::decode(&[126, 11, 1, 0xF8, 1, 0, 0x00, 0x80]).is_err());
        assert!(
            DateTime::encode(&DateTime {
                minute: 1,
                ..midnight
            })
            .is_err()
        );

        assert!(DateTime::decode(&[126, 13, 1, 0xF2, 24, 5, 0, 0]).is_err());
        assert!(DateTime::decode(&[126, 11, 1, 0xF2, 60, 5, 0, 0]).is_err());
        assert!(
            DateTime::encode(&DateTime {
                day_of_week: 8,
                ..sunday_evening()
            })
            .is_err()
        );
        assert!(
            DateTime::encode(&DateTime {
                year: 1899,
                ..sunday_evening()
            })
            .is_err()
        );
        assert!(
            DateTime::encode(&DateTime {
                year: 2156,
                ..sunday_evening()
            })
            .is_err()
        );

        // A clock without a date sends the date fields as 0 and says so.
        let no_date = DateTime {
            month: 0,
            day: 0,
            date_invalid: true,
            ..sunday_evening()
        };
        round_trip(&[126, 0, 0, 0xF2, 24, 5, 0x08, 0x80], &no_date);
    }

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
