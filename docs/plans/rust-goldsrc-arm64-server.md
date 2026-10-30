# Rust GoldSrc-Compatible Server: Technical Plan

## Goal

Create a native ARM64 dedicated server that accepts unmodified official Steam
Counter-Strike 1.6 clients. Replace the project's required AMX Mod X/plugin
features with native server capabilities.

## Explicit non-goals

- Supporting non-Steam clients, legacy protocol variants, or arbitrary AMX /
  Metamod binary plugins.
- Shipping Valve's HLDS binaries, game DLLs, or game assets.
- Claiming complete CS 1.6 compatibility based only on query-protocol support.

## Proposed workspace boundaries

- `goldsrc-protocol`: bounded packet parsing/encoding, connectionless messages,
  challenges, and client/server protocol versions.
- `goldsrc-transport`: UDP sockets, session lifecycle, packet limits, and
  protocol-level rate controls.
- `goldsrc-world`: BSP parsing, resources, entity state, collision, and map
  lifecycle. Load user-provided maps; keep licensing separate from code.
- `cs16-simulation`: fixed-tick simulation, player movement, weapons, rounds,
  and game rules required by the chosen CS 1.6 mode.
- `cs16-events`: typed authoritative events such as player connection, kill,
  round result, ban, and map change.
- `cs16-operations`: authenticated administration, health, Prometheus metrics,
  ranking, bans, and persistence, consuming authoritative events without a
  game plugin interface.
- `cs16-server`: process entrypoint, configuration, graceful shutdown, and
  composition of the modules above.

Keep the simulation loop deterministic and isolated from slow I/O. Metrics
should be updated in-process; durable rankings and history should be written
through an idempotent persistence boundary so restarts do not lose results.

## Delivery slices and acceptance evidence

1. **Compatibility inventory** — pin the official Steam client build(s),
   protocol version, server-browser expectations, Steam authentication, and the
   required game modes. Capture test fixtures from a legally acquired local
   client and a local reference server.
2. **Query PoC** — answer the current Steam server-query handshake and info
   request on loopback. Check the response with a query client and, where
   available, the unmodified Steam server browser. This milestone does not
   accept game connections.
3. **Connection vertical slice** — implement connectionless challenge,
   Steam authentication, sign-on, disconnect, and a single client join using
   the unmodified Steam client. Test only against local clients and fixtures.
4. **Playable slice** — load one user-supplied, licensed map; spawn and move
   players; implement the selected mode's round/weapon loop; verify client
   prediction and server authority with stock clients.
5. **Native operations** — implement admin authentication and actions, health
   and Prometheus metrics, event-driven ranking, bans, persistence, and
   recovery without AMX/Metamod.
6. **Production readiness** — native `aarch64-unknown-linux-gnu` and
   `x86_64-unknown-linux-gnu` builds, repeatable protocol/gameplay regression
   tests, resource limits, abuse controls, graceful shutdown, and measured
   load tests on ARM64 hardware.

## First PoC in this repository

`poc/steam-server-query` is an A2S_INFO query prototype. It defaults to
loopback and permits an explicitly selected private LAN or Tailscale IP for
browser testing. It validates a small piece of Steam server-browser
interoperability without pretending to implement the GoldSrc game connection
protocol. Its challenge value is fixed for deterministic testing; do not expose
it to the public internet.

## Current progress (2026-10-29)

- **Verified by user:** a Python standard-library UDP client completed the
  loopback A2S_INFO challenge/response exchange and received a 75-byte server
  info response. This verifies the query exchange only, not the official Steam
  browser, Steam authentication, player connection, or gameplay.
- **Verified by user:** the unmodified CS 1.6 Steam browser on a second PC found
  the PoC over Tailscale at `100.98.74.9:27025` and displayed
  `CS 1.6 Rust Query PoC`. The real CS server at `100.98.74.9:27015` is a
  separate server; the two ports serve different processes.
- **Verified in the development environment:** UDP `100.98.74.9:27025` was
  owned by `steam-server-query`, and an A2S_INFO challenge/response returned
  the expected `CS 1.6 Rust Query PoC` name.
- **Implemented:** `LISTEN_ADDRESS` can select an explicit loopback, RFC1918
  LAN, Tailscale IPv4, or private/link-local IPv6 address. It defaults to
  `127.0.0.1:27025` and rejects wildcard and public IPs.
- **Static checks passed:**
  - `cargo fmt --manifest-path poc/steam-server-query/Cargo.toml -- --check`
  - `cargo metadata --manifest-path poc/steam-server-query/Cargo.toml --no-deps --format-version 1`
  - `git diff --check`
- **Verified on 2026-10-29:** `cargo test --manifest-path
  poc/steam-server-query/Cargo.toml` passed all 6 unit tests. A separate
  `cargo build` was not run; `cargo test` compiled the test target.
- **Implemented and smoke-tested:** the query PoC has an opt-in
  `packet-trace` Cargo feature that writes local JSONL records of inbound and
  outbound UDP payloads. It is off by default, creates files with mode `0600`
  on Unix, and refuses to overwrite. A local A2S challenge/info exchange was
  captured and verified. Current builds redact connect/unknown inbound payloads.
