# Feather protocol — model

Who should read this: implementors. It answers three questions (after
[RFC 4101](https://www.rfc-editor.org/rfc/rfc4101.html)'s protocol model):
what problem the protocol solves, what the messages are, and which features
are important but not obvious. Byte-level detail is deliberately left to
[wire-format.md](wire-format.md) — read that after this.

## 1. The problem

Voice chat needs two very different transports at once:

- **Reliability-bound**: joins, leaves, state changes — late messages corrupt
  the room roster.
- **Timeliness-bound**: voice frames — late frames are worse than missing
  ones (a dropped 20 ms frame is masked by Opus PLC; a delayed frame forces
  everyone either to buffer or to break).

Unity in one channel is what makes WebRTC-style stacks top-heavy; Discord on
native apps and Mumble made the same split. Feather too splits into a TCP
**control** channel and a UDP **media** channel.

## 2. Broad overview

```
Client                                    Server
  |--- TCP HELLO (version, name, ssrc) --->|
  |<-- TCP HELLO_ACK ---------------------|
  |--- TCP JOIN room ---------------------->|
  |<-- TCP ACK ---------------------------|
  |--- UDP PING(...) ---------------------->|   connectivity check, NAT open
  |<-- UDP PING-echo ----------------------|
  |=== stream AUDIO over UDP ============>|   server broadcasts to room,
  |<== AUDIO from peers ====================|  sender excluded
  |--- TCP LEAVE (on exit) --------------->|   roster updated on TCP
```

The server behaves as a mini-SFU: it never decodes Opus, it forwards
datagrams verbatim via SSRC→room lookup. All session intelligence (jitter,
PLC, queueing) stays at endpoints — matching
[server-design.md](server-design.md).

## 3. Important but unobvious features

- **SSRC on every packet** keeps server forwarding SSRC-agnostic;
  collisions are resolved at JOIN by the server.
- **Terminator bit (T)** marks end-of-talkspurt; it is the simplified
  equivalent of RTP's marker bit and Mumble's terminator flag in one bit.
- **No DTX**: only seq exists (no timestamp), so gaps must always mean
  loss.
- **PING echo** with a `uint64` payload echoed verbatim — connectivity
  check, NAT-map opener and RTT probe in one packet type.
- **Version interop**: unknown kind values are dropped, unknown TCP message
  types tolerated — the wire format evolves without breaking mixed
  deployments.

## 4. Encryption roadmap

Until the last roadmap item lands: plaintext. The handshake target is per
[security-model.md](security-model.md); the format is already shaped so the
header serves as associated data and the payload becomes `nonce‖ciphertext`
without structural change.
