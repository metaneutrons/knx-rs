// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Cross-platform .knxprod generator for KNX ETS product databases.
//!
//! Replaces the Windows-only ETS DLLs for generating .knxprod files.
//! Takes a monolithic KNX product XML (as produced by `OpenKNXproducer`)
//! and generates a signed .knxprod ZIP archive importable by ETS.
//!
//! # Pipeline
//!
//! 1. **Parse** — extract metadata (namespace, manufacturer ID, application ID)
//! 2. **Split** — split monolithic XML into Catalog.xml, Hardware.xml, Application.xml
//! 3. **Sign** — hash and sign XML files (not yet implemented)
//! 4. **Package** — ZIP into .knxprod
//!
//! # Example
//!
//! ```rust,no_run
//! use std::path::Path;
//! use knx_prod::generate_knxprod;
//!
//! generate_knxprod(
//!     Path::new("NeoPixel.xml"),
//!     Path::new("NeoPixel.knxprod"),
//! ).expect("failed to generate knxprod");
//! ```

pub mod archive;
pub mod error;
pub mod hash;
pub mod parse;
pub mod sign;
pub mod split;

use std::path::Path;

use error::KnxprodError;

/// Generate a .knxprod file from a KNX product XML.
///
/// This is the main entry point. It parses the input XML, splits it into
/// separate files, and packages them into a .knxprod ZIP archive.
///
/// # Errors
///
/// Returns [`KnxprodError`] if any step fails.
pub fn generate_knxprod(input: &Path, output: &Path) -> Result<parse::KnxMetadata, KnxprodError> {
    let xml = std::fs::read_to_string(input).map_err(|e| KnxprodError::io(input, e))?;
    let metadata = parse::extract_metadata_from_str(&xml)?;

    let temp_dir = tempfile::tempdir().map_err(|e| KnxprodError::io(input, e))?;

    let split_result = split::split_xml(&xml, &metadata, temp_dir.path())?;

    let signed_result = sign::sign_application(&split_result)?;

    archive::create_knxprod(temp_dir.path(), output)?;

    // Update metadata with the new application ID (with correct fingerprint).
    let new_app_id = signed_result
        .application
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&metadata.application_id)
        .to_string();

    Ok(parse::KnxMetadata {
        application_id: new_app_id,
        ..metadata
    })
}
