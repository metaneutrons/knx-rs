// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Split a monolithic KNX product XML into separate files per ETS convention.
//!
//! Implements the `SplitXml` algorithm from `OpenKNX.Toolbox.Sign/SignHelper.cs`.
//! The input XML contains Catalog, Hardware, and `ApplicationPrograms` as siblings
//! under `<Manufacturer>`. Each is extracted into its own file with the shared
//! KNX/ManufacturerData/Manufacturer wrapper.

use std::fs;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::KnxprodError;
use crate::parse::KnxMetadata;

/// Result of splitting an XML file.
#[derive(Debug)]
pub struct SplitResult {
    /// Path to the Catalog.xml file.
    pub catalog: std::path::PathBuf,
    /// Path to the Hardware.xml file.
    pub hardware: std::path::PathBuf,
    /// Path to the Application Program XML file.
    pub application: std::path::PathBuf,
}

/// Split a KNX product XML into separate files.
///
/// Creates `output_dir/M-XXXX/` with Catalog.xml, Hardware.xml, and the
/// application program XML.
///
/// # Errors
///
/// Returns [`KnxprodError`] if the XML structure is invalid or files cannot be written.
pub fn split_xml(
    xml: &str,
    metadata: &KnxMetadata,
    output_dir: &Path,
) -> Result<SplitResult, KnxprodError> {
    let manu_dir = output_dir.join(&metadata.manufacturer_id);
    fs::create_dir_all(&manu_dir).map_err(|e| KnxprodError::io(&manu_dir, e))?;

    // Find the Manufacturer element boundaries and its children
    let manu_range = find_element_range(xml, "Manufacturer")
        .ok_or(KnxprodError::MissingElement("Manufacturer"))?;

    let catalog_range = find_child_element_range(xml, &manu_range, "Catalog")
        .ok_or(KnxprodError::MissingElement("Catalog"))?;
    let hardware_range = find_child_element_range(xml, &manu_range, "Hardware")
        .ok_or(KnxprodError::MissingElement("Hardware"))?;
    let app_range = find_child_element_range(xml, &manu_range, "ApplicationPrograms")
        .ok_or(KnxprodError::MissingElement("ApplicationPrograms"))?;
    let languages_range = find_child_element_range(xml, &manu_range, "Languages");
    let baggages_range = find_child_element_range(xml, &manu_range, "Baggages");

    // Build the wrapper: everything before the first child, and the closing tags
    let prefix = &xml[..manu_range.children_start];
    let suffix = &xml[manu_range.inner_end..];

    // Catalog.xml: wrapper + Catalog + catalog translations
    let catalog_path = manu_dir.join("Catalog.xml");
    let catalog_content = build_document(
        prefix,
        suffix,
        &[&xml[catalog_range.outer_start..catalog_range.outer_end]],
        languages_range
            .as_ref()
            .map(|lr| filter_translations(xml, lr, TranslationCategory::Catalog))
            .as_deref(),
    );
    fs::write(&catalog_path, &catalog_content).map_err(|e| KnxprodError::io(&catalog_path, e))?;

    // Hardware.xml: wrapper + Hardware + hardware translations
    let hardware_path = manu_dir.join("Hardware.xml");
    let hardware_content = build_document(
        prefix,
        suffix,
        &[&xml[hardware_range.outer_start..hardware_range.outer_end]],
        languages_range
            .as_ref()
            .map(|lr| filter_translations(xml, lr, TranslationCategory::Hardware))
            .as_deref(),
    );
    fs::write(&hardware_path, &hardware_content)
        .map_err(|e| KnxprodError::io(&hardware_path, e))?;

    // Application.xml: wrapper + ApplicationPrograms + app translations
    let app_filename = format!("{}.xml", metadata.application_id);
    let app_path = manu_dir.join(&app_filename);
    let app_content = build_document(
        prefix,
        suffix,
        &[&xml[app_range.outer_start..app_range.outer_end]],
        languages_range
            .as_ref()
            .map(|lr| filter_translations(xml, lr, TranslationCategory::Application))
            .as_deref(),
    );
    fs::write(&app_path, &app_content).map_err(|e| KnxprodError::io(&app_path, e))?;

    // Baggages.xml (optional)
    if let Some(ref br) = baggages_range {
        let baggages_path = manu_dir.join("Baggages.xml");
        let baggages_content =
            build_document(prefix, suffix, &[&xml[br.outer_start..br.outer_end]], None);
        fs::write(&baggages_path, &baggages_content)
            .map_err(|e| KnxprodError::io(&baggages_path, e))?;
    }

    Ok(SplitResult {
        catalog: catalog_path,
        hardware: hardware_path,
        application: app_path,
    })
}

