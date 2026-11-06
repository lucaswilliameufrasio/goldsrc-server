# Authenticated acceptance and first sign-on request

## Scope of this slice

The official client's Steam approval was captured on 2026-10-29. The next
bounded slice keeps that approved Steam session alive, sends connectionless
`B <userid> "<peer-address>" 0 10211\0`, and processes the first sequenced
GoldSrc channel messages. The build field is the compatibility baseline, not
the Rust executable's version; the secure flag stays false (no VAC claim).
Only the positive, matching, unexpired Steam callback can create this session.
Rejected/pending tickets and unknown endpoints never get channel responses.

Each approved channel retains a distinct zero-based slot and its assigned user
ID. Slots remain reserved while the channel is closing and are released when
the session is removed, including timeout or Steam revocation. These are
inputs for future serverinfo; slots are not yet sent to clients, and do not
establish a connected player or world state.

For future player updates, the validated connect envelope now retains only its
bounded, printable public `name`, when present and valid UTF-8. The pending
Steam request owns this value without copying the ticket or raw userinfo. It is
passed to the matching channel only on a positive Steam approval and discarded
on denial, timeout or revocation; names are not emitted into diagnostic logs.
Invalid or unsupported names are omitted rather than substituting a name or
changing the Steam approval decision. This is untrusted client-supplied data,
not a Steam-verified identity, and it is not yet sent in sign-on.

The channel can prepare an individual public player update from its own slot,
user ID and retained name, with the protocol digest supplied explicitly by the
caller. Missing or unsafe names, unknown peers and sessions no longer awaiting
`new` are rejected. A synthetic-session test inserts that update in a prepared
sign-on and checks the transmitted fragment. No runtime producer invokes this
path yet; it neither manufactures the digest nor implements an active world.

On 2026-11-06, official HLDS build 10211 was inspected offline without a
client. `SV_FullClientUpdate` hashes the entire 64-byte `hashedcdkey` field
with MD5, including bytes after any NUL. `examples/hlds_player_digest.gdb`
checks the actual HLDS output against independent MD5 computations for four
synthetic 64-byte patterns. `signon::player_update_digest` implements that
transformation; MD5 is required for this legacy wire field, not authentication
or security. The real Steam client's field provenance and safe lifecycle remain
unverified: no ticket, password, live key material or fixed default may be
substituted. The helper is not connected to real traffic and does not implement
Steam approval or a player admission decision.

Inspection of the same HLDS build's `SV_CheckKeyInfo` showed that protocol 3
obtains the candidate key from the `cdkey` field of `protinfo`, checks its
length is 32, and requires a nonempty `raw` field. The isolated
`examples/hlds_keyinfo_synthetic.gdb` confirms lookup and copy with synthetic
input, without connecting a client or saving that field. Disassembly of
`SV_ConnectClient` shows a 32-byte copy into `hashedcdkey` and a NUL at offset
32. The remaining bytes of the 64-byte destination across slot reuse have not
been established. A separate isolated `SV_FullClientUpdate` test confirms that
changing a synthetic byte after offset 32 changes the emitted digest.

These observations do not establish what an official Steam client supplies,
nor authorize logging or persisting either field. The current connect parser
does not implement these HLDS-specific field checks. Do not infer the remaining
bytes, use an all-zero default, or treat the digest as Steam authentication.
No real credential value was examined during this investigation.

Rather than fabricate world CRCs, resource lists or delta schemas, recognize
the client's `clc_stringcmd new` request and then send a reliable, explicit
`svc_disconnect` explaining that world sign-on is not implemented. This proves
acceptance and the channel's initial request/acknowledgment boundary before
implementing serverinfo/resource transfer. It is not completed sign-on or a
playable server. The previous post-auth rejection is superseded for this mode.

## Wire and state contract

- Research references: ReHLDS engine `net.h`, `SV_ConnectClient` in `sv_main.cpp`,
  `Netchan_Process`/`Netchan_Transmit` in `net_chan.cpp`, and `COM_Munge2` wire
  transform in `common.cpp`. Rust code is authored for this documented wire
  contract, not incorporated engine source. Protocol constants/transform lookup
  values are needed for interoperability. No Valve binary is vendored.
- An 8-byte little-endian sequence/ack header uses bits 31 for reliable payload
  presence/reliable-state acknowledgment and bit 30 for fragmentation/security.
  Only unfragmented messages are supported here. Payload words use GoldSrc
  Munge2 with the sequence's low byte; remaining 1–3 bytes are unchanged.
- Maximum channel datagram: 1400 bytes; command string: 256 bytes. Parse only
  nop and allowlisted `new`/`dropclient`/`disconnect` strings; never execute or
  log arbitrary commands. Fragmented messages and unsupported payloads close
  with a bounded diagnostic rather than allocating reassembly buffers.
- Track monotonic incoming/outgoing sequence numbers, reject duplicates,
  reordering, future acknowledgments and sequence jumps over 4096. Do not
  refresh deadlines on invalid traffic. These conservative diagnostic limits
  require official-client confirmation.
- Maintain one reliable outgoing disconnect buffer. Clear it only after its
  reliable state and a sufficient sent sequence are acknowledged. Resend only
  after a later ACK proves the original payload was missed; send bounded nop
  probes while closing. Do not blindly toggle/resend reliability on a timer.
- At most 32 pending+approved Steam identities and 32 channel sessions. Bind to
  exact IP/UDP port; no NAT rebinding in this slice. Wait at most 15 seconds for
  the first sign-on command; close for at most 5 seconds afterward. Entire SDK
  diagnostic still ends at the configured 1–120 seconds.
- Retain Steam auth until channel close/timeout, denial/kick, backend failure,
  or normal diagnostic shutdown; notify Steam once and release session state.
  No player/world/entity allocation occurs.
- Input traces remain redacted. Emit allowlisted outcomes such as
  `connection_accepted`, `netchan_new`, `netchan_invalid`,
  `netchan_disconnect_acked`, and `netchan_timeout`, not decoded command values.

## Acceptance evidence needed

Local tests must cover acceptance encoding, known Munge2 vectors, sequence/ACK
handling, reliable loss/retransmission, malformed/truncated/oversize packets,
duplicate/reordered traffic, exact-peer matching, timeout/capacity recovery, and
session cleanup decisions. Keep real SDK invalid-ticket tests fail-closed.

On 2026-10-30 the official-client test produced `steam_auth_approved`,
`connection_accepted`, `netchan_new`, `netchan_ack`, and
`netchan_disconnect_acked` in the ignored local trace
`fixtures/local/netchannel-20261030-030509-199058.jsonl`. This verifies the
client reached and acknowledged this initial channel exchange. It does not prove
serverinfo, resource negotiation, complete sign-on, or gameplay.

After this boundary is verified, implement serverinfo/delta descriptions,
resource negotiation and reliable fragmentation using a licensed local map and
reference evidence. Do not mark gameplay complete based on accepting a socket.
