# knxd interoperability tests

`knx-rs-ip` is tested against [knxd](https://github.com/knxd/knxd), an
independent KNXnet/IP implementation, in two directions:

| Test file | knxd is | Under test |
| --- | --- | --- |
| `../knxd_client.rs` | tunnelling server | `TunnelConnection` (connect, group write/read/response, acknowledgements, disconnect) |
| `../knxd_device_server.rs` | tunnelling client (`ipt` driver) | `DeviceServer` (what knxd accepts, forwards and does when a server misbehaves) |

## Running

```sh
cargo nextest run -p knx-rs-ip --test knxd_client --test knxd_device_server
cargo test -p knx-rs-ip --test knxd_client -- --nocapture   # prints the wire traces
cargo test -p knx-rs-ip --test knxd_client -- --ignored     # adds the 60 s periodic heartbeat test
```

Docker is required: any context whose daemon runs Linux containers (Docker
Desktop, colima, a Linux host). The first run builds the image from this
directory (Alpine edge, `knxd` from the testing repository, `python3`,
`tcpdump`); the tag carries a hash of the recipe. Without a usable Docker the
tests print `SKIP knxd interoperability test: ...` and pass. Set
`KNX_RS_REQUIRE_DOCKER=1` to turn that skip into a failure (use it on CI
runners that must have Docker). A container that does not start, or an image
that does not build, always fails the test.

## How the pieces connect

* **Host to knxd.** UDP published with `-p` does not reach the host on every
  setup (colima forwards UDP over SSH, which drops it). The tests therefore run
  `relay.py` in the container with `docker exec -i` and carry the datagrams over
  the pipe: `Relay` in `../common/knxd.rs` listens on a loopback UDP port for the
  client under test. It also records every datagram (the wire trace), rewrites
  the client's HPAIs to NAT mode (`0.0.0.0:0`, so knxd answers to the relay),
  and can withhold the client's acknowledgements.
* **knxd to the host.** knxd's `ipt` driver connects to `host.docker.internal`
  (defined by Docker Desktop and colima; added with `--add-host` on Linux).
  `DeviceServer` binds `0.0.0.0` so the container can reach it.
* **Injecting telegrams.** `knxtool` inside the container talks to knxd's unix
  socket and plays "another device on the bus".
* **Packet captures.** `Knxd::capture_start` / `capture_stop` run `tcpdump`
  in the container and return a hex dump of everything knxd sent and received.

## What this cannot cover

knxd is a router, not a device: it does not do ETS-style management (individual
address programming, memory and property access, download, reset). Those need
a real ETS; see the checklist that goes with the release.

The image pins nothing but the Alpine channel, so `knxd --version` (printed by
every test) may change over time.
