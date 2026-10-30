# Serverinfo preparation

The official client has confirmed initial Steam approval, connection acceptance,
`new`, and acknowledgment of the deliberate disconnect. The next wire message
for actual sign-on is `svc_serverinfo`.

`poc/steam-server-query/src/serverinfo.rs` contains a bounded, unit-tested
encoder for the `svc_serverinfo` fields, based on the field order in
[ReHLDS SV_SendServerinfo](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp).
The encoder is compiled only in tests: it is **not sent to clients**. Its API
requires callers to supply the already-encoded world CRC, real client-DLL MD5,
map model name, and session-specific values. It does not invent those fields,
compute the CRC/MD5, load a map, implement delta schemas, or transfer resources.
The fixed string bounds are conservative development limits, not established
GoldSrc maxima.

Before wiring this into live sign-on, acquire and inspect a reference HLDS
sign-on exchange or validate against an independent protocol fixture; calculate
map CRC and client-DLL MD5 from user-supplied licensed files; implement delta
descriptions, movevars, extra info, resource lists and reliable fragmentation.
Keep authentication fail-closed and current explicit disconnect until those
prerequisites are met. Synthetic serialization tests alone do not establish
official-client compatibility.
