# Feather wire format (v2)

Status: **Normative reference**. Defines the byte-level packet layout of the
Feather protocol. For session lifecycle and rationale see
[protocol.md](protocol.md). Encryption of both channels is defined in
[transport-security.md](transport-security.md). v2 does not interoperate with
v1, and there is no plaintext fallback.

## 1. Conventions

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" are to
be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html).

- All multi-byte fields are big-endian (network byte order).
- "Packet type" and "kind" refer to the value in the kind subfield of the
  flag/kind byte.
- "SSRC" is a 32-bit synchronization source identifier, randomly chosen by
  the client once per session (mirrors RFC 3550 terminology).
- Message kinds not covered in this document MUST be silently discarded;
  this is what allows version cross-deals.

## 2. Transport channels

Two channels (the rationale is spread out in [protocol.md](protocol.md)):

- **TCP control** (server's single listen port): HELLO, JOIN, LEAVE, ACK,
  roster. Runs inside TLS 1.3, framed per §7.
- **UDP media**: AUDIO, VIDEO and SYSAUDIO, forwarded per room by the server,
  plus PING and PATH between client and server. Every datagram is sealed per hop (§3). Sealed datagrams MUST NOT
  exceed 1024 bytes, except VIDEO (kind 2) which MAY use up to 1200 bytes.

## 3. UDP media packet

Byte-aligned, so the diagram follows RFC 2360's byte-wide form.

```
 0     1                 5                              13
+----+------------------+------------------------------+----------------------+
| FL |   SSRC (u32)     |        counter (u64)         | ciphertext ‖ tag(16) |
+----+------------------+------------------------------+----------------------+
```

The 13-byte header is present on every packet and is the AEAD associated data.
`ciphertext` is the kind-specific payload of §4–§5, encrypted with AES-256-GCM.
Keys, nonce, counter and the replay window are defined in
[transport-security.md](transport-security.md) §2. The server opens each
datagram and seals it again for every recipient, with that recipient's key and
counter.

The diagrams in §4–§5 show the `FL | SSRC` prefix followed by the payload before
sealing. On the wire the counter sits between SSRC and the encrypted payload.

The flag/kind byte (`FL`):

```
 7  6  5  4  3  2  1  0
+-----+---+-----+
|  V  | T |  K  |
+-----+---+-----+
V    2 bits  protocol version (1 in v2; 0 was the unsealed v1)
T    1 bit   terminator, only defined for kinds 0 (AUDIO) and 3 (SYSAUDIO), MUST be 0 on other kinds
K    5 bits  kind: 0 = AUDIO, 1 = PING, 2 = VIDEO, 3 = SYSAUDIO, 4 = PATH.
             5..31 reserved; receivers MUST drop unknown kinds without
             reporting.
```

SSRC: random per session (MUST NOT be 0x00000000), random per session, and
kept constant while the session is held. Collisions are resolved at HELLO by
the server asking the newcomer to re-pick.

## 4. AUDIO packet (kind 0)

```
   0    1    2    3    4            5         6     ...
+---+-------+--------+            +----------------------+
| FL|  SSRC | seq(u32)|           |  one Opus frame      |
+---+-------+--------+            +----------------------+
```

Fields:

- `seq` — `uint32`. MUST be monotonically increasing within a session,
  rolling over with a receiver window comparison of ±2³¹. Receivers treat
  greater-than-half-window values as late packets and drop them.
- `frame` — exactly one Opus packet as defined by RFC 6716 §3, not an Opus
  self-delimiting packet. Fixed frame size 20 ms (960 samples, 48 kHz, mono),
  `OPUS_APPLICATION_VOIP`. Max frame length 1275 bytes, but this format
  caps the whole sealed datagram at 1024 bytes by §2, so engines SHOULD target
  ~160 bytes (64 kb/s).

Semantics:

- **Continuous transmission.** The sender MUST NOT suspend packets during
  silence; Opus encodes silence at reduced bitrate without dropping frames.
  Receivers therefore see every sequence gap as genuine loss.
- **Terminator.** The T flag marks the last AUDIO packet of a talkspurt.
  Receivers MUST treat the next packet on the same SSRC with T set as the
  stream boundary: speaking-indicator ends and interrupt PLC state resets.
  This is the equivalent of RTP's marker bit (RFC 3551 §4.1) and Mumble's
  terminator bit in a single bit.

## 4b. Screen-share packets (kinds 2 and 3)

A session that shares its screen sends VIDEO (and optionally SYSAUDIO)
under its own SSRC. The server forwards these kinds **only to the sessions
that subscribed with `SCREEN_WATCH`** (§7.2), never to every room-mate.
A packet from a session that is not sharing MUST be dropped by the server.

**SYSAUDIO (kind 3)**: payload identical to AUDIO (§4).

**VIDEO (kind 2)**: one fragment of one H.264 access unit (Annex B).
The server MUST NOT parse the payload.

```
   0       4        8     9       11      13
+--------+--------+-----+-------+-------+----------------+
| frame  |  ts    |flags| idx   | count |  H.264 bytes   |
| u32    | u32 ms | u8  | u16   | u16   |                |
+--------+--------+-----+-------+-------+----------------+
```

- `frame`: increments by 1 per access unit (wraps).
- `ts`: capture time in milliseconds, sender clock, wraps.
- `flags`: bit 0 = keyframe (IDR); other bits MUST be 0.
- `idx`/`count`: fragment index (from 0) and total fragments of the frame.
- Each fragment carries at most 1200 − 13 − 16 − 13 = 1158 bytes of H.264
  (sealed header, tag, fragment header).

A receiver MUST drop a frame whose fragments are not all received before a
newer frame completes, and then request a keyframe (`SCREEN_KEYREQ`, §7.2).

## 5. PING packet (kind 1)

```
   0    1    2    3    4            5 .. 12
+---+-------+--------+            +------------------------+
| FL|  SSRC | (ignored on PING)  |  timestamp (u64)        |
+---+-------+--------+            +------------------------+
```

The client fills `timestamp` with an arbitrary `uint64`, and the server
answers on the same channel echoing it (kind 1, same `FL`, SSRC and payload,
sealed with the downlink key). The server echoes only a PING from the session's
validated address (§5b); from any other address a PING draws a PATH challenge
instead. This serves as NAT-mapping opener, UDP connectivity check, and RTT
probe at once. No AUDIO is handed over UDP until the first echo arrives
(Mumble's UDP connectivity check in spirit).

## 5b. PATH packet (kind 4)

```
   0    1    2    3    4              5 .. 12
+---+-------+--------+            +------------------------+
| FL|  SSRC |                     |  challenge (8 bytes)   |
+---+-------+--------+            +------------------------+
```

The server sends 8 random bytes to a new source address of a session; the client
sends the same bytes back from that address. Only then does the server send media
there. `SSRC` is the session's own in both directions, T is 0, and a payload of
any other length is dropped. Rules (trigger, rate limit, retries, expiry):
[transport-security.md](transport-security.md) §2.4.

## 6. Receiver processing rules

For every received UDP packet the server and the client MUST in order:

1. Reject total length < 30 (header, one payload byte, tag).
2. Reject protocol version bits != 1.
3. Reject kind unknown (drop silently).
4. Server: reject an SSRC that maps to no session.
5. Reject a datagram whose tag does not verify.
6. Apply the replay check ([transport-security.md](transport-security.md) §2.3).
7. For AUDIO: the server forwards the payload, resealed, to room peers. For
   VIDEO/SYSAUDIO: to the sender's watchers only (§4b).
8. For PING: the server echoes the timestamp back to the source, if it is the
   validated address (§5b).
9. For PATH: the server validates the source if the challenge matches; the client
   echoes it. It is never forwarded.

Rejected datagrams get no reply. These checks are normative because UDP under
NAT is inherently permissive about garbage input.

## 7. TCP control framing

The control stream is TLS 1.3 on the server's TCP port, and the framing below
runs inside it (certificate and pinning:
[transport-security.md](transport-security.md) §1). Before a successful LOGIN
the server accepts frames of at most 4096 bytes and drops a connection that has not logged in within
60 s (§1.3 there).

```
      0      1      2      3
+----+-------+-------+--------+
|    len (u32, big-endian)    |    then len bytes of message payload (JSON)
+-----------------------------+
```

`len` is the count of the following payload bytes, and `len` MUST be < 1 MiB.
Message schema lives in code (`server/`), canonical example messages are
`HELLO`, `JOIN`, `LEAVE`, `ACK`, `ROSTER`. Unrecognized message types MUST
be tolerated; the connection stays open (version interop rule).

### 7.1 Voice channel messages

All JSON, framed as above. A client MUST `LOGIN`, then `HELLO` (random non-zero
`ssrc`; the ACK omits `ssrc` if it is taken, so pick another), before `JOIN`.
The ACK to `LOGIN` carries `deviceToken`, which the client may send back in
later `LOGIN` requests
([transport-security.md](transport-security.md) §1.4).
The successful ACK to `HELLO` also carries `mediaKeyUp` and `mediaKeyDown`
(standard base64, 32 bytes each). HELLO succeeds once per connection.

| Message | Direction | Fields |
|---|---|---|
| `JOIN` | C→S | `channelId` (voice channel the user may access) |
| `ACK` to JOIN | S→C | `room`, `channelId`, `serverId`, `members` (everyone already there) |
| `STATE` | C→S | `muted`, `deafened` (no ACK) |
| `LEAVE` | C→S | none (no ACK; closing TCP does the same) |
| `MEMBER_JOINED` / `MEMBER_STATE` | S→C push | `channelId`, `member` = `{ssrc,name,muted,deafened,sharing}` |
| `MEMBER_LEFT` | S→C push | `channelId`, `ssrc` |

`LIST_CHANNELS` also returns `members` per channel. Pushes never use the
`ACK`/`ERROR` types, so they can't be mistaken for a reply to a request.

### 7.2 Screen-share messages

A session MUST be in a room (`JOIN`ed) to share or watch. A room holds at
most 4 simultaneous sharers.

| Message | Direction | Fields |
|---|---|---|
| `SCREEN_START` | C→S | none. ACK on success, `ERROR` ("cannot share") when not in a room or the room is full |
| `SCREEN_STOP` | C→S | none (no ACK). Leaving the room or closing TCP does the same |
| `SCREEN_WATCH` | C→S | `ssrc` of a sharing room-mate. ACK, or `ERROR` ("not sharing") |
| `SCREEN_UNWATCH` | C→S | `ssrc` (no ACK) |
| `SCREEN_KEYREQ` | C→S | `ssrc` of the presenter: viewer asks for a keyframe |
| `SCREEN_KEYREQ` | S→C push | none: the receiving presenter MUST send a keyframe soon. Sent automatically on every `SCREEN_WATCH` |

Sharing starts/stops are announced to the server's members as `MEMBER_STATE`
whose `member` carries `sharing: true` (omitted when false). A 1:1 DM call
has no server roster: there the other side gets `DM_CALL_STATE` with the
sharer's `ssrc` and `sharing`, and watches it with the same `SCREEN_WATCH`
flow.

A server MUST throttle viewer-sent `SCREEN_KEYREQ` to at most one per 500 ms
for each `(viewer, presenter SSRC)` pair. Requests outside that gap MAY be
dropped without a reply; the automatic key request sent on `SCREEN_WATCH` does
not consume that budget. Clients use the same 500 ms gap when asking, and
pace VIDEO fragments instead of sending a keyframe as one burst.

## 8. Worked examples

These show packets before sealing (`V` = 0 in `FL`, no counter). On the wire
`V` = 1, the counter follows SSRC and the payload is encrypted. A sealed test
vector is in [transport-security.md](transport-security.md) §3.

**AUDIO, seq = 7, single Opus frame on the wire:**

```
0x00 0x11 0x22 0x33 0x44 // header: kind=0, V=0, T=0, SSRC=0x11223344
0x00 0x00 0x00 0x07       // seq = 7
...160 bytes Opus...
```

**PATH, challenge 0xA1A2A3A4A5A6A7A8:**

```
0x04 0x11 0x22 0x33 0x44 // header (kind=4)
0xA1 0xA2 0xA3 0xA4 0xA5 0xA6 0xA7 0xA8 // challenge
```

**PING, timestamp 0x1122334455667788:**

```
0x01 0x11 0x22 0x33 0x44 // header (kind=1)
0x11 0x22 0x33 0x44 0x55 0x66 0x77 0x88 // timestamp
```

## 9. Conformance summary

| Feature | Section | Status |
|---|---|---|
| Big-endian fields | §1 | M |
| Unknown kinds/V dropped | §3 | M |
| Datagrams sealed, replays rejected | §3, §6 | M |
| TCP inside TLS 1.3 | §7 | M |
| SSRC != 0 | §3 | M |
| Continuous transmission (no DTX) | §4 | M |
| T flag set on final talkspurt packet | §4 | M |
| Sealed length ≤ 1024 (VIDEO ≤ 1200) | §2 | M |
| PING-echo before AUDIO | §5 | S |
| Media only to an address that echoed PATH | §5b | M |
| seq increases by ≥1 per packet | §4 | M |
| Tolerates unknown TCP message types | §7 | M |

Status codes: M = MUST, S = SHOULD, O = MAY, X = prohibited by this format.

## 10. Security considerations

Both channels are encrypted per hop: TLS 1.3 with a pinned server certificate
on TCP, AES-256-GCM on UDP ([transport-security.md](transport-security.md)).
This is **not** end-to-end encryption: the server can read every control message
and every media payload. E2EE is pending. The UDP header (`FL`, SSRC, counter),
datagram sizes and timing stay visible to an on-path observer. See
[security-model.md](security-model.md).

## 11. Revision history

- **v2** (transport security): TLS 1.3 control channel, sealed datagrams
  (`V` = 1, `u64` counter, 16-byte tag), media keys in the `HELLO` ACK, `LOGIN`
  required before `HELLO`, at most 1158 bytes of H.264 per VIDEO fragment.
  Breaks compatibility with v1. Kind 4 (PATH): the server validates a UDP
  address before sending media to it.
- **v1 + screen share** (2026-11-20): kinds 2 (VIDEO) and 3 (SYSAUDIO),
  VIDEO datagrams up to 1200 bytes, watcher-only forwarding, `SCREEN_*`
  control messages (§7.2), `sharing` on members, 500 ms `SCREEN_KEYREQ` limit.
- **v0 → v1** (2026-09-17): JOIN/LEAVE moved from UDP to TCP; terminator
  flag added; PING now carries an echoed timestamp; explicit max datagram
  1024 bytes; continuous transmission documented; compliance summary added.
