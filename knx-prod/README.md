# knx-prod

Cross-platform `.knxprod` generator for KNX ETS product databases — no Windows, no ETS, no .NET required.

## What it does

Takes a monolithic KNX product XML (as produced by [OpenKNXproducer](https://github.com/OpenKNX/OpenKNXproducer)) and generates a signed `.knxprod` ZIP archive importable by ETS.

The pipeline:

1. **Parse** — extract metadata (namespace, manufacturer ID, application ID)
2. **Split** — split monolithic XML into Catalog.xml, Hardware.xml, Application.xml
3. **Sign** — compute registration-relevant MD5 hash, patch fingerprint into IDs
4. **Package** — ZIP into `.knxprod`

## The hard part: hashing

The `Hash` attribute on `<ApplicationProgram>` is computed by the closed-source `Knx.Ets.XmlSigning.dll`. This crate contains a clean-room Rust reimplementation, verified byte-exact against the original C# DLL across **28 test files from 5 manufacturers**:

| Source | Files | Status |
|--------|-------|--------|
| MDT (Leakage, AKK, BE, JAL) | 4 | ✅ |
| Gira (Tastsensor, Busankoppler, Dimmaktor) | 3 | ✅ |
| ABB (SBRU, SBCU, SBSU) | 5 | ✅ |
| Siemens (LK, UP204, RDG, QAA, QFA, QPA, OCT) | 9 | ✅ |
| OpenKNX (SmartHomeBridge, LogicModule) | 2 | ✅ |
| Minimal synthetic | 1 | ✅ |
| + 4 additional prebytes-verified | 4 | ✅ |

All 89 registration-relevant element types from the ETS registry are implemented. See [HASHING.md](HASHING.md) for the full algorithm documentation.

## Usage

### As a library

```rust
use std::path::Path;
use knx_prod::generate_knxprod;

generate_knxprod(
    Path::new("MyDevice.xml"),
    Path::new("MyDevice.knxprod"),
).expect("failed to generate knxprod");
```

### Hash only

```rust
use knx_prod::hash::hash_application_program;

let xml = std::fs::read_to_string("MyDevice.xml").unwrap();
let result = hash_application_program(&xml).unwrap();
println!("MD5:         {}", result.hash_base64());
println!("Fingerprint: {}", result.fingerprint_hex());
```

## Testing

```sh
# Unit tests (fast, all fixtures included)
cargo test -p knx-prod

# OpenKNX integration tests (requires download, ~8s)
./knx-prod/scripts/fetch-openknx-fixtures.sh
cargo test -p knx-prod --test openknx
```

## How the hash works

The algorithm was reconstructed through analysis of the ETS signing process. Key aspects:

- Forward-only XML reader with recursively sorted children at each level
- `.NET InvariantCulture` string comparison for sort order (not ASCII)
- All 89 registration-relevant element types with typed attribute serialization
- Empty `<Script />` elements trigger an overshoot scan across element boundaries
- `TypeFloat` attributes serialized as IEEE 754 doubles
- Parent-conditional ordering for `ParameterRefRef` elements
- CDATA sections, XML entity decoding, `\r\n` normalization

Full details in [HASHING.md](HASHING.md).

## License

GPL-3.0-only
