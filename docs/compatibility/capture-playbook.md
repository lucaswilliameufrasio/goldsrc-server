# Sanitized CS 1.6 / HLDS capture playbook

**Purpose:** collect only version metadata, a sanitized query summary, and a
small server-side movement vector from an unmodified Steam CS 1.6 client and a
local Valve HLDS reference. This is a procedure, not evidence that a capture has
been run.

## Safety rules

- Do not create, request, or share a raw PCAP/PCAPNG, demo, packet hex dump, or
  memory dump for this task.
- Never run `status` for shared output. It can reveal player identities. Do not
  include Steam IDs, player names, IPs, passwords, Steam tickets, auth payloads,
  or complete unredacted console/log output.
- Keep the client-to-HLDS test on loopback or a private LAN/Tailscale path you
  control. Do not expose the diagnostic PoC publicly. Keep Steam server
  advertising disabled; no master-server listing test is part of this playbook.
- The only movement data to share is the small numeric vector described below.
  If a debugger view unexpectedly contains credentials or identity fields, stop,
  do not save it, and share none of that output.

## 1. Record build and protocol metadata

On the machine with the official Steam client:

1. Find its local `steamapps/appmanifest_10.acf` in the Steam library that
   contains Counter-Strike. Read only the `buildid` value; do not send the full
   manifest or a path containing a personal account name.
2. In the CS 1.6 console, run `version`. Transcribe only the executable
   version/build/date and protocol fields. Do not paste unrelated console text.
3. Record OS/architecture and that the installation is the unmodified Steam
   client. Do not use `status`.

On the local reference HLDS:

1. Record the HLDS app BuildID, the `version` output's engine/protocol/build
   fields, and the `PatchVersion` from `cstrike/steam.inf`.
2. Use one known local map and record only its map name. Do not infer the client
   build from protocol 48.

If the installation is not available locally, mark that field unavailable; do
not substitute a web page's historical protocol mapping for observed metadata.

## 2. Summarize an HLDS query without retaining packets

Run the repository's standard-library query summarizer against the local HLDS;
it does not save packets and prints only allowlisted fields:

```sh
python3 poc/steam-server-query/examples/hlds_query_summary.py 127.0.0.1 \
  --port 27015
```

Replace `127.0.0.1` with a private IP only if client and HLDS are on separate
machines. The tool rejects public targets, omits the server hostname/address and
optional Steam identity fields, and never prints raw bytes. It supports
single-datagram Source/legacy GoldSrc A2S_INFO responses; split responses are
reported as unsupported. Do not enable packet dumping or save a capture.
Transcribe only:

- whether the challenge/info exchange completed;
- request/response datagram lengths;
- protocol byte, game directory, map name, current/max players, and server
  version string;
- whether the returned query values match the HLDS console/configuration.

Replace the server hostname with `<redacted>` and omit endpoint addresses,
player names, IDs, and raw bytes. If the HLDS does not stay healthy enough to
answer, record that as a failed attempt; do not report the Rust query PoC as an
HLDS reference result.

### Optional: isolated Docker HLDS

The following pinned amd64 image was verified with the query helper. Use an
already verified rootless Docker context (replace `rootless` if your context has
another name). The image's default command binds `0.0.0.0`, so override it as
shown; do not publish ports. For a selected local map, bind-mount only that BSP
read-only from its existing checkout. Do not copy the BSP into this repository
or into a derived image.

```sh
MAP_DIR=/path/to/the/local/maps
MAP=de_dust2_2x2 # or fy_pool_day
case "$MAP" in
  de_dust2_2x2|fy_pool_day)
    :
    ;;
  *)
    exit 2
    ;;
esac
CONTAINER=cs16-hlds-query
MAP_BSP="$MAP_DIR/$MAP.bsp"
test -r "$MAP_BSP" || exit 2

docker --context rootless run --detach --rm \
  --name "$CONTAINER" --network none --cpus 2 --memory 1g \
  --pids-limit 256 --ulimit core=0 --stop-timeout 10 \
  --volume "$MAP_BSP:/home/hlds_user/hlds/cstrike/maps/$MAP.bsp:ro" \
  --entrypoint /bin/bash --workdir /home/hlds_user/hlds \
  hldsdocker/cstrike@sha256:484f7c3c433099e55f87c0c7a56b52511a98615fa2a07265fa4264cf3cf7ccc6 \
  -c "exec ./hlds_run -game cstrike +ip 127.0.0.1 +sv_lan 1 +map $MAP"

docker --context rootless inspect --format \
  'network={{.HostConfig.NetworkMode}} ports={{json .NetworkSettings.Ports}}' \
  "$CONTAINER"
docker --context rootless run --rm \
  --network "container:$CONTAINER" --read-only \
  --volume "$PWD/poc/steam-server-query/examples/hlds_query_summary.py:/hlds_query_summary.py:ro" \
  --entrypoint python3 \
  python@sha256:0f086103b026048cc50cb5da25ec435c023c5241a83fc44daea593377abc61c4 \
  /hlds_query_summary.py 127.0.0.1 --port 27015 --timeout 2.0

docker --context rootless stop --time 5 "$CONTAINER"
```

