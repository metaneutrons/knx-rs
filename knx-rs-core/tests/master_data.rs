#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Regression tests against the KNX master data (`knx_master.xml`).
//!
//! KNX publishes the file for every project schema (`https://update.knx.org/data/XML/project-23/
//! knx_master.xml`) and ETS ships it in every project export. It describes every datapoint type
//! and subtype: the size in bits and the fields with their widths. The tests compare that with the
//! wire sizes and codecs of this crate, so that a group ETS knows and this crate does not, or a
//! size that drifts, fails.
//!
//! The file is not part of this repository. Point `KNX_MASTER_XML` at a copy, for example
//! `curl -o /tmp/knx_master.xml https://update.knx.org/data/XML/project-23/knx_master.xml`, and run
//! `KNX_MASTER_XML=/tmp/knx_master.xml cargo test -p knx-rs-core --test master_data`. Without the
//! variable the tests report that and pass. CI fetches the file in the Linux test job and sets the
//! variable; if the download fails the job continues with a warning and the tests skip.

use std::collections::BTreeMap;

use knx_rs_core::dpt::typed::TypedDpt;
use knx_rs_core::dpt::{self, Dpt, DptValue, DptWireSize};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// One subtype with the widths of its fields (`None` when a field refers to another subtype).
struct Subtype {
    main: u16,
    sub: u16,
    name: String,
    width_sum: Option<u32>,
    /// `MinValue` and `MaxValue` of the first `Float` field.
    float_range: Option<(f64, f64)>,
}

struct Master {
    /// Main group number to `SizeInBit`.
    sizes: BTreeMap<u16, u32>,
    subtypes: Vec<Subtype>,
}

fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .unwrap()
                .into_owned()
        })
}

/// An empty subtype from its `DatapointSubtype` element.
fn new_subtype(e: &BytesStart<'_>, main: u16) -> Subtype {
    Subtype {
        main,
        sub: attr(e, "Number").unwrap().parse().unwrap(),
        name: attr(e, "Name").unwrap_or_default(),
        width_sum: Some(0),
        float_range: None,
    }
}

/// Add one field element to the subtype: its width, and the range of the first `Float`.
fn add_field(s: &mut Subtype, e: &BytesStart<'_>, floats: &mut u32) {
    let kind = e.name().as_ref().to_owned();
    let width = match kind.as_str() {
        "Bit" => Some(1),
        "RefType" => None,
        _ => attr(e, "Width").and_then(|w| w.parse::<u32>().ok()),
    };
    s.width_sum = s.width_sum.zip(width).map(|(a, b)| a + b);
    if kind == "Float" {
        *floats += 1;
        let lo = attr(e, "MinValue").and_then(|v| v.parse::<f64>().ok());
        let hi = attr(e, "MaxValue").and_then(|v| v.parse::<f64>().ok());
        s.float_range = (*floats == 1).then(|| lo.zip(hi)).flatten();
    } else if *floats > 0 {
        s.float_range = None;
    }
}

