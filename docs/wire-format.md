# Feather wire format (v0)

UDP, one datagram per packet. Big-endian. No fragmentation: an Opus frame at
20ms / 64kbps is ~160 bytes, so everything fits well under any real MTU.

```
byte 0       type
bytes 1..4   ssrc (uint32) — random per client, per session
```

| type | name  | rest of packet                          |
|------|-------|-----------------------------------------|
| 0    | JOIN  | room name, UTF-8, max 64 bytes          |
| 1    | AUDIO | seq (uint32), then the Opus frame       |
| 2    | LEAVE | empty                                   |
| 3    | PING  | empty — keeps the NAT mapping alive     |

The server never decodes audio. It forwards an AUDIO packet verbatim to every
other peer in the sender's room, so the sender's ssrc and seq reach the
receiver untouched and the jitter buffer runs entirely client-side.

## Not here yet

No encryption. Every packet is plaintext on the wire, so v0 is only safe on a
trusted network. The planned design is libsodium crossed with a per-room
symmetric key: an X25519 handshake over the existing TCP control channel, then
`crypto_secretbox` over the payload with the 5-byte header as associated data.
This must land before anyone runs this on the public internet.
