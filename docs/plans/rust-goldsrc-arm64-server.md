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
- **Verified / owner-authorized:** x86-64 server logon and positive official
  legacy-ticket approval are verified; the owner confirmed intended SDK use with
  App ID 10 on 2026-11-07. Public advertising is explicitly out of scope and
  remains disabled. Native ARM64 authentication is not yet verified. See the
  [Steam investigation](../compatibility/steam-authentication.md).
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
  invalid ticket. Positive official-client approval was verified on 2026-10-29;
  a fresh rerun of the latest revision is not recorded. Sign-on/admission are
  intentionally still disabled, including after approval.
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
- **Verified locally:** `bsp_inventory` inspected the v30 header and 15 bounded
  lump descriptors of the locally acquired `de_dust2.bsp`. This reads no map
  content and does not compute the world CRC or validate BSP internals. See
  [serverinfo preparation](../compatibility/serverinfo.md).
- **Verified against Valve HLDS (2026-10-30):** the Rust diagnostic computed
  `0x451b6da9` as the multiplayer CS world CRC for reference `de_dust2.bsp`;
  a debugger observed the same value returned by isolated HLDS build 10211
  `CRC_MapFile`. This is not conventional finalized CRC-32 and is not yet
  encoded into or sent as `svc_serverinfo`.
- **Verified against Valve HLDS (2026-10-30):** local reference `client.dll`
  MD5 is `cf385dcc19e71511ca48f58c68d14278`, matching HLDS `MD5_Hash_File`.
  However, the observed normal CS reference startup had `clientcrccheck=false`
  and a zero-filled 16-byte serverinfo DLL field. The digest tool does not
  unconditionally substitute the MD5 into live sign-on; see
  [serverinfo preparation](../compatibility/serverinfo.md).
- **Verified against Valve HLDS (2026-10-30):** the serverinfo encoder applies
  the correct per-player `COM_Munge3` transformation to the raw world CRC.
  For the reference map, slots 0, 1 and 31 match the actual HLDS function.
  The BSP diagnostic now assembles synthetic serverinfo from the measured map
  CRC and observed zero DLL field; no serverinfo is sent to clients yet.
- **Integrated, not enabled in live sign-on:** bounded outgoing normal-stream
  fragments use the authenticated channel's ACK and retry state. Synthetic
  tests cover loss; isolated official HLDS build 10211 reassembled three Rust
  datagrams byte for byte. Official-client interoperability is not verified.
  Local delta descriptions match HLDS bytes, while compression and resource
  negotiation remain unimplemented.
  See [sign-on fragments](../compatibility/signon-fragments.md).

### Progress (2026-11-06)

- **Verified against local Valve HLDS 10211:** the native BSP point trace now
  compares final positions and impact planes in addition to fractions and
  solid/open/water flags. The opt-in reference test covers 24 traces across the
  four world-model hulls; 10 impacts check position (and plane distance) within
  0.001 map units and plane normals within `1e-5`. This expands static collision
  evidence only; it is not player movement or a running world. See
  [serverinfo and BSP collision evidence](../compatibility/serverinfo.md) and
  the local capture recipe in
  [`hlds_hull_trace.gdb`](../../poc/steam-server-query/examples/hlds_hull_trace.gdb).
- **Still intentionally disabled:** no active world producer installs a
  serverinfo/resource plan after Steam approval. The server continues to
  disconnect after the client's initial `new`; completed sign-on, downloads and
  gameplay have not been demonstrated with the official client.
- **Checks passed on this revision:** `cargo fmt --manifest-path
  poc/steam-server-query/Cargo.toml -- --check`, `cargo build --manifest-path
  poc/steam-server-query/Cargo.toml`, `cargo clippy --manifest-path
  poc/steam-server-query/Cargo.toml --all-targets --all-features -- -D warnings`,
  and `cargo test --manifest-path poc/steam-server-query/Cargo.toml --all-targets`
  with and without `--all-features`. The opt-in
  `impact_positions_and_planes_match_hlds` reference test also passed against
  the local HLDS map. Other fixture-dependent HLDS tests remain ignored unless
  their specific local captures are supplied.

### Progress (2026-11-07)

- **Measured from local Valve HLDS 10211:** the existing isolated GDB capture
  now records `hull_t.clip_mins`/`clip_maxs` for all four world-model hulls.
  It returned `[0,0,0]`/`[0,0,0]` for hull 0, `[-16,-16,-36]`/`[16,16,36]`
  for hull 1, `[-32,-32,-32]`/`[32,32,32]` for hull 2, and
  `[-16,-16,-18]`/`[16,16,18]` for hull 3.
- **Implemented:** `hull_trace::BspHull` names those four indices and exposes
  the measured bounds; `trace_point` now takes this enum rather than a raw
  integer. Its inputs remain the direct model-local points for the low-level
  hull traversal; it does not apply `SV_HullForEntity`'s entity-origin offset.
