// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX demo device — demonstrates the full knx-rs stack.
//!
//! An ETS-programmable KNX IP device with 4 group objects:
//! - GO 1: Temperature sensor (DPT 9.001) — publishes simulated value every 5s
//! - GO 2: Switch (DPT 1.001) — receives on/off from bus
//! - GO 3: Dimmer (DPT 5.001) — receives percentage from bus
//! - GO 4: Display text (DPT 16.001) — receives string from bus
//!
//! Usage: `knx-demo-device [--address 1.1.10]`

use std::net::Ipv4Addr;

use knx_core::dpt::DptValue;
use knx_device::bau::Bau;
use knx_device::group_object::ComFlag;
use knx_ip::tunnel_server::{DeviceServer, ServerEvent};

use knx_demo_device::create_demo_bau;

fn log_updated_group_objects(bau: &mut Bau) {
    while let Some(asap) = bau.group_objects.next_updated() {
        if let Some(go) = bau.group_objects.get(asap) {
            match asap {
                1 => {
                    if let Some(temp) = go.value().ok().and_then(|v| v.as_f64()) {
                        tracing::info!(
                            ga = "1/0/1",
                            value = format!("{temp:.1}°C"),
                            "Temperature updated"
                        );
                    }
                }
                2 => {
                    if let Some(on) = go.value().ok().and_then(|v| v.as_bool()) {
                        tracing::info!(
                            ga = "1/0/2",
                            state = if on { "ON" } else { "OFF" },
                            "Switch updated"
                        );
                    }
                }
                3 => {
                    if let Some(val) = go.value().ok().and_then(|v| v.as_f64()) {
                        tracing::info!(
                            ga = "1/0/3",
                            percent = format!("{val:.0}%"),
                            "Dimmer updated"
                        );
                    }
                }
                4 => {
                    if let Some(text) = go.value().ok().and_then(|v| v.as_str().map(String::from)) {
                        tracing::info!(ga = "1/0/4", text, "Display text updated");
                    }
                }
                _ => {}
            }
        }
        if let Some(go) = bau.group_objects.get_mut(asap) {
            go.set_comm_flag(ComFlag::Ok);
        }
    }
}

async fn flush_outgoing(bau: &mut Bau, server: &DeviceServer) {
    bau.poll(0);
    while let Some(frame) = bau.next_outgoing_frame() {
        if let Err(e) = server.send_frame(frame).await {
            tracing::warn!(error = %e, "Failed to send frame");
        }
    }
}

fn parse_address(s: &str) -> u16 {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() == 3 {
        let area: u16 = parts[0].parse().unwrap_or(15);
        let line: u16 = parts[1].parse().unwrap_or(15);
        let device: u16 = parts[2].parse().unwrap_or(255);
        (area << 12) | (line << 8) | device
    } else {
        0xFFFF
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let address = args
        .windows(2)
        .find(|w| w[0] == "--address")
        .map(|w| parse_address(&w[1]))
        .unwrap_or(0xFFFF);

    let mut bau = create_demo_bau(address);

    tracing::info!(
        address = %bau.individual_address(),
        "KNX demo device starting"
    );
    tracing::info!("  GO 1: Temperature (1/0/1, DPT 9.001) — publishes every 5s");
    tracing::info!("  GO 2: Switch      (1/0/2, DPT 1.001) — receives on/off");
    tracing::info!("  GO 3: Dimmer      (1/0/3, DPT 5.001) — receives percentage");
    tracing::info!("  GO 4: Text        (1/0/4, DPT 16.001) — receives string");

    let server = DeviceServer::start(Ipv4Addr::UNSPECIFIED).await?;
    tracing::info!("Device server listening on port 3671");

    let mut temp_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
    let mut tick: u64 = 0;
    let mut server = server;

    loop {
        tokio::select! {
            Some(event) = server.recv() => {
                match event {
                    ServerEvent::TunnelFrame(frame) | ServerEvent::RoutingFrame(frame) => {
                        tracing::debug!(
                            src = %frame.source_address(),
                            dst = %frame.destination_address(),
                            "Received frame"
                        );
                        bau.process_frame(&frame, 0);
                        log_updated_group_objects(&mut bau);
                        flush_outgoing(&mut bau, &server).await;
                    }
                }
            }

            _ = temp_interval.tick() => {
                let temp = 22.0 + 4.0 * libm::sin(tick as f64 * 0.2);
                tick += 1;
                if let Some(go) = bau.group_objects.get_mut(1) {
                    let _ = go.set_value(&DptValue::Float(temp));
                }
                flush_outgoing(&mut bau, &server).await;
                tracing::info!(temp = format!("{temp:.1}°C"), ga = "1/0/1", "Publishing temperature");
            }
        }
    }
}
