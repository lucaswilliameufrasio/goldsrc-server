# GoldSrc Server

A Rust-native dedicated server project targeting unmodified official Steam
Counter-Strike 1.6 clients. The project aims to provide ARM64-native builds and
implement administration, metrics, rankings, bans, and game features in the
server rather than through AMX Mod X or other binary plugins.

This repository is at the design and protocol-validation stage. The PoC can
optionally authenticate with Steam and accept an initial channel, but cannot
complete sign-on or admit players. It does not register with the master server.
It binds to `127.0.0.1` by default. An explicit private LAN or
Tailscale IP can be selected with `LISTEN_ADDRESS`; wildcard and public IPs are
rejected. Challenge state is bounded and volatile, but the PoC has no measured
per-peer rate limits or production abuse capacity; keep it on private networks.

## Documents

- [Technical plan](docs/plans/rust-goldsrc-arm64-server.md)
- [Architecture decision](docs/decisions/rust-goldsrc-arm64-server.md)
- [First playable mode decision](docs/decisions/stock-cs16-first-playable.md)
- [Compatibility inventory](docs/compatibility/inventory.md)
- [Sanitized client/HLDS capture playbook](docs/compatibility/capture-playbook.md)
- [Proposed first gameplay scope and native capabilities](docs/plans/native-capabilities-proposal.md)

## Run the local query PoC

Requires a Rust toolchain. The default PoC uses `getrandom` for OS-backed
connection challenges and does not require the Steam SDK. The opt-in
`steam-auth` feature dynamically loads the owner's local Steamworks SDK.

```bash
cargo run --manifest-path poc/steam-server-query/Cargo.toml
```

It answers the Steam A2S_INFO challenge/server-info query and the official
client's `getchallenge steam` on `127.0.0.1:27025`. Game challenges are bound to
the source IP/port, expire after 30 seconds, and are consumed by a well-formed
connect. Subsequent game connections undergo bounded envelope validation and
receive explicit errors or a Steam-auth-unavailable rejection, never admission.
Source A2S_INFO challenges are independently random, source-IP/port-bound, and
expire after 30 seconds; at most 1024 pending A2S peers are retained. This is a
state bound, not a request-rate limit.
Without a loaded BSP, A2S_INFO reports `de_dust2`; with `GOLDSRC_MAP_BSP`, it
reports the validated map filename stem after static BSP/spawn/objective parsing.
World simulation and client sign-on remain disabled.
To test from another device, bind the server host's private address:

```bash
LISTEN_ADDRESS=100.x.y.z:27025 cargo run --manifest-path poc/steam-server-query/Cargo.toml
```

Use the host's Tailscale IPv4 address (`tailscale ip -4`) or its LAN IP. Allow UDP
port `27025` in the host firewall/Tailscale ACL from the test device only; do not
forward this port from the router to the public internet. Add
`<private-ip>:27025` to the Steam server browser's Favorites. This validates
only server-query packets; the PoC cannot accept a player or prove gameplay
compatibility.

For an opt-in local packet trace during development, create a fresh output file
and enable the `packet-trace` feature:

```bash
mkdir -p fixtures/local
LISTEN_ADDRESS=192.168.1.20:27025 \
GOLDSRC_PACKET_TRACE=fixtures/local/query-poc.jsonl \
  cargo run --manifest-path poc/steam-server-query/Cargo.toml --features packet-trace
```

Replace `192.168.1.20` with the server host's private LAN/Tailscale address when
testing from another computer; for a client on the same host, omit
`LISTEN_ADDRESS` to use loopback.

The trace records supported query/getchallenge datagrams as hex and peer
addresses. Inbound connect/unknown payloads are redacted; connect validation
adds allowlisted outcomes and, where parsed, numeric protocol/ticket length only
because they can contain Steam tickets or passwords. It is disabled by
default, refuses to overwrite an existing file, and is not telemetry sent over
the network. It can still contain private query payloads and peer addresses, so
do not share it. This PoC trace covers this PoC only, not a separate HLDS
process.

Do not enable this trace for compatibility evidence or share its output. Use the
[sanitized capture playbook](docs/compatibility/capture-playbook.md) and its
allowlisted A2S_INFO summarizer instead; it does not retain datagrams.

### Test the connection challenge over Tailscale

Stop your previous PoC with Ctrl+C, then run in the project root:

```bash
TS_IP="$(tailscale ip -4)" || exit 1
test -n "$TS_IP" || { echo "No Tailscale IPv4 available" >&2; exit 1; }
mkdir -p fixtures/local
TRACE_FILE="fixtures/local/challenge-$(date +%Y%m%d-%H%M%S)-$$.jsonl"
printf 'Server: %s:27025\nTrace: %s\n' "$TS_IP" "$TRACE_FILE"
LISTEN_ADDRESS="${TS_IP}:27025" GOLDSRC_PACKET_TRACE="$TRACE_FILE" \
  cargo run --manifest-path poc/steam-server-query/Cargo.toml --features packet-trace
```

In the official CS 1.6 console, run `connect <displayed-ip>:27025`. The expected
trace is `getchallenge steam`, our text challenge reply, then a redacted
`connect` if the client accepts the placeholder server identity. The client
will then see a development rejection. If it only retries `getchallenge`,
preserve that trace and the console error for analysis. A successful player join
is **not** expected. See [challenge design](docs/compatibility/connection-challenge.md).

For the current slice, look for a `connection_outcome` record with
`outcome: "steam_auth_unavailable"`: the envelope/challenge passed local checks,
but the ticket was **not authenticated**. Other outcomes identify malformed
packets, unsupported protocols/auth types, invalid challenges or ticket lengths.

### Next steps and prerequisites

1. Test this updated validator with the official client using the command above.
2. The owner confirmed the intended SDK use with App ID 10. Public advertising
   is explicitly out of scope and remains disabled; see
   [Steam investigation](docs/compatibility/steam-authentication.md).
3. Server logon and positive legacy ticket approval are verified on x86-64. The
   initial authenticated netchannel test reaches `new` and then intentionally
   disconnects; full sign-on and gameplay remain unimplemented. See
   [netchannel slice](docs/compatibility/authenticated-netchannel.md).

The SDK has since been supplied locally, and the real x86-64 runtime has passed
initialization and anonymous server logon in an isolated diagnostic. To reproduce:

```bash
python3 poc/steam-auth-probe/probe.py
python3 poc/steam-auth-probe/probe.py --logon --seconds 30
```

The second command contacts Steam, without personal credentials, with server
advertising disabled. See [probe scope](poc/steam-auth-probe/README.md).
This is server-login evidence only: the Rust PoC does not yet submit or validate
player tickets in its default mode. The opt-in `steam-auth` adapter now submits
fresh tickets to the real SDK and handles approval/denial/timeouts. Synthetic
invalid-ticket rejection and positive approval from an official-client user test
have been verified on x86-64. The initial post-approval channel has also been
verified with an official client; it intentionally disconnects after `new`.
Full sign-on remains unimplemented.

### Test real Steam player authentication over Tailscale

Stop the previous PoC. In the project root, run:

```bash
cargo build --manifest-path poc/steam-server-query/Cargo.toml --features packet-trace,steam-auth
TS_IP="$(tailscale ip -4)" || exit 1
test -n "$TS_IP" || exit 1
mkdir -p fixtures/local
python3 poc/steam-auth-probe/probe.py \
  --run-server poc/steam-server-query/target/debug/steam-server-query-poc \
  --listen "${TS_IP}:27025" --seconds 120 \
  --trace "fixtures/local/steam-auth-$(date +%Y%m%d-%H%M%S)-$$.jsonl"
```

Wait for `server_connected, ready=true`, then connect to the displayed server
IP/port with the official client. Leave the diagnostic running until its normal
two-minute exit so SDK cleanup occurs. Possible trace outcomes include
`steam_auth_rejected_basic`, `steam_auth_pending`, `steam_auth_approved`,
`steam_auth_denied` (with numeric SDK reason where available), or
`steam_auth_timeout`. After approval the server sends connection acceptance,
then recognizes the initial `new` command and explicitly disconnects because
world sign-on is not implemented. Look for `connection_accepted`, `netchan_new`
and `netchan_disconnect_acked` or `netchan_timeout` in the trace. Official-client
compatibility with this new slice remains unverified until tested.
See [isolation and verification](poc/steam-auth-probe/README.md).

## Verification

```bash
cargo fmt --manifest-path poc/steam-server-query/Cargo.toml -- --check
cargo clippy --manifest-path poc/steam-server-query/Cargo.toml --all-targets --all-features -- -D warnings
cargo build --manifest-path poc/steam-server-query/Cargo.toml --all-features
cargo test --manifest-path poc/steam-server-query/Cargo.toml
cargo test --manifest-path poc/steam-server-query/Cargo.toml --all-features
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s poc/steam-auth-probe -p 'test_*.py'
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s poc/steam-server-query/examples -p 'test_*.py'
git diff --check
```

The UDP integration test launches the actual PoC on an ephemeral loopback port,
checks game challenge/rejection and A2S exchanges, and cleans up its process.
With `packet-trace`, it also checks on-disk redaction and Unix permissions.
