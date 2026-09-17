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
- **UDP media**: AUDIO and PING only, forwarded per room by the server.
  Payloads MUST NOT exceed 1024 bytes.

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
T    1 bit   terminator, only defined for kind 0 (AUDIO), MUST be 0 on other kinds
K    5 bits  kind: 0 = AUDIO, 1 = PING. 2..31 reserved; receivers MUST drop
             unknown kinds without reporting.
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
| Packet length ≤ 1024 | §2 | M |
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

- **v0 → v1** (2026-09-17): JOIN/LEAVE moved from UDP to TCP; terminator
  flag added; PING now carries an echoed timestamp; explicit max datagram
  1024 bytes; continuous transmission documented; compliance summary added.
