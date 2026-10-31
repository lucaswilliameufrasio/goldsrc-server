# Connect validation and next delivery slices

## Current slice

Replace the game-only fixed challenge with a cryptographically random positive
31-bit token from the OS (`getrandom`, MIT/Apache-2.0), bound to source IP and UDP
port for 30 seconds. Retrying getchallenge reuses the unexpired token without
extending its lifetime. Keep at most 1024 pending peers; silently drop new
challenge requests when full, never evict another live peer. Consume a token
only after a structurally valid connect with the matching challenge. Restart
discards all pending tokens. This is a private development limit, not a measured
production capacity or rate limiter. A2S retains its separate fixed challenge.

Parse only bounded connectionless protocol-48 Steam connect envelopes. Validate
decimal protocol/challenge, two quoted info strings, auth type `prot=3`, and
the presence/length of the opaque binary ticket (1–1023 bytes). Do not decode,
trust, print, persist, or claim validation of the ticket or user identity.
Allow newline or NUL between the command line and opaque ticket, as GoldSrc
line reading supports both. Limit the command line to 2048 bytes, protinfo to
1023 and userinfo to 255; reject control characters, invalid key/value pairing,
and duplicate keys. These conservative boundaries need official-client testing.

Emit allowlisted outcomes in the opt-in local trace: challenge issued/busy,
malformed connect, unsupported protocol/auth, invalid challenge, missing/oversize
ticket, or `steam_auth_unavailable`. On the last outcome, consume the token and
explicitly reject; no player allocation, authentication bypass, or sign-on.
Log only numeric protocol and ticket length for parsed envelopes, never strings
or ticket contents. This makes the next official-client test useful even though
raw connect packets remain redacted.

Wire research uses the argument order and ticket boundary in ReHLDS
[`SV_ConnectClient`](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp),
not source code copied into the Rust implementation. The old fixed-game-challenge
client evidence remains historical; it is not validation of the new parser.

## Ordered remaining steps

1. Validate this parser and peer-bound challenge with the official client. The
   expected outcome is `steam_auth_unavailable`, not successful player admission.
2. Integrate a legally available, architecture-compatible Steam game-server
   runtime; obtain a real server identity and authenticate the actual GoldSrc
   ticket, with approval/denial callbacks, disconnect cleanup and timeouts.
3. Only after positive authentication, send connection acceptance and implement
   reliable netchannel/sign-on and a minimal world/resource description.
4. Implement map loading, player spawn and gameplay under the existing plan.

Update: the SDK has been received; the opt-in Rust runtime adapter and
approval/denial handling are implemented. Real server logon and synthetic invalid
ticket rejection passed. The outstanding gate before sign-on work is a new
official-client connection to verify the valid-ticket outcome, not another SDK
transfer. Default non-Steam-runtime operation still rejects with
`steam_auth_unavailable`.

The investigation and prerequisites for step 2 are in
[Steam authentication](steam-authentication.md). No personal Steam credentials
are needed for the local parser tests.

## Verification evidence

The documented README gate passed: format, Clippy (default and all features),
build, 17 default unit tests, 20 all-feature unit tests, and the subprocess UDP
integration test. The latter checks the actual OS-generated challenge, rejection
of a different source port, rejection after token consumption, A2S regression,
and on-disk trace outcomes/redaction/Unix permissions. Deterministic unit tests
cover TTL boundaries, bounded-capacity recovery, malformed envelopes, entropy
failure, and IP binding. Official-client acceptance of this parser is pending;
the previous client's redacted connect payload cannot be replayed as a fixture.
