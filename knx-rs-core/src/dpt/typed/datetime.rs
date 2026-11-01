// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! DPT 11.001 and 19.001, date and date and time.

use alloc::vec::Vec;

use super::{TypedDpt, bit, ensure, flag, octets};
use crate::dpt::{Dpt, DptError};

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::tests::round_trip;
    use super::*;

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
}