- **Verified:** `cargo test --manifest-path
  poc/steam-server-query/Cargo.toml --all-targets` passed both without and with
  `--all-features`; `cargo fmt --manifest-path
  poc/steam-server-query/Cargo.toml -- --check`, `cargo build --manifest-path
  poc/steam-server-query/Cargo.toml`, and `cargo clippy --manifest-path
  poc/steam-server-query/Cargo.toml --all-targets --all-features -- -D warnings`
  passed. With `GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local"`, the opt-in
  `world_hull_traces_match_hlds` and `impact_positions_and_planes_match_hlds`
  tests passed again, and `translated_bsp_entity_trace_matches_hlds` passed
  against the captured translated-entity case. Other
  fixture-dependent tests without their required captures remain ignored.

### Progress (2026-11-07): translated BSP hull traces

- **Captured from local Valve HLDS 10211:** `SV_HullForEntity` selects hull 0
  for X width `<= 8`, hull 1 for X width `<= 36` and Z height `> 36`, hull 3
  for X width `<= 36` and Z height `<= 36`, and hull 2 above width 36. Captured
  cases at widths 8/9/36/37 returned the expected indices and offsets; standard
  standing/crouched bounds returned zero offsets.
- **Implemented:** `select_bsp_hull_for_bounds` validates finite, ordered bounds
  and mirrors that selection/offset calculation. `trace_bsp_entity` converts
  world start/end to the selected model-local hull coordinates and restores a
  world-space end position. Rotation, entity movement and player simulation are
  still outside this static trace API.
- **Reference comparison passed:** `hlds_entity_hull.gdb` translated the HLDS
  world entity to `[10, -20, 30]`, then called `SV_ClipMoveToEntity`. HLDS
  returned fraction `0.0882900134`, end `[-445.85498046875, 891.7099609375,
  30]`, flags `[allsolid=0, startsolid=1, inopen=1, inwater=0]`, normal
  `[-0.0, 0.2425356209, 0.9701424837]` and distance `221.090897`. The opt-in
  `translated_bsp_entity_trace_matches_hlds` reproduces these values within the
  documented tolerances.
- **Remaining gameplay foundation:** player movement and world/entity movement
  are not implemented. The API is static and handles translation only; official
  client sign-on remains intentionally disabled.

### Progress (2026-11-07): collision-only movement primitive

- **Implemented:** `movement::slide_move` performs a bounded sweep/slide update
  for one entity against the unrotated static world BSP with at most two sweeps.
  It accepts origin, velocity, local bounds and frame duration; it returns the
  resulting origin, velocity, impact count and all-solid status. It rejects
  invalid state and propagates BSP trace failures. The primitive is not called
  by the live server.
- **Verified synthetically:** open movement, wall sliding with preserved tangent
  velocity, a walkable floor impact, a wholly solid sweep, and malformed inputs
  pass deterministic tests. This is collision-response evidence, not movement
  parity or playable gameplay.
- **Quality gate passed:** `cargo fmt --manifest-path
  poc/steam-server-query/Cargo.toml -- --check`, `cargo clippy --manifest-path
  poc/steam-server-query/Cargo.toml --all-targets --all-features -- -D warnings`,
  `cargo build --manifest-path poc/steam-server-query/Cargo.toml`, and
  `cargo test --manifest-path poc/steam-server-query/Cargo.toml --all-targets`
  with and without `--all-features` all passed. Normal runs retain fixture-only
  HLDS tests as ignored; the translated trace and movement tests were run
  separately against their available local/synthetic inputs.
- **Contract and exclusions:**
  [`player-movement.md`](../compatibility/player-movement.md) records ownership
  and failure behavior. Client command parsing, acceleration, gravity, friction,
  ground/water state, jumping/ducking/stairs, entities and game rules remain
  separate unfinished capabilities.
- **Next required movement evidence:** establish the actual client command and
  persistent player-state inputs from the official client/HLDS exchange, then
  compare one bounded movement case against HLDS. Do not infer the game mode,
  fabricate movevars or connect the primitive to live sign-on before that
  evidence exists.
- **Additional local sign-on fixture checks passed:** `assembled_message_matches_hlds`,
  `serverinfo_uses_crc_from_local_bsp_and_session_slot`,
  `prepared_sequence_with_local_world_inputs`,
  `reference_manifest_fragments_reassemble_and_wait_for_consistency`, and
  `prepared_signon_reassembles_via_fragment_channel` all passed with the local
  HLDS 10211 fixtures. These validate offline message preparation and reliable
  transfer sequencing only; no sign-on packet was sent to a client and no
  official-client gameplay was verified.
- **Expanded opt-in fixture verification (2026-11-07):** with
  `GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local"`, the ignored map suite (4),
  delta suite (2), resource suite (6), sign-on suite (2), and hull-trace suite
  (3) all passed against the available local HLDS/map captures. The four
  prepared netchannel/sign-on fixture tests listed above also passed. This is
  21 offline fixture checks total; the generated `fixtures/local` assets remain
  ignored, and none of these checks observes an official client's live response.
