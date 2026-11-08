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
4. **Playable slice** — load the two owner-selected local maps
   (`de_dust2_2x2` and `fy_pool_day`); spawn and move players; implement the
   stock bomb-defusal round/weapon loop; verify client prediction and server
   authority with stock clients.
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
protocol. Its A2S challenge is OS-random, bound to the requesting UDP endpoint,
and retained in bounded volatile state for 30 seconds. This is not a measured
rate limit or production capacity; do not expose the PoC to the public internet.

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
  `GLIBC_2.34`; this implies a glibc 2.34 minimum for that GCC-linked artifact,
  not a confirmed deployment baseline. The separate Zig-linked artifact below
  targets a lower symbol floor. The actual Debian/Ubuntu target release and
  native ARM64 runtime remain unknown; no native ARM64 execution was performed.
- **Pinned Zig cross-build (2026-11-07):** installed Zig 0.17.1 locally after
  verifying the x86-64 Linux archive against the SHA-256 published in Zig's
  official download index (`3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a`);
  installed `cargo-zigbuild` 0.24.0 with `--locked`.
  This command passed on the x86-64 workstation:

  ```sh
  cargo zigbuild --manifest-path poc/steam-server-query/Cargo.toml --all-targets --all-features --release --target aarch64-unknown-linux-gnu.2.17
  ```

  The resulting ELF is AArch64 PIE with interpreter
  `/lib/ld-linux-aarch64.so.1`, dynamic dependencies on `libc.so.6`,
  `libpthread.so.0`, and `libdl.so.2`, and imports through `GLIBC_2.17` only.
  Its SHA-256 is
  `23a4b90b12e231a880b235f0caa1e41d3093cd096a32e748d2853caf4ede398c`.
  The 2.17 symbol floor was selected provisionally, not from a confirmed host
  baseline. The owner identifies the intended host as ARM64, but the authorized
  read-only SSH probe failed at name resolution, so its OS release and GLIBC
  remain unverified; native execution was not performed.
- **Sanitized compatibility capture tooling (2026-11-07):** added a playbook for
  client/HLDS version metadata, query summaries, and server-side movement vectors
  without raw packet files, plus a standard-library A2S_INFO summarizer that
  accepts only loopback/private targets and emits allowlisted fields. Its five
  synthetic tests pass. A one-second host loopback query received no
  response. An isolated retry with the local HLDS image (`--network none`, LAN
  mode, no master server) found the process bound to UDP 27015 but still received
  no query response after startup; a loopback UDP self-test in that image passed.
  A second isolated attempt used the pinned `hldsdocker/cstrike` `steam_legacy`
  amd64 image (`sha256:484f7c3c433099e55f87c0c7a56b52511a98615fa2a07265fa4264cf3cf7ccc6`),
  `hlds_run`, explicit loopback `+ip 127.0.0.1`, `--network none`, and no published
  ports. The pinned Python helper (`sha256:0f086103b026048cc50cb5da25ec435c023c5241a83fc44daea593377abc61c4`)
  shared only that network namespace. Invocations with extra startup flags bound
  to loopback but timed out; the image's minimal documented startup style then
  returned sanitized A2S_INFO for stock `de_dust2` and both selected local maps.
  The responses were Source type, protocol `48`, game directory `cstrike`, game
  `Counter-Strike`, players `0/6`, version `1.1.2.7/Stdio`, and request/response
  lengths `25/107`, `25/111`, and `25/110` bytes respectively. The two custom
  BSPs were mounted read-only from the companion checkout; no map assets were
  copied. Bind checks confirmed loopback only; Docker reported network `none`
  and no published ports. Containers are stopped. Only sanitized summaries and
  boolean bind checks were retained; no raw logs or packets were retained. This
  proves HLDS map loading/query only, not client sign-on or gameplay.
  The Rust PoC was then queried on its loopback default using the same helper.
  It returned Source A2S_INFO, protocol `48`, `de_dust2`, `cstrike`,
  `Counter-Strike`, players `0/32`, version `0.1.0`, with the expected two-step
  challenge (`25/9`, then `29/75` bytes). HLDS answers directly (`25/107` for
  stock `de_dust2`) and reports `0/6` plus its own version. This characterizes a
  local query difference; the official Steam browser has not been tested.
- **ARM64 user-mode emulation smoke (2026-11-07):** Buildx exposes only amd64
  platforms here, so no binfmt or Docker configuration was changed. Instead,
  `qemu-aarch64-static` (Debian package `1:7.2+dfsg-7+deb12u18`) ran the
  cross-built ELF against a separately exported, pinned Debian Bookworm arm64
  rootfs (`debian@sha256:ec8534cda2cb30f29f44f23e135297af7a1496f2990fd8e4e84aa36b081a757a`).
  The initial tested ELF had SHA-256
  `d1ffff9a392644322875e47bc2a2971cd299bf0a3cd77e90e08af44fc75b60d0`. The
  latest cross-build after resource, spawn/objective validation, startup-metadata,
  loaded-map A2S reporting and peer-bound query challenges
  (`23a4b90b12e231a880b235f0caa1e41d3093cd096a32e748d2853caf4ede398c`) was
  smoke-tested again; the target loader reports `GLIBC 2.36`. With the rootfs
  and ELF mounted read-only, Docker `network none`, no published ports, and the
  program bound to `127.0.0.1:27025`, the sanitized query helper received the
  expected Source A2S_INFO challenge and response: protocol `48`, `de_dust2`,
  `cstrike`, `Counter-Strike`, 0/32 players, version `0.1.0`, response lengths
  `9/75`. This verifies startup and query behavior under AArch64 user-mode
  emulation on glibc 2.36 only; it is not native ARM64 execution, target-host
  baseline proof, or a performance measurement. The emulation container was
  stopped.
