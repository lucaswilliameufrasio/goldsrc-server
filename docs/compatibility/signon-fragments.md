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

`netchannel::Sessions::queue_fragments` is available only on an already approved
peer and rejects a busy channel. The channel applies the sequence/ACK bounds,
reliable and fragment bits, Munge2, retransmission after a later negative ACK,
periodic probes and a 15-second deadline. Revocation discards pending work. A
test reconstructs the complete message from transmitted fragments while
simulating packet and ACK loss; it is a synthetic receiver, not an HLDS or
official-client interoperability test. The delayed ACK of an earlier copy after
retransmission is accepted by the queue.

No production caller queues fragments yet: in particular, receiving `new` still
deliberately disconnects. The queue does not implement compression, incoming
fragment reassembly, or file streams. This is not a working `serverinfo` or
sign-on. Existing Steam authentication and cleanup behavior are unchanged.

## Required subsequent work

1. Establish redistribution rights before committing any derived vectors.
2. Replace the reference-derived offset catalog with verified owned structure
   layouts and implement game-specific callback behavior before using schemas
   for actual entity updates. The current prepared descriptions are not sent.
3. Validate fragment transmission and reassembly against the official client or
   independent reference; implement any needed compression and incoming streams.
4. Supply real map/client data, resource manifests, delta and movevars together;
   only then enable an opt-in serverinfo/sign-on test with the official client.