- **ARM64 cross-build and ELF inspection (2026-11-07):** the all-targets
  `aarch64-unknown-linux-gnu` check passed, then the release/all-features binary
  linked in a rootless Debian Bookworm container using Debian's AArch64 GCC 12
  cross-toolchain and glibc 2.36 sysroot. `file`/`readelf` confirm a dynamically
  linked AArch64 PIE with interpreter `/lib/ld-linux-aarch64.so.1`, BuildID
  `9ad8b7e4ac608fa478d75fd0d0ab29add8df36a4`, and direct dependencies only on
  `libc.so.6` and `libgcc_s.so.1`. Its highest imported GLIBC symbol version is
  `GLIBC_2.34`; this suggests a glibc 2.34 minimum for the current artifact, not
  a confirmed deployment baseline. The actual Debian/Ubuntu target release and
  native ARM64 runtime remain unknown; no ARM64 execution was performed.
- **Sanitized compatibility capture tooling (2026-11-07):** added a playbook for
  client/HLDS version metadata, query summaries, and server-side movement vectors
  without raw packet files, plus a standard-library A2S_INFO summarizer that
  accepts only loopback/private targets and emits allowlisted fields. Its four
  synthetic tests pass. A one-second query to loopback port 27015 received no
  response, so live HLDS/client evidence remains pending.

The compatibility inventory has been started in
[`docs/compatibility/inventory.md`](../compatibility/inventory.md). Protocol 48
is confirmed by the locally acquired HLDS executable (App BuildID `15961496`,
CS executable build `10211`); the official client build remains unknown. HLDS
did not stay healthy enough in this container to produce a query summary, and
no sanitized client/HLDS movement evidence exists yet. The opt-in Steam adapter
has received positive approval and verified the initial acceptance/netchannel
boundary with the official client. Next: implement serverinfo, resource
negotiation and the rest of sign-on, without enabling any partial plan before
its inputs and official-client behavior are verified.
The owner confirmed the intended SDK/App ID 10 use and selected no public
announcement; `SetAdvertiseServerActive(false)` remains in force. Client build
identification and a healthy reference-HLDS query/movement capture remain open
evidence gaps; do not treat local synthetic tests as those fixtures. The
[sanitized capture playbook](../compatibility/capture-playbook.md) avoids raw
packet captures and identity-bearing output.

The official Steamworks API documents anonymous game-server logon and
master-server heartbeat controls, while the local Rust adapter keeps advertising
disabled. The existing x86-64 positive legacy-ticket test establishes technical
authentication for that client/runtime only. App ID 10 listing eligibility and
actual public-browser visibility are unverified because public listing is out
of scope. Owner permission for private SDK/App ID 10 use is confirmed; native
ARM64 execution remains unverified. See the updated
[Steam investigation](../compatibility/steam-authentication.md).

## Open validation items

- Confirm the precise official Steam client build/protocol and obtain a
  sanitized query summary from a healthy reference HLDS.
- Keep the owner-authorized Steam SDK use private and `SetAdvertiseServerActive`
  disabled. Public listing is explicitly out of scope; no master-server or
  browser-visibility test is required.
- Obtain sanitized client/HLDS version and movement evidence using the
  [capture playbook](../compatibility/capture-playbook.md); do not create/share
  raw PCAPs or identity-bearing traces.
- Implement the accepted stock-CS first playable mode from
  [`stock-cs16-first-playable.md`](../decisions/stock-cs16-first-playable.md).
  The exact current plugin inventory and native replacements remain pending.
- Obtain the separate project's exact map directory and the server's plugin
  inventory; local searches found no `plugins.ini`, `.amxx`, or client
  `appmanifest_10.acf` under `/mnt/storage/projects`.
- Use the other project's maps locally only; do not package or redistribute
  them. Locate its exact map directory and add an exact ignore rule if it is
  inside this checkout. Confirm rights before any future redistribution.
- Implement a native world/session producer that binds validated map state and
  verified sign-on inputs to the currently approved Steam session; keep admission
  fail-closed until this is ready.
- Complete and verify the official-client sign-on path: `svc_serverinfo`,
  delta/movevars/extra-info messages, resource negotiation and downloads,
  reliable fragmentation, and the remaining client callbacks. The current
  serverinfo and resource paths are preparation-only and are not sent live.
- Build a playable native slice: instantiate map entities, select/spawn players,
  apply the HLDS hull-origin offset, implement player movement and hull/entity
  collision, then add the chosen mode's weapons, rounds and game rules. Current
  translated BSP traces and collision-only slide primitive are experimental;
  no complete movement tick or live world is active.
- Implement native operations replacing required plugins (administration,
  health/metrics, bans, ranking/history and persistence) and verify failure and
  restart behavior.
- Finish production readiness from the delivery slices: ARM64 and x86-64 builds,
  protocol/gameplay regressions, resource and abuse limits, graceful shutdown,
  and measured load tests on ARM64 hardware. The local AArch64 release binary
  links and imports GLIBC symbols through 2.34, but the deployment libc baseline
  and native ARM64 runtime validation still require confirmation.

## References

- [Valve Developer Community: Server queries](https://developer.valvesoftware.com/wiki/Server_queries)
- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [ReHLDS](https://github.com/rehlds/rehlds), an alternative compatibility
  implementation considered during the decision.