fn build_document(
    prefix: &str,
    suffix: &str,
    children: &[&str],
    translations: Option<&str>,
) -> String {
    let mut doc = String::with_capacity(prefix.len() + suffix.len() + 4096);
    doc.push_str(prefix);
    doc.push('\n');
    for child in children {
        doc.push_str("      ");
        doc.push_str(child);
        doc.push('\n');
    }
    if let Some(t) = translations {
        if !t.is_empty() {
            doc.push_str("      ");
            doc.push_str(t);
            doc.push('\n');
        }
    }
    doc.push_str("    ");
    doc.push_str(suffix);
    doc
}

// ── Element range finding ─────────────────────────────────────

/// Byte range of an XML element in the source string.
#[derive(Debug, Clone)]
struct ElementRange {
    /// Start of the opening tag `<`.
    outer_start: usize,
    /// Byte position where child content begins (after the opening tag `>`).
    children_start: usize,
    /// Byte position where child content ends (before `</Tag>`).
    inner_end: usize,
    /// End of the closing tag `>` + 1.
    outer_end: usize,
}

/// Find the byte range of a top-level element by local name.
fn find_element_range(xml: &str, local_name: &str) -> Option<ElementRange> {
    // Find opening tag
    let open_pattern = format!("<{local_name} ");
    let open_pattern2 = format!("<{local_name}>");
    let ns_open = format!(":{local_name} ");
    let ns_open2 = format!(":{local_name}>");

    let outer_start = xml
        .find(&open_pattern)
        .or_else(|| xml.find(&open_pattern2))
        .or_else(|| xml.find(&ns_open).map(|p| xml[..p].rfind('<').unwrap_or(p)))
        .or_else(|| {
            xml.find(&ns_open2)
                .map(|p| xml[..p].rfind('<').unwrap_or(p))
        })?;

    let children_start = xml[outer_start..].find('>')? + outer_start + 1;

    // Find closing tag
    let close_pattern = format!("</{local_name}>");
    let ns_close = format!(":{local_name}>");

    let inner_end = xml.rfind(&close_pattern).or_else(|| {
        xml.rfind(&ns_close)
            .map(|p| xml[..p].rfind('<').unwrap_or(p))
    })?;

    let outer_end = xml[inner_end..].find('>')? + inner_end + 1;

    Some(ElementRange {
        outer_start,
        children_start,
        inner_end,
        outer_end,
    })
}

