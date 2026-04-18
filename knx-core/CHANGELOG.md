# Changelog

## [0.1.0](https://github.com/metaneutrons/snapdog/releases/tag/knx-core-v0.1.0) — Initial Release

### Features

- **Addresses**: `IndividualAddress` (area.line.device), `GroupAddress` (3-level/2-level), `DestinationAddress`
- **CEMI frames**: parse/serialize with full read/write access to all control fields
- **TPDU/APDU**: structured PDU parsing with all ~60 APCI service codes
- **DPT framework**: 34 main groups (100% parity with knx-openknx C++ reference)
  - Numeric: 1-15, 17-19, 27, 29, 217, 219, 221, 225, 231, 232, 234, 235, 238, 239, 251
  - String: 16 (ASCII/Latin-1), 28 (UTF-8)
- **KNXnet/IP types**: frame header, 18 service types, connection header, HPAI
- **Protocol enums**: Priority, FrameFormat, MessageCode, ApduType, etc. with `TryFrom` impls
- **`no_std` + `alloc`**: works on embedded targets (verified on `thumbv7em-none-eabihf`)
- **Optional `serde`**: string serialization for addresses
- **Golden test vectors**: validated against C++ knx-openknx reference implementation
