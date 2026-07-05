// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! The top-level `knx_master.xml` a valid `.knxprod` carries at its ZIP root.
//!
//! This is a *verbatim* copy of KNX's official master-data file for the product
//! schema version — not product-specific and not generated. ETS ships it and it
//! carries KNX's own `MasterData/@Signature`; the manufacturer key does not
//! re-sign it. It is publicly downloadable, so bundling it raises no
//! reverse-engineering concern (unlike the `.signature`, see [`crate::signature`]).
//!
//! Obtain it from a local ETS copy via [`KnxMaster::from_path`], or download it
//! with the `fetch` feature via [`KnxMaster::download`].

use std::path::{Path, PathBuf};

use crate::error::KnxprodError;

/// The `knx_master.xml` contents for a given product schema version.
pub struct KnxMaster {
    /// Raw XML, stored verbatim.
    pub xml: String,
}

impl KnxMaster {
    /// The canonical download URL for a given namespace/schema version.
    ///
    /// E.g. `ns_version = 20` → `.../project-20/knx_master.xml`.
    #[must_use]
    pub fn master_url(ns_version: u32) -> String {
        format!("https://update.knx.org/data/XML/project-{ns_version}/knx_master.xml")
    }

    /// Load `knx_master.xml` from a local path (e.g. an ETS `Masters/` cache).
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError`] if the file cannot be read.
    pub fn from_path(path: &Path) -> Result<Self, KnxprodError> {
        let xml = std::fs::read_to_string(path).map_err(|e| KnxprodError::io(path, e))?;
        Ok(Self { xml })
    }

    /// Download `knx_master.xml` for the given schema version over HTTPS.
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError::MasterData`] if the request or read fails.
    #[cfg(feature = "fetch")]
    pub fn download(ns_version: u32) -> Result<Self, KnxprodError> {
        let url = Self::master_url(ns_version);
        let xml = ureq::get(&url)
            .call()
            .map_err(|e| KnxprodError::MasterData(format!("download {url}: {e}")))?
            .into_string()
            .map_err(|e| KnxprodError::MasterData(format!("read {url}: {e}")))?;
        Ok(Self { xml })
    }

    /// Write `knx_master.xml` into the archive staging directory root.
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError`] if the file cannot be written.
    pub fn write_to(&self, output_dir: &Path) -> Result<PathBuf, KnxprodError> {
        let path = output_dir.join("knx_master.xml");
        std::fs::write(&path, &self.xml).map_err(|e| KnxprodError::io(&path, e))?;
        Ok(path)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn master_url_uses_schema_version() {
        assert_eq!(
            KnxMaster::master_url(20),
            "https://update.knx.org/data/XML/project-20/knx_master.xml"
        );
    }

    #[test]
    fn writes_verbatim_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let master = KnxMaster {
            xml: "<KNX/>".into(),
        };
        let path = master.write_to(dir.path()).unwrap();
        assert_eq!(path, dir.path().join("knx_master.xml"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<KNX/>");
    }
}