/// Find a child element range within a parent's content.
/// Uses depth-aware parsing to handle same-name nested elements.
fn find_child_element_range(
    xml: &str,
    parent: &ElementRange,
    local_name: &str,
) -> Option<ElementRange> {
    let search_area = &xml[parent.children_start..parent.inner_end];
    let offset = parent.children_start;
    let name_bytes = local_name.as_bytes();

    let mut reader = Reader::from_str(search_area);
    let mut buf = Vec::new();
    let mut depth = 0u32;
    let mut outer_start = None;
    let mut children_start = None;

    loop {
        let event_offset = usize::try_from(reader.buffer_position()).unwrap_or(0);
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == name_bytes => {
                if depth == 0 {
                    outer_start = Some(event_offset + offset);
                    // children_start is after the '>' of this opening tag
                    let tag_end = xml[event_offset + offset..].find('>')?;
                    children_start = Some(event_offset + offset + tag_end + 1);
                }
                depth += 1;
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == name_bytes => {
                depth -= 1;
                if depth == 0 {
                    let inner_end = event_offset + offset;
                    let close_tag_end = xml[inner_end..].find('>')? + inner_end + 1;
                    return Some(ElementRange {
                        outer_start: outer_start?,
                        children_start: children_start?,
                        inner_end,
                        outer_end: close_tag_end,
                    });
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

// ── Translation splitting ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslationCategory {
    Catalog,
    Hardware,
    Application,
}

/// Filter translation units by category, returning a `Languages` XML fragment.
///
/// Keeps only `TranslationUnit` elements whose `RefId` matches the category:
/// - `Catalog`: `RefId` contains `_CS-` or `_CI-`
/// - `Hardware`: `RefId` contains `_H-` or `_HP-`
/// - `Application`: `RefId` contains `_A-` (or any unrecognized pattern)
fn filter_translations(
    xml: &str,
    languages_range: &ElementRange,
    category: TranslationCategory,
) -> String {
    let mut result = String::new();
    for lang_range in find_child_element_ranges(xml, languages_range, "Language") {
        let filtered_units = filter_units_in_language(xml, &lang_range, category);
        if !filtered_units.is_empty() {
            result.push_str(&xml[lang_range.outer_start..lang_range.children_start]);
            result.push_str(&filtered_units);
            result.push_str(&xml[lang_range.inner_end..lang_range.outer_end]);
        }
    }

    if result.is_empty() {
        String::new()
    } else {
        format!("<Languages>{result}</Languages>")
    }
}

/// Filter `TranslationUnit` elements within a single `Language` block.
fn filter_units_in_language(
    xml: &str,
    lang_range: &ElementRange,
    category: TranslationCategory,
) -> String {
    let mut result = String::new();

    for unit_range in find_child_element_ranges(xml, lang_range, "TranslationUnit") {
        let unit_xml = &xml[unit_range.outer_start..unit_range.outer_end];
        if let Some(ref_id) = extract_attribute(unit_xml, "RefId") {
            if matches_category(&ref_id, category) {
                result.push_str(unit_xml);
            }
        }
    }

    result
}

/// Find all direct child element ranges by local name within a parent.
fn find_child_element_ranges(
    xml: &str,
    parent: &ElementRange,
    local_name: &str,
) -> Vec<ElementRange> {
    let search_area = &xml[parent.children_start..parent.inner_end];
    let offset = parent.children_start;
    let name_bytes = local_name.as_bytes();

    let mut reader = Reader::from_str(search_area);
    let mut buf = Vec::new();
    let mut depth = 0u32;
    let mut outer_start = None;
    let mut children_start = None;
    let mut ranges = Vec::new();

    loop {
        let event_offset = usize::try_from(reader.buffer_position()).unwrap_or(0);
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == name_bytes => {
                if depth == 0 {
                    let start = event_offset + offset;
                    outer_start = Some(start);
                    if let Some(tag_end) = xml[start..].find('>') {
                        children_start = Some(start + tag_end + 1);
                    }
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) if e.local_name().as_ref() == name_bytes && depth == 0 => {
                let start = event_offset + offset;
                if let Some(tag_end) = xml[start..].find('>') {
                    let outer_end = start + tag_end + 1;
                    ranges.push(ElementRange {
                        outer_start: start,
                        children_start: outer_end,
                        inner_end: outer_end,
                        outer_end,
                    });
                }
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == name_bytes && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let inner_end = event_offset + offset;
                    if let Some(close_tag_end) = xml[inner_end..].find('>') {
                        ranges.push(ElementRange {
                            outer_start: outer_start.unwrap_or(inner_end),
                            children_start: children_start.unwrap_or(inner_end),
                            inner_end,
                            outer_end: inner_end + close_tag_end + 1,
                        });
                    }
                    outer_start = None;
                    children_start = None;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    ranges
}

/// Extract an attribute value from an XML element fragment.
fn extract_attribute(tag: &str, attr_name: &str) -> Option<String> {
    let mut reader = Reader::from_str(tag);
    let mut buf = Vec::new();
    let attr_name = attr_name.as_bytes();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == attr_name {
                        return Some(
                            attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_or_else(
                                    |_| String::from_utf8_lossy(&attr.value).into_owned(),
                                    std::borrow::Cow::into_owned,
                                ),
                        );
                    }
                }
                return None;
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Check if a `TranslationUnit` `RefId` matches the given category.
fn matches_category(ref_id: &str, category: TranslationCategory) -> bool {
    match category {
        TranslationCategory::Catalog => ref_id.contains("_CS-") || ref_id.contains("_CI-"),
        TranslationCategory::Hardware => ref_id.contains("_H-") || ref_id.contains("_HP-"),
        TranslationCategory::Application => {
            ref_id.contains("_A-")
                || (!ref_id.contains("_CS-")
                    && !ref_id.contains("_CI-")
                    && !ref_id.contains("_H-")
                    && !ref_id.contains("_HP-"))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    use super::*;
    use crate::parse::extract_metadata_from_str;

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
    fn split_creates_three_files() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        assert!(result.catalog.exists());
        assert!(result.hardware.exists());
        assert!(result.application.exists());
    }

    #[test]
    fn split_catalog_contains_catalog_element() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        let content = fs::read_to_string(&result.catalog).unwrap();
        assert!(content.contains("<Catalog>"));
        assert!(content.contains("CatalogSection"));
        assert!(!content.contains("<Hardware"));
        assert!(!content.contains("<ApplicationPrograms"));
    }

    #[test]
    fn split_hardware_contains_hardware_element() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        let content = fs::read_to_string(&result.hardware).unwrap();
        assert!(content.contains("<Hardware"));
        assert!(!content.contains("<Catalog>"));
        assert!(!content.contains("<ApplicationPrograms"));
    }

    #[test]
    fn split_application_contains_app_element() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        let content = fs::read_to_string(&result.application).unwrap();
        assert!(content.contains("<ApplicationPrograms"));
        assert!(content.contains("ApplicationProgram"));
        assert!(!content.contains("<Catalog>"));
        assert!(!content.contains("<Hardware Id="));
    }

    #[test]
    fn split_files_in_manufacturer_subdir() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        assert!(
            result
                .catalog
                .to_string_lossy()
                .contains("M-00FA/Catalog.xml")
        );
        assert!(
            result
                .hardware
                .to_string_lossy()
                .contains("M-00FA/Hardware.xml")
        );
        assert!(
            result
                .application
                .to_string_lossy()
                .contains("M-00FA_A-0001-00-0001.xml")
        );
    }

    #[test]
    fn split_output_is_valid_xml() {
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        for path in [&result.catalog, &result.hardware, &result.application] {
            let content = fs::read_to_string(path).unwrap();
            // Verify it parses as XML
            let mut reader = Reader::from_str(&content);
            let mut buf = Vec::new();
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Eof) => break,
                    Err(e) => panic!("invalid XML in {}: {e}", path.display()),
                    _ => {}
                }
                buf.clear();
            }
        }
    }

    #[test]
    fn filter_translations_application_includes_app_units() {
        let xml = std::fs::read_to_string("tests/fixtures/gira_small_app.xml").unwrap();
        let manu_range = find_element_range(&xml, "Manufacturer").unwrap();
        let lang_range = find_child_element_range(&xml, &manu_range, "Languages").unwrap();

        let result = filter_translations(&xml, &lang_range, TranslationCategory::Application);
        assert!(
            !result.is_empty(),
            "Application translations should not be empty"
        );
        assert!(result.contains("<Languages>"));
        assert!(result.contains("TranslationUnit"));
        assert!(result.contains("_A-")); // Application program RefId
    }

    #[test]
    fn filter_translations_catalog_excludes_app_units() {
        let xml = std::fs::read_to_string("tests/fixtures/gira_small_app.xml").unwrap();
        let manu_range = find_element_range(&xml, "Manufacturer").unwrap();
        let lang_range = find_child_element_range(&xml, &manu_range, "Languages").unwrap();

        let result = filter_translations(&xml, &lang_range, TranslationCategory::Catalog);
        // gira_small_app.xml only has _A- translation units, no catalog ones
        assert!(
            result.is_empty(),
            "Catalog translations should be empty for app-only fixture"
        );
    }

    #[test]
    fn filter_translations_uses_xml_parser_for_prefixed_units() {
        let xml = r#"
<KNX xmlns:k="http://knx.org/xml/project/20">
  <ManufacturerData>
    <k:Manufacturer RefId="M-00FA">
      <k:Languages>
        <k:Language Identifier="en-US">
          <k:TranslationUnit RefId='M-00FA_A-0001'>
            <k:Translation AttributeName="Name" Text="App" />
          </k:TranslationUnit>
          <k:TranslationUnit RefId="M-00FA_H-0001">
            <k:Translation AttributeName="Name" Text="Hardware" />
          </k:TranslationUnit>
        </k:Language>
      </k:Languages>
    </k:Manufacturer>
  </ManufacturerData>
</KNX>"#;
        let manu_range = find_element_range(xml, "Manufacturer").unwrap();
        let lang_range = find_child_element_range(xml, &manu_range, "Languages").unwrap();

        let result = filter_translations(xml, &lang_range, TranslationCategory::Application);
        assert!(result.contains("M-00FA_A-0001"));
        assert!(!result.contains("M-00FA_H-0001"));
    }

    #[test]
    fn filter_translations_returns_empty_for_no_languages() {
        // MINIMAL_XML has no Languages element, so this tests the None path in split_xml
        let meta = extract_metadata_from_str(MINIMAL_XML).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = split_xml(MINIMAL_XML, &meta, dir.path()).unwrap();

        // Verify the output files don't contain Languages
        let catalog = fs::read_to_string(&result.catalog).unwrap();
        assert!(!catalog.contains("<Languages>"));
    }
}
