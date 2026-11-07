# GoldSrc Compatibility Inventory

Initial inventory started on 2026-10-29. This records evidence and unknowns for
the project's compatibility target: an unmodified, official Steam Counter-Strike
1.6 client and a local Valve HLDS reference server. Do not treat assumptions or
the query PoC as proof of game-connection compatibility.

## Version and protocol baseline

| Target | Known | Still to record | Status |
| --- | --- | --- | --- |
| Official Steam CS 1.6 client | Steam app ID `10`; the target is the unmodified official client. | Steam `BuildID`, platform/OS, branch, GoldSrc executable version/build, and protocol reported by the installed client. | Not installed/found in the inspected local Steam paths; exact build unconfirmed. |
| GoldSrc game connection | Protocol `48` is the provisional reference. The Valve Developer Community's historical version mapping associates protocol 48 with `1.1.2.2/Stdio`; this is not evidence of the current Steam client's exact executable build. | Confirm the client's and reference HLDS's actual `version` output and record any negotiation behavior. | Provisional; not yet captured locally. |
| Valve HLDS reference | SteamCMD App ID `90`, app `BuildID` `15961496`; `cstrike/steam.inf` says `PatchVersion=1.1.2.7`. The executable reports protocol `48`, `1.1.2.7/Stdio (cstrike)`, build `10211` (`2024-10-08 01:34:04`). | Obtain a working runtime and produce a sanitized query/movement summary. | Acquired anonymously from Valve and version output verified in an x86-64 Debian container. Runtime/query summary is still pending; raw packet capture is intentionally not collected. |
| Steam server query | The query PoC encodes A2S_INFO protocol byte `48` (`poc/steam-server-query/src/main.rs`). | Compare sanitized response fields and lengths against a healthy local reference HLDS. | PoC implementation only; no observed HLDS result yet. |

The A2S_INFO response's protocol byte and the GoldSrc game-connection protocol
must be tracked separately. A matching query response does not establish a
successful client connection, Steam authentication, sign-on, or gameplay.

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
| Sanitized A2S_INFO summary | Local query client + Valve HLDS | Pending; no raw packet file will be retained/shared |

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
No sanitized client/HLDS query or movement evidence has been collected; raw
packet/demo files are intentionally excluded from this workflow. Existing PoC
unit tests are authored test vectors, not observations from an independent
implementation.

The reference executable printed the expected protocol/build information, but
did not remain healthy for a query summary in this container: its Steam runtime
reported missing `libcurl.so`/trusted certificates and the process exited with
signal 11. A later one-second loopback-only run of the sanitized query tool
received no response from `127.0.0.1:27015`. An isolated retry using the local
HLDS image with networking disabled, LAN mode enabled, and master-server access
disabled found the process bound to UDP 27015 but still received no query
response after startup. A loopback UDP self-test inside that network-isolated
image passed, so the query failure is not evidence that loopback was unavailable.
No logs or packet data were retained; none of these attempts is a successful
HLDS query result.

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
reference/test assets, not selected production maps. The separate CS 1.6
project's map directory has not been found; see the playbook before importing
any of its assets.

## Next actions

1. Run the sanitized [capture playbook](capture-playbook.md) when an official
   client and healthy local HLDS are available; record exact build/version
   output rather than inferring it from protocol 48.
2. Return only the completed sanitized template. Do not share original logs,
   packet files, full manifests, or GDB transcripts.
3. Use the versioned movement vector and local HLDS outputs as evidence before
   connecting experimental movement/sign-on code to live admission.
