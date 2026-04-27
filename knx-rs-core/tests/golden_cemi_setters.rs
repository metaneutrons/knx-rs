#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]
// SPDX-License-Identifier: GPL-3.0-only

//! CEMI setter golden vectors — validated against C++ knx-openknx.

use knx_rs_core::cemi::CemiFrame;
use serde::Deserialize;

#[derive(Deserialize)]
struct SetterVector {
    name: String,
    #[serde(rename = "base")]
    _base: Vec<u8>,
    result: Vec<u8>,
    priority: u8,
    source: u16,
    destination: u16,
    hop_count: u8,
    ack: u8,
    confirm: u8,
    frame_type: u8,
    address_type: u8,
}

#[test]
fn cemi_setter_golden_vectors() {
    let json = include_str!("fixtures/cemi_setter_vectors.json");
    let vectors: Vec<SetterVector> = serde_json::from_str(json).expect("parse");

    for v in &vectors {
        // Parse the base, apply the C++ result, and verify fields match
        let frame = CemiFrame::parse(&v.result).unwrap_or_else(|e| {
            panic!("{}: parse result failed: {e}", v.name);
        });

        assert_eq!(frame.priority() as u8, v.priority, "{}: priority", v.name);
        assert_eq!(frame.source_address().raw(), v.source, "{}: source", v.name);
        assert_eq!(
            frame.destination_address_raw(),
            v.destination,
            "{}: destination",
            v.name
        );
        assert_eq!(frame.hop_count(), v.hop_count, "{}: hop_count", v.name);
        assert_eq!(frame.ack() as u8, v.ack, "{}: ack", v.name);
        assert_eq!(frame.confirm() as u8, v.confirm, "{}: confirm", v.name);
        assert_eq!(
            frame.frame_type() as u8,
            v.frame_type,
            "{}: frame_type",
            v.name
        );
        assert_eq!(
            frame.address_type() as u8,
            v.address_type,
            "{}: address_type",
            v.name
        );
    }
}

#[test]
fn cemi_setter_roundtrip() {
    use knx_rs_core::address::IndividualAddress;
    use knx_rs_core::types::{AckType, Confirm, FrameFormat, Priority};

    let base = [
        0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81,
    ];
    let mut frame = CemiFrame::parse(&base).unwrap();

    // Modify every field
    frame.set_priority(Priority::System);
    assert_eq!(frame.priority(), Priority::System);

    frame.set_priority(Priority::Low);
    assert_eq!(frame.priority(), Priority::Low);

    frame.set_source_address(IndividualAddress::from_raw(0x1203));
    assert_eq!(frame.source_address().raw(), 0x1203);

    frame.set_destination_address_raw(0x1005);
    assert_eq!(frame.destination_address_raw(), 0x1005);

    frame.set_hop_count(3);
    assert_eq!(frame.hop_count(), 3);

    frame.set_ack(AckType::Requested);
    assert_eq!(frame.ack(), AckType::Requested);

    frame.set_confirm(Confirm::Error);
    assert_eq!(frame.confirm(), Confirm::Error);

    frame.set_frame_type(FrameFormat::Extended);
    assert_eq!(frame.frame_type(), FrameFormat::Extended);

    // Re-parse from bytes to verify serialization
    let reparsed = CemiFrame::parse(frame.as_bytes()).unwrap();
    assert_eq!(reparsed.source_address().raw(), 0x1203);
    assert_eq!(reparsed.hop_count(), 3);
}
