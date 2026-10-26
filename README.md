# GoldSrc Server

A Rust-native dedicated server project targeting unmodified official Steam
Counter-Strike 1.6 clients. The project aims to provide ARM64-native builds and
implement administration, metrics, rankings, bans, and game features in the
server rather than through AMX Mod X or other binary plugins.

This repository is at the design and protocol-validation stage. The query PoC
does **not** accept players, authenticate with Steam, or register with the
master server. It binds only to `127.0.0.1` and must not be exposed to a network.

## Documents

- [Technical plan](docs/plans/rust-goldsrc-arm64-server.md)
- [Architecture decision](docs/decisions/rust-goldsrc-arm64-server.md)

## Run the local query PoC

Requires a Rust toolchain. The PoC uses only the Rust standard library.

```bash
cargo run --manifest-path poc/steam-server-query/Cargo.toml
```

It answers the Steam A2S_INFO challenge and server-info query on
`127.0.0.1:27025`. This validates only server-query packets; it is not evidence
of game connection or gameplay compatibility.
