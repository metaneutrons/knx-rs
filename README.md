# knx-rs

[![CI](https://github.com/metaneutrons/knx-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/metaneutrons/knx-rs/actions/workflows/ci.yml)
[![knx-core crates.io](https://img.shields.io/crates/v/knx-core.svg?label=knx-core)](https://crates.io/crates/knx-core)
[![knx-ip crates.io](https://img.shields.io/crates/v/knx-ip.svg?label=knx-ip)](https://crates.io/crates/knx-ip)
[![knx-device crates.io](https://img.shields.io/crates/v/knx-device.svg?label=knx-device)](https://crates.io/crates/knx-device)
[![docs.rs](https://img.shields.io/docsrs/knx-core)](https://docs.rs/knx-core)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
[![MSRV: 1.85](https://img.shields.io/badge/MSRV-1.85-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

A platform-independent KNX protocol stack in Rust — for embedded devices, servers, and everything in between.

## Crates

| Crate | Description | `no_std` |
|-------|-------------|----------|
| **[knx-core](knx-core/)** | Protocol types, CEMI frames, DPT conversions, KNXnet/IP frame types | ✅ |
| **[knx-ip](knx-ip/)** | Async KNXnet/IP tunnel, router, discovery, and device server (tokio) | ❌ |
| **[knx-device](knx-device/)** | KNX device stack — group objects, ETS programming, BAU | ✅ |
| **[knx-tp](knx-tp/)** | TP-UART data link layer for embedded targets *(WIP)* | ✅ |

## Features

### knx-core

- **Addresses** — `IndividualAddress` (1.1.1), `GroupAddress` (1/0/1), with `Display`, `FromStr`, optional `serde`
- **CEMI frames** — parse and serialize with full read/write access to all control fields
- **TPDU / APDU** — structured PDU types with all ~60 APCI service codes
- **DPT conversions** — 34 main groups, 100% parity with the C++ reference implementation
- **KNXnet/IP types** — frame header, service types, connection header, HPAI
- **`no_std` + `alloc`** — runs on embedded targets (ARM Cortex-M, RISC-V)

### knx-ip

- **Tunnel connection** — connect handshake, 3× retry, heartbeat, auto-reconnect
- **Router connection** — multicast routing with rate limiting (50 pkt/s per KNX spec)
- **Device server** — accept incoming tunnel connections from ETS on port 3671, simultaneous multicast routing and unicast tunneling
- **Discovery** — search request/response for finding gateways on the local network
- **Multiplexer** — fan out one connection into multiple independent handles
- **URL parsing** — `udp://`, `tunnel://`, `router://` with multicast auto-detection

### knx-device

- **Property system** — data-backed and callback-backed properties with `const` metadata
- **Interface objects** — device object, application program, with unified indexed access
- **Table objects** — address table, association table, group object table (ETS-loadable)
- **Group objects** — `ComFlag` state machine, DPT-aware values (`value_as_f64`, `set_value_if_changed`), update callbacks
- **Bus Access Unit (BAU)** — processes CEMI frames, handles `GroupValueRead/Write`, `PropertyValueRead/Write`, `MemoryRead/Write`, `DeviceDescriptorRead`, `IndividualAddressWrite`, connected-mode transport
- **Memory management** — `MemoryBackend` trait, RAM backend, C++-compatible persistence format
- **`no_std` + `alloc`** — runs on embedded targets

## Quick Start

### Client: read from a KNX gateway

```rust
use knx_core::dpt::{self, DPT_VALUE_TEMP};
use knx_ip::{KnxConnection, connect, parse_url};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = parse_url("udp://192.168.1.50:3671")?;
    let mut conn = connect(spec).await?;

    while let Some(frame) = conn.recv().await {
        if let Ok(temp) = dpt::decode(DPT_VALUE_TEMP, frame.payload()) {
            println!("{}: {temp:.1}°C", frame.destination_address());
        }
    }
    Ok(())
}
```

### Device: ETS-programmable KNX IP device

```rust
use std::net::Ipv4Addr;
use knx_device::{bau::Bau, device_object, group_object::GroupObject};
use knx_ip::tunnel_server::{DeviceServer, ServerEvent};
use knx_core::dpt::DPT_VALUE_TEMP;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = device_object::new_device_object(
        [0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], // serial
        [0x00; 6],                               // hardware type
    );
    let mut bau = Bau::new(device, 10, 2);
    let mut server = DeviceServer::start(Ipv4Addr::UNSPECIFIED).await?;

    loop {
        tokio::select! {
            Some(event) = server.recv() => {
                match event {
                    ServerEvent::TunnelFrame(frame)
                    | ServerEvent::RoutingFrame(frame) => {
                        bau.process_frame(&frame);
                        bau.poll();
                        while let Some(out) = bau.next_outgoing_frame() {
                            server.send_frame(out).await?;
                        }
                    }
                }
            }
        }
    }
}
```

## DPT Coverage

All 34 main groups from the C++ reference are supported:

| DPT | Type | DPT | Type |
|-----|------|-----|------|
| 1 | Boolean | 17 | Scene number |
| 2 | Controlled boolean | 18 | Scene control |
| 3 | Controlled step | 19 | Date and time |
| 4 | Character | 26 | Scene info |
| 5 | Unsigned 8-bit | 27 | 32-bit field |
| 6 | Signed 8-bit | 28 | Unicode string |
| 7 | Unsigned 16-bit | 29 | Signed 64-bit |
| 8 | Signed 16-bit | 217 | Version |
| 9 | 16-bit float | 219 | Alarm info |
| 10 | Time of day | 221 | Serial number |
| 11 | Date | 225 | Scaling speed |
| 12 | Unsigned 32-bit | 231 | Locale |
| 13 | Signed 32-bit | 232 | RGB |
| 14 | IEEE 754 float | 234 | Language code |
| 15 | Access data | 235 | Active energy |
| 16 | String (ASCII/Latin-1) | 238/239/251 | Scene config / Flagged scaling / RGBW |

## Testing

184 tests validated against the [OpenKNX/knx](https://github.com/OpenKNX/knx) C++ reference stack:

- **Golden test vectors** — C++ harness (`test-vectors/generate.cpp`) generates JSON fixtures for CEMI frames, CEMI setters, and DPT conversions, verified byte-for-byte in Rust
- **Integration tests** — tunnel server ↔ client on real UDP loopback (connect, heartbeat, frame exchange, disconnect)
- **Unit tests** — every protocol layer, state machine, and parser

```sh
# Run all tests
cargo test -- --test-threads=1

# Run with all features
cargo test -p knx-core --all-features

# Verify no_std
cargo check -p knx-core --no-default-features --target thumbv7em-none-eabihf
```

## Architecture

```
Application code ←→ GroupObjects ←→ BAU ←→ DeviceServer (port 3671)
                                     ↕           ↕            ↕
                              InterfaceObjects  Multicast    Tunnel
                                     ↕         (routing)   (ETS)
                                DeviceMemory
```

## Acknowledgements

This project builds on the work of the [OpenKNX](https://github.com/OpenKNX) community and the original [thelsing/knx](https://github.com/thelsing/knx) C++ stack by Thomas Kunze. The DPT conversion logic, CEMI frame layout, and protocol constants are derived from the [OpenKNX/knx](https://github.com/OpenKNX/knx) fork (v2.3.1), which is maintained by the OpenKNX team.

We are grateful for their work in creating and maintaining an open-source KNX device stack that made this Rust reimplementation possible.

## License

GPL-3.0-only — see [LICENSE](LICENSE).
