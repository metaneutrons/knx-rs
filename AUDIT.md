# Code Audit Findings — 2026-04-22

## Status: COMPLETE

## CRITICAL — knx-device BAU C++ Reference Compliance

- [x] **BAU-1**: Missing `communicationEnable`/`writeEnable`/`readEnable` flag checks on GroupValue Write/Read
  - File: `knx-device/src/bau.rs:189-210`
  - C++ ref: `bau_systemB_device.cpp:groupValueWriteIndication` checks flags before processing
  - Fix: Consult `GroupObjectTable` descriptors for flag checks

- [x] **BAU-2**: Property Write does not send read-back response
  - File: `knx-device/src/bau.rs:225-236`
  - C++ ref: `bau_systemB.cpp:propertyValueWriteIndication` always sends read-back
  - Fix: Add `queue_property_response` call after successful write

- [x] **BAU-3**: `GroupValueResponse` treated as `GroupValueWrite`
  - File: `knx-device/src/application_layer.rs:100-103`
  - C++ ref: Response checks `responseUpdateEnable` (A-flag), not `writeEnable` (S-flag)
  - Fix: Add `GroupValueResponse` variant to `AppIndication`, handle separately in BAU

- [x] **BAU-4**: Property Read with `startIndex=0` does not return element count
  - File: `knx-device/src/bau.rs:213-222`
  - C++ ref: `startIndex==0` returns current element count as uint16
  - Fix: Add special case for startIndex=0

- [x] **BAU-5**: Failed property/memory reads send no error response
  - File: `knx-device/src/bau.rs:213-244`
  - C++ ref: Always sends response (count=0 on error)
  - Fix: Send error responses

- [x] **BAU-6**: Unsupported DeviceDescriptorRead types silently dropped
  - File: `knx-device/src/bau.rs:143`
  - Fix: Respond with descriptorType=0x3F

- [x] **BAU-7**: APDU encoding in BAU instead of Application Layer
  - File: `knx-device/src/bau.rs:248-340`
  - Fix: Move encoding to `application_layer.rs`, add outgoing encode functions

- [x] **BAU-8**: `outbox` is Vec with O(n) remove(0)
  - File: `knx-device/src/bau.rs:175`
  - Fix: Change to `VecDeque`

## CRITICAL — knx-device Memory Format

- [x] **MEM-1**: Persistence header 10 bytes, C++ uses 12 (missing firmwareVersion)
  - File: `knx-device/src/memory.rs:67`
  - Fix: Add firmwareVersion field to header

## HIGH — knx-ip Tunneling Robustness

- [x] **TUN-1**: `wait_for_ack` drops non-ack frames (data loss)
  - File: `knx-ip/src/tunnel.rs:296-325`
  - Fix: Buffer non-ack frames and re-inject after ack received

- [x] **TUN-2**: Server tunneling sends with proper ack-based retry (3x, 1s timeout). Stashed packets re-processed after ack wait.
  - File: `knx-ip/src/tunnel_server.rs:316-330`
  - Fix: Implement 3× retry with 1s timeout for server→client sends

- [x] **TUN-3**: `try_reconnect` infinite loop blocks `close()`
  - File: `knx-ip/src/tunnel.rs:222-249`
  - Fix: Use `tokio::select!` to also listen for `cmd_rx` during reconnect

- [x] **TUN-4**: `send_with_retry`/`send_heartbeat` block select loop
  - File: `knx-ip/src/tunnel.rs:270-356`
  - Fix: Restructure as state machine within the select loop

- [x] **TUN-5**: No `RoutingBusy` handling
  - File: `knx-ip/src/router.rs:188-199`
  - Fix: Parse RoutingBusy, pause sending for specified wait time

- [x] **TUN-6**: Connect response missing individual address
  - File: `knx-ip/src/tunnel_server.rs:241`
  - Fix: Assign and return individual address in CRD

- [x] **TUN-7**: Channel ID collision on wrap-around
  - File: `knx-ip/src/tunnel_server.rs:231-235`
  - Fix: Check existing tunnels before assigning ID

## HIGH — knx-core DPT

- [x] **DPT-1**: `data_length()` wrong for DPT 221 (6), 231 (4), 239 (2)
  - File: `knx-core/src/dpt/mod.rs:53-63`
  - Fix: Add to correct match arms

## MEDIUM — knx-prod Sign Module

- [x] **SIGN-1**: `patch_hash_attribute` may patch wrong element
  - File: `knx-prod/src/sign.rs:87-90`
  - Fix: Anchor regex to `<ApplicationProgram`

- [x] **SIGN-2**: Case-sensitive fingerprint matching
  - File: `knx-prod/src/sign.rs:95-98`
  - Fix: Use case-insensitive regex flag

- [x] **SIGN-3**: Dynamic regex from unescaped `old_fp`
  - File: `knx-prod/src/sign.rs:97`
  - Fix: Use `regex::escape(old_fp)`

- [x] **SIGN-4**: Unused `sha2` dependency
  - File: `knx-prod/Cargo.toml:14`
  - Fix: Remove

## DRY Refactoring

- [x] **DRY-1**: 6 identical byte-passthrough DPT function pairs → generic helper
  - File: `knx-core/src/dpt/convert/numeric.rs:598-693`

- [x] **DRY-2** (reviewed: acceptable — client vs server have different ownership models): Tunneling request handling duplicated tunnel.rs ↔ tunnel_server.rs
  - Extract shared `process_tunneling_request()` function

- [x] **DRY-3** (reviewed: acceptable — same reason as DRY-2): HPAI construction, frame building, NAT resolution duplicated
  - Extract shared utilities

- [x] **DRY-4**: `extract_manufacturer_id`/`extract_application_id` identical structure
  - File: `knx-prod/src/parse.rs:63-130`
  - Extract generic `extract_attribute(xml, element, attr)` helper

- [x] **DRY-5**: Start/Empty handling in `read_children`
  - File: `knx-prod/src/hash.rs:476-500`

## LOW — Polish

- [x] **DOC-1**: `DptValue` doc says 251→Bytes, code returns UInt
- [x] **DOC-2**: Duplicated doc comment in hash.rs:393/399
- [x] **DOC-3**: Copyright years mixed (2025 vs 2026)
- [x] **LINT-1**: Lint levels weaker than convention in knx-core
- [x] **SPLIT-1** (deferred: feature work, not a bug — translations are stripped by OpenKNXproducer before signing): `filter_translations` stub loses translation data
- [x] **CONST-1**: KNX namespace URI, magic numbers should be named constants
