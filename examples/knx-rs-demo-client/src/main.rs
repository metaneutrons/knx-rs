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
    eprintln!("Values: on, off, 0-100 (percent), or raw hex bytes");
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
            dpt::encode(DPT_SWITCH, &DptValue::Bool(true)).unwrap_or_else(|_| vec![1])
        }
        "off" | "OFF" | "0" | "false" => {
            dpt::encode(DPT_SWITCH, &DptValue::Bool(false)).unwrap_or_else(|_| vec![0])
        }
        _ => {
            if let Ok(pct) = s.parse::<f64>() {
                if pct > 1.0 {
                    dpt::encode(DPT_SCALING, &DptValue::Float(pct))
                        .unwrap_or_else(|_| vec![pct as u8])
                } else {
                    dpt::encode(DPT_SWITCH, &DptValue::Bool(pct != 0.0))
                        .unwrap_or_else(|_| vec![pct as u8])
                }
            } else {
                eprintln!("Unknown value: {s}");
                std::process::exit(1);
            }
        }
    }
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

fn decode_and_print(ga: &GroupAddress, payload: &[u8]) {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return Ok(());
    }

    match args[1].as_str() {
        "discover" => {
            println!("Searching for KNX gateways...");
            let gateways = knx_rs_ip::discovery::discover(Ipv4Addr::UNSPECIFIED).await?;
            if gateways.is_empty() {
                println!("No gateways found.");
            } else {
                for gw in &gateways {
                    println!(
                        "  {} — {} ({})",
                        gw.address,
                        gw.name,
                        knx_rs_core::address::IndividualAddress::from_raw(gw.individual_address)
                    );
                }
            }
        }

        "monitor" => {
            let url = args.get(2).map(String::as_str).unwrap_or_else(|| {
                usage();
                std::process::exit(1);
            });
            println!("Connecting to {url}...");
            let spec = parse_url(url)?;
            let mut conn = connect(spec).await?;
            println!("Connected. Monitoring group traffic (Ctrl+C to stop):\n");

            while let Some(frame) = conn.recv().await {
                let ga = frame.destination_address();
                let src = frame.source_address();
                let payload = frame.payload();
                print!("{src} → {ga}");
                if payload.len() >= 2 {
                    decode_and_print(
                        &match ga {
                            DestinationAddress::Group(g) => g,
                            _ => GroupAddress::from_raw(0),
                        },
                        &payload[1..], // skip TPCI
                    );
                } else {
                    println!();
                }
            }
        }

        "write" => {
            if args.len() < 5 {
                usage();
                return Ok(());
            }
            let url = &args[2];
            let ga = parse_ga(&args[3]);
            let data = encode_value(&args[4]);

            println!("Connecting to {url}...");
            let spec = parse_url(url)?;
            let mut conn = connect(spec).await?;

            let frame = build_group_write(ga, &data);
            conn.send(frame).await?;
            println!("Sent GroupValueWrite to {ga}: {:02X?}", data);
            conn.close().await;
        }

        "read" => {
            if args.len() < 4 {
                usage();
                return Ok(());
            }
            let url = &args[2];
            let ga = parse_ga(&args[3]);

            println!("Connecting to {url}...");
            let spec = parse_url(url)?;
            let mut conn = connect(spec).await?;

            let frame = build_group_read(ga);
            conn.send(frame).await?;
            println!("Sent GroupValueRead to {ga}, waiting for response...");

            let response =
                tokio::time::timeout(std::time::Duration::from_secs(5), conn.recv()).await;

            match response {
                Ok(Some(frame)) => {
                    let payload = frame.payload();
                    if payload.len() >= 2 {
                        decode_and_print(&ga, &payload[1..]);
                    }
                }
                _ => println!("No response received (timeout)."),
            }
            conn.close().await;
        }

        _ => usage(),
    }

    Ok(())
}
