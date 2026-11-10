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

The BSP header and CRC code is also available to the server process through
`GOLDSRC_MAP_BSP`. When explicitly set, startup reads and validates that local
file and prepares its CRC before opening the socket; errors abort startup. This
does not load a playable world or send serverinfo. Without the option, startup
behavior is unchanged. Maps remain operator-supplied and are not redistributed.

When `GOLDSRC_MAP_BSP` is supplied, startup also reads the BSP entity lump into
bounded raw key/value records and requires the first entity to have a consistent
`worldspawn` classname. The parser accepts quoted fields and line comments;
it rejects malformed, overlong or truncated input and applies explicit size,
entity-count and field-count limits. Duplicate worldspawn classname entries are
accepted only if they agree, as observed in the local reference map. This is a
conservative subset of entity syntax, not proof that every GoldSrc map loads.
It does not execute entity code, validate geometry, instantiate a game world or
enable sign-on. The CRC and entity lump are read from the same file handle, but
operators must not modify the file while it is being loaded.

Startup also loads the BSP planes and model records from that file, checking
record sizes, finite coordinates, model bounds, root node/clipnode indices,
visible-leaf counts and face ranges. Nodes, leaves and clipnodes are decoded;
their plane, child, leaf, visibility-offset and marksurface-range references are
checked. An iterative traversal rejects cycles even in unreachable components.
Negative clipnode children are retained as contents codes without asserting that
every code is supported by a future collision implementation. This is only a
geometry subset: face plane, side, surfedge range, texture-info and light
offsets are checked, as are marksurface-to-face, surfedge-to-edge and
edge-to-vertex indices. Referenced compressed visibility rows must terminate
without truncation or zero-length runs, with bounded aggregate validation work.
A final zero run may extend past the logical row, as in the reference map; no
expanded row is written by this validator. Vertex coordinates, texture data,
lightmap extents, semantic visibility, collision traversal and rendering data
are not yet fully validated. A successful load does not prove a playable or
safe collision world.

`Geometry::point_contents` traverses a selected model hull using model-local
coordinates and returns the legacy contents code. It bounds traversal even if a
caller mutates the public geometry after loading, rejects invalid indices and
nonfinite arithmetic, and does not transform entity coordinates. On the local
HLDS 10211 `de_dust2` reference, `examples/hlds_hull_points.gdb` recorded five
synthetic points for each of the four world-model hulls. The opt-in
`hull_point_contents_matches_local_hlds` test matches those 20 outputs. This
is not a swept trace, player movement, entity collision or a live world.

`hull_trace::trace_point` now provides a static point sweep in model-local
coordinates with the GoldSrc hull epsilon, impact plane, fraction and solid/open/
water flags. `BspHull` gives typed names to the four standard BSP hull indices;
their `clip_mins`/`clip_maxs` were read from local HLDS 10211 at
`SV_ActivateServer` and are asserted by a unit test:

| `BspHull` | Index | Clip mins | Clip maxs |
| --- | ---: | --- | --- |
| `Point` | 0 | `[0, 0, 0]` | `[0, 0, 0]` |
| `StandingPlayer` | 1 | `[-16, -16, -36]` | `[16, 16, 36]` |
| `Large` | 2 | `[-32, -32, -32]` | `[32, 32, 32]` |
| `CrouchedPlayer` | 3 | `[-16, -16, -18]` | `[16, 16, 18]` |

`select_bsp_hull_for_bounds` mirrors the measured `SV_HullForEntity` BSP branch:
width X `<= 8` selects hull 0; width X `<= 36` selects hull 3 when height Z is
`<= 36`, otherwise hull 1; larger X widths select hull 2. The reference GDB
capture checks these boundaries and the returned offsets. For standard standing
and crouched player bounds on the world model the offset is `[0, 0, 0]`; tested
nonstandard bounds produce the measured nonzero offsets.

`trace_point` is the low-level API: it takes a typed hull and direct model-local
start/end points, limits recursion to 128 levels and total work to one million
steps, and returns an error rather than a collision result if a bound is
exceeded. `trace_bsp_entity` applies the selected hull and entity-origin offset
before tracing a translated BSP entity, then returns the end position in world
coordinates. Its plane remains model-local. Rotated entities and collision
against other moving entities are not implemented.

The opt-in `world_hull_traces_match_hlds` compares fraction (tolerance `1e-5`)
and flags for four segments across all four world-model hulls against
`examples/hlds_hull_trace.gdb`. `impact_positions_and_planes_match_hlds` compares
position for 24 traces and plane normal/distance for 10 impacts (position and
distance tolerance `0.001`, normal tolerance `1e-5`). The new
`translated_bsp_entity_trace_matches_hlds` compares the wrapper with HLDS
`SV_ClipMoveToEntity` after translating the reference world entity by
`[10, -20, 30]`: fraction `0.0882900134`, end position
`[-445.85498, 891.70996, 30]`, and the captured impact plane/flags. The GDB
recipe is [`hlds_entity_hull.gdb`](../../poc/steam-server-query/examples/hlds_entity_hull.gdb);
it runs isolated with networking disabled and restores the temporary entity
origin before exit. With the locally acquired HLDS tree and prebuilt local GDB
image, the capture can be rerun from the repository root:

```sh
docker --context rootless run --rm --platform linux/amd64 --network none \
  -v "$PWD/fixtures/local/reference-hlds:/data/hlds:ro" \
  -v "$PWD/poc/steam-server-query/examples/hlds_entity_hull.gdb:/tmp/verify.gdb:ro" \
  -w /data/hlds -e LD_LIBRARY_PATH=. goldsrc-hlds-crc-local \
  timeout 30s gdb -q -batch -x /tmp/verify.gdb ./hlds_linux
```

These are bounded reference cases, not exhaustive parity. Player movement, live
game state and a running world remain unimplemented.

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  world_hull_traces_match_hlds -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  impact_positions_and_planes_match_hlds -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  translated_bsp_entity_trace_matches_hlds -- --ignored
```

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  hull_point_contents_matches_local_hlds -- --ignored
```

The authenticated netchannel also has a preparation-only `prepare_serverinfo`
path. It requires an existing session still awaiting `new`, uses that session's
allocated slot regardless of the supplied template slot, and validates the
remaining explicitly supplied fields. It returns bytes without sending them;
there is no production caller yet. Tests use two approved synthetic sessions
and the CRC of the local reference BSP to check the slot-specific CRC against
the previously observed HLDS values. `sendres` is a subsequent client command,
not part of the initial `new` response; resource transmission is not connected.

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  serverinfo_uses_crc_from_local_bsp_and_session_slot -- --ignored
```

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
