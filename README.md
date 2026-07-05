# knx-rs

[![CI](https://github.com/metaneutrons/knx-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/metaneutrons/knx-rs/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
[![MSRV: 1.85](https://img.shields.io/badge/MSRV-1.85-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![no_std](https://img.shields.io/badge/no__std-compatible-green.svg)](https://docs.rust-embedded.org/book/)

A platform-independent KNX protocol stack in Rust — for embedded devices, servers, and everything in between.

## Crates

| Crate | Version | Docs | `no_std` | Description |
|-------|---------|------|----------|-------------|
| [knx-rs-core](knx-rs-core/) | [![crates.io](https://img.shields.io/crates/v/knx-rs-core.svg)](https://crates.io/crates/knx-rs-core) | [![docs.rs](https://img.shields.io/docsrs/knx-rs-core)](https://docs.rs/knx-rs-core) | ✅ | Protocol types, CEMI frames, DPT conversions, KNXnet/IP frame types |
| [knx-rs-ip](knx-rs-ip/) | [![crates.io](https://img.shields.io/crates/v/knx-rs-ip.svg)](https://crates.io/crates/knx-rs-ip) | [![docs.rs](https://img.shields.io/docsrs/knx-rs-ip)](https://docs.rs/knx-rs-ip) | ❌ | Async KNXnet/IP tunnel, router, discovery, and device server (tokio) |
| [knx-rs-device](knx-rs-device/) | [![crates.io](https://img.shields.io/crates/v/knx-rs-device.svg)](https://crates.io/crates/knx-rs-device) | [![docs.rs](https://img.shields.io/docsrs/knx-rs-device)](https://docs.rs/knx-rs-device) | ✅ | KNX device stack — group objects, ETS programming, BAU |
| [knx-rs-tp](knx-rs-tp/) | [![crates.io](https://img.shields.io/crates/v/knx-rs-tp.svg)](https://crates.io/crates/knx-rs-tp) | [![docs.rs](https://img.shields.io/docsrs/knx-rs-tp)](https://docs.rs/knx-rs-tp) | ✅ | TP-UART data link layer for embedded targets *(WIP)* |
| [knx-rs-prod](knx-rs-prod/) | [![crates.io](https://img.shields.io/crates/v/knx-rs-prod.svg)](https://crates.io/crates/knx-rs-prod) | [![docs.rs](https://img.shields.io/docsrs/knx-rs-prod)](https://docs.rs/knx-rs-prod) | ❌ | `.knxprod` builder — registration hash, split, RSA signing (bring your own ETS key), and package |

## ⚠️ Migrating from 0.2 to 0.3

**0.3.0 contains breaking API changes.** Cargo treats each `0.x` minor as a breaking
boundary, so a `"0.2"` dependency will *not* pick up `0.3` automatically — bump your
version requirement deliberately and apply the changes below.

### knx-rs-core

- **`KnxIpError` → `KnxIpParseError`.** The frame-parse error was renamed to free the
  name `KnxIpError` for the (unrelated) connection error in `knx-rs-ip`. A
  `#[deprecated]` alias keeps the old name compiling, so existing code still builds
  (with a warning) — migrate references, including variant paths, at your leisure.
- **`Apdu::to_bytes` wire change (correctness):** a single group-value byte `> 0x3F`
  (e.g. a DPT 5 value such as 200) now uses the long form instead of being masked to
  6 bits and losing data. Golden byte-vector expectations for such values change;
  decoding still round-trips both forms.

### knx-rs-ip

- **New `KnxIpError` variants** — `Frame`, `Cemi`, `Dpt`, `Multicast`, `InvalidConfig`.
  Exhaustive `match`es must add arms (or a `_ =>`). Failures that previously surfaced
  as `Protocol(String)` (multicast-join, non-multicast target, frame/DPT errors) now
  use these typed variants, so string-matching on `Protocol` no longer catches them.
- A `pub type Result<T> = core::result::Result<T, KnxIpError>` alias is now exported
  from the crate root; existing `Result<_, KnxIpError>` signatures keep compiling.

### knx-rs-tp *(WIP)*

- **`TpFrame::from_cemi` now returns `Option<Self>`** (`None` when the APDU exceeds the
  frame buffer). Handle the `None` case instead of binding the value directly.
- **`TpIndication` gained an `Overrun` variant** — add a match arm; treat it as a
  recoverable resync event.

### knx-rs-device

- **`DataProperty::access() -> u8` deprecated** (still available) — prefer
  `access_level()` (typed `AccessLevel`), or `access_level() as u8` for the raw byte.
- `SystemNetworkParameterRead.test_info` is now sliced from the correct offset (was
  off by one), so consumers receive the corrected payload.

### knx-rs-prod

- The pipeline modules (`archive`, `parse`, `sign`, `split`, `hash`, `error`) remain
  public; `KnxprodError` and `KnxMetadata` are *additionally* re-exported at the crate
  root, so both `knx_rs_prod::KnxprodError` and `knx_rs_prod::error::KnxprodError` work.
- Two unused helpers were removed: `sign::signed_filename` and `parse::extract_metadata`
  (the `&Path` wrapper). Use `parse::extract_metadata_from_str` or `generate_knxprod`
  (which returns the `KnxMetadata`) instead.
- **Malformed input now errors** instead of producing a wrong-but-valid hash:
  unparseable numeric attributes and invalid base64 program images return
  `KnxprodError::InvalidStructure`. Well-formed product XML is unaffected.

## Features

### knx-rs-core

- **Addresses** — `IndividualAddress` (1.1.1), `GroupAddress` (1/0/1), with `Display`, `FromStr`, optional `serde`
- **CEMI frames** — parse and serialize with full read/write access to all control fields
- **TPDU / APDU** — structured PDU types with all ~60 APCI service codes
- **DPT conversions** — 34 main groups, 100% parity with the C++ reference implementation
- **KNXnet/IP types** — frame header, service types, connection header, HPAI
- **`no_std` + `alloc`** — runs on embedded targets (ARM Cortex-M, RISC-V)

### knx-rs-ip

- **Tunnel connection** — connect handshake, 3× retry, heartbeat, auto-reconnect
- **Router connection** — multicast routing with rate limiting (50 pkt/s per KNX spec)
- **Device server** — accept incoming tunnel connections from ETS on port 3671, simultaneous multicast routing and unicast tunneling
- **Discovery** — search request/response for finding gateways on the local network
- **Multiplexer** — fan out one connection into multiple independent handles
- **URL parsing** — `udp://`, `tunnel://`, `router://` with multicast auto-detection

### knx-rs-device

- **Property system** — data-backed and callback-backed properties with `const` metadata
- **Interface objects** — device object, application program, with unified indexed access
- **Table objects** — address table, association table, group object table (ETS-loadable)
- **Group objects** — `ComFlag` state machine, DPT-aware values, update callbacks
- **Bus Access Unit (BAU)** — processes CEMI frames, handles all KNX application-layer services including connected-mode transport
- **Memory management** — `MemoryBackend` trait, RAM backend, C++-compatible persistence format
- **`no_std` + `alloc`** — runs on embedded targets

### knx-rs-prod

- **Hash** — clean-room Rust reimplementation of the ETS `Knx.Ets.XmlSigning.dll` *registration-hash* algorithm, verified byte-exact against 28 test files from 5 manufacturers
- **Fingerprint** — compute the registration-relevant MD5 hash and patch the resulting fingerprint into the application ID
- **Split** — split monolithic XML into Catalog.xml, Hardware.xml, Application.xml with per-category translation filtering
- **Sign** — RSA-sign the `M-XXXX` folder into an `M-XXXX.signature` and embed `knx_master.xml`, using **a signing key you supply** (PEM or .NET `<RSAKeyValue>` XML) — see [Signing](#signing-bring-your-own-ets-key)
- **Package** — assemble the `M-XXXX/` folder and ZIP it into a `.knxprod`

> **Signing needs your own key — none is bundled.** ETS validates each product against an RSA-1024 signature produced by the closed-source `Knx.Ets.XmlSigning.dll`; the key lives inside that DLL and is not public (even OpenKNXproducer/Kaenx-Creator shell out to it). `knx-rs-prod` ships the signing *algorithm*, not a key: you extract the key from your **own licensed ETS** and pass it in. Without `--key`, the output is unsigned (maybe not importable by ETS). See [issue #9](https://github.com/metaneutrons/knx-rs/issues/9).

## Quick Start

### Client: read from a KNX gateway

```rust
use knx_rs_core::dpt::{self, DPT_VALUE_TEMP};
use knx_rs_ip::{KnxConnection, connect, parse_url};

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
use knx_rs_device::{bau::Bau, device_object, group_object::GroupObject};
use knx_rs_ip::tunnel_server::{DeviceServer, ServerEvent};
use knx_rs_core::dpt::DPT_VALUE_TEMP;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = device_object::new_device_object(
        [0x00, 0xFA, 0x01, 0x02, 0x03, 0x04], // serial
        [0x00; 6],                               // hardware type
    );
    let mut bau = Bau::new(device, 10, 2);
    let mut server = DeviceServer::start(Ipv4Addr::UNSPECIFIED).await?;

    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        tokio::select! {
            Some(event) = server.recv() => {
                match event {
                    ServerEvent::TunnelFrame(frame)
                    | ServerEvent::RoutingFrame(frame) => {
                        bau.process_frame(&frame, now);
                        bau.poll(now);
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

## Generating .knxprod Files

`knx-rs-prod` implements the `.knxprod` pipeline in pure Rust — generating the product XML, computing the byte-exact ETS **registration hash**, RSA-**signing** the product folder (with a key you supply), and packaging the archive — with no .NET, no Wine, and no Windows VM.

```
Rust source code (GO definitions, parameters)
         ↓  cargo xtask generate-xml
   MyDevice.xml (generated, not hand-written)
         ↓  knx-rs-prod --key signing-key.pem --fetch-master
   MyDevice.knxprod  (registration hash + M-XXXX.signature + knx_master.xml)
         ↓
   ETS Import
```

This is the approach used by [SnapDog](https://github.com/metaneutrons/snapdog): a Rust `xtask` reads the group object definitions from the device firmware (SSOT — the same constants that configure the BAU at runtime) and generates the complete ETS product XML. Then `knx-rs-prod` hashes, signs, and packages it. The XML is a build artifact, never hand-edited.

Without `--key`, `knx-rs-prod` still does everything except the RSA signature, producing an unsigned archive (maybe not importable by ETS). Authoring the XML by hand? [OpenKNXproducer](https://github.com/OpenKNX/OpenKNXproducer) remains a fine front end — `knx-rs-prod` replaces only its signing step.

### Signing (bring your own ETS key)

ETS accepts a `.knxprod` only if the `M-XXXX` folder carries a valid RSA-1024 `M-XXXX.signature` and the archive ships a `knx_master.xml`. That signature is produced by the closed-source `Knx.Ets.XmlSigning.dll`, whose signing key is not public — so **`knx-rs-prod` ships the signing algorithm, never a key.** You supply a key extracted from **your own licensed ETS installation** and pass it at runtime; the key never enters this crate. (Signing with key material you are licensed to use, for interoperability, is the supported path; the alternative — obtaining a KNX manufacturer registration so KNX signs — is for commercial products.)

Install and sign (signing is built in; `fetch` is only for auto-downloading the master):

```sh
cargo install knx-rs-prod --features fetch   # or plain `cargo install knx-rs-prod` + --knx-master

# PEM key + explicit master file
knx-rs-prod MyDevice.xml -o MyDevice.knxprod \
    --key signing-key.pem --knx-master knx_master.xml

# key + auto-downloaded master
knx-rs-prod MyDevice.xml -o MyDevice.knxprod \
    --key signing-key.pem --fetch-master
```

The `--key` file may be **PEM** (PKCS#8 or PKCS#1) or the **.NET `<RSAKeyValue>` XML** that `RSA.ToXmlString(true)` emits — the zero-conversion export from a .NET/ETS context. `knx_master.xml` is the public KNX master-data file for your schema version (`--fetch-master` pulls it from `update.knx.org`, or point `--knx-master` at a local copy). As a library, call `knx_rs_prod::generate_signed_knxprod`.

> **Verified byte-exact.** `knx-rs-prod` reproduces the `M-XXXX.signature` that the ETS `Knx.Ets.XmlSigning.dll` produces, byte-for-byte, validated against a reference `.knxprod`. The algorithm: for each file under the folder, `"<relpath>:Base64(SHA1(bytes))"`, sorted by path and joined with `,`, then RSA-PKCS#1-v1.5/SHA-1 signed. One residual caveat — `InvariantCulture` collation of deeply-nested `Baggages\…` names uses ordinal ordering here (correct for the common flat layout). See [issue #9](https://github.com/metaneutrons/knx-rs/issues/9).

### Writing an xtask for XML generation

Create a `xtask/` crate in your workspace that imports your device's GO definitions and generates the XML:

```rust
// xtask/src/main.rs
use std::path::Path;
use my_device::group_objects::{ZONE_GOS, CLIENT_GOS, MAX_ZONES};

fn main() {
    let xml = generate_product_xml();  // builds KNX XML from GO constants
    std::fs::write("MyDevice.xml", &xml).unwrap();

    // Optionally, run knx-rs-prod directly:
    knx_rs_prod::generate_knxprod(
        Path::new("MyDevice.xml"),
        Path::new("MyDevice.knxprod"),
    ).unwrap();
}
```

The key insight: your GO definitions, parameter memory layout, and DPT mappings are `const` data in your firmware crate. The xtask reads them at build time to generate the XML — no duplication, no drift between firmware and ETS configuration.

### Local usage (CLI)

```sh
# Install from crates.io
cargo install knx-rs-prod

# Or build from source
cargo build --release -p knx-rs-prod

# Generate .knxprod from product XML
knx-rs-prod MyDevice.xml -o MyDevice.knxprod
```

### As a library

Add `knx-rs-prod` to your `Cargo.toml` (without the `cli` feature) and call `knx_rs_prod::generate_knxprod()` — see the xtask example above.

### CI Integration

Add `.knxprod` generation to your GitHub Actions workflow — runs on Linux, no Windows runner needed:

```yaml
jobs:
  knxprod:
    name: Generate .knxprod
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      # Option A: xtask generates XML + knxprod in one step
      - run: cargo xtask knxprod

      # Option B: knx-rs-prod CLI on existing XML
      # - run: cargo run --release -p knx-rs-prod -- firmware/MyDevice.xml -o MyDevice.knxprod

      - uses: actions/upload-artifact@v4
        with:
          name: knxprod
          path: "*.knxprod"
```

For release workflows, attach the `.knxprod` as a release asset alongside your firmware binary.

### How the hash works

The `Hash` attribute on `<ApplicationProgram>` is computed by a clean-room Rust reimplementation of the closed-source `Knx.Ets.XmlSigning.dll`. The algorithm was reconstructed through analysis of the ETS signing process and verified byte-exact against 28 test files from 5 manufacturers (MDT, Gira, ABB, Siemens, OpenKNX).

Key aspects: forward-only XML reader with recursively sorted children, .NET `InvariantCulture` string comparison, 89 registration-relevant element types, IEEE 754 double serialization for float attributes, parent-conditional ordering for `ParameterRefRef` elements.

Full documentation: [knx-rs-prod/HASHING.md](knx-rs-prod/HASHING.md)

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

Validated against the [OpenKNX/knx](https://github.com/OpenKNX/knx) C++ reference stack:

- **Golden test vectors** — C++ harness (`test-vectors/generate.cpp`) generates JSON fixtures for CEMI frames, CEMI setters, and DPT conversions, verified byte-for-byte in Rust
- **Integration tests** — tunnel server ↔ client on real UDP loopback (connect, heartbeat, frame exchange, disconnect)
- **Unit tests** — extensive coverage across all crates: protocol types, DPT conversions, parsers, the load state machine, and the BAU service handlers
- **knxprod hash verification** — 28 test files from 5 manufacturers, byte-exact match with ETS DLL output

```sh
# Run all tests
cargo test -- --test-threads=1

# Run with all features
cargo test -p knx-rs-core --all-features

# Verify no_std
cargo check -p knx-rs-core --no-default-features --target thumbv7em-none-eabihf

# knxprod hash tests
cargo test -p knx-rs-prod
```

## Architecture

```
Application code ←→ GroupObjects ←→ BAU ←→ DeviceServer (port 3671)
                                     ↕           ↕            ↕
                              InterfaceObjects  Multicast    Tunnel
                                     ↕         (routing)   (ETS)
                                DeviceMemory

Rust xtask / OpenKNXproducer ──→ Product XML ──→ knx-rs-prod ──→ .knxprod ──→ ETS
```

## Development

```sh
# Build everything
cargo build --workspace

# Run all tests (integration tests need single-threaded)
cargo test -- --test-threads=1

# Clippy (pedantic + nursery)
cargo clippy --workspace

# Format
cargo fmt --all

# Generate docs
cargo doc --no-deps --open

# Check no_std targets
cargo check -p knx-rs-core --no-default-features --target thumbv7em-none-eabihf
cargo check -p knx-rs-device --no-default-features --target thumbv7em-none-eabihf
```

## Acknowledgements

This project builds on the work of the [OpenKNX](https://github.com/OpenKNX) community and the original [thelsing/knx](https://github.com/thelsing/knx) C++ stack by Thomas Kunze. The DPT conversion logic, CEMI frame layout, and protocol constants are derived from the [OpenKNX/knx](https://github.com/OpenKNX/knx) fork (v2.3.1), which is maintained by the OpenKNX team.

The `.knxprod` hashing algorithm was reconstructed through analysis of the `Knx.Ets.XmlSigning.dll` from the ETS distribution. No ETS source code was used — the implementation is a clean-room reimplementation verified against the DLL's output.

We are grateful for the OpenKNX community's work in creating and maintaining an open-source KNX device stack that made this Rust reimplementation possible.

## License

GPL-3.0-only — see [LICENSE](LICENSE).
