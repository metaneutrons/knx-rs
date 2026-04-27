// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! ZIP packaging for .knxprod files.

use std::fs;
use std::io::Write;
use std::path::Path;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::error::KnxprodError;

/// Package a directory into a .knxprod (ZIP) file.
///
/// All files in `source_dir` are added to the ZIP with their relative paths preserved.
///
/// # Errors
///
/// Returns [`KnxprodError`] if files cannot be read or the ZIP cannot be written.
pub fn create_knxprod(source_dir: &Path, output: &Path) -> Result<(), KnxprodError> {
    let file = fs::File::create(output).map_err(|e| KnxprodError::io(output, e))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_directory_recursive(&mut zip, source_dir, source_dir, options)?;

    zip.finish().map_err(KnxprodError::Zip)?;
    Ok(())
}

fn add_directory_recursive<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    base: &Path,
    dir: &Path,
    options: SimpleFileOptions,
) -> Result<(), KnxprodError> {
    let entries = fs::read_dir(dir).map_err(|e| KnxprodError::io(dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| KnxprodError::io(dir, e))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .map_err(|e| KnxprodError::InvalidStructure(e.to_string()))?;
        let name = relative.to_string_lossy();

        if path.is_dir() {
            add_directory_recursive(zip, base, &path, options)?;
        } else {
            zip.start_file(name, options).map_err(KnxprodError::Zip)?;
            let content = fs::read(&path).map_err(|e| KnxprodError::io(&path, e))?;
            zip.write_all(&content)
                .map_err(|e| KnxprodError::io(&path, e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn create_knxprod_produces_valid_zip() {
        let dir = tempfile::tempdir().unwrap();
        let manu_dir = dir.path().join("M-00FA");
        fs::create_dir_all(&manu_dir).unwrap();
        fs::write(manu_dir.join("Catalog.xml"), "<Catalog/>").unwrap();
        fs::write(manu_dir.join("Hardware.xml"), "<Hardware/>").unwrap();

        let output = dir.path().join("test.knxprod");
        create_knxprod(dir.path(), &output).unwrap();

        // Verify it's a valid ZIP
        let file = fs::File::open(&output).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(names.iter().any(|n| n.contains("Catalog.xml")));
        assert!(names.iter().any(|n| n.contains("Hardware.xml")));
    }

    #[test]
    fn zip_preserves_directory_structure() {
        let source = tempfile::tempdir().unwrap();
        let manu_dir = source.path().join("M-00FA");
        fs::create_dir_all(&manu_dir).unwrap();
        fs::write(manu_dir.join("test.xml"), "content").unwrap();

        let out_dir = tempfile::tempdir().unwrap();
        let output = out_dir.path().join("test.knxprod");
        create_knxprod(source.path(), &output).unwrap();

        let file = fs::File::open(&output).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("M-00FA/")),
            "no M-00FA/ entry found in: {names:?}"
        );
    }
}
