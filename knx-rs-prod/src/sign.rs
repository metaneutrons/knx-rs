// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Sign KNX `ApplicationProgram` XML files.
//!
//! Computes the registration-relevant MD5 hash, patches the `Hash` attribute
//! and the fingerprint portion of the `Id` attribute, and renames the file
//! to match the new fingerprint.

use regex::Regex;

use crate::error::KnxprodError;
use crate::hash::hash_application_program;
use crate::split::SplitResult;

/// Sign the `ApplicationProgram` XML produced by [`crate::split::split_xml`].
///
/// This:
/// 1. Reads the application XML
/// 2. Computes the registration-relevant hash
/// 3. Patches the `Hash` attribute with `Base64(MD5)`
/// 4. Replaces the fingerprint in all `Id`/`RefId` attributes and the filename
/// 5. Writes the patched XML back (with the new filename)
///
/// Returns the updated [`SplitResult`] with the new application path.
///
/// # Errors
///
/// Returns [`KnxprodError`] if the file cannot be read/written or hashing fails.
pub fn sign_application(split: &SplitResult) -> Result<SplitResult, KnxprodError> {
    let app_path = &split.application;
    let xml = std::fs::read_to_string(app_path).map_err(|e| KnxprodError::io(app_path, e))?;

    // Compute hash on the original XML (before patching).
    let hash = hash_application_program(&xml)?;
    let new_hash_b64 = hash.hash_base64();
    let new_fingerprint = hash.fingerprint_hex();

    // Extract the old fingerprint from the filename.
    // Filename pattern: M-XXXX_A-YYYY-ZZ-FFFF.xml or M-XXXX_A-YYYY-ZZ-FFFF-OSUFFIX.xml
    let filename = app_path
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| KnxprodError::InvalidStructure("invalid application filename".into()))?;

    let old_fingerprint = extract_fingerprint(filename).ok_or_else(|| {
        KnxprodError::InvalidStructure(format!(
            "cannot extract fingerprint from filename: {filename}"
        ))
    })?;

    // Compile the fingerprint regex once — reused for the app and every sibling.
    let fp_re = fingerprint_regex(&old_fingerprint);

    // Patch the application XML: Hash attribute, then the fingerprint in its Id
    // and all internal refs.
    let patched = patch_hash_attribute(&xml, &new_hash_b64);
    let patched = patch_fingerprint(&fp_re, &patched, &new_fingerprint);

    // Compute new filename
    let new_filename = filename.replace(&old_fingerprint, &new_fingerprint);
    let new_path = app_path.with_file_name(&new_filename);

    // Write patched XML to the new path, then remove the old one. Propagate a
    // removal failure: a leftover old app XML would be re-patched by the
    // sibling loop below and archived alongside the new one, yielding a
    // duplicate/ETS-rejected `.knxprod`.
    std::fs::write(&new_path, patched.as_bytes()).map_err(|e| KnxprodError::io(&new_path, e))?;
    if new_path != *app_path {
        std::fs::remove_file(app_path).map_err(|e| KnxprodError::io(app_path, e))?;
    }

    // The same fingerprint appears in the Catalog/Hardware (and Baggages) files —
    // as ApplicationProgramRef (`_A-`) and Hardware2Program (`_HP-`) ids. ETS keys
    // its lookup dictionaries on these, so they must be patched to the new
    // fingerprint too, or import fails with "the given key was not present in the
    // dictionary".
    let manu_dir = app_path
        .parent()
        .ok_or_else(|| KnxprodError::InvalidStructure("application path has no parent".into()))?;
    for entry in std::fs::read_dir(manu_dir).map_err(|e| KnxprodError::io(manu_dir, e))? {
        let sibling = entry.map_err(|e| KnxprodError::io(manu_dir, e))?.path();
        if sibling == new_path
            || !sibling.is_file()
            || sibling
                .extension()
                .and_then(|s| s.to_str())
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("xml"))
        {
            continue;
        }
        let content =
            std::fs::read_to_string(&sibling).map_err(|e| KnxprodError::io(&sibling, e))?;
        let fixed = patch_fingerprint(&fp_re, &content, &new_fingerprint);
        if fixed != content {
            std::fs::write(&sibling, fixed.as_bytes())
                .map_err(|e| KnxprodError::io(&sibling, e))?;
        }
    }

    Ok(SplitResult {
        catalog: split.catalog.clone(),
        hardware: split.hardware.clone(),
        application: new_path,
    })
}

/// Extract the 4-char hex fingerprint from a filename like `M-0083_A-00B0-32-0DFC.xml`.
fn extract_fingerprint(filename: &str) -> Option<String> {
    let re = Regex::new(r"_A-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{2}-([0-9A-Fa-f]{4})").ok()?;
    re.captures(filename).map(|c| c[1].to_string())
}

