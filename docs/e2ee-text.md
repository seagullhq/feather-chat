# End-to-end encryption for text (direct messages and server channels)

Status: **Normative**. Runs on top of [transport-security.md](transport-security.md).
Voice, screen share and call media are not covered.

## 1. What it protects

| Threat | Direct messages | Server channels |
|---|---|---|
| Theft of the database or backups | Protected, except weak passwords (§1.1) | Same |
| Server that only reads what passes through it | Protected | Protected |
| Server that substitutes a user's keys | Detected after first use; first contact only by comparing safety numbers | Same |
| Server that adds a member | Not possible: a DM has exactly its two users | **Not protected** (§1.1) |
| Forged or altered message or edit | Rejected | Rejected |
| Removed member reading later messages | n/a | Protected, if the server reports the removal |

### 1.1 Accepted limits

- Joining a server with its invite code is enough to receive channel keys,
  including those for earlier history. Members' clients share them
  automatically. A malicious server can therefore add a ghost member and read
  everything in a channel. Future permissions are expected to restrict this.
- Deletion, message order, timestamps and withheld messages are controlled by
  the server.
- No forward secrecy: identity secrets plus the database open all history.
- A server can test password guesses against the stored data, one Argon2id
  evaluation per guess.
- Metadata (who, when, size, membership, read markers) is visible.
- Messages stored before this feature stay readable by the server. Call log
  entries are written by the server and are never encrypted.
- A password change or recovery keeps `AK` and the identity: an old password or
  recovery code plus an old copy of the database still opens everything, later
  history included. Only an operator reset replaces the keys.
- On a device that never used scheme 1 for an account, a malicious server can
  claim scheme 0 and receive the real password if the user consents.
- Replay and edit-rollback detection lasts one client session.
- PRELOGIN shows which accounts are still on scheme 0, and a username and email
  of one account get the same salt.
- A server has at most 1000 members, banned ones included; joining a full one
  is refused with `server is full`.
- A malicious client build defeats all of this.

## 2. Encoding

Integers are big-endian. `u64` user and conversation IDs. `label("x")` is the
ASCII bytes of `FC1:x` followed by one zero byte. `‖` is concatenation. In JSON
every binary value is standard base64 with padding. Receivers reject wrong
lengths.

HKDF is HKDF-SHA256 with an empty salt and 32 bytes of output. AEAD is
XChaCha20-Poly1305 with a random 24-byte nonce. "box(key, pt, aad)" is
`nonce (24) ‖ ciphertext ‖ tag (16)`. Signatures are Ed25519 (64 bytes);
clients use strict verification.

## 3. Account keys

### 3.1 Password

The server never receives the password of an upgraded account.

```
master   = Argon2id v1.3(password UTF-8, salt (16 random), m=65536 KiB, t=3, p=4, len=32)
authKey  = HKDF(master, label("auth"))
wrapKey  = HKDF(master, label("pwwrap"))
```

The parameters are constants of `authScheme` 1. They are never negotiated.
The server stores `salt` and `SHA-256(authKey)` and compares in constant time.
Clients require 8 to 72 bytes of password, as before.

### 3.2 Keys

Per account: `AK` (32 random bytes), an Ed25519 identity key (32-byte seed) and
a separate X25519 encryption key.

```
akPassword = box(wrapKey,     AK, label("akpw")  ‖ identityPub)            72 bytes
akRecovery = box(recWrapKey,  AK, label("akrec") ‖ identityPub)            72 bytes
bundle     = box(AK, identitySeed ‖ encSecret, label("bundle") ‖ identityPub)   104 bytes

keyRecord  = label("keys") ‖ userId ‖ identityPub (32) ‖ encPub (32)
keySig     = Ed25519(identity, keyRecord)
```

After unwrapping, the client MUST check that the bundle's secrets derive the
published `identityPub` and `encPub`.

### 3.3 Recovery code

16 random bytes followed by the first 2 bytes of their SHA-256, written in
Crockford base32 (29 characters), shown in groups. Shown once when keys are
created and after each recovery; the user confirms having saved it. The 18 bytes
are encoded most significant bit first, the last character padded with one zero
bit (decoders require it), and shown in groups of five separated by hyphens. Input
ignores case, spaces and hyphens and maps O to 0 and I, L to 1.

```
recAuthKey = HKDF(code16, label("recauth"))     server stores SHA-256(recAuthKey)
recWrapKey = HKDF(code16, label("recwrap"))
```

Without password and code, an operator resets the account (§3.5). Old encrypted
history is then lost to that user.

### 3.4 Messages

All are control frames. New JSON fields:

| Field | Content |
|---|---|
| `authScheme` | 0 legacy bcrypt, 1 this document |
| `salt`, `authKey`, `newSalt`, `newAuthKey` | 16 / 32 bytes |
| `recoveryAuthKey`, `newRecoveryAuthKey` | 32 bytes |
| `accountKeys` | object: `keyId`, `identityPub`, `encPub`, `keySig`, `akPassword`, `akRecovery`, `bundle` |

**PRELOGIN** (before login) `{identifier}` → `ACK {authScheme, salt}`. For an
unknown identifier the server answers `authScheme` 1 with
`salt = HMAC-SHA256(secret, label("fakesalt") ‖ lowercase trimmed identifier)[:16]`,
where `secret` is 32 random bytes stored in the database.
It counts against the per-IP authentication rate limit. For a scheme 0 account
the answer has no `salt`.

**REGISTER** `{username, email, salt, authKey}`. `password` is refused.

**LOGIN**
- scheme 1: `{identifier, authKey, deviceToken}`.
- scheme 0: `{identifier, password, deviceToken}`, accepted only for an account
  still on bcrypt.
- Rate limits, gates and the uniform refusal of transport-security.md §1.3 apply
  to both. Device tokens cover the stored credential (bcrypt hash or verifier).
- The ACK carries `userId` (needed for `keyRecord` before HELLO), `authScheme`
  and, if keys exist, `accountKeys` without
  `akRecovery`.
- A session of an account without keys (legacy, new or reset) may only send
  `SET_KEYS`, plus what is allowed before login. Everything else, including
  HELLO, is refused with `account keys required`.

**SET_KEYS** (account without keys) `{accountKeys (without keyId),
recoveryAuthKey}`, plus `salt` and `authKey` if the account is on scheme 0, in
which case they are required and replace the bcrypt hash. Done in one
transaction; the server verifies `keySig`, and ignores `salt` and `authKey` for
a scheme 1 account. ACK carries `accountKeys` (without `akRecovery`) with the
new `keyId` and a new `deviceToken`. Refused if the account already has keys.

**Client rule for scheme 0:** the client asks the user for explicit consent
before sending a password in the legacy form, then requires a **new password,
different from the old one** (the server has seen the old one). The consent text
says that this is expected only for an account that was never upgraded or was
just reset, and that otherwise the password must not be sent. A client that has
previously used scheme 1 for this host and identifier MUST refuse scheme 0,
unless the password entered starts with `FEATHER-RESET-` (§3.5). After every
scheme 1 login or setup it remembers scheme 1 for the identifier used and, when
known, for the username and email.

**DELETE_USER** `{authKey}`: refused with `invalid credentials` without the
current `authKey`; it shares the failed login budgets.

A password change, a recovery and an operator reset end every other logged-in
session of the account. The server also rechecks each session's stored
credential at least every 30 seconds (a reset runs in another process).

**UPDATE_PASSWORD** `{authKey, newSalt, newAuthKey, accountKeys: {akPassword}}`.
Atomic: verifies the old `authKey`, replaces salt, verifier and `akPassword`.
The ACK carries a new `deviceToken`.

**RECOVER_BEGIN** (before login) `{identifier, recoveryAuthKey}` →
`ACK {userId, accountKeys}` with `akRecovery`.

**RECOVER_FINISH** (before login) `{identifier, recoveryAuthKey, newSalt,
newAuthKey, newRecoveryAuthKey, accountKeys: {akPassword, akRecovery}}`.
Atomic. The identity does not change. Both recovery messages count against the
per-IP limit only (a 128-bit code needs no per-account budget, which strangers
could exhaust) and fail with one uniform error.

### 3.5 Operator reset

`server -reset-user <username>`: sets a random temporary password (bcrypt,
scheme 0) of the form `FEATHER-RESET-<random>`, at most 72 bytes, printed once; retires the current keys; deletes the wrapped account
keys and the conversation key wraps addressed to the user; exits. The next login runs the upgrade flow. Peers see a changed identity key.

Retired public keys are kept (`keyId` stays valid) so old signatures verify.
They also outlive a deleted account, and its encrypted messages, epochs and wraps
keep its original user ID (`authorId`, `creatorId`, `senderId`).

## 4. Conversation keys

A conversation is a DM or a server channel, identified by `conversations.id`.
Each has numbered **epochs** (from 1), each with a random 32-byte key `K`.

```
convDesc  = kind (1 byte) ‖ a (u64) ‖ b (u64)
            DM: kind 1, a = lower user ID, b = higher. Channel: kind 2,
            a = server ID, b = channel ID.
keyCommit = HKDF(K, label("commit") ‖ conversationId ‖ epoch (u32))
epochRec  = label("epoch") ‖ conversationId ‖ convDesc ‖ epoch (u32) ‖ creatorId ‖ creatorKeyId (u64) ‖ keyCommit
epochSig  = Ed25519(creator identity, epochRec)

wrapHead  = label("wrap") ‖ conversationId ‖ convDesc ‖ epoch (u32) ‖ senderId ‖ senderKeyId (u64)
            ‖ recipientId ‖ recipientKeyId (u64) ‖ recipientEncPub ‖ keyCommit
(enc, ct) = HPKE base seal(recipientEncPub, info = wrapHead, aad = empty, K)
wrapSig   = Ed25519(sender identity, wrapHead ‖ enc (32) ‖ ct (48))
```

