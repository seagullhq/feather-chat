<h1 align="center">feather-chat</h1>

<p align="center">
  Self-hosted voice chat that stays out of your way.<br>
  A native Rust ([gpui](https://gpui.rs)) desktop client and a Go server you run yourself.
</p>

<p align="center">
  <img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-blue">
  <img alt="Status: early" src="https://img.shields.io/badge/status-early-orange">
  <img alt="Platforms: Linux, macOS, Windows" src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-lightgrey">
</p>

Holding a conversation should not cost a gigabyte of RAM and a bundled browser
engine. feather-chat does what Discord does for voice — rooms you drop into and
talk — as a native binary, on a server you own.

> [!NOTE]
> **Early.** Voice works, and the client–server hop is encrypted, but this is
> **not end-to-end encryption**: the server can read everything. What is
> settled is the [wire format](docs/wire-format.md) and
> [transport security](docs/transport-security.md); what is missing is on the
> [roadmap](#roadmap).

## Quick start

With Rust (stable), Go and [Task](https://taskfile.dev) in place:

```bash
git clone https://github.com/seagullhq/feather-chat
cd feather-chat

cp .env.example .env     # local PostgreSQL credentials (docker compose reads it)
docker compose up -d     # the PostgreSQL the server stores accounts and servers in

task server              # control on :7700, media on :7701
task client              # builds and opens the desktop client
```

They talk now: register from the client, create a server, drop into a voice
channel. On the first connection the client asks you to
[verify the server fingerprint](#verify-the-server-fingerprint). What is missing
is on the [roadmap](#roadmap).

No `task`? Every task is a plain command, listed [below](#commands).

On Linux, gpui needs the usual Wayland/X11, Vulkan, xkbcommon and ALSA
development packages. The first client build compiles gpui from source
(about a minute and a half on a recent machine); later builds are incremental.

## Verify the server fingerprint

The control channel is TLS 1.3. The client does not use certificate
authorities: it pins the SHA-256 fingerprint of the server certificate per host.

1. At startup the server logs
   `TLS certificate SHA-256 fingerprint: <64 hex characters>`. Read it from the
   server log (Docker:
   `docker compose -f docker-compose.prod.yaml logs server`), or get it from the
   operator over a channel you trust.
2. On the first connection to a host the client shows the fingerprint, split
   over two lines. Compare all 64 characters.
3. Choose **Trust and connect** only if they match. The client sends nothing,
   your password included, before you accept.

Accepted pins are stored in `known_servers.json` in the client's config
directory. If a pinned fingerprint changes, the client refuses to connect and
shows the old and new values. Do not choose **Trust the new fingerprint** unless
the operator confirms the change: someone may be intercepting the connection.

## Server certificate

| Flag | Meaning |
|---|---|
| `-tls-cert`, `-tls-key` | your own PEM certificate and private key; give both or neither |
| `-tls-dir` | directory of `cert.pem` and `key.pem`, used when the two flags above are not given (environment `FEATHER_TLS_DIR`, default `./tls`) |

If both files in the directory are missing, the server generates a self-signed
pair. If only one is missing, or the pair does not load, it exits. Keep the
files: a new certificate means a new fingerprint, which every user has to verify
and trust again. `./tls` is relative to the working directory, so `task server`
uses `tls/` and the plain command [below](#commands) uses `server/tls/`.

The Docker image sets `FEATHER_TLS_DIR=/tls`, and `docker-compose.prod.yaml`
mounts the named volume `tls` there. Keep that volume across redeploys. If you
mount a host directory instead, UID 65532 must be able to read it, and to write
it if the server has to generate the pair.

## Upgrading

Protocol v2 breaks compatibility. There is no plaintext fallback: an older
client cannot talk to a new server, and a new client cannot talk to an older
server. Upgrade the server and every client together.

## Layout

| | |
|---|---|
| `client/` | Rust + gpui desktop client (Linux, Windows, macOS) |
| `server/` | Go backend — PostgreSQL for accounts and servers |
| `docs/` | [docs index](docs/README.md) — protocol, wire format, transport security, audio path, server design, security model, ADR |

## Commands

[Task](https://taskfile.dev) is a convenience, never a requirement: one binary,
no shell or make needed on any OS. `brew install go-task`, `winget install Task.Task`,
`scoop install task`, `snap install task --classic`, or `go install github.com/go-task/task/v3/cmd/task@latest`.

| | | plain equivalent |
|---|---|---|
| `task build` | build the client | `cd client && cargo build --release` |
| `task client` | build it, then run it | `cd client && cargo run --release` |
| `task server` | build and run the server on `:7700` | `cd server && go build -o ../dist/server/feather-chat-server ./cmd/server && ../dist/server/feather-chat-server -tcp :7700` |
| `task test` | run the Go and Rust tests | `cd server && go test -race ./... && cd ../client && cargo test` |
| `task check` | format, vet, test and build | all of the above |
| `task clean` | delete build output | `rm -rf client/target dist/server` |

`task server PORT=:5000` picks another control port for the server; the client always
dials `7700` (`CONTROL_PORT` in `client/src/api/transport.rs`).

`task fmt` runs `cargo fmt` and `gofmt`.

## Roadmap

Tracked in the [project board](https://github.com/orgs/seagullhq/projects/1),
in rough order:

Voice works end to end today — Opus capture and playback over the UDP
protocol in [`docs/wire-format.md`](docs/wire-format.md), a server that
forwards frames per room, a jitter buffer per speaker, accounts, persistent
servers and channels, a device picker, a remembered login. What is left:

- **Push to talk.** The only missing input mode.
- **End-to-end encryption — the last item.** Transport encryption is in place
  ([`docs/transport-security.md`](docs/transport-security.md)), but the server
  still reads everything. E2EE is pending. Nothing ship-ready goes public until
  this is done.

## Security

Traffic between client and server is encrypted: the control channel uses
TLS 1.3 with a pinned certificate, and every UDP media datagram is sealed with
AES-256-GCM. Details are in
[`docs/transport-security.md`](docs/transport-security.md).

**This is not end-to-end encryption.** The server can read everything: voice,
shared screens, messages and your password at login. Whoever runs or compromises
the server can read and record it. Connect only to a server whose operator you
trust, and [verify its fingerprint](#verify-the-server-fingerprint). End-to-end
encryption is still pending, so keep deployments on a local network until it
lands. See the [security model](docs/security-model.md).

Found a problem in what does exist? Open an issue.

## License

[AGPL-3.0](LICENSE). Run it, modify it and self-host it freely. If you offer a
modified version to others over a network, you have to publish your changes
too.

Third-party assets and their licenses are listed in [CREDITS.md](CREDITS.md).