- **Implemented and locally verified:** the development connection-challenge
  slice responds to `getchallenge steam` and explicitly rejects subsequent
  `connect` packets without authentication or sign-on. Unit tests and a real
  subprocess UDP integration test cover challenge, rejection, query regression,
  and trace redaction. See the
  [design/limitations](../compatibility/connection-challenge.md).
- **Verified by official-client user test and local trace inspection:** the
  client accepted the game challenge sufficiently to send a 452-byte connect
  packet, redacted in the trace. The PoC sent its expected explicit development
  rejection. This proves progression to connect, not Steam authentication or
  sign-on.
- **Implemented and locally verified:** peer-bound random
  game challenges expire after 30 seconds, with a 1024-peer bound and one-use
  consumption. The bounded protocol-48 connect parser checks envelope/auth type
  and ticket length, not ticket authenticity. Local traces expose safe outcome
  codes. See [connect validation](../compatibility/connect-validation.md).
  Formatting, Clippy (default and all features), build, 17 default unit tests,
  20 all-feature unit tests, and the real UDP integration test passed. This is
  synthetic local evidence; an official-client rerun of this revision is pending.
- **Researched, integration gated:** real Steam authentication requires an
  authorized local SDK and confirmation of applicable App ID 10 permissions.
  [Investigation and exact next actions](../compatibility/steam-authentication.md)
  distinguish the legacy GoldSrc ticket API from modern auth-session tickets.
- **Verified with the supplied SDK:** the isolated Steam diagnostic loaded the
  actual x86-64 runtime, initialized with advertising disabled, and received a
  server-connected callback with logged-on state and nonzero server identity.
  It then logged off and shut down. ARM64 runtime execution is still unverified.
  See [probe](../../poc/steam-auth-probe/README.md) and
  [authentication evidence](../compatibility/steam-authentication.md).
- **Implemented, opt-in:** `steam-auth` loads the local SDK dynamically in the
  Rust PoC, uses the real server identity, submits borrowed tickets without
  logging them, and handles approval/denial/timeout with bounded pending state.
  A real-SDK diagnostic verified Rust server logon and rejection of a synthetic
  invalid ticket. Positive official-client approval requires a fresh user test.
  Sign-on/admission are intentionally still disabled, including after approval.
- **Verified with the official-client user test (2026-10-29):** the opt-in Rust
  server recorded `challenge_issued`, a redacted 452-byte connect,
  `steam_auth_pending` (protocol 48, 206-byte ticket), then `steam_auth_approved`.
  The positive legacy Steam approval gate passed on x86-64. The next slice is
  authenticated connection acceptance and reliable netchannel/sign-on, not
  gameplay or unauthenticated admission.
- **Implemented and locally tested:** authenticated connection acceptance and a
  bounded first-netchannel diagnostic handle the initial `new` command and send
  a reliable explicit disconnect rather than fabricate world sign-on. The Steam
  session stays active until close/timeout/revocation. Unit tests exercise wire
  transform, sequencing, retransmission and limits; official-client validation
  is still required. See [netchannel](../compatibility/authenticated-netchannel.md).
- **Verified by official-client test on 2026-10-30:** Steam approval, connection
  acceptance, `netchan_new`, `netchan_ack`, and `netchan_disconnect_acked` were
  recorded. The intentional disconnect follows the first `new`; this is not
  serverinfo, completed sign-on, or gameplay.
- **Prepared but not wired:** a bounded `svc_serverinfo` encoder with synthetic
  layout tests; see [serverinfo preparation](../compatibility/serverinfo.md).
  Actual map CRC, client DLL digest, delta descriptions, resource lists and
  fragmentation remain prerequisites before sending it to the official client.

The compatibility inventory has been started in
[`docs/compatibility/inventory.md`](../compatibility/inventory.md). Protocol 48
is confirmed by the locally acquired HLDS executable (App BuildID `15961496`,
CS executable build `10211`); the official client build remains unknown. HLDS
did not stay healthy enough in this container to capture a query response, and
no client/HLDS packet fixture exists yet. The opt-in Steam adapter has now
received positive approval and verified the initial acceptance/netchannel
boundary with the official client. Next: implement serverinfo, resource
negotiation and the rest of sign-on.
Applicable permissions remain the owner's responsibility. Client build
identification and reference fixtures remain open evidence
gaps; do not treat local synthetic tests as those fixtures.

## Open validation items

- Confirm the precise official Steam client build/protocol and obtain a working
  reference-HLDS query capture.
- Verify whether Steam game-server authentication and master-server listing
  can be implemented and tested by this project, and under what SDK terms.
- Obtain and review local connection/sign-on fixtures from an unmodified official
  client and a local reference HLDS server.
- Decide the first gameplay mode and the exact set of native features replacing
  currently used plugins.
- Confirm permissions for every map and asset before adding it to a distributable
  image or FastDL bundle.

## References

- [Valve Developer Community: Server queries](https://developer.valvesoftware.com/wiki/Server_queries)
- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [ReHLDS](https://github.com/rehlds/rehlds), an alternative compatibility
  implementation considered during the decision.
