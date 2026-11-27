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
| A2 | Per-session media keys; per-room E2EE key (pending) | Secret |
| A3 | Session integrity (SSRC → session mapping) | Integrity |
| A4 | Server availability | Availability |
| A5 | Shared screen content | Confidential |
| A6 | Passwords and messages | Confidential |
| A7 | Server TLS private key | Secret |

## Trust boundaries

- Public internet ↔ client–server hop on both TCP and UDP.
- Server→room traffic stays on one host (no federation assumed).
- The server terminates both encrypted hops and sees all content in the clear,
  so users have to trust it.
- Client GUI ↔ OS audio providers and the C library Opus.
- Client GUI ↔ OS screen-capture APIs and the screen-recording permission they
  require before any frame is produced.

## Threats and dispositions

| ID | Threat | Class | Rating | Disposition |
|---|---|---|---|---|
| T1 | Passive on-path listener recording calls | Info disclosure | High | Mitigated: UDP sealed per hop with AES-256-GCM |
| T2 | Impersonation of a room member | Spoofing | High | Mitigated against network attackers: AEAD tag and replay window on every datagram, keys issued only after LOGIN. The server itself can still impersonate (T8) |
| T3 | Query flood on control channel | DoS | Medium | Partly mitigated: connection caps (total and per IP), handshake and pre-login limits, LOGIN/REGISTER and failed-login rate limits. No general rate limit on requests after login (open) |
| T4 | Malformed UDP packets interpreted as valid | Tampering | Medium | Mitigated: header bytes are AAD; GCM authenticates before the server or client acts on a packet |
| T5 | Joining a room without credentials | Elevation | Medium | Mitigated: HELLO requires LOGIN; JOIN is limited to channels the user may access |
| T6 | CPU exhaust on malformed Opus length | DoS | Low | Domain bound: max packet 1024 bytes first [wire-format.md](wire-format.md) |
| T7 | Passive on-path listener recording a shared screen | Info disclosure | High | Mitigated: VIDEO and SYSAUDIO are sealed like voice |
| T8 | Malicious or compromised server reading or altering content | Info disclosure, tampering | High | **Open**: needs end-to-end encryption (pending) |
| T9 | Active man-in-the-middle on the client–server hop | Spoofing | High | Mitigated by certificate pinning, provided the user compares the fingerprint on first connection and on any change |
| T10 | Traffic analysis (SSRC, kind, counter, sizes, timing) | Info disclosure | Low | Accepted |
| T11 | Theft of the server TLS key | Spoofing | High | Generated key file has mode 0600. No revocation: replace the pair, and every user trusts the new fingerprint |
| T12 | Logged-in user spoofs a UDP source address to aim media at a victim | DoS | Medium | Mitigated: the server sends media only to an address that echoed a PATH challenge ([transport-security.md](transport-security.md) §2.4) |
| T13 | Strangers lock an owner out by failing logins, or keep newcomers from logging in | DoS | Medium | Partly mitigated: device tokens and eviction priority for known sources; limits in [transport-security.md](transport-security.md) §1.5 |

## Transport encryption (in place)

Defined in [transport-security.md](transport-security.md):

- TLS 1.3 on the control channel; the client pins the SHA-256 fingerprint of the
  server certificate per host.
- AES-256-GCM on every UDP datagram, with fresh keys per session and direction,
  the header as associated data and a replay window.
- No plaintext fallback.

## End-to-end encryption (pending, last roadmap item)

Transport encryption is **not** end-to-end encryption. The server holds the keys
of every hop and can read, record or alter voice, screen shares and messages,
and it receives passwords at login. E2EE, where the server never holds the
content keys, is not implemented.

Until the last item lands: local-network deployment only (README gate), with a
server operator you trust.
