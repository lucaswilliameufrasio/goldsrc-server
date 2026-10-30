# Steam authentication investigation

Status: server logon, invalid-ticket rejection and positive official-client
approval verified on x86-64. Sign-on/admission remain unimplemented.
References checked 2026-10-29.

## Runtime and anonymous-logon evidence

An isolated [probe](../../poc/steam-auth-probe/README.md) now exercises the real
SDK in a bounded native child, with an isolated temporary HOME, no personal
Steam profile, loopback query binding, advertising disabled before logon, and
logoff/shutdown on normal exit. Python ctypes is diagnostic scaffolding, not a
replacement for the Rust server or its future typed adapter.

Verified with SDK 1.65's x86-64 libsteam_api and the previously acquired reference
HLDS x86-64 steamclient runtime:

```text
python3 poc/steam-auth-probe/probe.py
library_loaded: architecture=x86_64
initialized: advertising=false, logon_requested=false
shutdown

python3 poc/steam-auth-probe/probe.py --logon --seconds 30
library_loaded: architecture=x86_64
initialized: advertising=false, logon_requested=true
server_connected: logged_on=true, identity_valid=true
shutdown
```

The success criterion was callback 101 (`SteamServersConnected_t`) together with
`BLoggedOn()` and a nonzero `GetSteamID()`. No ticket was supplied to Steam, and
no player was authenticated. This is not native ARM64 execution evidence or
proof that master-list absence was externally observed. SDK permissions remain
the owner's responsibility, not a result of successful logon.

The probe's five unit tests passed, checking architecture selection/rejection,
Linux callback packing, SDK interface negotiation strings, and invalid-library
preflight. Initialization and actual anonymous logon were each run twice and
succeeded on x86-64. One normal SDK shutdown printed a worker-queue-discard
warning; SDK shutdown returned, but this is not a claim of fully drained internal
SDK work. The Rust format, Clippy, build, default/all-feature test suites and
UDP integration regression also passed after adding this diagnostic.

## SDK received and inspected (2026-10-29)

The owner transferred the SDK to ignored `fixtures/local/steamworks-sdk/`.
`Readme.txt` identifies version 1.65 (2026-07-23). Static inspection confirms:

- Headers include `SteamGameServer015`, anonymous logon and legacy
  `SendUserConnectAndAuthenticate_DEPRECATED`, plus approval/denial callbacks.
- `redistributable_bin/linux64/libsteam_api.so` is an x86-64 ELF library and
  `redistributable_bin/linuxarm64/libsteam_api.so` is an AArch64 ELF library.
- Both libraries export the legacy authentication wrapper, anonymous-logon
  wrapper and `SteamAPI_SteamGameServer_v015` accessor (`readelf --dyn-syms`).
- The legacy authentication function is explicitly deprecated in the headers.
  Symbol presence is not proof of runtime support or successful authentication.
- The transferred SDK remains ignored by Git. Linux tar warnings about
  `LIBARCHIVE.xattr.com.apple.quarantine` refer to ignored macOS extended
  attributes, not missing SDK files. Metadata `._*` files are not build inputs.

SDK availability is no longer the local prerequisite. Runtime loading and
server logon have since been verified above on x86-64; actual legacy ticket
approval and native ARM64 execution remain unverified. No SDK code was executed
as part of the initial static inspection itself.

## Findings

