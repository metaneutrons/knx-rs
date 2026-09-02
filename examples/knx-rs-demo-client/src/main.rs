// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX demo client — connects to a KNXnet/IP gateway or device.
//!
//! Demonstrates tunnel and router connections, group read/write,
//! and DPT decoding.
//!
//! # Usage
//!
//! ```sh
//! # Monitor all group traffic via multicast
//! knx-demo-client monitor udp://224.0.23.12:3671
//!
//! # Monitor via tunnel to a gateway
//! knx-demo-client monitor udp://192.168.1.50:3671
//!
//! # Write a switch value
//! knx-demo-client write udp://192.168.1.50:3671 1/0/2 on
//!
//! # Write a dimmer percentage
//! knx-demo-client write udp://192.168.1.50:3671 1/0/3 75
//!
//! # Read a group value
//! knx-demo-client read udp://192.168.1.50:3671 1/0/1
//!
//! # Discover gateways on the local network
//! knx-demo-client discover
//! ```

use std::net::Ipv4Addr;
use std::str::FromStr;

use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::dpt::{self, DPT_SCALING, DPT_SWITCH, DPT_VALUE_TEMP, DptValue};
use knx_rs_core::message::MessageCode;
use knx_rs_core::types::Priority;
use knx_rs_ip::{KnxConnection, connect, parse_url};

fn usage() {
    eprintln!("Usage:");
    eprintln!("  knx-demo-client discover");
    eprintln!("  knx-demo-client monitor <url>");
    eprintln!("  knx-demo-client read <url> <group-address>");
    eprintln!("  knx-demo-client write <url> <group-address> <value>");
    eprintln!();
    eprintln!("URLs: udp://192.168.1.50:3671, udp://224.0.23.12:3671");
    eprintln!("Values: on, off, or 0-100 (percent)");
}

fn parse_ga(s: &str) -> GroupAddress {
    GroupAddress::from_str(s).unwrap_or_else(|_| {
        eprintln!("Invalid group address: {s}");
        std::process::exit(1);
    })
}

fn encode_value(s: &str) -> Vec<u8> {
    match s {
        "on" | "ON" | "1" | "true" => {
            encode_or_exit(s, dpt::encode(DPT_SWITCH, &DptValue::Bool(true)))
        }
        "off" | "OFF" | "0" | "false" => {
            encode_or_exit(s, dpt::encode(DPT_SWITCH, &DptValue::Bool(false)))
        }
        _ => {
            let pct = s.parse::<f64>().unwrap_or_else(|_| {
                eprintln!("Unknown value: {s}");
                std::process::exit(1);
            });
            if !(0.0..=100.0).contains(&pct) {
                eprintln!("Percentage must be between 0 and 100: {s}");
                std::process::exit(1);
            }
            encode_or_exit(s, dpt::encode(DPT_SCALING, &DptValue::Float(pct)))
        }
    }
}

fn encode_or_exit(s: &str, result: Result<Vec<u8>, dpt::DptError>) -> Vec<u8> {
    result.unwrap_or_else(|error| {
        eprintln!("Cannot encode value {s}: {error}");
        std::process::exit(1);
    })
}

fn build_group_write(ga: GroupAddress, data: &[u8]) -> CemiFrame {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.push(0x00); // TPCI
    if data.len() == 1 && data[0] <= 0x3F {
        payload.push(0x80 | (data[0] & 0x3F));
    } else {
        payload.push(0x80);
        payload.extend_from_slice(data);
    }
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        &payload,
    )
}

fn build_group_read(ga: GroupAddress) -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        &[0x00, 0x00],
    )
}

fn decode_and_print(ga: GroupAddress, payload: &[u8]) {
    // Try common DPTs
    if let Some(f) = dpt::decode(DPT_VALUE_TEMP, payload)
        .ok()
        .and_then(|v| v.as_f64())
    {
        println!("  {ga}: {f:.1}°C (DPT 9.001)");
        return;
    }
    if let Some(on) = payload
        .first()
        .filter(|_| payload.len() == 1)
        .and_then(|_| dpt::decode(DPT_SWITCH, payload).ok())
        .and_then(|v| v.as_bool())
    {
        println!("  {ga}: {} (DPT 1.001)", if on { "ON" } else { "OFF" });
        return;
    }
    // Fallback: hex
    print!("  {ga}: ");
    for b in payload {
        print!("{b:02X} ");
    }
    println!();
}

type CommandResult = Result<(), Box<dyn std::error::Error>>;

fn command_arg(args: &[String], index: usize) -> &str {
    args.get(index).map_or_else(
        || {
            usage();
            std::process::exit(1);
        },
        String::as_str,
    )
}

async fn discover() -> CommandResult {
    println!("Searching for KNX gateways...");
    let gateways = knx_rs_ip::discovery::discover(Ipv4Addr::UNSPECIFIED).await?;
    if gateways.is_empty() {
        println!("No gateways found.");
    } else {
        for gateway in &gateways {
            println!(
                "  {} — {} ({})",
                gateway.address,
                gateway.name,
                IndividualAddress::from_raw(gateway.individual_address)
            );
        }
    }
    Ok(())
}

async fn monitor(url: &str) -> CommandResult {
    println!("Connecting to {url}...");
    let spec = parse_url(url)?;
    let mut connection = connect(spec).await?;
    println!("Connected. Monitoring group traffic (Ctrl+C to stop):\n");

    while let Some(frame) = connection.recv().await {
        let destination = frame.destination_address();
        let source = frame.source_address();
        let payload = frame.payload();
        print!("{source} → ");
        if payload.len() < 2 {
            println!("{destination}");
            continue;
        }

        match destination {
            DestinationAddress::Group(group) => decode_and_print(group, &payload[1..]),
            DestinationAddress::Individual(individual) => {
                println!("{individual}: {:02X?}", &payload[1..]);
            }
        }
    }
    Ok(())
}

async fn write(url: &str, address: &str, value: &str) -> CommandResult {
    let group = parse_ga(address);
    let data = encode_value(value);

    println!("Connecting to {url}...");
    let spec = parse_url(url)?;
    let mut connection = connect(spec).await?;
    connection.send(build_group_write(group, &data)).await?;
    println!("Sent GroupValueWrite to {group}: {data:02X?}");
    connection.close().await;
    Ok(())
}

async fn read(url: &str, address: &str) -> CommandResult {
    let group = parse_ga(address);
    println!("Connecting to {url}...");
    let spec = parse_url(url)?;
    let mut connection = connect(spec).await?;
    connection.send(build_group_read(group)).await?;
    println!("Sent GroupValueRead to {group}, waiting for response...");

    let response = tokio::time::timeout(std::time::Duration::from_secs(5), connection.recv()).await;
    match response {
        Ok(Some(frame)) if frame.payload().len() >= 2 => {
            decode_and_print(group, &frame.payload()[1..]);
        }
        Ok(Some(_)) => println!("Received an empty response."),
        Ok(None) => println!("Connection closed before a response arrived."),
        Err(_) => println!("No response received (timeout)."),
    }
    connection.close().await;
    Ok(())
}

#[tokio::main]
async fn main() -> CommandResult {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    match command_arg(&args, 1) {
        "discover" => discover().await?,
        "monitor" => monitor(command_arg(&args, 2)).await?,
        "write" => {
            write(
                command_arg(&args, 2),
                command_arg(&args, 3),
                command_arg(&args, 4),
            )
            .await?;
        }
        "read" => read(command_arg(&args, 2), command_arg(&args, 3)).await?,
        _ => usage(),
    }

    Ok(())
}