fn parse(xml: &str) -> Master {
    let mut reader = Reader::from_str(xml);
    let mut sizes = BTreeMap::new();
    let mut subtypes = Vec::new();
    let mut main = 0u16;
    let mut current: Option<Subtype> = None;
    let mut floats = 0u32;
    loop {
        let event = reader.read_event().unwrap();
        let (e, is_start) = match &event {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(e) => {
                if e.name().as_ref() == "DatapointSubtype" {
                    subtypes.extend(current.take());
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        match e.name().as_ref() {
            "DatapointType" => {
                main = attr(e, "Number").unwrap().parse().unwrap();
                sizes.insert(main, attr(e, "SizeInBit").unwrap().parse().unwrap());
            }
            "DatapointSubtype" => {
                floats = 0;
                current = Some(new_subtype(e, main));
                if !is_start {
                    subtypes.extend(current.take());
                }
            }
            "Reserved" | "Bit" | "UnsignedInteger" | "SignedInteger" | "Float" | "Enumeration"
            | "String" | "RefType" => {
                if let Some(s) = current.as_mut() {
                    add_field(s, e, &mut floats);
                }
            }
            _ => {}
        }
    }
    Master { sizes, subtypes }
}

fn load() -> Option<Master> {
    let Ok(path) = std::env::var("KNX_MASTER_XML") else {
        eprintln!("KNX_MASTER_XML is not set: master data tests skipped");
        return None;
    };
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    Some(parse(xml.trim_start_matches('\u{feff}')))
}

#[test]
fn every_group_in_the_master_data_has_the_same_size_here() {
    let Some(master) = load() else { return };
    assert!(master.sizes.len() > 50, "the master data lists the groups");
    for (&main, &bits) in &master.sizes {
        match Dpt::new(main, 1).wire_size() {
            DptWireSize::Unknown => {
                panic!("main group {main} is in the master data but unknown here")
            }
            // Variable-length strings (24, 28): the master data gives a maximum.
            DptWireSize::VariableBytes => assert!(bits >= 16, "group {main}"),
            DptWireSize::Bits(n) => assert_eq!(bits, u32::from(n), "group {main}"),
            DptWireSize::Bytes(n) => assert_eq!(bits, u32::from(n) * 8, "group {main}"),
            _ => panic!("unexpected wire size for group {main}"),
        }
    }
}

#[test]
fn the_fields_of_every_subtype_add_up_to_the_size_of_the_group() {
    let Some(master) = load() else { return };
    let mut checked = 0;
    for s in &master.subtypes {
        let (Some(sum), Some(&size)) = (s.width_sum, master.sizes.get(&s.main)) else {
            continue;
        };
        // Subtypes of the variable-length strings and those without a format have no sum.
        if sum == 0 || size > 128 {
            continue;
        }
        assert_eq!(sum, size, "{}.{} {}", s.main, s.sub, s.name);
        checked += 1;
    }
    assert!(checked > 100, "only {checked} subtypes had a format");
}

#[test]
fn every_group_encodes_and_decodes_a_zero_value_of_the_size_in_the_master_data() {
    let Some(master) = load() else { return };
    for (&main, &bits) in &master.sizes {
        let dpt = Dpt::new(main, 1);
        let octets = (bits as usize).div_ceil(8);
        // The value type differs by group (number, text, octets); one of them has to be accepted.
        let candidates = [
            DptValue::Bool(false),
            DptValue::UInt(0),
            DptValue::Int(0),
            DptValue::Float(0.0),
            DptValue::Int64(0),
            DptValue::Text(String::new()),
            DptValue::Bytes(vec![0; octets]),
        ];
        let (value, bytes) = candidates
            .iter()
            .find_map(|v| dpt::encode(dpt, v).ok().map(|b| (v, b)))
            .unwrap_or_else(|| panic!("group {main}: no zero value encodes"));
        // The variable-length strings (24, 28) have no fixed size.
        if !matches!(main, 24 | 28) {
            assert_eq!(bytes.len(), octets, "group {main} ({value:?})");
        }
        dpt::decode(dpt, &bytes).unwrap_or_else(|e| panic!("group {main}: {e}"));
    }
}

#[test]
fn the_float_subtypes_encode_the_range_the_master_data_gives() {
    let Some(master) = load() else { return };
    let mut checked = 0;
    for s in master.subtypes.iter().filter(|s| matches!(s.main, 9 | 14)) {
        let Some(range) = s.float_range else {
            continue;
        };
        let dpt = Dpt::new(s.main, s.sub);
        for limit in [range.0, range.1] {
            let bytes = dpt::encode(dpt, &DptValue::Float(limit))
                .unwrap_or_else(|e| panic!("{}.{} {}: {limit}: {e}", s.main, s.sub, s.name));
            let back = dpt::decode(dpt, &bytes).unwrap().as_f64().unwrap();
            // F16 has 11 bits of mantissa: one step is at most 2^15 * 0.01.
            let tolerance = if s.main == 9 {
                330.0
            } else {
                limit.abs().mul_add(1e-6, 1e-9)
            };
            assert!(
                (back - limit).abs() <= tolerance,
                "{}.{} {}: {limit} became {back}",
                s.main,
                s.sub,
                s.name
            );
        }
        checked += 1;
    }
    assert!(checked > 20, "only {checked} float subtypes had a range");
}

/// The `Bit` fields of one subtype with their position counted from the least significant bit of
/// the whole value, in the order the master data lists them (most significant first).
fn bit_positions(xml: &str, id: &str) -> Vec<(String, u32)> {
    let mut reader = Reader::from_str(xml);
    let mut inside = false;
    let mut fields: Vec<(Option<String>, u32)> = Vec::new();
    loop {
        let event = reader.read_event().unwrap();
        let (e, is_start) = match &event {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(e) => {
                if inside && e.name().as_ref() == "DatapointSubtype" {
                    break;
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.name().as_ref().to_owned();
        if name == "DatapointSubtype" && attr(e, "Id").as_deref() == Some(id) {
            inside = true;
            if !is_start {
                break;
            }
            continue;
        }
        if !inside {
            continue;
        }
        match name.as_str() {
            "Bit" => fields.push((attr(e, "Name"), 1)),
            "Reserved" | "UnsignedInteger" | "SignedInteger" | "Float" | "Enumeration" => {
                fields.push((None, attr(e, "Width").unwrap().parse().unwrap()));
            }
            _ => {}
        }
    }
    let total: u32 = fields.iter().map(|(_, w)| w).sum();
    let mut offset = 0;
    let mut bits = Vec::new();
    for (name, width) in fields {
        offset += width;
        if let Some(name) = name {
            bits.push((name, total - offset));
        }
    }
    bits
}

/// A flag of the master data: its name, how to read it from the typed value, and whether the
/// wire bit is set when the flag is not (the validity bits of DPT 235).
type Flag<T> = (&'static str, fn(&T) -> bool, bool);

/// Every flag the master data lists sits on the bit it gives: the whole value with only that bit
/// set reads as exactly that flag, and no other flag moves.
fn check<T: TypedDpt>(xml: &str, id: &str, flags: &[Flag<T>]) {
    let bits = bit_positions(xml, id);
    assert_eq!(bits.len(), flags.len(), "{id}: number of flags");
    for (name, position) in bits {
        let (_, read, invert) = flags
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap_or_else(|| panic!("{id}: no field for the master data flag {name}"));
        let mut octets = vec![0u8; T::LEN];
        // Counted from the least significant bit, the last octet first.
        let octet = T::LEN - 1 - (position / 8) as usize;
        octets[octet] |= 1 << (position % 8);
        // Fields that must be valid for the others to decode are in range at all zeros.
        let set = T::decode(&octets).unwrap();
        let zero = T::decode(&vec![0u8; T::LEN]).unwrap();
        assert_ne!(read(&set), read(&zero), "{id}: {name} on bit {position}");
        assert_eq!(read(&set), !invert, "{id}: {name}");
        // And no other flag moved.
        for (other, read_other, _) in flags.iter().filter(|(n, _, _)| *n != name) {
            assert_eq!(
                read_other(&set),
                read_other(&zero),
                "{id}: {name} moved {other}"
            );
        }
    }
}

#[test]
fn the_typed_flags_sit_on_the_bits_the_master_data_gives() {
    use knx_rs_core::dpt::typed::{
        DateTime, MeteringValue, Rgbw, StatusLightingActuator, TariffActiveEnergy,
    };

    let Ok(path) = std::env::var("KNX_MASTER_XML") else {
        eprintln!("KNX_MASTER_XML is not set: master data tests skipped");
        return;
    };
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let xml = xml.trim_start_matches('\u{feff}');

    check::<StatusLightingActuator>(
        xml,
        "DPST-207-600",
        &[
            ("Failure", |v| v.failure, false),
            ("LocalOverride", |v| v.local_override, false),
            ("Dimming", |v| v.dimming, false),
            (
                "StaircaseLighting Function",
                |v| v.staircase_lighting,
                false,
            ),
            ("NightModeActive", |v| v.night_mode_active, false),
            ("Forced", |v| v.forced, false),
            ("Locked", |v| v.locked, false),
            ("ValidActualValue", |v| v.valid_actual_value, false),
        ],
    );
    check::<TariffActiveEnergy>(
        xml,
        "DPST-235-1",
        &[
            ("ElectricalEngergyValidity", |v| v.energy_valid, true),
            ("TariffValidity", |v| v.tariff_valid, true),
        ],
    );
    check::<Rgbw>(
        xml,
        "DPST-251-600",
        &[
            ("mR", |v| v.red_valid, false),
            ("mG", |v| v.green_valid, false),
            ("mB", |v| v.blue_valid, false),
            ("mW", |v| v.white_valid, false),
        ],
    );
    check::<MeteringValue>(
        xml,
        "DPST-229-1",
        &[
            ("AlarmUnAck", |v| v.status.alarm_unacknowledged, false),
            ("InAlarm", |v| v.status.in_alarm, false),
            ("Overridden", |v| v.status.overridden, false),
            ("Fault", |v| v.status.fault, false),
            ("OutOfService", |v| v.status.out_of_service, false),
        ],
    );
    // The date and time flags: the master data lists the clock quality, not the SRC bit. Every
    // flag the master data lists must be one that decodes. Fields that hold a range cannot be
    // all zero, so the zero value is a valid date with only the flags to look at.
    let bits = bit_positions(xml, "DPST-19-1");
    let by_name = |n: &str| {
        bits.iter()
            .find(|(name, _)| name == n)
            .map(|(_, p)| *p)
            .unwrap()
    };
    let positions = [
        ("Fault", by_name("Fault")),
        ("Working Day", by_name("Working Day")),
        ("No WD", by_name("No WD")),
        ("No Year", by_name("No Year")),
        ("No Date", by_name("No Date")),
        ("No Day of Week", by_name("No Day of Week")),
        ("No Time", by_name("No Time")),
        ("Standard Summer Time", by_name("Standard Summer Time")),
        ("Quality of clock", by_name("Quality of clock")),
    ];
    let base = [126u8, 11, 1, 0xF2, 24, 5, 0, 0];
    let read: [fn(&DateTime) -> bool; 9] = [
        |v| v.fault,
        |v| v.working_day,
        |v| v.working_day_invalid,
        |v| v.year_invalid,
        |v| v.date_invalid,
        |v| v.day_of_week_invalid,
        |v| v.time_invalid,
        |v| v.summer_time,
        |v| v.clock_synchronised,
    ];
    for ((name, position), read) in positions.iter().zip(read) {
        let mut octets = base;
        octets[7 - (position / 8) as usize] |= 1 << (position % 8);
        assert!(read(&DateTime::decode(&octets).unwrap()), "19.001: {name}");
        assert!(
            !read(&DateTime::decode(&base).unwrap()),
            "19.001: {name} at zero"
        );
    }
}
