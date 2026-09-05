// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Regression coverage for the ETS import failures reported in issue #45.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::io::Read as _;

use knx_rs_prod::author::{
    AppProgram, CatalogItem, CatalogSection, Hardware, Hardware2Program, Parameter, ParameterType,
    Product, ProgramInfo, SegMemoryType, Segment,
};
use quick_xml::{Reader, events::Event};

const APP_ID: &str = "M-013A_A-0004-03-0000";

fn elements(xml: &str, name: &str) -> Vec<HashMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(node) | Event::Empty(node) if node.name().as_ref() == name => {
                result.push(
                    node.attributes()
                        .map(|attr| {
                            let attr = attr.expect("valid attribute");
                            (
                                attr.key.as_ref().to_string(),
                                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .expect("valid attribute value")
                                    .into_owned(),
                            )
                        })
                        .collect(),
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    result
}

fn product_document(serial: &str, order: &str, section: &str, type_name: &str) -> String {
    let mut app = AppProgram::new(APP_ID);
    app.add_catalog_section(
        CatalogSection::new(section, section, "1", "en-US")
            .with_item(CatalogItem::new("Device", "1", serial, 1, order, "en-US")),
    );
    app.add_hardware(
        Hardware::new(serial, 1, "Device")
            .with_product(Product::new(order, "Device", "en-US"))
            .with_program(Hardware2Program::new("MT-0", "0001/1")),
    );
    let segment = app.add_segment(Segment::Absolute {
        name: None,
        address: 16384,
        size: 2,
        memory_type: Some(SegMemoryType::Eeprom),
        user_memory: false,
    });
    app.set_parameter_segment(segment);
    app.add_parameter_type(ParameterType::enumeration(
        type_name,
        16,
        vec![("0 ms".into(), 0), ("100 ms".into(), 100)],
    ));
    let ty = app.parameter_type_id(type_name);
    app.add_param(Parameter::new(
        "StartupTime",
        "StartupTime",
        ty,
        "Delay",
        "0",
        0,
    ));
    let mut xml = String::new();
    app.write_knx_document(
        &ProgramInfo::new("RIOT KNX Device", "MV-07B0", "en-US", 4, 3),
        "knx-rs-prod",
        "test",
        &mut xml,
    );
    xml
}

#[test]
fn catalog_and_hardware_encode_components_without_changing_identity_attributes() {
    for (raw, encoded) in [
        ("RIOT-KNX-DEVICE-1CH", "RIOT.2DKNX.2DDEVICE.2D1CH"),
        ("RIOT KNX Device 123", "RIOT.20KNX.20Device.20123"),
        ("General_StartupTime", "General.5FStartupTime"),
        ("literal.2D", "literal.2E2D"),
        ("a&\"<>/\\", "a.26.22.3C.3E.2F.5C"),
        (
            "Ger\u{e4}t\u{4e2d}\u{1f600}",
            "Ger.C3.A4t.E4.B8.AD.F0.9F.98.80",
        ),
    ] {
        let xml = product_document(raw, raw, raw, "Delay");
        let hardware_id = format!("M-013A_H-{encoded}-1");
        let product_id = format!("{hardware_id}_P-{encoded}");
        let program_id = format!("{hardware_id}_HP-0004-03-0000");
        let catalog = &elements(&xml, "CatalogItem")[0];
        let hardware = &elements(&xml, "Hardware")[1];
        let product = &elements(&xml, "Product")[0];
        let section = &elements(&xml, "CatalogSection")[0];

        assert_eq!(hardware["Id"], hardware_id);
        assert_eq!(hardware["SerialNumber"], raw);
        assert_eq!(product["Id"], product_id);
        assert_eq!(product["OrderNumber"], raw);
        assert_eq!(catalog["ProductRefId"], product_id);
        assert_eq!(catalog["Hardware2ProgramRefId"], program_id);
        assert_eq!(catalog["Id"], format!("{program_id}_CI-{encoded}-1"));
        assert_eq!(elements(&xml, "Hardware2Program")[0]["Id"], program_id);
        assert_eq!(section["Id"], format!("M-013A_CS-{encoded}"));
        assert_eq!(section["Name"], raw);
    }
}

#[test]
fn parameter_types_and_their_references_share_encoded_ids() {
    let mut app = AppProgram::new(APP_ID);
    for (ty, encoded) in [
        (
            ParameterType::enumeration("General_StartupTime", 16, vec![("0 ms".into(), 0)]),
            "General.5FStartupTime",
        ),
        (
            ParameterType::text("Text & \"Name\"", 160),
            "Text.20.26.20.22Name.22",
        ),
        (
            ParameterType::number("Number.2D", 8, "unsignedInt", 0, 100),
            "Number.2E2D",
        ),
    ] {
        let raw_name = ty.name().to_string();
        let handle = app.add_parameter_type(ty);
        assert_eq!(app.parameter_type_id(&raw_name), handle);
        app.add_param(Parameter::new(encoded, "Value", handle, "Value", "0", 0));
    }
    let mut xml = String::new();
    app.write_parameter_types(0, &mut xml);
    app.write_parameters(0, &mut xml);
    let types = elements(&xml, "ParameterType");
    let parameters = elements(&xml, "Parameter");
    assert_eq!(types.len(), 3);
    assert_eq!(parameters.len(), 3);
    for ((ty, parameter), (raw, encoded)) in types.iter().zip(&parameters).zip([
        ("General_StartupTime", "General.5FStartupTime"),
        ("Text & \"Name\"", "Text.20.26.20.22Name.22"),
        ("Number.2D", "Number.2E2D"),
    ]) {
        let id = format!("{APP_ID}_PT-{encoded}");
        assert_eq!(ty["Id"], id);
        assert_eq!(ty["Name"], raw);
        assert_eq!(parameter["ParameterType"], id);
    }
    assert_eq!(
        elements(&xml, "Enumeration")[0]["Id"],
        format!("{APP_ID}_PT-General.5FStartupTime_EN-0")
    );
}

#[test]
fn absolute_segments_use_schema_memory_type_values() {
    let mut app = AppProgram::new(APP_ID);
    for memory_type in [
        Some(SegMemoryType::Ram),
        Some(SegMemoryType::Eeprom),
        Some(SegMemoryType::Flash),
        None,
    ] {
        app.add_segment(Segment::Absolute {
            name: None,
            address: 16384,
            size: 2,
            memory_type,
            user_memory: false,
        });
    }
    let mut xml = String::new();
    app.write_code(0, &mut xml);
    let segments = elements(&xml, "AbsoluteSegment");
    assert_eq!(segments.len(), 4);
    for (segment, expected) in
        segments
            .iter()
            .zip([Some("RAM"), Some("EEPROM"), Some("FLASH"), None])
    {
        assert_eq!(segment.get("MemoryType").map(String::as_str), expected);
    }
}

#[test]
fn issue_45_ids_survive_normalization_hashing_and_packaging() {
    let xml = product_document(
        "0x060504030201",
        "RIOT-KNX-DEVICE-1CH",
        "RIOT KNX Device 123",
        "General_StartupTime",
    );
    let normalized = knx_rs_prod::normalize_ids(&xml).expect("normalization succeeds");
    assert_eq!(
        knx_rs_prod::normalize_ids(&normalized).expect("second normalization"),
        normalized
    );
    let temp = tempfile::tempdir().expect("temporary directory");
    let input = temp.path().join("device.xml");
    let output = temp.path().join("device.knxprod");
    std::fs::write(&input, &normalized).expect("write product XML");
    let metadata = knx_rs_prod::generate_knxprod(&input, &output).expect("generate archive");
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(output).expect("archive")).expect("valid ZIP");
    let mut documents = Vec::new();
    for file in [
        "Catalog.xml".to_string(),
        "Hardware.xml".to_string(),
        format!("{}.xml", metadata.application_id),
    ] {
        let mut xml = String::new();
        zip.by_name(&format!("M-013A/{file}"))
            .expect("product XML in archive")
            .read_to_string(&mut xml)
            .expect("UTF-8 XML");
        documents.push(xml);
    }
    let catalog = &elements(&documents[0], "CatalogItem")[0];
    let product = &elements(&documents[1], "Product")[0];
    assert_eq!(
        product["Id"],
        "M-013A_H-0x060504030201-1_P-RIOT.2DKNX.2DDEVICE.2D1CH"
    );
    assert_eq!(catalog["ProductRefId"], product["Id"]);
    assert_eq!(
        catalog["Hardware2ProgramRefId"],
        elements(&documents[1], "Hardware2Program")[0]["Id"]
    );
    assert_eq!(
        elements(&documents[1], "ApplicationProgramRef")[0]["RefId"],
        metadata.application_id
    );
    assert_eq!(
        elements(&documents[0], "CatalogSection")[0]["Id"],
        "M-013A_CS-RIOT.20KNX.20Device.20123"
    );
    let type_id = format!("{}_PT-General.5FStartupTime", metadata.application_id);
    assert_eq!(elements(&documents[2], "ParameterType")[0]["Id"], type_id);
    assert_eq!(
        elements(&documents[2], "Parameter")[0]["ParameterType"],
        type_id
    );
    assert_eq!(
        elements(&documents[2], "Enumeration")[0]["Id"],
        format!("{type_id}_EN-0")
    );
    assert_eq!(
        elements(&documents[2], "AbsoluteSegment")[0]["MemoryType"],
        "EEPROM"
    );
    knx_rs_prod::sanity::sanity_check(&documents[2])
        .expect("packaged application has valid references");
}
