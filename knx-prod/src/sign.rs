// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Sign KNX `ApplicationProgram` XML files.
//!
//! Computes the registration-relevant MD5 hash, patches the `Hash` attribute
//! and the fingerprint portion of the `Id` attribute, and renames the file
//! to match the new fingerprint.

use std::path::{Path, PathBuf};

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

    // Patch the XML:
    // 1. Replace Hash="..." attribute value
    let patched = patch_hash_attribute(&xml, &new_hash_b64);
    // 2. Replace old fingerprint with new in _A-XXXX-YY-FFFF pattern only
    let patched = patch_fingerprint(&patched, &old_fingerprint, &new_fingerprint);

    // Compute new filename
    let new_filename = filename.replace(&old_fingerprint, &new_fingerprint);
    let new_path = app_path.with_file_name(&new_filename);

    // Write patched XML (to new path, then remove old if different)
    std::fs::write(&new_path, patched.as_bytes()).map_err(|e| KnxprodError::io(&new_path, e))?;
    if new_path != *app_path {
        let _ = std::fs::remove_file(app_path);
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
    let re = Regex::new(r#"Hash="[^"]*""#).expect("valid regex");
    re.replace(xml, format!("Hash=\"{new_hash}\"")).into_owned()
}

/// Replace the fingerprint in `_A-XXXX-YY-FFFF` patterns throughout the XML.
/// Only targets the specific 4-hex-char fingerprint position, not arbitrary occurrences.
#[allow(clippy::expect_used)]
fn patch_fingerprint(xml: &str, old_fp: &str, new_fp: &str) -> String {
    let pattern = format!(r"(_A-[0-9A-Fa-f]{{4}}-[0-9A-Fa-f]{{2}}-){old_fp}");
    let re = Regex::new(&pattern).expect("valid regex");
    re.replace_all(xml, format!("${{1}}{new_fp}")).into_owned()
}

/// Compute the new application path after signing (for external callers that
/// need to predict the filename).
#[must_use]
pub fn signed_filename(original: &Path, fingerprint: &str) -> PathBuf {
    let filename = original.file_name().and_then(|f| f.to_str()).unwrap_or("");
    extract_fingerprint(filename).map_or_else(
        || original.to_path_buf(),
        |old_fp| original.with_file_name(filename.replace(&old_fp, fingerprint)),
    )
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
}