HPKE (RFC 9180): DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305.

Clients and server build `convDesc` from the address (and the caller's own ID),
never from a value the other side supplies separately. A client also stores the
address of every `conversationId` per host and rejects a second address for an
ID, or a second ID for an address.

### 4.1 Members and staleness

Members of a DM: its two users. Of a channel: the server owner and every
non-banned member. A **keyed member** is one with current keys.

The current epoch is **stale** if one of its recorded recipients is no longer a
member, or has retired the key it was recorded with. Each epoch stores its
recipient list at creation, extended by later shares, without a foreign key, so
deleted users still count. Members who lack a wrap do not make it stale: they get
one by sharing. It is also stale after the rejection described under REJECT_WRAP.

The server refuses a message whose epoch is not the current one, or is stale,
with `ERROR` `stale epoch`. The client then fetches keys, rotates if needed and
re-encrypts (same `msgId`), at most 3 times.

### 4.2 Messages

Conversations are addressed like existing text messages: DM by the peer's user
ID, channel by server and channel ID (same JSON fields as the existing send
requests).

**GET_CONVERSATION_KEYS** `{<address>, beforeEpoch?}` → `ACK`:

```
conversationId, currentEpoch (0 = none), stale,
members:  [{userId, keyId, identityPub, encPub, keySig}]   keyed current members
epochs:   [{epoch, creatorId, creatorKeyId, keyCommit, epochSig,
            wraps: [{senderId, senderKeyId, recipientKeyId, enc, ct, wrapSig}]}]
            newest first, at most 100 epochs below beforeEpoch; wraps are the
            caller's own, not rejected, the epoch creator's first, at most 3 per
            epoch (empty if none)
moreEpochs: bool
missing:  [{userId, epoch}]   keyed members lacking a non-rejected wrap for an
            epoch of this page that the caller holds one for, excluding those
            who rejected the caller's wrap; at most 200, newest epochs first
```

A DM request (any message of this section) is allowed only if the two users share
a server, are friends, or already have a message between them (otherwise `ERROR`
`DM channel not found`). The read takes no row lock. All requests of this section share a per-user rate limit (burst 60,
then 2 per second; error `too many attempts, try again later`).

**REJECT_WRAP** `{<address>, epoch, senderId}` → `ACK {conversationId}`. The
caller reports that the wrap addressed to it from `senderId` has a valid
signature but fails HPKE opening or `keyCommit`. The server marks it rejected: it
is never returned again, no longer counts, and that sender cannot replace it. If
`senderId` is the creator of the current epoch, the epoch becomes stale; one user
can cause that at most once per minute per conversation. Unknown wraps give
`invalid key share`. Clients send it automatically, then fetch keys again.

**CREATE_EPOCH** `{<address>, epoch, keyCommit, creatorKeyId, epochSig, wraps:
[{recipientId, recipientKeyId, enc, ct, wrapSig}]}`. Accepted only if, in one
transaction holding a lock on the conversation row: `epoch = currentEpoch + 1`;
there is no current epoch or it is stale, or it holds at least 1000 messages;
`wraps` covers exactly the keyed members with their current `keyId`; the caller
is among them; every signature verifies. Otherwise `ERROR`
`epoch conflict`, and the client discards its key and fetches again. At most
1000 wraps (the control frame limit applies).

The ACK carries `conversationId` and `epoch`.

**SHARE_KEYS** `{<address>, senderKeyId, shares: [{epoch, recipientId,
recipientKeyId, enc, ct, wrapSig}]}`, at most 200. The caller and recipient are
members, the caller holds a wrap for that epoch, signatures verify, key IDs are
current. One bad share rejects the whole request with `ERROR`
`invalid key share`. The ACK carries `conversationId`. A duplicate
(conversation, epoch, recipient, sender) is ignored and not pushed. Then
the recipient's sessions get a push `KEYS_CHANGED` with `conversationId` and the
address (for a DM, `userId` = the peer from the recipient's view).

**GET_USER_KEYS** `{userId, keyId}` → `ACK {userKeys: {userId, keyId,
identityPub, encPub, keySig, retired}}`. `keyId` 0 means current. Allowed when
the two users share a server or a conversation: the same user, a common server,
a DM between them, or a conversation the caller is a member of with an epoch whose
recipients include the target. Otherwise `ERROR` `user keys not found`. Needed
to verify old messages.

Clients share missing keys after opening a conversation, and after
`SERVER_MEMBERS_CHANGED` for conversations whose keys they have loaded.

### 4.3 Client rules

A client MUST:

1. Verify every `keySig`, `epochSig` and `wrapSig` before use, check HPKE output
   against `keyCommit`, and never use an epoch key that fails.
2. Pin the identity key of each user on first use (`known_keys.json` in the
   config directory, per host). If a different identity key appears, it MUST NOT
   wrap keys to it, and MUST NOT send in a DM with that user, until the user
   accepts it after a visible warning.
3. Wrap only to the members of the conversation. For a DM, only to itself and the
   peer, whatever the server lists.
4. Remember the highest epoch per conversation and never send under a lower one,
   nor accept a different `keyCommit` for a known epoch.
5. Never fall back to plaintext.
6. Only the currently pinned identity of a user permits sending under, sharing
   or recording an epoch whose creator or wrap sender uses it. An identity
   pinned earlier and since replaced, or a retired key never pinned (the
   server's word only), may only verify and decrypt old data; messages relying on
   a never-pinned key are marked unverified. An epoch that is not fully trusted
   and is numbered at or above the highest recorded epoch MUST be refused.
   Unverified messages never displace verified ones in duplicate detection.
7. Refuse epoch numbers above 1000000.

**Safety number:** SHA-256(label("safety") ‖ (userId ‖ identityPub) for both
users, lower userId first), shown as hex in groups of four. Users compare it
outside the app.

## 5. Messages

```
msgKey  = HKDF(K, label("msgkey") ‖ senderId)
padded  = length (u32) ‖ UTF-8 text ‖ zeros, to the smallest of
          256, 512, 1024, 2048, 4096, 8192, 16384 bytes that fits
msgAad  = label("aad") ‖ conversationId ‖ senderId ‖ senderKeyId (u64) ‖ msgId (16)
          ‖ revision (u32) ‖ epoch (u32)
box     = box(msgKey, padded, msgAad)             nonce ‖ ciphertext ‖ tag
msgSig  = Ed25519(sender identity, label("msg") ‖ msgAad ‖ SHA-256(box))
```

Text is 1 to 4000 characters (Unicode code points) and not blank (not only white
space), enforced by the sending client and rechecked after decryption, together
with the padding: smallest fitting size, all padding bytes zero.

JSON object `e2ee`: `{epoch, msgId, revision, senderKeyId, box, msgSig}`.

- The existing send and update requests carry `e2ee` instead of
  `directContent` / `serverContent`. The server refuses plaintext content.
- Pushes and history entries carry `e2ee` and no `content`. Rows written before
  this feature, and call log rows, carry `content` and no `e2ee`.
- Server checks: `box` length is 40 plus an allowed padded size; signature
  verifies with the sender's current key (`senderKeyId`); epoch is current and
  not stale. New message: `revision` 0 and `(conversation, sender, msgId)` unused.
  Update: by the author, same `msgId`, `revision` = stored + 1, current epoch.
  Rows without `e2ee` cannot be updated.
- `ERROR` texts: `plaintext content refused`, `encrypted message required`,
  `invalid encrypted message` (shape, key or signature), `stale epoch`,
  `invalid message revision`, `duplicate message id`.
- Deleting removes the stored box.
- List responses (DM and channel history, conversation list) stay under 512 KiB
  of message payload and carry `hasMore`. The conversation list preview carries
  `e2ee` and the author ID. Channel history also keeps `serverHasMore` (same
  value). The next page of history uses `dmBeforeId` / `serverBeforeId` as before;
  the next page of the conversation list sends `beforeConversationId` = the last
  `channelId` received.

Receiving client: verify `msgSig` with the key `(authorId, senderKeyId)`
(pinned, or an older key of the same user fetched with GET_USER_KEYS), then
decrypt with the epoch key, check padding and text rules. Drop a second message
with the same `(sender, msgId)` and a different server ID; never lower a
revision already shown. A message that cannot be verified or decrypted is shown
as an explicit placeholder, never as text. Plaintext rows are shown marked as
not encrypted.

## 6. Test vectors

Both implementations MUST test `keyRecord`, `epochRec`, `wrapHead` and `msgAad`
with these inputs and compare full hex constants: `userId` = 1, `senderId` = 1,
`recipientId` = 2, `conversationId` = 3, `epoch` = 4, `senderKeyId` =
`creatorKeyId` = 5, `recipientKeyId` = 6, `convDesc` = kind 2, a = 8, b = 9, `revision` = 7, `identityPub` = 32 ×
`0x11`, `encPub` = `recipientEncPub` = 32 × `0x22`, `keyCommit` = 32 × `0x33`,
`msgId` = 16 × `0x44`.
