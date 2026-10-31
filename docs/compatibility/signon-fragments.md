# Sign-on delta descriptions and outgoing fragmentation

Status: isolated fragment framing/state prototype; not used by the live server.
Delta-description serialization is an offline diagnostic only; compression and
complete sign-on remain open.

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

## Fragment prototype

`src/fragments.rs` splits an already-encoded normal-stream message into bounded
256–1024 byte pieces (up to 65536 bytes total). Each un-munged piece has one
normal-stream-present byte, little-endian packed `(index << 16) | count`,
16-bit offset zero, 16-bit length, file-stream-absent byte, then content. Tests
exercise ordering, offsets, exact boundaries, retransmission decision, both
reliable-bit values and capacity checks. Its header format is based on
[`Netchan_Transmit`](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/net_chan.cpp)
and `MAKE_FRAGID` in `net.h`; test vectors are authored, not HLDS captures.

The caller must validate packet sequence/ack bounds before passing an ack to the
queue, set the netchannel reliable/fragment bits, and apply Munge2 to the entire
fragment payload. The queue does not perform compression, network I/O, timers,
or incoming fragment reassembly. It cannot by itself send `serverinfo`, and
its tests do not prove interoperability with the official client. The current
authenticated connection still deliberately disconnects on `new`; existing
Steam authentication and cleanup behavior are unchanged.

## Required subsequent work

1. Establish redistribution rights before committing any derived vectors.
2. Implement runtime schema generation from licensed local data, including actual
   field offsets and quantization semantics, and handle game-specific callbacks.
   The current independently compared encoder is offline-only.
3. Integrate fragmentation with the authenticated channel's sequence/ACK
   lifecycle and optional compression; exercise packet loss, retries, and a
   complete reassembly against a reference implementation.
4. Supply real map/client data, resource manifests, delta and movevars together;
   only then enable an opt-in serverinfo/sign-on test with the official client.
