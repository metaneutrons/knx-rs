// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Error types for knxprod generation.

use std::path::PathBuf;

/// Errors that can occur during .knxprod generation.
///
/// Further variants may follow in minor releases.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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
    Xml(#[source] XmlError),

    /// ZIP archive error.
    #[error("ZIP error: {0}")]
    Zip(#[source] ArchiveError),

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

    pub(crate) const fn xml(source: quick_xml::Error) -> Self {
        Self::Xml(XmlError(source))
    }

    pub(crate) const fn zip(source: zip::result::ZipError) -> Self {
        Self::Zip(ArchiveError(source))
    }
}

/// An XML document could not be read or written.
///
/// Message and source chain are the XML parser's. The parser's own error type stays
/// out of this crate's API, so a new parser release is not a breaking change here.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct XmlError(quick_xml::Error);

/// A ZIP archive could not be read or written.
///
/// Message and source chain are the ZIP library's. Its error type stays out of this
/// crate's API, so a new ZIP release is not a breaking change here.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ArchiveError(zip::result::ZipError);

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::error::Error as _;

    use super::KnxprodError;

    /// The wrappers keep the libraries out of the API, not out of the message: the text
    /// and the source chain read as they did when the variants held the library errors.
    #[test]
    fn wrapped_errors_read_like_the_library_errors() {
        let inner = zip::result::ZipError::FileNotFound;
        let expected = inner.to_string();
        let err = KnxprodError::zip(inner);
        assert_eq!(err.to_string(), format!("ZIP error: {expected}"));
        assert_eq!(err.source().unwrap().to_string(), expected);

        let mut reader = quick_xml::Reader::from_str("<a></b>");
        let inner = loop {
            match reader.read_event() {
                Err(e) => break e,
                Ok(quick_xml::events::Event::Eof) => panic!("malformed XML parsed"),
                Ok(_) => {}
            }
        };
        let expected = inner.to_string();
        let err = KnxprodError::xml(inner);
        assert_eq!(err.to_string(), format!("XML error: {expected}"));
        assert_eq!(err.source().unwrap().to_string(), expected);
    }
}
