// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Error types for knxprod generation.

use std::path::PathBuf;

/// Errors that can occur during .knxprod generation.
#[derive(Debug, thiserror::Error)]
pub enum KnxprodError {
    /// I/O error reading or writing files.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The file path involved.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// XML parsing error.
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    /// ZIP archive error.
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// Required XML element is missing.
    #[error("missing required element: {0}")]
    MissingElement(&'static str),

    /// Invalid XML structure.
    #[error("invalid XML structure: {0}")]
    InvalidStructure(String),

    /// RSA signing error (key parsing or signature generation).
    #[error("signing error: {0}")]
    Signing(String),

    /// `knx_master.xml` could not be obtained.
    #[error("knx_master error: {0}")]
    MasterData(String),

    /// The product XML failed a structural sanity check (id format, dangling
    /// references, duplicate ids). Reaching this before ETS import turns a
    /// cryptic ETS runtime failure into an actionable build error.
    #[error("validation failed:\n{0}")]
    Validation(String),
}

impl KnxprodError {
    /// Create an I/O error with path context.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
