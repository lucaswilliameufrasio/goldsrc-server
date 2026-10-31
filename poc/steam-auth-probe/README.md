# Steam runtime/logon probe

This isolated diagnostic is not the Rust server's authentication implementation.
It uses Python's standard-library `ctypes` with the locally supplied SDK's flat
C API, avoiding a C++ toolchain dependency. No SDK files are committed/copied.

The probe supports Linux x86-64 and AArch64, requires SDK 1.65-compatible headers
and a same-architecture Steam runtime (`steamclient.so`), and reads the exact
initialization interface list from the local SDK headers. Native code runs in a
bounded child process with an isolated temporary HOME/cwd and minimal environment,
not the user's Steam profile. This is isolation for diagnostics, not a sandbox.

Initialization alone is the default. `--logon` explicitly enables anonymous
Steam backend traffic for App ID 10. Server advertising is disabled before logon;
query sockets bind loopback, no gameplay socket is served, and Python-only probe
modes submit no player tickets. Logoff/shutdown run on normal exit. A parent timeout bounds native
hangs. Forced termination cannot guarantee SDK cleanup.

```bash
python3 poc/steam-auth-probe/probe.py
python3 poc/steam-auth-probe/probe.py --logon --seconds 30
python3 -m unittest discover -s poc/steam-auth-probe -p 'test_*.py'
```

## Opt-in Rust authentication diagnostic

The Rust PoC now has a `steam-auth` feature using typed flat-C function pointers
from the trusted local SDK, loaded with optional ISC-licensed `libloading`.
No SDK files are linked into the repository or required by the default build.
The Python launcher only supplies isolated HOME/runtime paths and SDK interface
versions; the Rust process performs server login, challenge identity, ticket
submission, callback handling and auth cleanup itself.

```bash
cargo build --manifest-path poc/steam-server-query/Cargo.toml --features packet-trace,steam-auth
python3 poc/steam-auth-probe/probe.py \
  --run-server poc/steam-server-query/target/debug/steam-server-query-poc \
  --listen 127.0.0.1:27025 --seconds 120 \
  --trace fixtures/local/steam-auth-new.jsonl
```

`--run-server` explicitly enables anonymous Steam backend access even without
`--logon`. `--seconds` bounds the Rust diagnostic (1–120 seconds). The launcher
also enforces a larger native-hang timeout. Normal expiration performs auth
disconnect cleanup, server logoff and SDK shutdown. Ctrl+C/forced termination
does not guarantee native cleanup; let the bounded diagnostic finish normally.

At most 64 pending auth requests are tracked, one per source endpoint, with a
20-second approval deadline. Basic ticket acceptance is only `steam_auth_pending`,
not player approval. Only a matching, unexpired `GSClientApprove_t` while logged
on emits `steam_auth_approved`; denials, kicks and timeouts remain fail-closed.
Raw tickets/userinfo never enter the trace; numeric denial codes may be recorded.
Even after approval the server explicitly rejects admission because sign-on is
not implemented, then notifies Steam of disconnect.

### Live diagnostic verification (separate from offline tests)

```bash
cargo build --manifest-path poc/steam-server-query/Cargo.toml --all-features
GOLDSRC_VERIFY_STEAM=1 PYTHONDONTWRITEBYTECODE=1 \
  python3 -m unittest discover -s poc/steam-auth-probe -p 'test_live_sdk.py' -v
```

This explicit opt-in contacts Steam: the actual Rust binary logs on, advertises
a real server identity in its challenge, and rejects a synthetic invalid ticket
using the real SDK. It checks trace redaction and normal timed exit. It does
**not** prove that an official client's valid ticket will receive approval; that
requires a fresh user-run client connection, not a synthetic or replayed ticket.

By default use `fixtures/local/steamworks-sdk` and the acquired reference HLDS's
`linux64` runtime (x86-64). Use `--sdk`/`--runtime` to supply another authorized
installation; ARM64 requires its own ARM64 Steam runtime, not the x86-64 HLDS
runtime. Existing game/PoC processes are not stopped or reconfigured.

JSON output distinguishes initialization failure, initialization success,
server-connected callback, backend failure/disconnect and timeout. A successful
server logon does not prove legacy player authentication, sign-on, ARM64 runtime
support on another host, master-server absence verified externally, or App ID
usage/redistribution permission. SDK-native diagnostics may also appear on
stdout/stderr. Keep diagnostic outputs local and review before sharing.
