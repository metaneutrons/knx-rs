# Changelog

## [0.1.0](https://github.com/metaneutrons/snapdog/releases/tag/knx-ip-v0.1.0) — Initial Release

### Features

- **Tunnel connection**: full KNXnet/IP tunneling protocol
  - Connect/disconnect handshake
  - 3× retry with 1s timeout per KNX spec
  - Heartbeat every 60s, 3 failures = disconnect
  - Auto-reconnect with exponential backoff (optional)
  - `TunnelConfig` builder
- **Router connection**: KNXnet/IP multicast routing
  - Rate limiting: 50 packets/sec sliding window per KNX 3.2.6
  - Default multicast group `224.0.23.12:3671`
- **Discovery**: search request/response for gateway discovery on local network
- **Multiplexer**: fan out one connection into multiple independent handles
- **URL parsing**: `udp://`, `tunnel://`, `router://` with multicast auto-detection
- **`KnxConnection` trait**: unified async send/recv/close interface
- **`AnyConnection` enum**: runtime-dispatched tunnel or router
