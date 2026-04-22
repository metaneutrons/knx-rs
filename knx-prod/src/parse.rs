// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! XML parsing and metadata extraction for KNX product XML files.

use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::KnxprodError;

/// Metadata extracted from a KNX product XML file.
#[derive(Debug, Clone)]
pub struct KnxMetadata {
    /// The XML namespace version (e.g. 20 for `project/20`).
    pub ns_version: u32,
    /// The manufacturer ID (e.g. `M-00FA`).
    pub manufacturer_id: String,
    /// The application program ID (e.g. `M-00FA_A-AD01-01-0000`).
    pub application_id: String,
}

/// Extract metadata from a KNX product XML file.
///
/// # Errors
///
/// Returns [`KnxprodError`] if the file cannot be read or required elements are missing.
pub fn extract_metadata(path: &Path) -> Result<KnxMetadata, KnxprodError> {
    let content = std::fs::read_to_string(path).map_err(|e| KnxprodError::io(path, e))?;
    extract_metadata_from_str(&content)
}

/// Extract metadata from XML content.
///
/// # Errors
///
/// Returns [`KnxprodError`] if required elements are missing.
pub fn extract_metadata_from_str(xml: &str) -> Result<KnxMetadata, KnxprodError> {
    let ns_version = extract_ns_version(xml)?;
    let manufacturer_id = extract_manufacturer_id(xml)?;
    let application_id = extract_application_id(xml)?;

    Ok(KnxMetadata {
        ns_version,
        manufacturer_id,
        application_id,
    })
}

fn extract_ns_version(xml: &str) -> Result<u32, KnxprodError> {
    // Look for xmlns="http://knx.org/xml/project/NN"
    let marker = "http://knx.org/xml/project/";
    let pos = xml
        .find(marker)
        .ok_or(KnxprodError::MissingElement("KNX namespace"))?;
    let after = &xml[pos + marker.len()..];
    let end = after
        .find('"')
        .ok_or(KnxprodError::MissingElement("KNX namespace version"))?;
    after[..end]
        .parse()
        .map_err(|_| KnxprodError::InvalidStructure("namespace version is not a number".into()))
}

fn extract_manufacturer_id(xml: &str) -> Result<String, KnxprodError> {
    extract_xml_attribute(xml, b"Manufacturer", b"RefId", "Manufacturer/@RefId")
}

fn extract_application_id(xml: &str) -> Result<String, KnxprodError> {
    extract_xml_attribute(xml, b"ApplicationProgram", b"Id", "ApplicationProgram/@Id")
}

/// Extract an attribute value from the first matching XML element.
fn extract_xml_attribute(
    xml: &str,
    element: &[u8],
    attr_name: &[u8],
    error_context: &'static str,
) -> Result<String, KnxprodError> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) if e.local_name().as_ref() == element => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == attr_name {
                        return Ok(String::from_utf8_lossy(&attr.value).into_owned());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(KnxprodError::Xml(e)),
            _ => {}
        }
        buf.clear();
    }

    Err(KnxprodError::MissingElement(error_context))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MINIMAL_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<KNX xmlns="http://knx.org/xml/project/20">
  <ManufacturerData>
    <Manufacturer RefId="M-00FA">
      <Catalog><CatalogSection Id="M-00FA_CS-1" Name="Test" /></Catalog>
      <ApplicationPrograms>
        <ApplicationProgram Id="M-00FA_A-0001-00-0001" Name="Test" ApplicationNumber="1" ApplicationVersion="1">
        </ApplicationProgram>
      </ApplicationPrograms>
      <Hardware><Hardware Id="M-00FA_H-0001-1" Name="Test" /></Hardware>
    </Manufacturer>
  </ManufacturerData>
</KNX>"#;

    #[test]
    fn extract_ns_version_from_minimal() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        assert_eq!(meta.ns_version, 20);
    }

    #[test]
    fn extract_manufacturer_id_from_minimal() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        assert_eq!(meta.manufacturer_id, "M-00FA");
    }

    #[test]
    fn extract_application_id_from_minimal() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        assert_eq!(meta.application_id, "M-00FA_A-0001-00-0001");
    }
}
