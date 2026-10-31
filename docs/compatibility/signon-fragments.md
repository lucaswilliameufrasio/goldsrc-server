# Sign-on delta descriptions and outgoing fragmentation

Status: outgoing fragment state is integrated into the authenticated channel,
but has no live sign-on producer and is not sent during normal operation.
Delta descriptions can be prepared at startup from explicit local inputs, but
are not sent to clients. Compression and complete sign-on remain open.

## Local HLDS binary evidence

On 2026-10-31, official HLDS build 10211 with the local CS installation and
`de_dust2` was started in a rootless container with networking disabled and the
installation mounted read-only. GDB stopped at `SV_ActivateServer`, where
`g_sv_delta` was non-null. Before any client connected, the debugger cleared
the isolated process's `net_message.cursize` and invoked
`SV_WriteDeltaDescriptionsToClient(&net_message)`. The function wrote 6027
bytes; `net_message.flags` remained zero. The process was then terminated.

The resulting binary is local only, at
`fixtures/local/hlds-delta-build10211.bin` (SHA-256
`0a80697b50614b6d4ba36faeb36423c50e0f75101016149689119d445e71a3bb`).
It is ignored by Git; redistribution rights have not been established. This
is direct output from the engine function, not a captured network exchange.
It does not establish complete sign-on or
fragment interoperability.

The registered fields were extracted independently from `g_sv_delta` at the
same breakpoint in a second isolated HLDS run using
`poc/steam-server-query/examples/hlds_delta_fields.gdb`. Its output is also
ignored and stays local: `fixtures/local/hlds-delta-fields-build10211.tsv`.
The observed registry contains seven schemas and 213 fields. Each field includes
the runtime type, name, offset, size, significant bits and two multipliers;
conditional encoder callbacks are not represented by the transmitted metadata.

`examples/delta_reference.rs` serializes these inputs, writing the seven-member
meta-delta mask and little-endian bit stream, and compares the entire result
against the separate HLDS binary capture. The local build 10211 comparison
matched all **6027 bytes**. The input data came from the engine's in-memory
structures rather than decoding the output; the verification does not require
network access or Steam credentials. The diagnostic supports this bounded subset
and does not parse `delta.lst`, supply runtime schemas, implement callback
behavior, or establish compatibility for all mods and values.

### Startup preparation from local inputs

`src/delta.rs` also prepares descriptions directly from the local `delta.lst`:
field order, types, bit widths and multipliers come from that file; schema order
and field offsets come from an explicitly supplied local catalog. Currently the
catalog is the TSV produced by the GDB script above. The loader uses only its
schema/field names and offsets for generation, but validates the complete format.
It does **not** obtain offsets from `delta.lst`, verify the catalog's build or
ABI provenance, or independently generate offsets for another architecture.
The operator must supply a matching catalog from the same GoldSrc definitions.
Each input is bounded to 64 KiB. Unsupported syntax and missing fields fail
closed before the UDP socket is opened.

This is a staging mechanism, not an authorization to distribute Valve data and
not a substitute for owned engine structure layouts. The parser records the
names of conditional callbacks but does not implement or execute them; metadata
is prepared only, never transmitted. A comparison of this startup path using
the reference `delta.lst` and local offset catalog matched all 6027 HLDS bytes.

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  generated_list_matches_local_hlds -- --ignored
```

Opt in at startup by setting both `GOLDSRC_DELTA_LIST` and
`GOLDSRC_DELTA_CATALOG` to files the operator may use locally. If only one is
set, startup fails. If neither is set, behavior is unchanged. A successful preparation
does not enable sign-on; the server still deliberately disconnects after `new`.

Run the explicit local-only comparison (fails if the two captures are missing):

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --example delta_reference \
  matches_local_hlds_capture -- --ignored
```

The default tests do not require or redistribute proprietary inputs. To
regenerate field input, use the GDB script inside a network-disabled rootless
container with the reference installation mounted read-only at `/data/hlds`
and an ignored local output directory mounted at `/out`. The script refuses to
overwrite an existing field capture. Compare only captures from matching builds
and configuration; verify licensing before redistributing either file.

