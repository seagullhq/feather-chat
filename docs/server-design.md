# Server design (Go side)

The server is an SFU in the Discord sense: it forwards media per room and never
decodes Opus or H.264. It also owns accounts, servers, channels, friends and
messages, stored in PostgreSQL. Protocol on the wire:
[wire-format.md](wire-format.md); encryption:
[transport-security.md](transport-security.md); protocol overview:
[protocol.md](protocol.md).

## Architecture

```
cmd/server -> internal/control (TLS accept loop, one goroutine per client)
                 |-> accounts, servers, friends, messages -> PostgreSQL
                 `-> internal/room.Manager (rooms, claimed SSRCs; RWMutex)
                        ^
UDP socket <--> Manager.ServeUDP (one goroutine)
```

| Package (`internal/`) | Role |
|---|---|
| `control` | TLS control channel: routes each message type to a handler, enforces connection and login limits |
| `room` | Sessions, rooms, screen-share subscriptions, media keys, UDP receive and forwarding |
| `protocol` | Wire formats: UDP header, sealed datagrams, replay window, TCP framing, control messages |
| `accounts`, `servers`, `friends`, `messages` | Database stores |
| `migrations` | Embedded SQL migrations, applied at startup |
| `tlscert` | Loads or generates the server certificate |
| `config` | Environment configuration |

- `room.Manager` holds `map[room]map[ssrc]*Session` and every SSRC claimed at
  HELLO. Control goroutines change it under its lock.
- One goroutine reads the UDP socket. For each datagram it parses the header,
  resolves the session by SSRC, verifies and opens it with that session's uplink
  key, checks the replay window, then seals the payload again for each recipient
  with the recipient's downlink key and counter. AUDIO goes to room peers,
  VIDEO and SYSAUDIO only to watchers, never back to the sender.
- A session gets media only at an address that echoed a PATH challenge
  ([transport-security.md](transport-security.md) §2.4).
- The server stays opaque to the codecs: sequence numbers, jitter buffering and
  frame reassembly belong to the client. It can read the payloads: encryption is
  per hop, not end to end.

## Integrity rules

- A client must LOGIN before HELLO. Session ID = SSRC chosen at HELLO; the
  server refuses a taken or zero one, and a second HELLO on the same connection.
- Media keys are created per accepted HELLO and die with the TCP connection.
- Server to client datagrams preserve the sender's SSRC, so receivers can key
  decoders and jitter buffers off it.

## Testing

`go test ./...` (add `-race` where cgo is available): wire-format round trips and
the sealing test vector, UDP forwarding over loopback, control limits, and
integration tests that drive clients through a whole server (they need
PostgreSQL). No mocking framework.

## Out of scope

End-to-end encryption (pending, see
[security-model.md](security-model.md)), federation, and NAT traversal beyond
following the client's validated address.
