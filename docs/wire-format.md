# Feather wire format (v1)

Status: **Normative reference**. Defines the byte-level packet layout of the
Feather protocol. For session lifecycle and rationale see
[protocol.md](protocol.md).

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
  roster, and eventually the X25519 handshake. Framed per §7.
- **UDP media**: AUDIO, PING, VIDEO and SYSAUDIO, forwarded per room by the
  server. Datagrams MUST NOT exceed 1024 bytes, except VIDEO (kind 2) which
  MAY use up to 1200 bytes.

## 3. UDP media packet

Byte-aligned, so the diagram follows RFC 2360's byte-wide form.

```
   0         1         2         3         4
+---+---+---+---+---+---+
| FL|     SSRC (u32)    |   <- 5-byte header, identical on every packet
+---+---+---+---+---+---+
| ... payload, kind-specific ...      |
```

The flag/kind byte (`FL`):

```
 7  6  5  4  3  2  1  0
+-----+---+-----+
|  V  | T |  K  |
+-----+---+-----+
V    2 bits  protocol version (0 in v1)
T    1 bit   terminator, only defined for kinds 0 (AUDIO) and 3 (SYSAUDIO), MUST be 0 on other kinds
K    5 bits  kind: 0 = AUDIO, 1 = PING, 2 = VIDEO, 3 = SYSAUDIO. 4..31 reserved;
             receivers MUST drop unknown kinds without reporting.
```

SSRC: random per session (MUST NOT be 0x00000000), random per session, and
kept constant while the session is held. Collisions are resolved at JOIN by
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
  caps the whole datagram at 1024 bytes by §2, so engines SHOULD target
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
- Each fragment carries at most 1200 − 5 − 13 = 1182 bytes of H.264.

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
answers on the same channel echoing it verbatim (kind 1 with the same 5-byte
header structure the server owers responses too). This serves as NAT-mapping
opener, UDP connectivity check, and RTT probe at once. No AUDIO is handed
over UDP until the first echo arrives (Mumble's UDP connectivity check in
spirit).

## 6. Receiver processing rules

For every received UDP packet the server and the client MUST in order:

1. Reject total length < 5 (header only).
2. Reject unknown protocol version bits != 0.
3. Reject kind unknown (drop silently).
4. For AUDIO: reject zero SSRC and zero-len frames, then forward verbatim.
   For VIDEO/SYSAUDIO: reject zero SSRC, then forward verbatim to the
   sender's watchers only (§4b).
5. For PING: echo the timestamp back to the source.

Square brackets of checks are normative because UDP under NAT is inherently
permissive about garbage input.

## 7. TCP control framing

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

All JSON, framed as above. A client MUST `HELLO` (random non-zero `ssrc`; the
ACK omits `ssrc` if it is taken, so pick another) before `JOIN`.

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
whose `member` carries `sharing: true` (omitted when false).

A server MUST throttle viewer-sent `SCREEN_KEYREQ` to at most one per 500 ms
for each `(viewer, presenter SSRC)` pair. Requests outside that gap MAY be
dropped without a reply; the automatic key request sent on `SCREEN_WATCH` does
not consume that budget. Clients use the same 500 ms gap when asking, and
pace VIDEO fragments instead of sending a keyframe as one burst.

## 8. Worked examples

**AUDIO, seq = 7, single Opus frame on the wire:**

```
0x00 0x11 0x22 0x33 0x44 // header: kind=0, V=0, T=0, SSRC=0x11223344
0x00 0x00 0x00 0x07       // seq = 7
...160 bytes Opus...
```

**PING, timestamp 0x1122334455667788:**

```
0x00 0x11 0x22 0x33 0x44 // header (kind=1)
0x11 0x22 0x33 0x44 0x55 0x66 0x77 0x88 // timestamp
```

## 9. Conformance summary

| Feature | Section | Status |
|---|---|---|
| Big-endian fields | §1 | M |
| Unknown kinds/V dropped | §3 | M |
| SSRC != 0 | §3 | M |
| Continuous transmission (no DTX) | §4 | M |
| T flag set on final talkspurt packet | §4 | M |
| Packet length ≤ 1024 (VIDEO ≤ 1200) | §2 | M |
| PING-echo before AUDIO | §5 | S |
| seq increases by ≥1 per packet | §4 | M |
| Tolerates unknown TCP message types | §7 | M |

Status codes: M = MUST, S = SHOULD, O = MAY, X = prohibited by this format.

## 10. Security considerations

v1 is plaintext. Every byte of this document and its packets are readable to
any on-path observer, and JOIN/LEAVE on TCP carry no authentication until
the roadmap item lands. Encryption is shaped in [security-model.md](security-model.md):
header (byte 0 + SSRC) as associated data and payload replaced by
`nonce ‖ ciphertext`. Until that lands, deployments deserve to run only on
trusted networks.

## 11. Revision history

- **v1 + screen share** (2026-11-20): kinds 2 (VIDEO) and 3 (SYSAUDIO),
  VIDEO datagrams up to 1200 bytes, watcher-only forwarding, `SCREEN_*`
  control messages (§7.2), `sharing` on members, 500 ms `SCREEN_KEYREQ` limit.
- **v0 → v1** (2026-09-17): JOIN/LEAVE moved from UDP to TCP; terminator
  flag added; PING now carries an echoed timestamp; explicit max datagram
  1024 bytes; continuous transmission documented; compliance summary added.