- **Bounded process shutdown (2026-11-07):** the PoC now catches Unix `SIGINT`
  and `SIGTERM`, stops processing newly received UDP packets, and returns through
  normal Rust drop cleanup; the Steam adapter disconnects its owned pending and
  active SDK sessions, disables advertising, logs off, and shuts down. The UDP
  integration test sends both `SIGINT` and `SIGTERM` after loopback readiness,
  verifies that a subsequent query receives no response, and requires a
  successful exit within two seconds for each. This process owns no durable
  queue or background worker, so there is no queued work to drain; diagnostic
  sessions are volatile and are not resumable. This is not yet a production
  server shutdown contract for live players or durable operations.
- **Current-revision fixture regression (2026-11-07):** after the shutdown,
  A2S-challenge, selected-map query and static geometry changes, the available
  opt-in HLDS references passed again: all 9 library tests covering map/CRC/hull/
  delta behavior and 13 binary tests covering resources, serverinfo, sign-on
  assembly, netchannel sequencing and both selected-map wire digests. An
  all-features UDP integration test also verified that a complete resource
  snapshot is accepted before the listener responds while world and sign-on
  remain disabled. The local selected-map inventory test passed (eight parser
  successes; only `35hp_2` failed its strict geometry gate), as did the normal
  default/all-feature suites. These remain offline checks, not client sign-on or
  gameplay evidence.
- **Selected-map resource wire comparison (2026-11-07):** the isolated HLDS
  metadata exporter loaded `de_dust2_2x2` and `fy_pool_day` with their BSPs
  mounted read-only, `network none`, and only a temporary map-directory mount
  writable. It retained no raw packet/memory dump and did not copy either BSP.
  Rust matched the full `SV_SendResources` message length and MD5 for both maps:
  754 resources/62 checks/19047 bytes/
  `3fb3a1b41adb2f087ae86d72f6a8d257`, and 757/62/18993 bytes/
  `6c199f97e42c0901d2fedec5da585826`. `fy_pool_day` includes one signed HLDS
  `nDownloadSize=-1`, represented as the corresponding 24-bit wire value
  `0x00ffffff`; the complete digest matched. The local metadata remains ignored
  under `fixtures/local/selected-map-resource-evidence-wire24/`. This proves
  offline serialization only, not client sign-on, downloads or gameplay. An
  all-features UDP integration test confirms complete resource snapshots are
  validated before the listener responds, while explicitly keeping world and
  sign-on disabled.
- **CS spawn/objective metadata preparation (2026-11-07):** added bounded
  helpers that extract the raw origins of stock `info_player_start` and
  `info_player_deathmatch` entities without assigning teams or moving a player.
  The raw optional `angle` is preserved without a default or view policy.
  `func_bomb_target` references resolve to validated non-world BSP submodels
  with finite, non-empty bounds; malformed and degenerate target bounds fail
  closed. The opt-in map test parsed spawn records for the three entity
  candidates and launched the real PoC with each selected BSP; both listened
  only after startup logged the spawn/objective metadata. Synthetic malformed-
  input tests, formatting, Clippy, default/all-feature tests, Python suites, and
  the opt-in local map inventory passed. The test still reports only `35hp_2`
  as a strict geometry failure among nine local candidates. It also traced each
  raw spawn origin in the three spawn-map candidates with the measured standing-
  player hull; none began in solid. This is a static raw-origin clearance check,
  not verification of the game DLL's spawn adjustment or live placement. Team
  selection, runtime spawn positioning, trigger occupancy and bomb/round
  behavior remain unimplemented.
- **Loaded-map query metadata (2026-11-07):** when `GOLDSRC_MAP_BSP` is set, the
  validated BSP filename stem is now used in A2S_INFO instead of always
  advertising `de_dust2`. The opt-in startup integration test completed the real
  two-step UDP query against each selected local map and parsed the returned map
  field. Synthetic tests cover allowed names and unsafe/overlong stems. This
  remains query metadata only, not client sign-on or active world evidence.
- **A2S challenge hardening (2026-11-07):** the formerly fixed query token is
  now an OS-random 32-bit value bound to the requesting UDP endpoint, with a
  30-second TTL and a 1024-peer state cap. Retries reuse the token without
  extending expiry; valid challenges remain reusable until expiry. Capacity,
  TTL, wrong-peer/token, malformed-packet and entropy-failure behavior have
  deterministic tests, and real loopback UDP tests still complete A2S_INFO.
  This prevents a known fixed token from directly eliciting the larger response
  from a spoofed endpoint; it is not per-peer rate limiting or a production
  capacity claim. Steam browser compatibility remains unverified.