- The public [game-server API](https://partner.steamgames.com/doc/api/ISteamGameServer)
  supports anonymous server logon (`LogOnAnonymous`) and a real server identity
  (`GetSteamID`). Anonymous logon is not the same as bypassing player validation.
- GoldSrc uses the legacy connection-ticket flow. The documented
  `SendUserConnectAndAuthenticate` accepts the opaque blob produced by
  `ISteamUser::InitiateGameConnection`. Its immediate boolean result means basic
  checks only; wait for `GSClientApprove_t`/`GSClientDeny_t` before admission and
  call `SendUserDisconnect` for cleanup. This is the integration to investigate
  first, rather than pretending a modern `BeginAuthSession` ticket is equivalent.
- Valve explicitly says not to mix the old user-auth API with the new API.
  A wrapper exposing only `BeginAuthSession` is not evidence of GoldSrc support.
- The acquired reference HLDS includes an i386 `libsteam_api.so`/`steamclient.so`
  and an x86-64 `linux64/steamclient.so`. There are no SDK headers or ARM64
  libraries in that installation. These artifacts alone do not establish a
  usable native Rust ABI or redistribution rights for our implementation.
- [Valve's current ARM64 guidance](https://partner.steamgames.com/doc/steamhardware/steamframe/engines/custom)
  says Steamworks SDK 1.63+ includes Linux ARM64 libraries. Therefore do not
  claim ARM64 Steamworks is categorically unavailable. Dedicated-server/legacy
  ticket support on that target must still be checked in the actual SDK.
- SDK download/use is governed by the
  [Steamworks SDK Access Agreement](https://partner.steamgames.com/documentation/sdk_access_agreement/).
  The [official download route](https://partner.steamgames.com/doc/sdk) goes through
  Steamworks. At initial investigation this session had no authenticated access or owner confirmation
  of applicable SDK permissions. Do not accept agreements for the owner, copy
  SDKs from arbitrary mirrors, or request personal passwords.

## Prerequisite owned by the project owner

The local SDK is now available as recorded above. Confirm whether using the required interfaces
for Counter-Strike App ID 10 in this independent implementation is permitted;
seek Valve clarification if the terms do not establish that. Do not send Steam
credentials, player tickets, or web API keys.

SDK availability does not by itself prove this project is permitted to use App
ID 10, that the latest SDK retains legacy ticket functions, or that logon and
player authentication will succeed. These remain explicit validation gates.

## Agent actions after the prerequisite

1. Inspect SDK version, permitted redistribution boundary, interface versions,
   supported architectures, and legacy authentication symbols/headers.
2. Implement a narrowly scoped runtime adapter, anonymous logon, callback pump,
   connection timeout and disconnect cleanup. Keep master listing disabled for
   local validation and never mark the placeholder Steam ID as authenticated.
3. Verify initialization/logon on x86-64 and native ARM64 separately. Inspect
   real success/failure callbacks, not just successful linking.
4. Run a fresh official-client connection. Keep the ticket opaque and transient;
   admit only after a positive Steam approval. Verify denial/timeouts do not
   allocate a player, and clean up sessions on disconnect/shutdown.
5. Implement reliable netchannel and sign-on only after that authentication gate.

## Concrete next step

### Official-client approval evidence (2026-10-29)

The user ran the opt-in Rust Steam diagnostic and connected with the official
CS 1.6 client. Inspection of ignored local
`fixtures/local/steam-auth-20261029-214247-199058.jsonl` found:

```text
challenge_issued
connect: 452 bytes, raw payload redacted
steam_auth_pending: protocol=48, ticket_length=206
steam_auth_approved
```

The approval outcome is emitted only for a matching pending identity after
`GSClientApprove_t`, with valid callback size, a logged-on backend and an
unexpired request. This confirms the positive legacy ticket path for this client
and x86-64 runtime, not just basic local ticket checks. No raw ticket or player
identity is reproduced here. It does not prove sign-on, gameplay, VAC support,
other client builds, ARM64 execution, or every failure path. The current code
still rejects admission after approval and notifies Steam of disconnect.

The Rust adapter is now implemented behind opt-in feature `steam-auth`. It uses
the typed flat-C ABI, real logged-on server identity, borrowed in-memory ticket
submission, manual approval/denial callbacks, a 64-request pending bound and
20-second auth timeout. Session cleanup calls legacy disconnect, logoff and
shutdown; a bounded launcher isolates HOME/cwd/runtime paths. The default build
still operates without a Steam runtime and remains `steam_auth_unavailable`.

The real Rust runtime passed logon and a live synthetic invalid-ticket rejection
test, with trace redaction and timed cleanup (repeated successful live runs,
including after the final code change). The offline gate passed 22 all-feature Rust unit tests,
17 default unit tests, real UDP integration in both configurations, five Python
unit tests, Clippy, build and format checks. The live SDK test is skipped by
default and enabled only with `GOLDSRC_VERIFY_STEAM=1`.
The locally available SDK/runtime
are loaded dynamically, not committed or redistributed. An approval callback
only triggers an explicit authenticated-but-sign-on-unavailable rejection and
Steam disconnect; it never allocates a player. Symbol/ABI and deterministic
callback-decision tests do not stand in for a real player's positive approval.

The fresh official-client approval gate has passed as recorded above. The next
implementation slice is authenticated connection acceptance and reliable
netchannel/sign-on, with session cleanup extended beyond this diagnostic's
immediate post-approval rejection. Do not switch to unauthenticated admission.
