# Server design (Go side)

The server is an SFU in the Discord sense: it forwards Opus frames verbatim
per room and never decodes. Protocol on the wire:
[wire-format.md](wire-format.md); protocol overview: [protocol.md](protocol.md).

## Architecture

```
main ─► control TCP listener ─► session per client ─► RoomManager
        ▲                                              (map[string]*Room
        └── UDP socket ◄── goroutine reader            + RWMutex)
```

- `RoomManager` holds `map[room]map[ssrc]*Session`; TCP joins/leaves mutate
  the wiring; UDP sends only update `lastSeen`.
- One goroutine reads the UDP socket; each packet parses the 5-byte header,
  resolves the session by SSRC, and forwards verbatim to room peers,
  excluding the sender.
- The server stays entirely opaque to Opus: sequence numbers live on the
  client side and the whole client-side jitter story is client's to own.

## Integrity rules

- Session ID = SSRC chosen at HELLO; rejected on collision by the server.
- Server→client packets preserve the sender's SSRC, so receivers can key
  decoders and jitter buffers off it.

## Testing

`go test -race ./...`: golden-file round trips for the header helpers plus
an integration test that spins two clients through one server. No mocking
framework, stdlib only.

## Out of scope

Accounts, persistence, better NAT tricks — out of scope. Encryption is the
last roadmap item; the gate is documented in
[security-model.md](security-model.md).
