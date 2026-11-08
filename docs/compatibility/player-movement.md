# Player movement foundation

**Status:** experimental collision primitive only; not connected to client
packets, sessions, sign-on or a running simulation.

## Ownership and contract

The future authoritative simulation supplies one entity's world-space origin,
velocity, entity-relative bounds and frame duration to `movement::slide_move`.
The function sweeps the entity through one unrotated static world BSP model and
returns its resulting origin, velocity, impact count and all-solid status.

- `movement` owns only bounded collision response.
- `hull_trace` owns BSP hull choice, entity-origin adjustment and the static
  collision query.
- A future command decoder will own client input validation; the game simulation
  will own acceleration, gravity, player state and rules. Neither currently
  calls this primitive.
- Nonfinite state, negative time, invalid bounds/model, malformed impact data,
  or a failed BSP query returns an error; no unchecked endpoint is accepted.
- A sweep wholly inside solid stops velocity and reports `all_solid`.

The operation has a fixed work bound of two BSP sweeps per call. It removes the
velocity component into the first collision plane, tries one remaining-time
sweep, and stops if that sweep hits again. It does not retain state between
calls. This intentionally small response is experimental; it is not a claim of
client-prediction parity.

## Explicit exclusions

This is not a full GoldSrc `PM_Move` implementation. It does not decode
`clc_move`/`usercmd`, apply movevars, accelerate, apply gravity or friction,
categorize ground/water, jump, duck, step up stairs, track touches, recover stuck
players, collide with moving entities, or update network-visible player state.
The accepted first playable target is stock CS 1.6, but this code is not enabled
in live admission.

## Local HLDS reference (synthetic)

`examples/hlds_pmove_wall.gdb` makes one bounded direct call to the stock CS
game DLL's `PM_Move` in the local HLDS 10211 installation. GDB stops after the
engine and game DLL are loaded, uses their DWARF types, and supplies a synthetic
world-only movement state; it does not connect a client or enter `SV_RunCmd`.
Run it only in the network-disabled container shown below.

The case starts at the local `de_dust2` `info_player_deathmatch` origin
`[-832, -704, 176]`, with standing bounds, `MOVETYPE_FLY`, velocity
`[0, 1000, 0]`, 50 ms frame/command duration, and the local server movevars
(`gravity=800`). The zeroed command movement fields make this an isolated
collision probe, not a client-derived command. HLDS ends at
`[-832, -656.03125, 175]`, with velocity `[0, 0, -40]` and impact normal
`[0, -1, 0]`. The opt-in `matches_local_hlds_pmove_wall_impact_on_horizontal_components`
test runs `movement::slide_move` against the same local BSP and confirms the
horizontal impact position and velocity within `0.001`; the primitive leaves
`z=176` and vertical velocity zero because gravity is explicitly out of scope.

Reproduce the isolated capture from the repository root:

```sh
docker --context rootless run --rm --platform linux/amd64 --network none \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  -v "$PWD/fixtures/local/reference-hlds:/data/hlds:ro" \
  -v "$PWD/poc/steam-server-query/examples/hlds_pmove_wall.gdb:/tmp/verify.gdb:ro" \
  -w /data/hlds -e HOME=/tmp -e LD_LIBRARY_PATH=. \
  --entrypoint timeout goldsrc-hlds-crc-local:latest 30s \
  gdb -q -batch -x /tmp/verify.gdb ./hlds_linux
```

Then run the local-BSP comparison test:

```sh
GOLDSRC_DELTA_REFERENCE_DIR="$PWD/fixtures/local" cargo test \
  --manifest-path poc/steam-server-query/Cargo.toml --lib \
  movement::tests::matches_local_hlds_pmove_wall_impact_on_horizontal_components \
  -- --ignored
```

This verifies one synthetic horizontal wall impact only. It is not evidence of
official-client sign-on, client prediction, a real `usercmd`, ordinary walking,
or playable gameplay; sign-on and live admission remain disabled.

## Verification

`movement::tests` exercises open motion, wall sliding, a floor impact, fully
solid motion, and invalid inputs against synthetic one-plane BSPs. This proves
the local collision primitive's contract only. The opt-in HLDS comparison above
covers one synthetic horizontal wall impact, not official-client movement.
Separately, the opt-in local map inventory traces the measured standing-player
hull at each raw spawn origin; this checks static clearance only, not runtime
spawn adjustment. The next movement slice must obtain validated client
command/state inputs and extend comparison to real player movement before
integration.
