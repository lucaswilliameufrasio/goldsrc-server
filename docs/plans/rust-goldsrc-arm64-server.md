# Rust GoldSrc-Compatible Server: Technical Plan

## Goal

Create a native ARM64 dedicated server that accepts unmodified official Steam
Counter-Strike 1.6 clients. Replace the project's required AMX Mod X/plugin
features with native server capabilities.

## Explicit non-goals

- Supporting non-Steam clients, legacy protocol variants, or arbitrary AMX /
  Metamod binary plugins.
- Shipping Valve's HLDS binaries, game DLLs, or game assets.
- Claiming complete CS 1.6 compatibility based only on query-protocol support.

## Proposed workspace boundaries

- `goldsrc-protocol`: bounded packet parsing/encoding, connectionless messages,
  challenges, and client/server protocol versions.
- `goldsrc-transport`: UDP sockets, session lifecycle, packet limits, and
  protocol-level rate controls.
- `goldsrc-world`: BSP parsing, resources, entity state, collision, and map
  lifecycle. Load user-provided maps; keep licensing separate from code.
- `cs16-simulation`: fixed-tick simulation, player movement, weapons, rounds,
  and game rules required by the chosen CS 1.6 mode.
- `cs16-events`: typed authoritative events such as player connection, kill,
  round result, ban, and map change.
- `cs16-operations`: authenticated administration, health, Prometheus metrics,
  ranking, bans, and persistence, consuming authoritative events without a
  game plugin interface.
- `cs16-server`: process entrypoint, configuration, graceful shutdown, and
  composition of the modules above.

Keep the simulation loop deterministic and isolated from slow I/O. Metrics
should be updated in-process; durable rankings and history should be written
through an idempotent persistence boundary so restarts do not lose results.

## Delivery slices and acceptance evidence

1. **Compatibility inventory** — pin the official Steam client build(s),
   protocol version, server-browser expectations, Steam authentication, and the
   required game modes. Capture test fixtures from a legally acquired local
   client and a local reference server.
2. **Query PoC** — answer the current Steam server-query handshake and info
   request on loopback. Check the response with a query client and, where
   available, the unmodified Steam server browser. This milestone does not
   accept game connections.
3. **Connection vertical slice** — implement connectionless challenge,
   Steam authentication, sign-on, disconnect, and a single client join using
   the unmodified Steam client. Test only against local clients and fixtures.
4. **Playable slice** — load one user-supplied, licensed map; spawn and move
   players; implement the selected mode's round/weapon loop; verify client
   prediction and server authority with stock clients.
5. **Native operations** — implement admin authentication and actions, health
   and Prometheus metrics, event-driven ranking, bans, persistence, and
   recovery without AMX/Metamod.
6. **Production readiness** — native `aarch64-unknown-linux-gnu` and
   `x86_64-unknown-linux-gnu` builds, repeatable protocol/gameplay regression
   tests, resource limits, abuse controls, graceful shutdown, and measured
   load tests on ARM64 hardware.

## First PoC in this repository

`poc/steam-server-query` is intentionally a loopback-only A2S_INFO query
prototype. It validates a small piece of Steam server-browser interoperability
without pretending to implement the GoldSrc game connection protocol. Its
challenge value is fixed for deterministic local testing and is not suitable
for exposure to a network.

## Open validation items

- Confirm the precise official Steam client build/protocol baseline.
- Verify whether Steam game-server authentication and master-server listing
  can be implemented and tested by this project, and under what SDK terms.
- Obtain protocol fixtures from an unmodified official client and a local
  reference HLDS server.
- Decide the first gameplay mode and the exact set of native features replacing
  currently used plugins.
- Confirm permissions for every map and asset before adding it to a distributable
  image or FastDL bundle.

## References

- [Valve Developer Community: Server queries](https://developer.valvesoftware.com/wiki/Server_queries)
- [Valve Developer Community: GoldSrc engine versions](https://developer.valvesoftware.com/wiki/GoldSrc/Engine_versions)
- [ReHLDS](https://github.com/rehlds/rehlds), an alternative compatibility
  implementation considered during the decision.
