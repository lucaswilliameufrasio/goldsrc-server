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

## Verification

`movement::tests` exercises open motion, wall sliding, a floor impact, fully
solid motion, and invalid inputs against synthetic one-plane BSPs. This proves
the local collision primitive's contract only. It does not compare player
movement output with HLDS or the official Steam client. Separately, the opt-in
local map inventory traces the measured standing-player hull at each raw spawn
origin; this checks static clearance only, not runtime spawn adjustment. The
next movement slice must establish validated command/state inputs and compare a
bounded movement case with the local HLDS reference before integration.