The local official CS installation contains `cstrike/delta.lst`; its fields
include game-specific names, bit widths, scaling and encoder names. The HLDS
does not simply send this text: `SV_WriteDeltaDescriptionsToClient` serializes
registered in-memory delta descriptions using a meta-delta bit format. A parser
or copied text file alone is not evidence that the resulting binary wire format
is correct. The file stays local under `fixtures/local/`.

## Outgoing fragment channel

`src/fragments.rs` splits an already-encoded normal-stream message into bounded
256–1024 byte pieces (up to 65536 bytes total). Each un-munged piece has one
normal-stream-present byte, little-endian packed `(index << 16) | count`,
16-bit offset zero, 16-bit length, file-stream-absent byte, then content. Tests
exercise ordering, offsets, exact boundaries, retransmission decision, both
reliable-bit values and capacity checks. Its header format is based on
[`Netchan_Transmit`](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/net_chan.cpp)
and `MAKE_FRAGID` in `net.h`; test vectors are authored, not HLDS captures.

### Independent HLDS receive and reassembly check

On 2026-10-31, the real Rust `Sessions` fragment path produced three synthetic
datagrams for a 2300-byte pattern. In a second, network-isolated process, GDB
fed those datagrams to the official HLDS build 10211 `Netchan_Process`, then
called `Netchan_CopyNormalFragments`. The HLDS accepted sequences 1, 2, 3,
reported reliable states 1, 0, 1, marked the normal stream ready only after the
third fragment, and reassembled all 2300 original bytes exactly. No real client,
Steam ticket, or network traffic was involved. This validates these uncompressed
normal-stream packets and HLDS's receiver, not official-client sign-on, loss
handling in HLDS, compression, incoming file streams or every fragment size.

To reproduce with the separately acquired local HLDS and prebuilt local GDB
image, first choose a clean ignored fixture directory and generate the packets:

```sh
GOLDSRC_FRAGMENT_FIXTURE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  export_fragments_for_hlds -- --ignored
docker --context rootless run --rm --platform linux/amd64 --network none \
  -v "$PWD/fixtures/local/reference-hlds:/data/hlds:ro" \
  -v "$PWD/fixtures/local:/fixtures:ro" \
  -v "$PWD/poc/steam-server-query/examples/hlds_fragment_receiver.gdb:/tmp/verify.gdb:ro" \
  -w /data/hlds -e LD_LIBRARY_PATH=. goldsrc-hlds-crc-local \
  timeout 30s gdb -q -batch -x /tmp/verify.gdb ./hlds_linux
```

The exporter creates `rust-fragments-hlds.bin` exclusively and refuses to
overwrite an existing file. The second command requires that file under
`fixtures/local`; the GDB script terminates its isolated process after the
check. The packet fixture is synthetic but remains ignored with other captures.

`netchannel::Sessions::queue_fragments` is available only on an already approved
peer and rejects a busy channel. The channel applies the sequence/ACK bounds,
reliable and fragment bits, Munge2, retransmission after a later negative ACK,
periodic probes and a 15-second deadline. Revocation discards pending work. A
test reconstructs the complete message from transmitted fragments while
simulating packet and ACK loss; that loss test uses a synthetic receiver. The
delayed ACK of an earlier copy after retransmission is accepted by the queue.

No production caller queues fragments yet: in particular, receiving `new` still
deliberately disconnects. The queue does not implement compression, incoming
fragment reassembly, or file streams. This is not a working `serverinfo` or
sign-on. Existing Steam authentication and cleanup behavior are unchanged.

## Prepared serverinfo message

`src/signon.rs` is compiled only in tests. It assembles the HLDS
`SV_SendServerinfo` message: optional print banner, serverinfo, extra info,
delta descriptions, movevars, CD track and view entity. It accepts explicit
session data and movevars; it does not infer live settings, validate a delta
byte stream or transmit anything. Resource exchange is absent.

