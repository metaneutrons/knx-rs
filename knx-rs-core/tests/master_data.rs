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
                current = Some(Subtype {
                    main,
                    sub: attr(e, "Number").unwrap().parse().unwrap(),
                    name: attr(e, "Name").unwrap_or_default(),
                    width_sum: Some(0),
                    float_range: None,
                });
                if !is_start {
                    subtypes.extend(current.take());
                }
            }
            "Reserved" | "Bit" | "UnsignedInteger" | "SignedInteger" | "Float" | "Enumeration"
            | "String" | "RefType" => {
                let Some(s) = current.as_mut() else { continue };
                let kind = e.name().as_ref().to_owned();
                let width = match kind.as_str() {
                    "Bit" => Some(1),
                    "RefType" => None,
                    _ => attr(e, "Width").and_then(|w| w.parse::<u32>().ok()),
                };
                s.width_sum = s.width_sum.zip(width).map(|(a, b)| a + b);
                if kind == "Float" {
                    floats += 1;
                    let lo = attr(e, "MinValue").and_then(|v| v.parse::<f64>().ok());
                    let hi = attr(e, "MaxValue").and_then(|v| v.parse::<f64>().ok());
                    s.float_range = (floats == 1).then(|| lo.zip(hi)).flatten();
                } else if floats > 0 {
                    s.float_range = None;
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
