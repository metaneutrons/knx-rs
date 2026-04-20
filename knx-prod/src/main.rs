// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! CLI for generating .knxprod files from KNX product XML.

use std::path::PathBuf;
use std::process;

use clap::Parser;

/// Cross-platform .knxprod generator for KNX ETS product databases.
#[derive(Parser)]
#[command(name = "knx-prod", version, about)]
struct Cli {
    /// Input KNX product XML file.
    input: PathBuf,

    /// Output .knxprod file path.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let output = cli.output.unwrap_or_else(|| {
        let stem = cli.input.file_stem().unwrap_or_default().to_string_lossy();
        PathBuf::from(format!("{stem}.knxprod"))
    });

    eprintln!("Input:  {}", cli.input.display());
    eprintln!("Output: {}", output.display());

    match knx_prod::generate_knxprod(&cli.input, &output) {
        Ok(meta) => {
            eprintln!("Manufacturer: {}", meta.manufacturer_id);
            eprintln!("Application:  {}", meta.application_id);
            eprintln!("Namespace:    project/{}", meta.ns_version);
            eprintln!("Done.");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}