The test-only `NewResponse` appends user-message registrations, the
`fullserverinfo` command and explicitly supplied client updates. Its userinfo
filter strips keys starting with `_`, rejects control characters and unsafe
command delimiters, and requires the caller to provide each 16-byte client
digest. This is a conservative subset of GoldSrc info strings, not a guarantee
that all possible other keys are non-sensitive. The caller must supply only
appropriate server and public player information. No real player data was used.

For the local HLDS build 10211, a synthetic two-registration, one-player tail
was generated using `examples/hlds_new_tail.gdb` in the same isolated container;
Rust matched all 154 bytes including removal of a fake `_secret` key. After the
original 118-byte comparison, disassembly of the official `SV_New_f` revealed
two additional `svc_stufftext` messages, `allow_shaders` and `allow_autoaim`;
both are now explicit inputs and included in this new comparison. The output
capture is ignored at `fixtures/local/hlds-new-tail-synthetic-v3.bin`.

At world precache the debugger observed `RegUserMsg("VoiceMask", 8)` called
from the CS game DLL's voice manager initialization. At the start and end of
`SV_ActivateServer`, the consolidated `sv_gpUserMsgs` was null while
`sv_gpNewUserMsgs` was non-null. Disassembly of `SV_New_f` shows it walks the
consolidated list before invoking the game DLL's `ClientConnect`; the standalone
one-argument `SV_SendUserReg` instead reads `sv_gpNewUserMsgs`. Registration
promotion and callback outcomes during a real client connection remain
unverified; no fake Steam approval or game connection was performed here.

At the end of `SV_ActivateServer` a hardware watchpoint on `sv_gpUserMsgs`
subsequently stopped inside `Host_Map`: the consolidated pointer changed from
null to the then-pending list pointer. This establishes the initial map-load
promotion for this build, not what a `ClientConnect` callback later registers
or how a rejected/approved real client proceeds.

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  new_response_tail_matches_local_hlds -- --ignored
```

On 2026-10-31, GDB called the official HLDS build 10211
`SV_WriteMovevarsToClient` and `SV_SendServerinfo` at `SV_ActivateServer` in
the same isolated setup described above, using a local client slot without any
network client or ticket. Its output stayed ignored in `fixtures/local/` as
`hlds-movevars-build10211.bin` and `hlds-serverinfo-build10211.bin`. The Rust
test loads delta inputs separately, uses the observed movevars and extracts the
variable hostname, map name and map cycle from the capture. The generated
movevars matched all 102 bytes and the full assembled message matched all
6413 bytes. This is one configuration, not independent verification of every
field or a successful client connection.

Another opt-in test feeds an assembled message using the real local delta input
through the approved-channel fragment path and reconstructs it with a synthetic
receiver. These tests do not enable any live caller:

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  assembled_message_matches_hlds -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  prepared_signon_reassembles_via_fragment_channel -- --ignored
```

## Resource-message subset

`src/resources.rs` is compiled only in tests and encodes `svc_resourcerequest`
and `svc_resourcelist` for explicit resources and optional consistency indices,
without a download URL or custom hashes. Nonzero 32-byte reserved fields are
serialized; unsupported flags and unrepresentable indices are rejected;
the encoder does not scan a map or filesystem, transfer resources, process a
client response, or provide live negotiation. In an isolated HLDS 10211 process
`examples/hlds_resources.gdb` supplied two synthetic entries and called
`SV_SendResources`. The generated Rust result matched the captured 54 bytes.
That capture stays ignored at `fixtures/local/hlds-resources-synthetic.bin`.

An additional capture from the installed `de_dust2` reference after activation
contained 781 resources, including 52 with nonzero reserved fields, and 62
consistency entries. `examples/hlds_resource_manifest.gdb` exports the resource
metadata and calls the official HLDS with consistency **disabled only in its
isolated process**. The test matches all 19259 resulting bytes using the 781
local entries; it explicitly omits the consistency marker from the input to
match that deliberately modified HLDS run. This does not validate the normal
62-entry consistency list or a real resource negotiation. Both reference files
remain ignored in `fixtures/local/`; do not redistribute without rights.

