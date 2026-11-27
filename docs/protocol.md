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
  |=== TLS 1.3 handshake (pinned cert) ====|   all TCP below runs inside it
  |--- TCP LOGIN -------------------------->|
  |<-- TCP ACK ---------------------------|
  |--- TCP HELLO (name, ssrc) ------------->|
  |<-- TCP ACK (ssrc, media keys) --------|
  |--- TCP JOIN room ---------------------->|
  |<-- TCP ACK ---------------------------|
  |--- UDP PING(...) ---------------------->|   connectivity check, NAT open
  |<-- UDP PATH challenge ------------------|   new source address only
  |--- UDP PATH echo ---------------------->|   server adopts the address
  |--- UDP PING(...) ---------------------->|
  |<-- UDP PING-echo -----------------------|
  |=== stream AUDIO over UDP ============>|   server broadcasts to room,
  |<== AUDIO from peers ====================|  sender excluded
  |--- TCP SCREEN_START / SCREEN_WATCH --->|   share control (§7.2)
  |=== stream VIDEO fragments over UDP ===>|  forwarded to watchers only
  |--- TCP LEAVE (on exit) --------------->|   roster updated on TCP
```

The server behaves as a mini-SFU: it never decodes Opus. It opens each
datagram, finds the room by SSRC, and seals the unchanged payload again for each
recipient. All session intelligence (jitter,
PLC, queueing) stays at endpoints — matching
[server-design.md](server-design.md).

## 3. Important but unobvious features

- **SSRC on every packet** keeps server forwarding SSRC-agnostic;
  collisions are resolved at HELLO by the server.
- **Terminator bit (T)** marks end-of-talkspurt; it is the simplified
  equivalent of RTP's marker bit and Mumble's terminator flag in one bit.
- **No DTX**: only seq exists (no timestamp), so gaps must always mean
  loss.
- **PING echo** with a `uint64` payload echoed unchanged — connectivity
  check, NAT-map opener and RTT probe in one packet type.
- **Version interop**: unknown kind values are dropped, unknown TCP message
  types tolerated — the wire format evolves without breaking mixed
  deployments.
- **Screen share is watcher-only**: `VIDEO` datagrams reach only sessions that
  sent `SCREEN_WATCH`, and the `sharing` roster flag drives the UI. The
  control messages and fragment layout are in [wire-format.md](wire-format.md)
  (sections 4b and 7.2); key requests are limited to one per 500 ms per viewer
  and presenter.

## 4. Encryption

Both channels are encrypted between client and server, with no plaintext
fallback: TLS 1.3 with a pinned server certificate on TCP, and per-session
AES-256-GCM keys on UDP, handed out in the `HELLO` ACK. The rules are in
[transport-security.md](transport-security.md). v2 does not interoperate with v1.

This is per hop. Text messages are also end-to-end encrypted
([e2ee-text.md](e2ee-text.md)): `PRELOGIN`, then `LOGIN` with a derived key (and
`SET_KEYS` once); `GET_CONVERSATION_KEYS`, `CREATE_EPOCH`, `SHARE_KEYS`,
`REJECT_WRAP` and `GET_USER_KEYS` manage conversation keys; send and update
requests carry `e2ee`.
Voice, screen share and calls are not end-to-end encrypted; see
[security-model.md](security-model.md).
