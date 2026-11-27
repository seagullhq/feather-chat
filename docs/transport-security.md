# Transport security (protocol v2)

Status: **Normative**. Protects the hop between client and server. It is not
end-to-end encryption: the server can read everything described here.

There is no plaintext fallback. A v1 client and a v2 server do not
interoperate.

## 1. Control channel: TLS

- TLS 1.3 only, on the same TCP port (7700). The framing of
  [wire-format.md](wire-format.md) §7 runs inside TLS.
- Session resumption is disabled on both ends.

### 1.1 Server certificate

- Flags `-tls-cert` and `-tls-key` load an operator-provided PEM pair. Both or
  neither.
- Otherwise the server uses `cert.pem` and `key.pem` in the directory given by
  `-tls-dir` (environment `FEATHER_TLS_DIR`, default `./tls`). If both files
  are missing it generates an ECDSA P-256 self-signed certificate, valid for
  20 years, and writes the key with mode 0600. If exactly one is missing, or a
  file does not parse, the server exits: it never silently replaces a key.
- At startup the server logs `TLS certificate SHA-256 fingerprint: <hex>`.
  The fingerprint is SHA-256 over the DER leaf certificate, written as
  lowercase hex, 64 characters, no separators.

### 1.2 Client trust

The client pins the leaf fingerprint per host. It does not use the system CA
store, names or expiry dates.

- Pins live in `known_servers.json` in the application config directory:
  `{"<host as typed, lowercased>": "<fingerprint>"}`.
- **Unknown host:** the handshake fails. The UI shows the fingerprint and asks
  the user to compare it with the server log. Only after the user accepts is the
  pin saved and a new connection opened. No application byte (in particular no
  password) is sent before that.
- **Changed fingerprint:** the handshake fails with a warning that shows the old
  and new values. Trusting the new one is a separate, explicit user action.
- The handshake signature MUST still be verified with the TLS library's normal
  routines. Only chain, name and expiry validation are replaced by the pin.

### 1.3 Server limits

| Limit | Value |
|---|---|
| TLS handshake deadline | 10 s |
| Time from the TLS handshake to successful LOGIN (frames do not extend it) | 60 s |
| Max control frame before successful LOGIN | 4096 bytes |
| Concurrent connections, total | 1024 |
| Concurrent connections per IP (IPv6: per /64) | 16 |
| Concurrent connections without a successful LOGIN | 256 |
| LOGIN + REGISTER + UPDATE_PASSWORD per IP | burst 10, then 1 every 6 s |
| Failed LOGIN per account (user ID) and IP | burst 5, then 1 every 30 s |
| Failed LOGIN per account (user ID), all IPs | burst 50, then 1 every 6 s |
| Failed LOGIN per device token | burst 5, then 1 every 30 s |

When the unauthenticated limit or the total limit is reached, a new connection
is admitted by closing an unauthenticated one. Sources that have logged in
successfully since the server started (the last 4096 are remembered) are spared
while others wait. Among the rest, the network (IPv4 /24, IPv6 /48) holding the
most loses, within it the source holding the most, and within that the oldest
connection. A successful LOGIN frees the unauthenticated slot, and logged-in
connections are never closed this way. A new connection is refused if its source
is at the per-IP limit, or if the total limit is reached and every connection is
logged in.

The account limits make username and email share one budget. To avoid revealing
which identifiers belong to one account, going over them is answered exactly
like a wrong password, after one bcrypt comparison against a dummy hash (the
real hash is not compared): `invalid credentials, or too many failed attempts:
try again later`.

Over the per-IP rate limit the server answers `ERROR` with `too many attempts,
try again later` and does not hash the password. Limiter entries expire so the
maps stay bounded. A write that times out closes the connection (a TLS stream is
unusable afterwards).

Usernames cannot contain `@`, so a new username never matches another account's
email. Where an older one does, LOGIN picks the email match.

### 1.4 Device tokens

Every successful `ACK` to `LOGIN` carries `deviceToken`. The client stores it per
host and identifier in the OS keyring, keeps it across sign-out, and sends it as
`deviceToken` in later `LOGIN` requests.

- The token is base64url (no padding) of `nonce (16) ‖ expiry (u64) ‖
  HMAC-SHA256`. The MAC covers the user ID, nonce, expiry and the stored password
  hash, under a key derived from the TLS private key.
- It lasts 90 days. Changing the password or the TLS key voids it.
- It never replaces the password. A LOGIN with a token valid for the account
  spends the token's own failure budget instead of the account's budgets, so
  failures from elsewhere do not lock the owner out. The per-IP limit still
  applies.
- An invalid token is ignored.

### 1.5 Known limits

- A device without a valid token (first login, new device, expired token, changed
  password) can be locked out while about ten or more source addresses keep an
  account's all-IPs budget empty.
- Enough source addresses, spread over many networks and reconnecting quickly,
  can still evict a first-time client before its LOGIN arrives. Stopping that
  needs filtering in front of the server.

## 2. Media channel (UDP 7701)

Each datagram is sealed per hop with AES-256-GCM. The server authenticates and
opens every datagram from a client, then seals it again for each recipient.

### 2.1 Keys

