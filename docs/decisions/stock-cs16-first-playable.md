# Stock CS 1.6 as the first playable mode

- Status: Accepted
- Date: 2026-11-07

## Context

The project needs a bounded first playable target but has no confirmed AMX /
Metamod plugin inventory or server-mode configuration. The owner described the
goal as joining the server and playing CS, without asking for a custom or
competitive mode.

## Decision

The first playable slice targets stock Counter-Strike 1.6 behavior with no
custom plugin-driven mode or tournament rules. The map's stock entities define
the objective path (for example, bomb defusal or hostage rescue). Implement and
validate one locally supplied map/objective path first; the map itself remains
unselected until the other project's map directory is located.

Use local Valve HLDS and an unmodified Steam CS 1.6 client as behavioral
references. Do not invent round/buy settings or custom rules to fill evidence
gaps. Keep all maps/assets local; this decision does not authorize packaging or
redistribution.

## Alternatives considered

- Select a competitive 5v5 preset: rejected because the owner did not request a
  tournament format or its specific settings.
- Select a custom community mode such as deathmatch, zombie, jailbreak, or
  gun-game: rejected because no such preference or plugin inventory was
  supplied.
- Implement every stock map/objective before the first playable test: deferred
  because it expands the first compatibility slice without improving the
  initial join-and-play proof.

## Consequences

- The first proof must cover stock team selection, spawn, the selected map's
  objective entities, round transitions, player movement, and the relevant
  weapons/damage behavior.
- Exact stock defaults must be measured against the chosen reference HLDS; this
  decision does not define unverified values.
- Native administration, bans, metrics, ranking, and optional community
  features remain separate capabilities. See the
  [native-capability proposal](../plans/native-capabilities-proposal.md); the
  proposal is not evidence that any named plugin is currently in use.

## Revisit when

Revisit if the owner supplies a plugin/config inventory showing that the
intended server is expected to run a custom mode or competitive ruleset.
