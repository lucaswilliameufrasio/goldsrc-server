# Serverinfo preparation

The official client has confirmed initial Steam approval, connection acceptance,
`new`, and acknowledgment of the deliberate disconnect. The next wire message
for actual sign-on is `svc_serverinfo`.

`poc/steam-server-query/src/serverinfo.rs` contains a bounded, unit-tested
encoder for the `svc_serverinfo` fields, based on the field order in
[ReHLDS SV_SendServerinfo](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp).
The encoder is compiled only in tests: it is **not sent to clients**. Its API
requires callers to supply the raw world CRC, client-DLL field,
map model name, and session-specific values. It does not invent those fields,
compute the CRC/MD5, load a map, implement delta schemas, or transfer resources.
The fixed string bounds are conservative development limits, not established
GoldSrc maxima.

The encoder now applies `COM_Munge3` to the raw world CRC using
`(-1 - player_index) & 255`, as the HLDS `SV_SendServerinfo` does. With the
reference world CRC `0x451b6da9`, the official HLDS build 10211 function
`COM_Munge3` was invoked in a network-isolated, read-only GDB session and
returned `0x562d43e0`, `0x572d43e1`, and `0x492d43ff` for zero-based slots 0,
1, and 31 respectively. Rust unit tests assert these three independent values
and their little-endian positions in `svc_serverinfo` with the observed zero
DLL field. This verifies the transform and field layout, not full sign-on.

To inspect the end-to-end map CRC → slot transform → encoded serverinfo offline:

```sh
cargo run --manifest-path poc/steam-server-query/Cargo.toml --example bsp_inventory -- \
  fixtures/local/reference-hlds/cstrike/maps/de_dust2.bsp --serverinfo-slot 0
```

The printed bytes are explicitly **synthetic and not sent**: spawn count,
hostname and capacity are diagnostic placeholders, the DLL field reflects the
observed reference setting, and the rest of the sign-on message is absent.

## Reference client DLL digest

On 2026-10-30, `client_dll_inventory` read the local HLDS reference
`cstrike/cl_dlls/client.dll` (1,184,616 bytes) and computed MD5
`cf385dcc19e71511ca48f58c68d14278`, matching both `md5sum` and the official
HLDS build 10211 `MD5_Hash_File` output. For that last comparison, GDB called the
HLDS function against the local DLL with no seed, in the same read-only,
network-isolated rootless container used for the map CRC; it returned success
and the same 16 bytes. This DLL is a server-distributed reference file, not
evidence of the exact build or DLL on the user's official client PC.

**Important:** at `CRC_MapFile` during normal reference-server startup, GDB
observed `gmodinfo.clientcrccheck = false` and `sv.clientdllmd5` as 16 zero
bytes. The DLL's actual MD5 is therefore *not* the value this reference startup
would put in `svc_serverinfo`. The HLDS only invokes that DLL hash path when
the client CRC check is enabled. Do not unconditionally use the digest or claim
this observation proves every mod/build or configuration behaves identically.
MD5 here is a legacy compatibility field, not a security or trust guarantee.

Reproduce the local read-only digest without committing the DLL:

```sh
cargo run --manifest-path poc/steam-server-query/Cargo.toml \
  --example client_dll_inventory -- fixtures/local/reference-hlds/cstrike/cl_dlls/client.dll
```

The tool uses RustCrypto `md-5` as a dev dependency, streams at most 256 MiB,
rejects empty/nonregular files, and does not embed or redistribute the DLL. It
does not select the runtime serverinfo value or activate client CRC checking.

## Local BSP inventory

`poc/steam-server-query/examples/bsp_inventory.rs` reads the 124-byte BSP v30
header, bounds-checks all 15 signed lump offsets and lengths against the opened
file, and enforces a 256 MiB diagnostic limit. It streams lumps 1–14 in descriptor
order (excluding entity lump 0) to calculate the multiplayer CS GoldSrc map CRC.
GoldSrc stores the CRC-32 internal state without the conventional final inversion;
it is not the CRC of the complete BSP. The tool does not validate lump contents
or establish map safety. `crc32fast` is a development-only dependency.
Empty lumps may have offset zero. This diagnostic intentionally does not reject
overlapping nonempty lumps; overlap and contents require deeper validation before
use by a game server.

Run against a locally supplied map (never commit the map itself):

```sh
cargo run --manifest-path poc/steam-server-query/Cargo.toml \
  --example bsp_inventory -- fixtures/local/reference-hlds/cstrike/maps/de_dust2.bsp
```

Verified 2026-10-30 on the locally acquired reference HLDS `de_dust2.bsp`: the
tool reported version 30, 15 in-bounds lumps, entity lump length 9802, and raw
GoldSrc world CRC `0x451b6da9`. Independently, Python zlib gave conventional
final CRC `0xbae49256`, whose bitwise inverse is `0x451b6da9`.

The decisive comparison used official Valve HLDS `engine_i486.so`, build 10211,
in a rootless Debian container with networking disabled and the local HLDS tree
mounted read-only. GDB stopped in `CRC_MapFile` during `+maxplayers 2 +sv_lan 1
+map de_dust2`; after successful return, the actual HLDS `worldmapCRC` pointer
read `0x451b6da9`. The debugger terminated the isolated process. The ordering
and lack of final inversion are also visible in
[ReHLDS CRC_MapFile](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/crc.cpp)
and its `SV_SpawnServer` call site.

This match validates this specific installed reference map and HLDS build, not
a redistribution grant or a working sign-on fixture. Single-player HLDS uses
zero instead of the map CRC; the diagnostic computes the multiplayer CS map CRC.

Before wiring this into live sign-on, acquire and inspect a reference HLDS
sign-on exchange or validate against an independent protocol fixture; encode
the verified map CRC for the player slot, choose the client-DLL field according
to the observed configuration, and implement delta
descriptions, movevars, extra info, resource lists and reliable fragmentation.
Keep authentication fail-closed and current explicit disconnect until those
prerequisites are met. Synthetic serialization tests alone do not establish
official-client compatibility.