The compatibility inventory in
[`docs/compatibility/inventory.md`](../compatibility/inventory.md) records the
observed HLDS baseline: protocol 48, App BuildID `15961496`, and CS executable
build `10211`. The official client build remains unknown. The pinned HLDS
container produced the query summaries above, but no sanitized client/HLDS
movement evidence exists yet. The opt-in Steam adapter
has received positive approval and verified the initial acceptance/netchannel
boundary with the official client. Next: implement serverinfo, resource
negotiation and the rest of sign-on, without enabling any partial plan before
its inputs and official-client behavior are verified.
The owner confirmed the intended SDK/App ID 10 use and selected no public
announcement; `SetAdvertiseServerActive(false)` remains in force. Client build
identification and client/HLDS movement capture remain open evidence gaps; do
not treat local synthetic tests as those fixtures. The
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

- **Blocked on the official client:** it is not installed in the inspected local
  Steam paths. Provide an owner-controlled local client path or run the sanitized
  [capture playbook](../compatibility/capture-playbook.md) and return its
  allowlisted client build/protocol and movement fields. Do not create/share raw
  PCAPs or identity-bearing traces; the isolated HLDS query summary is already
  recorded above.
- Keep the owner-authorized Steam SDK use private and `SetAdvertiseServerActive`
  disabled. Public listing is explicitly out of scope; no master-server or
  browser-visibility test is required.
- Implement the accepted stock-CS first playable mode from
  [`stock-cs16-first-playable.md`](../decisions/stock-cs16-first-playable.md).
  The tracked companion `plugins.ini` has no active `.amxx` entries, but its
  running deployment may differ; no plugin-specific replacement is established.
- Implement and validate the owner-selected `de_dust2_2x2` and `fy_pool_day`
  bomb-defusal paths. Their map BSPs are outside this checkout and remain local;
  HLDS loading/A2S and the selected-map resource wire messages now match local
  references, but live client resource acceptance, objective completion and
  playable rounds remain unverified.
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
  spawn and bomb-target extraction, translated BSP traces and collision-only
  slide primitive are preparatory/experimental; no team selection, player
  placement, trigger handling, complete movement tick or live world is active.
- Implement native operations replacing required plugins (administration,
  health/metrics, bans, ranking/history and persistence) and verify failure and
  restart behavior. The inspected companion `plugins.ini` has no active plugin
  entries; the running deployment inventory and exact required operations are
  not established, so do not invent plugin replacements or admin/data contracts.
- Finish production readiness from the delivery slices: ARM64 and x86-64 builds,
  protocol/gameplay regressions, resource and abuse limits, graceful shutdown,
  and measured load tests on ARM64 hardware. Game and A2S challenge state is
  bounded but has no per-peer request-rate control; these are development caps,
  not measured production capacity. A native-host x86-64 GNU release build of
  all targets/features passed on 2026-11-07 (SHA-256
  `ac07fb12f3bf8fc374c59e92c652c077a9da7a0945557b39fb6ef9cd1de2cb2c`). That
  artifact imports through `GLIBC_2.34` (an artifact minimum, not a confirmed
  deployment baseline). It completed loopback A2S challenge/INFO exchanges with
  the default map and each selected local BSP; responses reported `de_dust2`
  (75 bytes), `de_dust2_2x2` (79 bytes), and `fy_pool_day` (78 bytes). Both BSP
  runs also logged parsed spawn/objective metadata before listening. These verify
  native x86-64 startup/query and static map preparation only, not client
  sign-on, gameplay, a deployment ABI, or production load. The local
  `cargo zigbuild` artifact
  imports through GLIBC 2.17 and passed the emulated Bookworm smoke above, but
  the deployment release/baseline and native ARM64 runtime validation remain
  unverified. The workstation has no route to the target host and Buildx reports
  no ARM64/QEMU execution platform (the explicit user-mode run above is not a
  registered Buildx platform). If direct
  connectivity is unavailable, transfer the existing artifact through an
  owner-controlled channel to the separate authorized ARM64 test host, then run
  this read-only smoke check there (do not use `oraclepersonalarm`):

  ```sh
  uname -m
  . /etc/os-release
  printf 'os=%s version=%s\n' "$ID" "$VERSION_ID"
  getconf GNU_LIBC_VERSION
  sha256sum ./steam-server-query-poc
  env -i PATH=/usr/bin:/bin ./steam-server-query-poc
  ```

  The program binds only to `127.0.0.1:27025` by default and has no Steam or
  advertising configuration unless explicitly supplied. Stop it with Ctrl-C;
  return only architecture, OS/version, GLIBC, SHA-256, and whether the
  loopback-start message appeared. Do not send full environment dumps or logs.

## References

- [Valve Developer Community: Server queries](https://developer.valvesoftware.com/wiki/Server_queries)
- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [ReHLDS](https://github.com/rehlds/rehlds), an alternative compatibility
  implementation considered during the decision.
