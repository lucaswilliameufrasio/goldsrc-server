# Development connection challenge

The official-client trace collected on 2026-10-29 contains four unanswered
`getchallenge steam\n` requests. The next incremental PoC responds to that
request, not to legacy/non-Steam authentication variants.

Wire reference: ReHLDS `SVC_GetChallenge` in
[`sv_main.cpp`](https://github.com/rehlds/ReHLDS/blob/master/rehlds/engine/sv_main.cpp):
the Steam reply is connectionless, has a text `A00000000` prefix, decimal
challenge, auth protocol `3`, server Steam ID and secure flag, newline and NUL.
This is protocol research, not source code incorporated from that project.
It is not yet validated against a healthy Valve HLDS capture.

The initial private development probe used a fixed challenge distinct from
the A2S challenge and advertised Steam ID `0`, secure flag `0`. These are explicit
unavailable-service placeholders, not a registered Steam server identity or a
claim of Steam authentication. Do not fall back to legacy auth. Reject any
subsequent `connect` with an explicit not-implemented reason; never allocate a
player or claim success. The purpose is to observe whether the official client
advances past getchallenge. Some clients may refuse the placeholder identity;
that requires further Steam SDK investigation, not an auth bypass.

Since this step may elicit Steam tickets, opt-in traces now retain only packet
length and a redaction marker for inbound packets other than the supported
query/getchallenge messages. They must never log raw connect payloads. This
deliberately trades raw-handshake debugging detail for safer local fixtures.
Existing traces from older builds are not retroactively sanitized.

The fixed game challenge is now superseded by
[peer-bound expiring challenges and connect validation](connect-validation.md).
The placeholder identity and fail-closed rejection remain until supported Steam
authentication is implemented.
Public deployment, sign-on, and gameplay remain out of scope for this probe.

## Official-client validation (2026-10-29)

The user ran the updated PoC and attempted to connect with the official client.
The resulting local trace `fixtures/local/challenge-20261029-201130-199058.jsonl`
was inspected without exposing payloads that could contain credentials:

- One incoming 23-byte `getchallenge steam` request.
- One outgoing 32-byte text game challenge reply.
- One incoming 452-byte `connect`, with its payload redacted in the trace.
- One outgoing 77-byte development connection rejection.
- A2S_INFO also completed with a 75-byte response in the same session.

This confirms that this client advances past the challenge, even with the
development placeholder identity. It does not confirm the contents of connect,
the client's exact build, Steam authentication, sign-on, or successful admission.
