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
> **Early.** Both sides are a hello world today. What is settled is the
> toolchain and the [wire format](docs/wire-format.md), not the code. No audio
> flows yet. See the [roadmap](#roadmap).

## Quick start

With Rust (stable), Go and [just](https://github.com/casey/just) in place:

```bash
git clone https://github.com/seagullhq/feather-chat
cd feather-chat

cp .env.example .env     # local MongoDB credentials (docker compose reads it)
docker compose up -d     # the MongoDB the server stores accounts and servers in

just server              # control on :7700, media on :7701
just client              # builds and opens the desktop client
```

They do not talk to each other yet — that is the
[audio path](#roadmap), and it is the next thing to build.

No `just`? Every recipe is a plain command, listed [below](#commands).

On Linux, gpui needs the usual Wayland/X11, Vulkan, xkbcommon and ALSA
development packages. The first client build compiles gpui from source
(about a minute and a half on a recent machine); later builds are incremental.

## Layout

| | |
|---|---|
| `client/` | Rust + gpui desktop client (Linux, Windows, macOS) |
| `server/` | Go backend — MongoDB for accounts and servers |
| `docs/` | [docs index](docs/README.md) — protocol, wire format, audio path, server design, security model, ADR |

## Commands

[just](https://github.com/casey/just) is a convenience, never a requirement:
`dnf install just`, `brew install just`, `pacman -S just`,
`winget install Casey.Just`, or `cargo install just`.

| | | plain equivalent |
|---|---|---|
| `just build` | build the client | `cd client && cargo build --release` |
| `just client` | build it, then run it | `cd client && cargo run --release` |
| `just server` | build and run the server on `:7700` | `cd server && go build -o ../dist/server/feather-chat-server . && ../dist/server/feather-chat-server -tcp :7700` |
| `just test` | run the Go and Rust tests | `cd server && go test -race ./... && cd ../client && cargo test` |
| `just check` | format, vet, test and build | all of the above |
| `just clean` | delete build output | `rm -rf client/target dist/server` |

`just server :5000` picks another control port for the server; the client always
dials `7700` (`CONTROL_PORT` in `client/src/net.rs`).

`just fmt` runs `cargo fmt` and `gofmt`.

## Roadmap

In rough order:

- **Audio path.** Opus capture and playback over the UDP protocol in
  [`docs/wire-format.md`](docs/wire-format.md), and a server that forwards
  frames per room. 
- **Jitter buffer**, so packet reordering stops turning into clicks.
- Accounts, persistent rooms, a device picker, push to talk.
- **Encryption — the last item.** The wire format is shaped for it already
  (`docs/wire-format.md` reserves the layout), so everything above lands on
  plaintext first; encryption goes in right before the first public-internet
  deployment. Nothing ship-ready goes public until this is done.

## Security

There is no audio yet, so there is nothing to intercept today. What matters is
what the design does *not* have: the protocol in
[`docs/wire-format.md`](docs/wire-format.md) has no encryption and no
authentication, so once audio flows, anyone on the network path could record a
call and anyone who reaches the server's port could join a room whose name they
guess.

Encryption lands before this is worth pointing at the public internet. Until
then, treat any deployment as a trusted-network toy.

Found a problem in what does exist? Open an issue.

## License

[AGPL-3.0](LICENSE). Run it, modify it and self-host it freely. If you offer a
modified version to others over a network, you have to publish your changes
too.
