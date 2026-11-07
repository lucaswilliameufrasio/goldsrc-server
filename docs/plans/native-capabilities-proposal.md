# Proposed first gameplay scope and native capabilities

**Status:** the stock-CS first playable mode is selected in the
[accepted decision](../decisions/stock-cs16-first-playable.md). The capability
replacement phases below remain suggestions, not an accepted feature inventory.
The owner's companion server repository was inspected locally on 2026-11-07.
Its tracked `files/plugins.ini` has no active `.amxx` entries and no `.amxx`
binary is tracked; AMX Mod X/Metamod/dproto are optional build extras. This does
not establish what is installed on a running deployment.

## First playable slice

Use **stock Counter-Strike 1.6**, with no custom AMX/Metamod game mode or
competitive plugin rules. In practical terms, a player connects, picks
Counter-Terrorists or Terrorists, spawns on a supported stock CS map, and plays
the objective that map's stock entities define (bomb defusal or hostage
rescue). Preserve the stock CS 1.6 rules and defaults; do not choose a 5v5
tournament format, custom round limit, buy rules, or a community mod without
confirmation.

The owner selected two user-supplied local maps for the initial validation
scope: `de_dust2_2x2` and `fy_pool_day` (“options 1 and 2”, 2026-11-07). Both
exercise the bomb-defusal objective path. The local map inventory contains nine
BSPs: `35hp_2`, `aim_headshot`,
`aim_map`, `awp_india`, `cs_deathmatch-final`, `cs_deathmatch_2005c`,
`de_dust2_2x2`, `fy_pool_day`, and `fy_snow`. A bounded BSP v30 entity-lump
inspection found these objective candidates:

- `de_dust2_2x2`: one `func_bomb_target`, 20 `info_player_start`, and 20
  `info_player_deathmatch` entities.
- `fy_pool_day`: one `func_bomb_target`, one `func_buyzone`, and both spawn
  entity types.
- `cs_deathmatch-final`: two `hostage_entity` and two `info_hostage_rescue`
  entities, plus both spawn entity types.

Entity presence alone does not prove a map loads or plays correctly.
`de_dust2_2x2` and `fy_pool_day` are both listed in the source project's
`files/mapcycle.txt`; the owner selected these two maps without authorizing BSP
copying, packaging, or redistribution. An isolated HLDS check later confirmed
both selected map names in sanitized A2S_INFO responses after startup. This
proves map-load/query behavior only, not objective completion or a playable
round. `cs_deathmatch-final` remains an entity/parser candidate only. Do not
silently substitute the reference HLDS fixture map as production content; all
BSPs remain in the companion checkout.

An opt-in test using the Rust `load_world` parser was run against all nine local
BSPs. It returned successfully for eight; `35hp_2` failed strict geometry
validation. The three candidates above parsed, exposed non-empty collision
geometry, and matched the listed objective/spawn entity counts. The selected
two maps also loaded in isolated HLDS, but neither parser nor query evidence
proves the bomb objective, round transitions, or player gameplay. Re-run the
Rust parser check without copying assets with:

```sh
GOLDSRC_COMPANION_MAPS_DIR=/path/to/local/maps \
  cargo test --manifest-path poc/steam-server-query/Cargo.toml \
  --test local_map_inventory -- --ignored --nocapture
```

`poc/steam-server-query/src/spawns.rs` extracts the raw finite origins of
`info_player_start` and `info_player_deathmatch` from parsed BSP entities. The
optional scalar `angle` key is preserved without applying a default or view
policy. At startup, `GOLDSRC_MAP_BSP` now validates and reports spawn metadata
alongside the parsed map/geometry, while keeping simulation and sign-on disabled.
The selected-map inventory test validates those records against local maps, and
synthetic tests reject missing, duplicate, non-finite and malformed origins.
The local map inventory also tests each candidate spawn's raw BSP origin with the
measured standing-player hull against the world collision BSP; all origins in
the three parsed spawn-map candidates are outside solid. This is only a static
clearance check at the raw map origin. It does not establish the game DLL's
runtime spawn adjustment, assign teams, choose a spawn, or instantiate a player.
`poc/steam-server-query/src/objectives.rs` also resolves each selected-map
`func_bomb_target` model reference to a validated non-world BSP submodel and
its raw bounds. This validates static entity-to-geometry linkage only; it does
not implement trigger occupancy, bomb planting/defusal, explosions, or round
transitions.

## Suggested native replacement phases

The tracked plugin list currently identifies no enabled third-party plugin.
If deployment-specific plugins are later found, collect their names, config,
commands, hooks/events, data stores, and expected behavior before implementing
replacements.

| Phase | Native capability | Why / acceptance boundary |
| --- | --- | --- |
| Playable foundation | Stock teams, spawn selection, map entity/objective handling, round transitions, weapons/damage, chat, and map rotation/configuration | Required to enter and play a real stock CS round. Preserve stock semantics against local HLDS/client evidence; do not add tournament variants. |
| Private-server operations | Authenticated admin actions (restart/map change/kick/ban), explicit roles, bounded command audit, and durable expiring bans | Replaces common server-control plugins. Admin identity, authorization method, ban scope/retention, and recovery policy need owner input before implementation. Never log tickets/passwords or expose Steam IDs in shared evidence. |
| Operations visibility | Health/readiness, player/session counts, network rejection categories, tick duration/overruns, and Prometheus metrics | Replaces status/monitoring plugins without putting player names, IPs, or Steam IDs in metric labels. |
| Persistent results | Round/player events, idempotent history storage, and optional rankings/statistics | Add only after authoritative round events and restart recovery are verified. Agree on identity, retention, reset, and privacy behavior first. |
| Optional community features | Vote-map, chat moderation, custom ranks/MVP, warmup/ready-up, tournament rules, anti-cheat integrations | Defer until named plugins and exact expected behavior are supplied; these change gameplay or operational policy. |

## Inputs needed before closing this proposal

- Official-client validation of objective completion and gameplay on both
  selected maps. HLDS map-load/A2S query checks passed, but no client joined and
  no playable round was observed. Maps remain local to the companion checkout
  and are not copied into this repository.
- A deployment-specific plugin inventory, if the running server enables plugins
  not present in the tracked `plugins.ini`; do not share secret-bearing config.
- Admin authentication/roles and ban policy before operating those controls.
- Ranking/statistics identity and retention policy before storing persistent
  personal results.

Until then, build the compatibility foundation against the official client and
local Valve HLDS without enabling sign-on or gameplay based on this proposal
alone.
