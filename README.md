# GoldSrc Server

A Rust-native dedicated server project targeting unmodified official Steam
Counter-Strike 1.6 clients. The project aims to provide ARM64-native builds and
implement administration, metrics, rankings, bans, and game features in the
server rather than through AMX Mod X or other binary plugins.

This repository is at the design and protocol-validation stage. The query PoC
does **not** accept players, authenticate with Steam, or register with the
master server. It binds to `127.0.0.1` by default. An explicit private LAN or
Tailscale IP can be selected with `LISTEN_ADDRESS`; wildcard and public IPs are
rejected. Because the PoC uses a fixed challenge, do not expose it to the public
internet.

## Documents

- [Technical plan](docs/plans/rust-goldsrc-arm64-server.md)
- [Architecture decision](docs/decisions/rust-goldsrc-arm64-server.md)

## Run the local query PoC

Requires a Rust toolchain. The PoC uses only the Rust standard library.

```bash
cargo run --manifest-path poc/steam-server-query/Cargo.toml
```

It answers the Steam A2S_INFO challenge and server-info query on
`127.0.0.1:27025`. To test from another device, bind the Mac's private address:

```bash
LISTEN_ADDRESS=100.x.y.z:27025 cargo run --manifest-path poc/steam-server-query/Cargo.toml
```

Use the Mac's Tailscale IPv4 address (`tailscale ip -4`) or its LAN IP. Allow UDP
port `27025` in the Mac firewall/Tailscale ACL from the test device only; do not
forward this port from the router to the public internet. Add
`<private-ip>:27025` to the Steam server browser's Favorites. This validates
only server-query packets; the PoC cannot accept a player or prove gameplay
compatibility.
