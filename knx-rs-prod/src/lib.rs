// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Cross-platform `.knxprod` builder for KNX ETS product databases.
//!
//! Takes a monolithic KNX product XML — authored with this crate's own [`author`]
//! model or produced by `OpenKNXproducer` / an ETS export — computes the byte-exact
//! ETS registration hash, splits it into the per-file layout, and packages it into a
//! `.knxprod` ZIP archive.
//!
//! # Authoring
//!
//! Rather than hand-writing the product XML, build it code-first with the [`author`]
//! model: register typed parameters, com-objects, parameter types, and the
//! `<Dynamic>` UI tree into an [`author::AppProgram`], then emit the whole document
//! with [`AppProgram::write_knx_document`](author::AppProgram::write_knx_document).
//! Every cross-reference is an opaque handle, so a dangling reference is a compile- or
//! build-time error instead of the opaque `NullReferenceException` ETS reports on
//! import. The [`knxprod!`](macro@crate::knxprod) macro is a declarative front-end to
//! the same model (see [`author::dsl`]). Its output feeds straight into the pipeline
//! below.
//!
//! # Signing
//!
//! An ETS-importable `.knxprod` needs a top-level `knx_master.xml` and an
//! RSA-1024 `M-XXXX.signature`. [`generate_signed_knxprod`] produces both: it
//! RSA-signs the folder — reproducing the ETS `Knx.Ets.XmlSigning.dll` output
//! byte-exact (see [`signature`]) — with a key *you* supply from your own
//! licensed ETS install (this crate **never ships a key**), and embeds a
//! `knx_master.xml` you provide or download (see [`knx_master`]).
//! [`generate_knxprod`] is the unsigned variant (registration hash + package
//! only), e.g. for feeding to `OpenKNXproducer` for the signing step.
//! Tracking: <https://github.com/metaneutrons/knx-rs/issues/9>.
//!
//! # Pipeline
//!
//! 1. **Parse** — extract metadata (namespace, manufacturer ID, application ID)
//! 2. **Split** — split monolithic XML into Catalog.xml, Hardware.xml, Application.xml
//! 3. **Hash** — compute the registration-relevant MD5 hash and patch the fingerprint
//! 4. **Sign** — RSA-sign the `M-XXXX` folder; embed `knx_master.xml` (signed variant)
//! 5. **Package** — ZIP into `.knxprod`
//!
//! # Example
//!
//! ```rust,no_run
//! use std::path::Path;
//! use knx_rs_prod::generate_knxprod;
//!
//! generate_knxprod(
//!     Path::new("NeoPixel.xml"),
//!     Path::new("NeoPixel.knxprod"),
//! ).expect("failed to generate knxprod");
//! ```

// The pipeline modules are public so each stage (split, sign, archive, hash,
// metadata extraction) can be used as a standalone building block, not only via
// the generate_knxprod facade. KnxprodError and KnxMetadata are additionally
// re-exported at the crate root for convenient imports.
pub mod archive;
pub mod author;
pub mod error;
pub mod hash;
pub mod knx_master;
pub mod parse;
pub mod renumber;
pub mod sanity;
pub mod sign;
pub mod signature;
pub mod split;
mod xml_scan;

pub use error::KnxprodError;
pub use parse::KnxMetadata;

use std::path::Path;

/// Normalise a product XML for ETS import: renumber all `ApplicationProgram` id
/// suffixes to pure integers ([`renumber::renumber_ids`]) and then verify the
/// result structurally ([`sanity::sanity_check`]).
///
/// This is the pure-Rust replacement for `OpenKNXproducer`'s renumber/sanity
/// passes. Run it on product XML that uses readable string id suffixes (as
/// snapdog's `xtask` emits) before splitting/signing, otherwise ETS rejects the
/// archive at import time (`'G' is not a legal digit for base 10`).
///
/// # Errors
///
/// Returns [`KnxprodError`] if the `ApplicationProgram` id cannot be extracted
/// ([`KnxprodError::MissingElement`]/[`KnxprodError::Xml`]), the id remap fails
/// ([`KnxprodError::InvalidStructure`] — undeclared parent or non-integer key), or
/// the sanity check finds a problem ([`KnxprodError::Validation`]).
pub fn normalize_ids(xml: &str) -> Result<String, KnxprodError> {
    let renumbered = renumber::renumber_ids(xml)?;
    sanity::sanity_check(&renumbered)?;
    Ok(renumbered)
}

/// Generate a .knxprod file from a KNX product XML.
///
/// This is the main entry point. It parses the input XML, splits it into
/// separate files, and packages them into a .knxprod ZIP archive.
///
/// # Errors
///
/// Returns [`KnxprodError`] if any step fails.
pub fn generate_knxprod(input: &Path, output: &Path) -> Result<KnxMetadata, KnxprodError> {
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

    Ok(KnxMetadata {
        application_id: new_app_id,
        ..metadata
    })
}

/// Generate a **signed** `.knxprod` from a KNX product XML.
///
/// Like [`generate_knxprod`], but additionally RSA-signs the `M-XXXX` folder
/// with the caller-supplied `key` and embeds `master` (`knx_master.xml`) at the
/// archive root — the two artifacts ETS requires for import.
///
/// The `key` must come from the caller's own licensed ETS installation; this
/// crate never bundles one. The signature is reproduced byte-exact against real
/// ETS output — see [`signature`] and <https://github.com/metaneutrons/knx-rs/issues/9>.
///
/// # Errors
///
/// Returns [`KnxprodError`] if any step (parse, split, hash, sign, package) fails.
pub fn generate_signed_knxprod(
    input: &Path,
    output: &Path,
    key: &signature::SigningKey,
    master: &knx_master::KnxMaster,
) -> Result<KnxMetadata, KnxprodError> {
    let xml = std::fs::read_to_string(input).map_err(|e| KnxprodError::io(input, e))?;
    let metadata = parse::extract_metadata_from_str(&xml)?;

    let temp_dir = tempfile::tempdir().map_err(|e| KnxprodError::io(input, e))?;

    let split_result = split::split_xml(&xml, &metadata, temp_dir.path())?;
    let signed = sign::sign_application(&split_result)?;

    // RSA-sign the M-XXXX folder → sibling M-XXXX.signature at the archive root.
    let manu_dir = temp_dir.path().join(&metadata.manufacturer_id);
    signature::sign_directory(&manu_dir, key)?;

    // Embed knx_master.xml at the archive root.
    master.write_to(temp_dir.path())?;

    archive::create_knxprod(temp_dir.path(), output)?;

    let new_app_id = signed
        .application
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&metadata.application_id)
        .to_string();

    Ok(KnxMetadata {
        application_id: new_app_id,
        ..metadata
    })
}
