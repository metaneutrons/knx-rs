// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! CLI for generating .knxprod files from KNX product XML.

use std::path::PathBuf;
use std::process;

use clap::Parser;

/// Cross-platform .knxprod builder for KNX ETS product databases.
#[derive(Parser)]
#[command(name = "knx-rs-prod", version, about)]
struct Cli {
    /// Input KNX product XML file.
    input: PathBuf,

    /// Output .knxprod file path.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// RSA signing key (PEM or .NET `<RSAKeyValue>` XML) extracted from your OWN
    /// licensed ETS installation. Produces a signed, ETS-importable archive.
    #[arg(long, value_name = "FILE")]
    key: Option<PathBuf>,

    /// Path to a `knx_master.xml` to embed (used with `--key`).
    #[arg(long, value_name = "FILE")]
    knx_master: Option<PathBuf>,

    /// Download `knx_master.xml` from update.knx.org (used with `--key`).
    /// Requires `--features fetch`.
    #[arg(long)]
    fetch_master: bool,

    /// Renumber all `ApplicationProgram` id suffixes to pure integers and run the
    /// structural sanity check before splitting/signing. Required for XML that
    /// uses readable string id suffixes (e.g. `_UP-Z01000`), which ETS rejects
    /// at import (`'G' is not a legal digit for base 10`).
    #[arg(long)]
    renumber: bool,

    /// Validate the (renumbered) XML against an ETS `project/NN` XSD before
    /// packaging, via `xmllint` on `PATH`. The schema is ETS-proprietary and
    /// caller-supplied — never bundled.
    #[arg(long, value_name = "FILE")]
    xsd: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let output = cli.output.clone().unwrap_or_else(|| {
        let stem = cli.input.file_stem().unwrap_or_default().to_string_lossy();
        PathBuf::from(format!("{stem}.knxprod"))
    });

    eprintln!("Input:  {}", cli.input.display());
    eprintln!("Output: {}", output.display());

