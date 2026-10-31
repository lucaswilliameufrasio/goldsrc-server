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
| Valve HLDS reference | SteamCMD App ID `90`, app `BuildID` `15961496`; `cstrike/steam.inf` says `PatchVersion=1.1.2.7`. The executable reports protocol `48`, `1.1.2.7/Stdio (cstrike)`, build `10211` (`2024-10-08 01:34:04`). | Obtain a working runtime and capture actual query/game-connection exchanges. | Acquired anonymously from Valve and version output verified in an x86-64 Debian container. A query capture is still pending. |
| Steam server query | The query PoC encodes A2S_INFO protocol byte `48` (`poc/steam-server-query/src/main.rs`). | Compare against a packet captured from the reference HLDS and official browser. | PoC implementation only; not an observed reference fixture. |

The A2S_INFO response's protocol byte and the GoldSrc game-connection protocol
must be tracked separately. A matching query response does not establish a
successful client connection, Steam authentication, sign-on, or gameplay.

### Evidence sources

- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [Valve Developer Community: GoldSrc version command](https://developer.valvesoftware.com/wiki/Version_(GoldSrc))
- Local PoC source: `poc/steam-server-query/src/main.rs` (`PROTOCOL_VERSION = 48`)

## Local fixture inventory

| Fixture | Expected producer | Local status |
| --- | --- | --- |
| Client-to-HLDS connectionless challenge/connect exchange | Official Steam client | Not captured |
| HLDS-to-client challenge/connect response | Valve HLDS reference | Not captured |
| Sign-on and first successful player join | Official client + Valve HLDS | Not captured |
| A2S_INFO exchange for comparison | Official server browser/client + Valve HLDS | Not captured |

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
No reference-HLDS `.pcap`, `.pcapng`, or `.dem` fixture has been captured. Existing PoC unit
tests are authored test vectors, not captures from an independent implementation.

The reference executable printed the expected protocol/build information, but
did not remain healthy for a query capture in this container: its Steam runtime
reported missing `libcurl.so`/trusted certificates and the process exited with
signal 11. Do not treat the attempted runtime as a successful HLDS query test.

## Capture procedure

Use only a legally acquired, unmodified client and a local reference HLDS that
you control. Keep raw captures outside version control at `fixtures/local/`.
Packet captures can contain IP addresses, player identifiers, and Steam
authentication material; do not publish or commit raw captures. Derive minimal
test vectors only after reviewing and removing sensitive data and confirming
that redistribution is appropriate.

For each installation, record:

- capture date, OS/architecture, and whether the binaries are unmodified;
- client Steam app ID and `BuildID` (from the local Steam app manifest), plus
  the in-game `version` output;
- HLDS `version` output and relevant `steam.inf` version fields;
- protocol value reported by each endpoint, game directory, map, and UDP port;
- capture interface, endpoint addresses (redact before sharing), and fixture
  filename/SHA-256;
- scenario and direction for each extracted packet (query, challenge, connect,
  authentication, sign-on, first join).

The query PoC can write an opt-in JSONL trace with
`GOLDSRC_PACKET_TRACE=fixtures/local/query-poc.jsonl` and Cargo feature
`packet-trace`; see the [README](../../README.md). This captures only UDP
datagrams received/sent by the PoC. Connect and unknown inbound payloads are
redacted by current builds. It cannot observe a separate HLDS
process. For the reference server, capture on the
HLDS host/interface as below.

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

Example capture for a client and HLDS on the same Linux host (adjust interface,
address, and port as needed):

```sh
mkdir -p fixtures/local
sudo tcpdump -i lo -nn -s 0 -w fixtures/local/client-hlds.pcap \
  'udp and host 127.0.0.1 and port 27015'
```

Capture a fresh connection attempt through the first successful join, then stop
the capture. Do not include credentials, server passwords, or Steam tickets in
shared test vectors.

## Next actions

1. Locate or install the official Steam client and an unmodified local Valve
   HLDS reference; record exact build/version output rather than inferring it
   from the protocol number.
2. Capture the scenarios above and populate the fixture inventory with paths,
   hashes, and redacted metadata.
3. Compare the captured protocol exchange before implementing connection/sign-on.
