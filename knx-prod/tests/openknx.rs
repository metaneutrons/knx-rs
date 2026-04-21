#![allow(clippy::expect_used, clippy::doc_markdown, clippy::format_collect)]
// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Integration tests against real OpenKNX release XMLs.
//!
//! These tests require large fixture files (~50-65 MB each) that are not
//! stored in the repository. Fetch them first:
//!
//! ```sh
//! ./scripts/fetch-openknx-fixtures.sh
//! ```
//!
//! Then run:
//!
//! ```sh
//! cargo test -p knx-prod --test openknx
//! ```

use std::path::Path;

use knx_prod::hash::hash_application_program;

fn read_fixture(name: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openknx")
        .join(name);
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        eprintln!(
            "SKIP: {name} not found. Run scripts/fetch-openknx-fixtures.sh to download fixtures."
        );
        None
    }
}

/// OpenKNX SmartHomeBridge v2.1.1 — 62 MB XML, 18.3M prebytes.
/// Verified against C# reference output.
#[test]
fn smarthomebridge_hash() {
    let Some(xml) = read_fixture("SmartHomeBridge.xml") else {
        return;
    };
    let result = hash_application_program(&xml).expect("hash");
    let hash: String = result.md5.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hash, "70b0ca899be3f8a4ad8e5dd9f4856d67");
    assert_eq!(result.fingerprint_hex(), "7067");
}

/// OpenKNX LogicModule v3.5.2 — 51 MB XML, 16.3M prebytes.
/// Verified against C# reference output.
#[test]
fn logicmodule_hash() {
    let Some(xml) = read_fixture("LogicModule.xml") else {
        return;
    };
    let result = hash_application_program(&xml).expect("hash");
    let hash: String = result.md5.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hash, "192e408ebcea7766a9cd1d45f90b3416");
    assert_eq!(result.fingerprint_hex(), "1916");
}
