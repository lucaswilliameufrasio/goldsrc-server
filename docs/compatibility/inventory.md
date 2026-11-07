# GoldSrc Compatibility Inventory

Initial inventory started on 2026-10-29. This records evidence and unknowns for
the project's compatibility target: an unmodified, official Steam Counter-Strike
1.6 client and a local Valve HLDS reference server. Do not treat assumptions or
the query PoC as proof of game-connection compatibility.

## Version and protocol baseline

| Target | Known | Still to record | Status |
| --- | --- | --- | --- |
| Official Steam CS 1.6 client | Steam app ID `10`; the target is the unmodified official client. | Steam `BuildID`, platform/OS, branch, GoldSrc executable version/build, and protocol reported by the installed client. | Not installed/found in the inspected local Steam paths; exact build unconfirmed. |
| GoldSrc game connection | Reference HLDS reports protocol `48`, `1.1.2.7/Stdio`, build `10211`; historical version mappings are not evidence of the current Steam client's executable build. | Confirm the official client's reported protocol/build and observe negotiation behavior. | HLDS baseline observed; official-client build/protocol remains unconfirmed. |
| Valve HLDS reference | SteamCMD App ID `90`, app `BuildID` `15961496`; `cstrike/steam.inf` says `PatchVersion=1.1.2.7`. The executable reports protocol `48`, `1.1.2.7/Stdio (cstrike)`, build `10211` (`2024-10-08 01:34:04`). | Obtain sanitized client movement/gameplay evidence. | Acquired anonymously from Valve. A pinned amd64 HLDS container answered sanitized A2S_INFO for stock `de_dust2` and both selected local maps; query evidence does not establish gameplay. Raw packet capture is intentionally not collected. |
| Steam server query | The Rust PoC and pinned HLDS both return Source-format A2S_INFO with protocol byte `48`. | Confirm behavior with the official Steam browser when the client is available; do not infer game-connection compatibility from query results. | Sanitized responses from both servers are recorded below. They differ in challenge behavior, `max_players`, and version; no official browser test was run. |

The A2S_INFO response's protocol byte and the GoldSrc game-connection protocol
must be tracked separately. A matching query response does not establish a
successful client connection, Steam authentication, sign-on, or gameplay.

### Query comparison (2026-11-07)

The same allowlisted query summarizer was used against the Rust PoC on loopback
and the pinned HLDS container:

| Endpoint | Response | Challenge | Request/response lengths | Other sanitized fields |
| --- | --- | --- | --- | --- |
| Rust query PoC (`127.0.0.1:27025`, loopback-only) | Source A2S_INFO; protocol `48` | Two-step challenge | `25/9`, then `29/75` bytes | `de_dust2`, `cstrike`, `Counter-Strike`, 0/32 players, version `0.1.0` |
| HLDS (`127.0.0.1:27015`, isolated container) | Source A2S_INFO; protocol `48` | Direct info response | `25/107` bytes | `de_dust2`, `cstrike`, `Counter-Strike`, 0/6 players, version `1.1.2.7/Stdio` |

The query protocol/response type and map/game fields agree; the query challenge,
player limit, and version are not identical. This is a sanitized query-level
comparison only, not official-browser validation or proof of game sign-on.

### Evidence sources

- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [Valve Developer Community: GoldSrc version command](https://developer.valvesoftware.com/wiki/Version_(GoldSrc))
- Local PoC source: `poc/steam-server-query/src/main.rs` (`PROTOCOL_VERSION = 48`)

## Local fixture inventory

| Evidence item | Expected producer | Local status |
| --- | --- | --- |
| Sanitized client-to-HLDS challenge/connect progression (no raw bytes) | Official Steam client + Valve HLDS | Not collected |
| Sanitized HLDS challenge/connect response progression (no raw bytes) | Valve HLDS reference | Not collected |
| Sign-on and first successful player join outcome summary | Official client + Valve HLDS | Not collected |
| Server-side movement vector | Official client + Valve HLDS | Not collected |
| Sanitized A2S_INFO summary | Local query client + Valve HLDS | Observed in an isolated amd64 container for `de_dust2`, `de_dust2_2x2`, and `fy_pool_day`; summary only, no raw packet file retained/shared |

### Official client against the Rust PoC (not HLDS)

User test on 2026-10-29 produced an ignored local JSONL fixture: seven A2S_INFO
requests, four binary query challenge responses, three 75-byte info responses,
and four `getchallenge steam\n` requests without replies in the then-current PoC.
This proves query exchange and arrival of a game-connection attempt, not player
admission. The subsequent [connection challenge slice](connection-challenge.md)
now replies to that request. A second user test produced one getchallenge,
one game challenge reply, one redacted 452-byte connect, and one explicit
development rejection. This verifies client progression to connect, not
authentication or admission; see the challenge slice's validation evidence.

The initial inspection found no client or HLDS installation in the usual Steam
paths. HLDS has since been acquired into the ignored
`fixtures/local/reference-hlds/` directory; the official client is still absent.
No sanitized client-to-HLDS connection, sign-on, or movement evidence has been
collected; the A2S_INFO query evidence is recorded below. Raw packet/demo files
are intentionally excluded from this workflow. Existing PoC unit tests are
authored test vectors, not observations from an independent implementation.

The reference executable printed the expected protocol/build information, but
did not remain healthy for a query summary in this container: its Steam runtime
reported missing `libcurl.so`/trusted certificates and the process exited with
signal 11. A later one-second loopback-only run of the sanitized query tool
received no response from `127.0.0.1:27015`. An isolated retry using the local
HLDS image with networking disabled, LAN mode enabled, and master-server access
disabled found the process bound to UDP 27015 but still received no query
response after startup. A loopback UDP self-test inside that network-isolated
image passed, so the query failure is not evidence that loopback was unavailable.
A second retry used the pinned amd64
`hldsdocker/cstrike@sha256:484f7c3c433099e55f87c0c7a56b52511a98615fa2a07265fa4264cf3cf7ccc6`
image under rootless Docker, `--network none`, no published ports, the `hlds_run`
wrapper, and a loopback-only bind. Earlier invocations with extra startup flags
bound to loopback but timed out. A minimal invocation matching the image's
documented startup style then returned sanitized Source A2S_INFO for stock
`de_dust2` (request/response `25/107` bytes), `de_dust2_2x2` (`25/111`), and
`fy_pool_day` (`25/110`). Each reported protocol `48`, game directory `cstrike`,
game `Counter-Strike`, 0/6 players, and `1.1.2.7/Stdio`; no challenge roundtrip
occurred. For the two selected maps, individual BSPs were bind-mounted
read-only from the companion checkout, never copied into the image. All
successful runs used `--network none`, no published ports, and loopback-only
binding. Containers are stopped. Only sanitized summaries and boolean bind
checks were retained; no raw logs or packet data were retained. This confirms
HLDS map loading and query responses, not client sign-on or gameplay.

## Capture procedure

Use only a legally acquired, unmodified client and a local reference HLDS that
you control. For this task, do not create or share raw PCAP/PCAPNG, packet hex,
demo, or memory dumps. Capture only the sanitized version metadata, query
summary, and numeric server-side movement vector described in the
[capture playbook](capture-playbook.md). Never include tickets, passwords, Steam
IDs, player names, or endpoint addresses.

For each installation, record:

- capture date, OS/architecture, and whether the binaries are unmodified;
- client Steam app ID and `BuildID` (from the local Steam app manifest), plus
  only the version/build/protocol fields from the in-game `version` output;
- HLDS `version` output and relevant `steam.inf` version fields;
- protocol value reported by each endpoint, game directory, and map;
- query response lengths and allowlisted fields, with hostname and endpoint
  omitted/redacted;
- movement scenario, `usercmd` movement fields, state before/after, and the
  relevant movevars; no player or connection identity.

The query PoC can write an opt-in JSONL trace with
`GOLDSRC_PACKET_TRACE=fixtures/local/query-poc.jsonl` and Cargo feature
`packet-trace`; see the [README](../../README.md). This captures only UDP
datagrams received/sent by the PoC, including peer address fields and supported
query bytes. Connect and unknown inbound payloads are redacted by current builds.
It is not a sanitized artifact to share and cannot observe a separate HLDS
process. The requested procedure avoids packet traces entirely; use the
[capture playbook](capture-playbook.md), which includes a standard-library
summarizer that records only sanitized A2S_INFO fields and datagram lengths.

SteamCMD can obtain the reference HLDS without personal Steam credentials; its
HLDS App ID is `90`, and the Counter-Strike mod must be selected before update.
The local install was acquired with these SteamCMD commands:

```text
login anonymous
app_set_config 90 mod cstrike
app_update 90 validate
```

Keep that installation under `fixtures/local/reference-hlds/`; the directory is
ignored by Git and the Valve binaries/assets are not part of this repository.

The existing reference HLDS installation and its maps under
`fixtures/local/reference-hlds/` are already ignored by Git. They are local
reference/test assets, not the selected map targets. The separate server
project's map directory has now been located in a persistent local checkout.
The owner selected `de_dust2_2x2` and `fy_pool_day` for initial local validation;
their BSPs remain in that checkout and were not copied into this repository.
Both maps were loaded by isolated HLDS and identified in sanitized A2S_INFO;
objective and client gameplay remain unverified. See
[`native-capabilities-proposal.md`](../plans/native-capabilities-proposal.md)
for the bounded entity-lump and Rust-parser evidence.

## Next actions

1. Run the sanitized [capture playbook](capture-playbook.md) when an official
   client and healthy local HLDS are available; record exact build/version
   output rather than inferring it from protocol 48.
2. Return only the completed sanitized template. Do not share original logs,
   packet files, full manifests, or GDB transcripts.
3. Use the versioned movement vector and local HLDS outputs as evidence before
   connecting experimental movement/sign-on code to live admission.
