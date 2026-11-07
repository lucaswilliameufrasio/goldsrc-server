# Steam authentication investigation

Status: server logon, invalid-ticket rejection and positive official-client
approval verified on x86-64. Sign-on/admission remain unimplemented.
References checked 2026-10-29 and 2026-11-07.

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
proof that master-list absence was externally observed. On 2026-11-07, the owner
confirmed permission to use the Steamworks SDK with App ID 10 for this project.
That is owner-provided authorization, not an independent legal opinion. Public
advertising is explicitly out of scope and remains disabled.

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

SDK availability is no longer the local prerequisite. Runtime loading, server
logon, and positive legacy-ticket approval have since been verified above on
x86-64; native ARM64 execution remains unverified. No SDK code was executed as
part of the initial static inspection itself.

## Findings

- The public [game-server API](https://partner.steamgames.com/doc/api/ISteamGameServer)
  supports anonymous server logon (`LogOnAnonymous`) and a real server identity
  (`GetSteamID`). Anonymous logon is not the same as bypassing player validation.
- GoldSrc uses the legacy connection-ticket flow. The documented
  `SendUserConnectAndAuthenticate` accepts the opaque blob produced by
  `ISteamUser::InitiateGameConnection`. Its immediate boolean result means basic
  checks only; wait for `GSClientApprove_t`/`GSClientDeny_t` before admission and
  call `SendUserDisconnect` for cleanup. This is the implemented and x86-64-
  verified integration; a modern `BeginAuthSession` ticket is not treated as
  equivalent.
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
  The [official download route](https://partner.steamgames.com/doc/sdk) goes
  through Steamworks. The owner confirmed App ID 10 use for this project on
  2026-11-07. Do not accept agreements for the owner, copy SDKs from arbitrary
  mirrors, or request personal passwords.

## Master-server listing and SDK terms (2026-11-07)

The current official
[ISteamGameServer API reference](https://partner.steamgames.com/doc/api/ISteamGameServer)
documents `LogOnAnonymous` and the master-server heartbeat control
`EnableHeartbeats`; the API also exposes server-advertising controls. This
confirms that the supported SDK has a technical server-logon/listing path. It
does **not** establish that App ID 10 will accept this independent server or
that it will appear in the CS 1.6 browser. The owner confirmed the intended
private SDK/App ID 10 use; public listing is explicitly not selected. The
current Rust adapter keeps advertising disabled via
`SetAdvertiseServerActive(false)`; no heartbeat or public-listing test was run.

Valve's [dedicated-server distribution guidance](https://partner.steamgames.com/doc/sdk/uploading/distributing_gs)
describes publishing a dedicated server as a Steamworks `TOOL` app, enabling
dedicated-server redistributables, and associating it with the base game's App
ID. That is a distribution workflow, not evidence that this project has an
eligible App ID or should be distributed through Steam.

The public [SDK Access Agreement](https://partner.steamgames.com/documentation/sdk_access_agreement)
grants a licensee local SDK use to develop its software and distribution of
`redistributable_bin` components with that software; it also restricts reverse
engineering and requires Steamworks-service communication through the SDK API.
This summary is not legal advice and does not decide whether the owner's SDK
agreement covers every possible distribution scenario. The owner confirmed the
intended local SDK/App ID 10 use on 2026-11-07. No SDK distribution or public
advertising is authorized by that confirmation; those remain separate decisions.

**Status:** legacy player-ticket approval is technically verified with one
official client on x86-64, and official SDK documentation describes the listing
mechanism. Owner permission for the intended App ID 10 SDK use is confirmed.
Native ARM64 auth is unverified. App ID 10 listing eligibility and actual browser
appearance are not being tested because public listing is explicitly out of
scope. Keep advertising disabled.

## Authorization and scope

The local SDK and owner confirmation for the intended App ID 10 use are recorded
above. This authorization covers private development/testing only; it does not
authorize SDK redistribution or public master-server advertising. The owner
selected **no public announcement**. Do not send Steam credentials, player
tickets, or web API keys.

SDK availability and owner permission do not prove that every SDK function works
on every architecture. Native ARM64 initialization/authentication remains an
explicit technical validation gate.

## Remaining validation/actions

1. Continue to load the owner's local SDK dynamically; do not commit or
   redistribute it.
2. Keep `SetAdvertiseServerActive(false)` and do not call a master-server listing
   test. Private client↔server testing is the selected scope.
3. Build/link for ARM64 locally and verify native SDK initialization/logon and
   callbacks on a suitable ARM64 test host before any runtime claim.
4. Preserve opaque in-memory ticket handling and positive Steam approval as the
   admission prerequisite. Verify denial/timeouts and disconnect cleanup without
   logging ticket contents or player identifiers.
5. Continue reliable netchannel/sign-on only behind that authentication gate.

## Verification evidence

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

The fresh official-client approval and initial netchannel boundary have passed
as recorded above. The server intentionally disconnects after the initial `new`
command because sign-on/world state is not implemented. The next implementation
slice is to complete sign-on from verified serverinfo, resource, and movement
inputs. Do not switch to unauthenticated admission.
