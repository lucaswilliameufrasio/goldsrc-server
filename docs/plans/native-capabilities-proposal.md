# Proposed first gameplay scope and native capabilities

**Status:** the stock-CS first playable mode is selected in the
[accepted decision](../decisions/stock-cs16-first-playable.md). The capability
replacement phases below remain suggestions, not an accepted feature inventory.
No AMX/Metamod plugin inventory was found in the inspected local project paths.

## First playable slice

Use **stock Counter-Strike 1.6**, with no custom AMX/Metamod game mode or
competitive plugin rules. In practical terms, a player connects, picks
Counter-Terrorists or Terrorists, spawns on a supported stock CS map, and plays
the objective that map's stock entities define (bomb defusal or hostage
rescue). Preserve the stock CS 1.6 rules and defaults; do not choose a 5v5
tournament format, custom round limit, buy rules, or a community mod without
confirmation.

Keep the first playable target to one user-supplied local map and one objective
path. The map name and its source path are not yet selected: the separate
project's map directory was not found. Do not silently substitute the reference
HLDS fixture map as production content. Do not package or redistribute map
assets; the owner's choice is local-only use.

## Suggested native replacement phases

These are candidate capabilities, not a claim that particular plugins are in
use. First collect each plugin's name, config, commands, hooks/events, data
stores, and expected behavior; implement only entries confirmed by that
inventory.

| Phase | Native capability | Why / acceptance boundary |
| --- | --- | --- |
| Playable foundation | Stock teams, spawn selection, map entity/objective handling, round transitions, weapons/damage, chat, and map rotation/configuration | Required to enter and play a real stock CS round. Preserve stock semantics against local HLDS/client evidence; do not add tournament variants. |
| Private-server operations | Authenticated admin actions (restart/map change/kick/ban), explicit roles, bounded command audit, and durable expiring bans | Replaces common server-control plugins. Admin identity, authorization method, ban scope/retention, and recovery policy need owner input before implementation. Never log tickets/passwords or expose Steam IDs in shared evidence. |
| Operations visibility | Health/readiness, player/session counts, network rejection categories, tick duration/overruns, and Prometheus metrics | Replaces status/monitoring plugins without putting player names, IPs, or Steam IDs in metric labels. |
| Persistent results | Round/player events, idempotent history storage, and optional rankings/statistics | Add only after authoritative round events and restart recovery are verified. Agree on identity, retention, reset, and privacy behavior first. |
| Optional community features | Vote-map, chat moderation, custom ranks/MVP, warmup/ready-up, tournament rules, anti-cheat integrations | Defer until named plugins and exact expected behavior are supplied; these change gameplay or operational policy. |

## Inputs needed before closing this proposal

- The other project's exact map directory; keep it local and ignored.
- A plugin inventory with configurations and examples of the behavior to retain.
- Admin authentication/roles and ban policy before operating those controls.
- Ranking/statistics identity and retention policy before storing persistent
  personal results.

Until then, build the compatibility foundation against the official client and
local Valve HLDS without enabling sign-on or gameplay based on this proposal
alone.
