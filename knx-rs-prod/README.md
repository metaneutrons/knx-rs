# knx-rs-prod

Cross-platform `.knxprod` builder for KNX ETS product databases — pure Rust, no .NET, no Wine, no Windows VM.

Part of the [knx-rs](https://github.com/metaneutrons/knx-rs) repository.

## What it does

Takes a monolithic KNX product XML (from [OpenKNXproducer](https://github.com/OpenKNX/OpenKNXproducer), or generated from Rust — see the [knx-rs README](https://github.com/metaneutrons/knx-rs#generating-knxprod-files)) and produces a **signed, ETS-importable** `.knxprod`:

1. **Parse** — extract metadata (namespace, manufacturer ID, application ID)
2. **Split** — split monolithic XML into Catalog.xml, Hardware.xml, Application.xml
3. **Hash** — compute the registration-relevant MD5 `Hash` and patch the fingerprint into IDs
4. **Sign** — RSA-sign the `M-XXXX` folder into `M-XXXX.signature` and embed `knx_master.xml`
5. **Package** — ZIP into `.knxprod`

## Making readable ids ETS-importable (`--renumber`)

ETS parses the integer suffix of every id (`_P-`, `_UP-`, `_O-`, `_R-`, `_PB-`, `_PS-`) as a base-10 number **at import time** — a rule the `project/NN` XSD does *not* encode. Product XML authored with **readable string ids** (e.g. `_UP-Z01000`, `_O-Zone1Play`, `_PB-General`) validates against the schema but then fails import with `'G' is not a legal digit for base 10`.

`--renumber` rewrites every `ApplicationProgram`-scoped id suffix to a unique integer and remaps **every** reference (`RefId`, `RefRefId`, `ParamRefId`, …) in lock-step, then runs a structural **sanity check** (id format, dangling references, duplicate ids). It is the pure-Rust equivalent of OpenKNXproducer's `Renumber`/`ConvertKoIds` passes — so you can author products with human-readable ids and let `knx-rs-prod` normalise them:

```sh
knx-rs-prod MyDevice.xml -o MyDevice.knxprod --renumber \
    --key signing-key.pem --fetch-master
```

`--xsd <schema.xsd>` additionally validates the normalised XML against an ETS `project/NN` schema via `xmllint` (bring your own schema — it is ETS-proprietary).

As a library, `normalize_ids(&xml)` runs renumber + sanity and returns the rewritten XML; `renumber::renumber_ids` and `sanity::sanity_check` are also exposed individually.

## Signing (bring your own ETS key)

ETS validates each product against an RSA-1024 `M-XXXX.signature` produced by the closed-source `Knx.Ets.XmlSigning.dll`, whose signing key is not public. `knx-rs-prod` reimplements the signing **algorithm** — reproducing ETS's output byte-for-byte (verified against a reference `.knxprod`) — but **never ships a key**. You supply a key extracted from **your own licensed ETS installation** (PEM or the .NET `<RSAKeyValue>` XML that `RSA.ToXmlString(true)` emits); it never enters this crate.

Without `--key`, the tool still runs steps 1–3 and 5, producing an **unsigned** archive (not importable by ETS as-is) — e.g. to hand off to OpenKNXproducer for the signing step. Tracking: [issue #9](https://github.com/metaneutrons/knx-rs/issues/9).

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

### CLI

```sh
cargo install knx-rs-prod            # add `--features fetch` for --fetch-master

# Signed, ETS-importable (bring your own ETS key):
knx-rs-prod MyDevice.xml -o MyDevice.knxprod \
    --key signing-key.pem --knx-master knx_master.xml

# Readable string ids → integers, validated, then signed:
knx-rs-prod MyDevice.xml -o MyDevice.knxprod \
    --renumber --xsd knx_project-20.xsd \
    --key signing-key.pem --fetch-master

# Unsigned (hash + package only):
knx-rs-prod MyDevice.xml -o MyDevice.knxprod
```

`--key` accepts PEM or .NET `<RSAKeyValue>` XML. `knx_master.xml` is the public KNX master file for your schema version — pass a local copy with `--knx-master`, or `--fetch-master` (requires `--features fetch`) to download it from `update.knx.org`. `--renumber` normalises readable string ids to integers (see above); `--xsd` validates against an ETS schema via `xmllint`.

### As a library

```rust
use std::path::Path;
use knx_rs_prod::{generate_signed_knxprod, signature::SigningKey, knx_master::KnxMaster};

let key = SigningKey::from_path(Path::new("signing-key.pem")).unwrap();
let master = KnxMaster::from_path(Path::new("knx_master.xml")).unwrap();
generate_signed_knxprod(
    Path::new("MyDevice.xml"),
    Path::new("MyDevice.knxprod"),
    &key,
    &master,
).expect("failed to generate signed knxprod");
```

`generate_knxprod(input, output)` is the unsigned variant. To normalise readable
string ids before generating, run `knx_rs_prod::normalize_ids(&xml)` (renumber +
sanity) and feed the result — or call `renumber::renumber_ids` / `sanity::sanity_check`
directly.

### Authoring identifiers

Pass raw hardware serial numbers, product order numbers, catalog section keys,
and parameter type names to the `author` builders. Their ID components are
encoded using ETS's `.XX` notation for non-alphanumeric UTF-8 bytes. Definitions
and references use the same encoding, while attributes such as `OrderNumber`,
`SerialNumber`, and `Name` retain the original text (with XML escaping).

For example, `RIOT-KNX-DEVICE-1CH` becomes `RIOT.2DKNX.2DDEVICE.2D1CH` in a
product ID, `General_StartupTime` becomes `General.5FStartupTime` in a parameter
type ID, and spaces in catalog section keys become `.20`.

Do not pre-encode these inputs: a literal `.2D` in a raw name is encoded as
`.2E2D`. Explicit full IDs, including those passed to `translate_raw`, must
already use the final ID grammar. Readable suffixes for integer-typed IDs still
require `normalize_ids`; that pass does not repair arbitrary hand-written
product, catalog, or parameter type IDs.

### Hash only

```rust
use knx_rs_prod::hash::hash_application_program;

let xml = std::fs::read_to_string("MyDevice.xml").unwrap();
let result = hash_application_program(&xml).unwrap();
println!("MD5:         {}", result.hash_base64());
println!("Fingerprint: {}", result.fingerprint_hex());
```

## Testing

```sh
# Unit tests (fast, all fixtures included)
cargo test -p knx-rs-prod

# OpenKNX integration tests (requires download, ~8s)
./knx-rs-prod/scripts/fetch-openknx-fixtures.sh
cargo test -p knx-rs-prod --test openknx
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
