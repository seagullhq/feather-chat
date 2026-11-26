# Security model

Living threat model — updated after each roadmap item. Method:
STRIDE-lite, one row per threat (same structure public templates like
[martinholovsky/sota-skills](https://github.com/martinholovsky/sota-skills)
recommend for in-repo models). Review by reading the table; supporting
sections are scaffolding.

## Assets

| ID | Asset | Class |
|---|---|---|
| A1 | Voice content | Confidential |
| A2 | Per-room symmetric key | Secret |
| A3 | Session integrity (SSRC → session mapping) | Integrity |
| A4 | Server availability | Availability |
| A5 | Shared screen content | Confidential |

## Trust boundaries

- Public internet ↔ client–server hop on both TCP and UDP.
- Server→room traffic stays on one host (no federation assumed).
- Client GUI ↔ OS audio providers and the C libraries Opus/libsodium.
- Client GUI ↔ OS screen-capture APIs and the screen-recording permission they
  require before any frame is produced.

## Threats and dispositions

| ID | Threat | Class | Rating | Disposition |
|---|---|---|---|---|
| T1 | Passive on-path listener recording calls | Info disclosure | High | Mitigate via per-room key + secretbox (planned) |
| T2 | Impersonation of a room member | Spoofing | High | HMAC (well, AEAD tag) on every packet after crypto lands |
| T3 | Query flood on control channel | DoS | Medium | Per-IP rate limit on TCP (open) |
| T4 | Malformed UDP packets interpreted as valid | Tampering | Medium | Header bytes act as AAD; secretbox authenticates (planned) |
| T5 | Joining a room without credentials | Elevation | Medium | ACLs once accounts land (open) |
| T6 | CPU exhaust on malformed Opus length | DoS | Low | Domain bound: max packet 1024 bytes first [wire-format.md](wire-format.md) |
| T7 | Passive on-path listener recording a shared screen | Info disclosure | High | Plaintext VIDEO like voice today; covered by the same planned per-room key |

## Planned encryption (last roadmap item)

- X25519 handshake on the TCP control channel yields a per-room key.
- libsodium `crypto_secretbox` seals packets; header bytes are used as
  associated data — the wire format is already shaped for this.
- Client source: libsodium via CMake FetchContent; server:
  `golang.org/x/crypto/nacl` (stdlib-adjacent, one dependency total).

Until the last item lands: local-network deployment only (README gate).
Screen-share video uses the same plaintext UDP path as voice, so assume any
shared window is visible to anyone who can observe that network.