/// Replace the `Hash="..."` attribute value in the `ApplicationProgram` element.
#[allow(clippy::expect_used)]
fn patch_hash_attribute(xml: &str, new_hash: &str) -> String {
    // Anchored to ApplicationProgram to avoid patching Hash on other elements
    let re = Regex::new(r#"(<ApplicationProgram[^>]*?)Hash="[^"]*""#).expect("valid regex");
    re.replace(xml, format!("${{1}}Hash=\"{new_hash}\""))
        .into_owned()
}

/// Build the fingerprint-replacement regex for `old_fp`, matching both
/// application-program (`_A-XXXX-YY-FFFF`) and hardware-to-program
/// (`_HP-XXXX-YY-FFFF`) ids. Both embed the app fingerprint and ETS keys its
/// lookup dictionaries on them; the regex targets only the 4-hex-char
/// fingerprint position, not arbitrary occurrences.
#[allow(clippy::expect_used)]
fn fingerprint_regex(old_fp: &str) -> Regex {
    let escaped = regex::escape(old_fp);
    let pattern = format!(r"(?i)(_(?:A|HP)-[0-9A-Fa-f]{{4}}-[0-9A-Fa-f]{{2}}-){escaped}");
    Regex::new(&pattern).expect("valid regex")
}

/// Replace the matched fingerprint with `new_fp` using a pre-compiled regex.
fn patch_fingerprint(re: &Regex, xml: &str, new_fp: &str) -> String {
    re.replace_all(xml, format!("${{1}}{new_fp}")).into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::hash::hash_application_program;

    #[test]
    fn sign_patches_hash_and_fingerprint() {
        // Use the MDT leakage sensor XML
        let xml = include_str!("../tests/fixtures/leakage_app.xml");
        let original_hash = hash_application_program(xml).unwrap();

        // Write to temp file
        let dir = tempfile::tempdir().unwrap();
        let app_path = dir.path().join("M-0083_A-014F-10-0000.xml");
        std::fs::write(&app_path, xml).unwrap();

        let split = SplitResult {
            catalog: dir.path().join("Catalog.xml"),
            hardware: dir.path().join("Hardware.xml"),
            application: app_path,
        };

        let result = sign_application(&split).unwrap();

        // Verify the file was renamed with the correct fingerprint
        let new_name = result.application.file_name().unwrap().to_str().unwrap();
        assert!(
            new_name.contains(&original_hash.fingerprint_hex()),
            "filename should contain fingerprint {}, got {new_name}",
            original_hash.fingerprint_hex()
        );

        // Verify the Hash attribute was patched
        let patched_xml = std::fs::read_to_string(&result.application).unwrap();
        assert!(
            patched_xml.contains(&format!("Hash=\"{}\"", original_hash.hash_base64())),
            "XML should contain Hash attribute"
        );

        // Note: re-hashing the patched XML gives a DIFFERENT hash because
        // the fingerprint replacement changed Id values throughout the XML.
        // This is expected — ETS computes the hash once and patches once.
    }

    #[test]
    fn sign_patches_fingerprint_in_catalog_and_hardware() {
        let xml = include_str!("../tests/fixtures/leakage_app.xml");
        let fp = hash_application_program(xml).unwrap().fingerprint_hex();

        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("M-0083_A-014F-10-0000.xml");
        std::fs::write(&app, xml).unwrap();
        let catalog = dir.path().join("Catalog.xml");
        let hardware = dir.path().join("Hardware.xml");
        std::fs::write(
            &catalog,
            r#"<CatalogItem Id="M-0083_H-1_HP-014F-10-0000_CI-1" Hardware2ProgramRefId="M-0083_H-1_HP-014F-10-0000" />"#,
        )
        .unwrap();
        std::fs::write(
            &hardware,
            r#"<Hardware2Program Id="M-0083_H-1_HP-014F-10-0000"><ApplicationProgramRef RefId="M-0083_A-014F-10-0000" /></Hardware2Program>"#,
        )
        .unwrap();

        let split = SplitResult {
            catalog: catalog.clone(),
            hardware: hardware.clone(),
            application: app,
        };
        sign_application(&split).unwrap();

        // The stale `-0000` fingerprint must be gone from BOTH the `_A-` (app ref)
        // and `_HP-` (hardware-to-program) ids in Catalog and Hardware.
        let c = std::fs::read_to_string(&catalog).unwrap();
        let h = std::fs::read_to_string(&hardware).unwrap();
        assert!(
            c.contains(&format!("HP-014F-10-{fp}")),
            "catalog HP id patched"
        );
        assert!(
            !c.contains("HP-014F-10-0000"),
            "no stale fp left in catalog"
        );
        assert!(
            h.contains(&format!("_A-014F-10-{fp}")),
            "hardware app ref patched"
        );
        assert!(
            h.contains(&format!("HP-014F-10-{fp}")),
            "hardware HP id patched"
        );
        assert!(!h.contains("-0000"), "no stale fp left in hardware");
    }
}