The helper shares only the isolated container's network namespace and prints
allowlisted query fields. If the first query occurs during startup, retry after
a short delay. Stop the server when done; keep only the sanitized summary. Do
not retain or share logs or packet dumps; neither is needed.

## 3. Capture one movement vector at the HLDS simulation boundary

This uses GDB's typed view of the local reference process, not a network capture.
The HLDS build must expose the `SV_RunCmd` symbol and argument/local variable
names. The local reference scripts in `poc/steam-server-query/examples/` use the
same GDB-with-symbols approach. Function arguments can differ by HLDS build; if
GDB cannot resolve `ucmd` or `sv_player`, stop and record the missing symbol—do
not guess memory offsets or switch to dumping packets.

1. Start one local HLDS instance with the target map and a private-only test
   setup (`sv_lan 1`; bind/firewall it to loopback or a private interface). Do
   not run a public server or enable listing.
2. Start GDB on that local HLDS binary. Enable pending breakpoints, stop once at
   `SV_ActivateServer`, then set a breakpoint at `SV_RunCmd` and continue. This
   lets the game DLL/world finish loading before the movement breakpoint is
   installed. Connect one official client directly over the private path.
3. At a `SV_RunCmd` stop for the intended simple movement (one client, on
   ordinary ground, no ladder/water/teleport; command duration at most 50 ms),
   use `info args` to confirm `ucmd` and inspect only these fields:

   ```gdb
   p ucmd->msec
   p ucmd->lerp_msec
   p ucmd->viewangles
   p ucmd->forwardmove
   p ucmd->sidemove
   p ucmd->upmove
   p ucmd->buttons
   p sv_player->v.origin
   p sv_player->v.velocity
   p sv_player->v.mins
   p sv_player->v.maxs
   p sv_player->v.flags
   p sv_player->v.waterlevel
   p sv_player->v.movetype
   p sv_movevars.gravity
   p sv_movevars.maxspeed
   p sv_movevars.accelerate
   p sv_movevars.airaccelerate
   p sv_movevars.friction
   p sv_movevars.stopspeed
   p sv_movevars.stepsize
   ```

   Record the numeric values as `before` plus the `usercmd`. Do not print the
   enclosing client/player structures.
4. Run GDB's `finish` for that `SV_RunCmd` invocation, then record
   `sv_player->v.origin`, `velocity`, `flags`, `waterlevel`, and `movetype` again
   as `after`. For this first vector, choose a command with `msec <= 50` so the
   engine does not split it into multiple movement calls. If `finish` does not
   return after the command or the state is not available, leave the vector
   incomplete rather than pairing unrelated samples.
5. Repeat only for a few named scenarios if needed: idle, straight movement on
   level ground, strafe, jump, and crouch. Keep each sample separate and label
   the scenario; avoid combat, chat, names, and player status output.

`usercmd_t` fields are documented in Valve's public
[`common/usercmd.h`](https://github.com/ValveSoftware/halflife/blob/master/common/usercmd.h).
The reference flow is `SV_ParseMove` → `SV_RunCmd` → movement setup/run/finish;
see the public
[ReHLDS `sv_user.cpp`](https://github.com/rehlds/rehlds/blob/master/rehlds/engine/sv_user.cpp)
as a source-level navigation aid, not as proof that every HLDS build has an
identical ABI.

## 4. Share only this sanitized template

```text
capture_date: YYYY-MM-DD
client: official Steam CS 1.6; OS/arch=<...>; BuildID=<...>
client_version: <executable version/build/date>; protocol=<...>
hlds: App BuildID=<...>; PatchVersion=<...>; engine build=<...>; protocol=<...>
query: completed=<yes/no>; lengths=<request>/<response>; protocol=<...>
       game=<...>; map=<...>; players=<n>/<max>; version=<...>
       hostname=<redacted>; endpoint=<omitted>
movement:
  scenario=<...>; map=<...>; command_duration_ms=<...>
  cmd={lerp_ms, viewangles, forwardmove, sidemove, upmove, buttons}
  before={origin, velocity, mins, maxs, flags, waterlevel, movetype}
  after={origin, velocity, flags, waterlevel, movetype}
  movevars={gravity, maxspeed, accelerate, airaccelerate, friction,
            stopspeed, stepsize}
  debugger_symbols=<resolved/not resolved>
```

Do not attach the original app manifest, logs, packet files, full GDB transcript,
or an unredacted server config. Keep working notes under `fixtures/local/`, which
is already ignored by Git; do not add Steam SDKs, maps, captures, or other local
assets to commits.

## 5. Maps from the other CS 1.6 project

The other project's map directory has been located in a separate persistent
local checkout. Use its maps there only; they were not copied into this
repository and must not be packaged or redistributed. The selected checkout is
outside this repository, so no `.gitignore` rule is needed. See the local-only
entity inventory in
[`native-capabilities-proposal.md`](../plans/native-capabilities-proposal.md);
entity presence is not proof that a map loads or plays correctly.
