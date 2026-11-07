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

Keep the first playable target to one user-supplied local map and one objective
path. The local map inventory contains nine BSPs: `35hp_2`, `aim_headshot`,
`aim_map`, `awp_india`, `cs_deathmatch-final`, `cs_deathmatch_2005c`,
`de_dust2_2x2`, `fy_pool_day`, and `fy_snow`. A bounded BSP v30 entity-lump
inspection found these objective candidates:

- `de_dust2_2x2`: one `func_bomb_target`, 20 `info_player_start`, and 20
  `info_player_deathmatch` entities.
- `fy_pool_day`: one `func_bomb_target`, one `func_buyzone`, and both spawn
  entity types.
- `cs_deathmatch-final`: two `hostage_entity` and two `info_hostage_rescue`
  entities, plus both spawn entity types.

These are entity-presence observations only; they do not prove a map loads or
plays correctly. `de_dust2_2x2` is the strongest bomb-defusal candidate from this
inventory and is also listed in the source project's `files/mapcycle.txt`, but
no map/objective has been selected yet. Do not silently substitute the reference
HLDS fixture map as production content. The BSPs remain in the companion
checkout; do not copy, package, or redistribute them.

An opt-in test using the Rust `load_world` parser was run against all nine local
BSPs. It returned successfully for eight; `35hp_2` failed strict geometry
validation. The three candidates above parsed, exposed non-empty collision
geometry, and matched the listed objective/spawn entity counts. This is parser
evidence only, not a successful HLDS load or playable round. Re-run without
copying the assets with:

```sh
GOLDSRC_COMPANION_MAPS_DIR=/path/to/local/maps \
  cargo test --manifest-path poc/steam-server-query/Cargo.toml \
  --test local_map_inventory -- --ignored --nocapture
```

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

- Owner confirmation of the first map/objective candidate; maps remain local to
  the companion checkout and are not copied into this repository.
- A deployment-specific plugin inventory, if the running server enables plugins
  not present in the tracked `plugins.ini`; do not share secret-bearing config.
- Admin authentication/roles and ban policy before operating those controls.
- Ranking/statistics identity and retention policy before storing persistent
  personal results.

Until then, build the compatibility foundation against the official client and
local Valve HLDS without enabling sign-on or gameplay based on this proposal
alone.
