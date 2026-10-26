# Rust GoldSrc-Compatible Server for ARM64

- Status: Accepted
- Date: 2026-10-26

## Context

The current server image runs the x86-64 GoldSrc/HLDS distribution and is
configured as `linux/amd64`. The project wants native ARM64 operation, built-in
metrics, ranking, administration, and game features without relying on AMX Mod
X or other server plugins. The compatibility target is the unmodified,
official Counter-Strike 1.6 Steam client only.

## Decision

Build a new dedicated server in Rust with GoldSrc wire-protocol compatibility
for official Steam CS 1.6 clients. Treat ARM64 as a first-class native target.
Implement the project's game mode, administration, metrics, ranking, and ban
features as owned server capabilities rather than third-party game plugins.

Do not include Valve's server binaries or game assets in the implementation.
Maps and other content must be supplied or redistributed only where their
respective rights permit it.

## Alternatives considered

- Run the existing HLDS x86-64 image under emulation on ARM64. This retains
  compatibility but keeps the legacy binary and its operational constraints.
- Port or adopt an existing GoldSrc-compatible engine such as ReHLDS or Xash3D.
  This may reach compatibility sooner, but does not meet the greenfield Rust
  engine objective and still needs validation of ARM64, Steam clients, and game
  modules.
- Define a new game protocol. This would simplify implementation but would
  require modified clients, so it does not meet the compatibility target.

## Consequences

- The wire protocol, Steam authentication, query behavior, game simulation,
  map/entity handling, and client-predicted movement become project-owned
  compatibility responsibilities.
- Non-Steam clients, legacy protocol variants, and binary AMX/Metamod plugins
  are explicitly out of scope. Desired plugin features must be implemented as
  native capabilities.
- Query and handshake compatibility must be proven with unmodified official
  Steam clients before investing in full gameplay implementation.
- ARM64 builds must be native and must not depend on executing x86 binaries.
- A small server query prototype is only a protocol milestone; it is not proof
  of client connection, Steam authentication, or playable gameplay.

## Revisit when

Revisit this decision if official-client compatibility proves infeasible,
Steam server authentication or master-server registration is unavailable to an
independent implementation, or the effort required for compatible movement and
gameplay exceeds the project's acceptable scope.