The successful `ACK` to `HELLO` (the one that carries `ssrc`) also carries:

| Field | Value |
|---|---|
| `mediaKeyUp` | standard base64 of 32 random bytes; client seals, server opens |
| `mediaKeyDown` | standard base64 of 32 random bytes; server seals, client opens |

Both keys are fresh for every accepted HELLO and die with the TCP connection. A
session can complete HELLO successfully only once: a second HELLO on a session
that already owns an SSRC is refused (ACK without `ssrc` and without keys).
HELLO requires a successful LOGIN.

### 2.2 Datagram

```
 0     1                 5                              13
+----+------------------+------------------------------+----------------------+
| FL |   SSRC (u32)     |        counter (u64)         | ciphertext ‖ tag(16) |
+----+------------------+------------------------------+----------------------+
```

- `FL` as in wire-format.md §3, with `V` = 1.
- All integers big-endian.
- `ciphertext` is the encryption of the v1 payload (everything that followed the
  5-byte header in v1).
- AAD = the 13 header bytes.
- Nonce (12 bytes) = four zero bytes ‖ `counter`.
- `counter` starts at 0 and increases by 1 for every datagram sealed with that
  key, whatever its kind. One atomic counter per key, shared by all sending
  threads. It never wraps and is never reused. The client's uplink counter
  belongs to the keys, not to the media engine: leaving and rejoining a channel
  on the same TCP connection MUST continue it, never restart it at 0.
- Uplink: `SSRC` is the sender's own. Downlink: `SSRC` is the original
  sender's (the recipient's own for a PING echo and for PATH), `counter` is the
  server's counter for that recipient's downlink key, and `FL` is copied (PATH,
  which the server originates, has T = 0).

Size limits apply to the whole sealed datagram: 1200 bytes for VIDEO, 1024 for
other kinds. The overhead over v1 is 24 bytes, so one VIDEO fragment carries at
most 1200 − 13 − 16 − 13 = **1158** bytes of H.264.

### 2.3 Receiver rules

In order: length ≥ 30 (empty plaintext is rejected); `V` = 1; known
kind; SSRC maps to a session (server); tag verifies; replay check; only then act.

- Replay window: 1024 counters per key. Reject a counter already seen or older
  than the highest accepted minus 1023. Update the window only after the tag
  verifies.
- Unauthenticated datagrams get no reply.
- The server sets or changes a session's UDP address only as §2.4 allows, and
  forwards nothing to a session that has no validated address.

### 2.4 Return routability (PATH, kind 4)

An authenticated datagram does not prove that its sender receives at the source
address: a logged-in client can spoof it and aim a room's media at a victim. So
the server sends media to an address only after the client has echoed a
challenge sent there. The payload of PATH, in both directions, is exactly 8
bytes; the sealed datagram is 37 bytes. PATH is never forwarded.

Server:

- A session has at most one validated address (none after HELLO) and at most one
  pending challenge.
- Trigger: an authenticated, non-replayed datagram of a kind other than PATH,
  whose counter is the highest accepted so far and whose source differs from the
  validated address. The server sends a PATH with 8 random bytes, sealed with
  the downlink key, to that source.
- At most one PATH per session every 100 ms, whatever the source. A retry to the
  challenged address repeats the same 8 bytes, so an echo slower than the retry
  period still matches. A challenge to another address uses new random bytes and
  replaces the pending one. There is no retry timer: the client's next datagram
  triggers it.
- A PATH from the client validates its source if that is the challenged address,
  the payload equals the pending bytes and the latest challenge is at most 10 s
  old. It need not carry the highest counter (it must still pass the replay
  check). The challenge is then consumed. Anything else changes nothing and gets
  no reply.
- Until then all media for the session keeps going to the old validated address,
  if any.
- A PING is echoed only if it comes from the validated address, and the echo
  goes there. Other PINGs only trigger a challenge.
- AUDIO, VIDEO and SYSAUDIO are forwarded to the room whatever their source:
  they are authenticated. However, the server drops them until the
  session has had its first PING echo.

Client:

- On a PATH carrying its own SSRC, send the same 8 bytes back in a PATH from the
  same socket. While it has had no PING echo yet, it SHOULD also send a new PING
  at once.
- Each new socket starts by sending PINGs (every 200 ms until the first echo,
  then every second) and sends no voice before that echo. Screen-share media
  instead waits for the `SCREEN_START` acknowledgement. A lost challenge or echo
  is repaired by the next PING or media datagram.

A NAT rebinding is repaired the same way, after about one round trip; until then
downlink media goes to the old mapping.

## 3. Test vector

AES-256-GCM, key = bytes `00 01 02 … 1f`, FL = `0x40` (V=1, AUDIO),
SSRC = `0x11223344`, counter = 7, plaintext = ASCII `feather`.

- header (AAD): `40 11223344 0000000000000007`
- nonce: `00000000 0000000000000007`

Both implementations MUST have a test that seals this input and compares the
full datagram with the same constant, produced by the Go implementation
(`crypto/cipher` GCM) and written here:

- datagram: `401122334400000000000000078608726b7f722c7db028f2e43f9e807e9cce80a420a711`