    // Optionally normalise ids (renumber + sanity) and/or XSD-validate first,
    // writing the result to a temp file that becomes the effective input.
    let _tmp; // keeps the temp file alive for the rest of main.
    let input: PathBuf = match prepare_input(&cli) {
        Ok(Some(tmp)) => {
            let p = tmp.path().to_path_buf();
            _tmp = tmp;
            p
        }
        Ok(None) => cli.input.clone(),
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let result = if cli.key.is_some() {
        run_signed(&cli, &input, &output)
    } else {
        knx_rs_prod::generate_knxprod(&input, &output)
    };

    match result {
        Ok(meta) => {
            eprintln!("Manufacturer: {}", meta.manufacturer_id);
            eprintln!("Application:  {}", meta.application_id);
            eprintln!("Namespace:    project/{}", meta.ns_version);
            if cli.key.is_none() {
                eprintln!(
                    "WARNING: unsigned — maybe not importable by ETS. Pass --key to sign (issue #9)."
                );
            }
            eprintln!("Done.");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

/// Normalise ids (`--renumber`) and/or XSD-validate (`--xsd`) the input.
///
/// Returns `Some(tempfile)` holding the transformed XML when `--renumber` is
/// set, or `None` when the original input should be used as-is. When `--xsd` is
/// set, `xmllint` is run against whichever of the two is the effective input.
fn prepare_input(cli: &Cli) -> Result<Option<tempfile::NamedTempFile>, knx_rs_prod::KnxprodError> {
    use knx_rs_prod::KnxprodError;

    let tmp: Option<tempfile::NamedTempFile> = if cli.renumber {
        let xml =
            std::fs::read_to_string(&cli.input).map_err(|e| KnxprodError::io(&cli.input, e))?;
        let normalized = knx_rs_prod::normalize_ids(&xml)?;
        let file = tempfile::Builder::new()
            .suffix(".xml")
            .tempfile()
            .map_err(|e| KnxprodError::io(&cli.input, e))?;
        std::fs::write(file.path(), &normalized).map_err(|e| KnxprodError::io(file.path(), e))?;
        eprintln!("Renumbered ids to integers; sanity check passed.");
        Some(file)
    } else {
        None
    };

    if let Some(xsd) = cli.xsd.as_ref() {
        let target = tmp
            .as_ref()
            .map_or_else(|| cli.input.clone(), |t| t.path().to_path_buf());
        run_xmllint(xsd, &target)?;
    }

    Ok(tmp)
}

/// Validate `xml` against `xsd` using `xmllint` on `PATH`.
fn run_xmllint(
    xsd: &std::path::Path,
    xml: &std::path::Path,
) -> Result<(), knx_rs_prod::KnxprodError> {
    use knx_rs_prod::KnxprodError;

    eprintln!("Validating against XSD: {}", xsd.display());
    let out = process::Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(xsd)
        .arg(xml)
        .output()
        .map_err(|e| {
            KnxprodError::Validation(format!(
                "could not run xmllint (is libxml2 installed and on PATH?): {e}"
            ))
        })?;
    if out.status.success() {
        eprintln!("XSD: valid.");
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // xmllint exit codes: 3/4 = the document failed validation; anything else
    // (1 usage, 5 schema-compile error, …) is a tooling/config problem, not a defect
    // in the product XML — don't mislabel it "validation failed".
    match out.status.code() {
        Some(3 | 4) => Err(KnxprodError::Validation(stderr)),
        other => Err(KnxprodError::Validation(format!(
            "xmllint tooling/config error (exit {}) — check the --xsd path and libxml2 install:\n{stderr}",
            other.map_or_else(|| "signal".to_string(), |c| c.to_string()),
        ))),
    }
}

/// Run the signing pipeline (key + `knx_master.xml`).
fn run_signed(
    cli: &Cli,
    input: &std::path::Path,
    output: &std::path::Path,
) -> Result<knx_rs_prod::KnxMetadata, knx_rs_prod::KnxprodError> {
    use knx_rs_prod::KnxprodError;
    use knx_rs_prod::knx_master::KnxMaster;
    use knx_rs_prod::signature::SigningKey;

    let Some(key_path) = cli.key.as_ref() else {
        return Err(KnxprodError::Signing("no --key provided".into()));
    };
    let key = SigningKey::from_path(key_path)?;

    // Resolve knx_master.xml: explicit path, else download (needs `fetch`).
    let master = if let Some(path) = cli.knx_master.as_ref() {
        KnxMaster::from_path(path)?
    } else if cli.fetch_master {
        fetch_master(input)?
    } else {
        return Err(KnxprodError::MasterData(
            "signing needs a knx_master.xml: pass --knx-master <FILE> or --fetch-master".into(),
        ));
    };

    eprintln!("Signing with key: {}", key_path.display());
    knx_rs_prod::generate_signed_knxprod(input, output, &key, &master)
}

/// Download `knx_master.xml` for the input's schema version (needs `fetch`).
#[cfg(feature = "fetch")]
fn fetch_master(
    input: &std::path::Path,
) -> Result<knx_rs_prod::knx_master::KnxMaster, knx_rs_prod::KnxprodError> {
    let xml =
        std::fs::read_to_string(input).map_err(|e| knx_rs_prod::KnxprodError::io(input, e))?;
    let meta = knx_rs_prod::parse::extract_metadata_from_str(&xml)?;
    eprintln!(
        "Fetching {}",
        knx_rs_prod::knx_master::KnxMaster::master_url(meta.ns_version)
    );
    knx_rs_prod::knx_master::KnxMaster::download(meta.ns_version)
}

#[cfg(not(feature = "fetch"))]
fn fetch_master(
    _input: &std::path::Path,
) -> Result<knx_rs_prod::knx_master::KnxMaster, knx_rs_prod::KnxprodError> {
    Err(knx_rs_prod::KnxprodError::MasterData(
        "--fetch-master requires building with `--features fetch`".into(),
    ))
}