In a separate isolated HLDS 10211 run, `examples/hlds_consistency.gdb` retained
the 62 consistency entries, verified `mp_consistency=1` and a non-proxy local
slot, and confirmed that HLDS marked a consistency response as required. The
full list and resource message was 19314 bytes. With the independently captured
781-entry metadata and 62 marked positions, Rust matched every byte. The format
uses 5-bit relative positions for gaps up to 31 and 10-bit absolute positions
for larger gaps; unrepresentable absolute positions are rejected rather than
truncated. This validates the outgoing serialization in one configuration,
not a client's response or actual enforcement of file checks.

### Consistency response: hashes and model bounds

The test-only `validate_hash_response` reads the signed 16-bit payload length,
reverses `COM_Munge` with the spawn count, and parses 1-bit entry markers,
12-bit resource positions and 32-bit hashes. It requires exactly one response
for every explicitly supplied expected hash and rejects unknown, duplicate,
missing, truncated or mismatched entries. These duplicate/omission checks are
intentionally stricter than simply counting entries in the reference code.
The test-only `validate_consistency_response` additionally handles exact bounds,
specified bounds, and specified bounds if available (all six values `-1`).
It rejects nonfinite values, inverted bounds and invalid expected bounds as a
deliberately stricter safety policy. `check_from_metadata` derives each expected
hash or model-bounds mode from an explicitly selected resource's hash and
spawn-specific munged reserved bytes. The caller must select the correct
`RES_CHECKFILE` entries and provide the correct server-side digest and spawn
count. The game-DLL `InconsistentFile` callback policy is not implemented. A
hash match is not Steam authentication or a security guarantee.

In isolated HLDS 10211, `examples/hlds_consistency_response.gdb` used the
engine's own bit writer and `COM_Munge` to create a synthetic one-file hash
response and called `SV_ParseConsistencyResponse` with a local test slot.
The HLDS accepted it (`msg_badread=0` and `has_force_unmodified=0`). Rust
accepts those same eight bytes and rejects a wrong hash, invalid lengths and
other malformed synthetic responses. No real client, ticket or game connection
was used. The parser is not connected to live traffic.

An independent synthetic three-model fixture was produced and accepted by the
official HLDS using its bit writer, `COM_Munge` and `SV_ParseConsistencyResponse`;
it covered the three bounds modes and was accepted by Rust byte for byte. The
ignored fixture remains at `fixtures/local/hlds-consistency-bounds-synthetic.bin`.
`examples/hlds_inconsistent_bounds.gdb` separately observed that an exact
bounds mismatch reached the actual CS game DLL `InconsistentFile` callback,
which returned 1; HLDS then reached `SV_DropClient`. The debugger stopped at
drop entry without executing a real disconnection. This is evidence for the
synthetic mismatch and observed CS build, not all game DLLs or policy cases.
Rust deliberately returns `BoundsMismatch` instead of assuming callback approval.
Neither test uses a network client or provides evidence of playable sign-on.

`examples/hlds_consistency_metadata.gdb` independently calls the official
HLDS `COM_UnMunge` on each selected entry of the local map manifest. Rust's
`check_from_metadata` matched all 62 results, including 52 model bounds and 10
hash checks. The metadata is local and ignored at
`fixtures/local/hlds-consistency-metadata-10211.tsv`; this comparison does not
verify a real client response or implement the resource exchange.

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  model_bounds_response_matches_hlds -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  real_consistency_metadata_matches_hlds_decoding -- --ignored
```

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  resource_list_matches_hlds -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  local_manifest_matches_hlds_without_consistency -- --ignored
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --all-features \
  local_manifest_matches_hlds_with_consistency -- --ignored
```

## Required subsequent work

1. Establish redistribution rights before committing any derived vectors.
2. Replace the reference-derived offset catalog with verified owned structure
   layouts and implement game-specific callback behavior before using schemas
   for actual entity updates. The current prepared descriptions are not sent.
3. Validate loss/retransmission and the complete exchange against the official
   client; implement any needed compression and incoming streams.
4. Establish registration promotion and client callback behavior, and supply
   validated live map/client/session data and resource manifests; only then
   enable an opt-in sign-on test with the official client.
