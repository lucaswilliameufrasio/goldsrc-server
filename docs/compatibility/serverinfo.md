# Serverinfo preparation

The official client has confirmed initial Steam approval, connection acceptance,
`new`, and acknowledgment of the deliberate disconnect. The next wire message
for actual sign-on is `svc_serverinfo`.

`poc/steam-server-query/src/serverinfo.rs` contains a bounded, unit-tested
encoder for the `svc_serverinfo` fields, based on the field order in
[ReHLDS SV_SendServerinfo](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp).
The encoder is compiled only in tests: it is **not sent to clients**. Its API
requires callers to supply the already-encoded world CRC, real client-DLL MD5,
map model name, and session-specific values. It does not invent those fields,
compute the CRC/MD5, load a map, implement delta schemas, or transfer resources.
The fixed string bounds are conservative development limits, not established
GoldSrc maxima.

## Local BSP inventory

`poc/steam-server-query/examples/bsp_inventory.rs` reads only the 124-byte BSP
v30 header, bounds-checks all 15 signed lump offsets and lengths against the
opened file, and enforces a 256 MiB diagnostic limit. It does not load/validate
the contents of lumps, compute a GoldSrc map CRC, or establish map safety.
Empty lumps may have offset zero. This diagnostic intentionally does not reject
overlapping nonempty lumps; overlap and contents require deeper validation before
use by a game server.

Run against a locally supplied map (never commit the map itself):

```sh
cargo run --manifest-path poc/steam-server-query/Cargo.toml \
  --example bsp_inventory -- fixtures/local/reference-hlds/cstrike/maps/de_dust2.bsp
```

Verified 2026-10-30 on the locally acquired reference HLDS `de_dust2.bsp`: the
tool reported version 30, 15 in-bounds lumps, and entity lump length 9802.
This is evidence for header inventory only, not a licensed redistribution grant,
the world CRC, or a working sign-on fixture.

Before wiring this into live sign-on, acquire and inspect a reference HLDS
sign-on exchange or validate against an independent protocol fixture; calculate
map CRC and client-DLL MD5 from user-supplied licensed files; implement delta
descriptions, movevars, extra info, resource lists and reliable fragmentation.
Keep authentication fail-closed and current explicit disconnect until those
prerequisites are met. Synthetic serialization tests alone do not establish
official-client compatibility.
