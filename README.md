<h1 align="center">feather-chat</h1>

<p align="center">
  Self-hosted voice chat that stays out of your way.<br>
  A native Qt desktop client and a Go server you run yourself.
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

With Qt 6.8, CMake, Ninja and Go already installed:

```bash
git clone https://github.com/seagullhq/feather-chat
cd feather-chat

just server      # terminal 1 — starts the server on :4444
just client      # terminal 2 — builds and launches the client
```

No `just`? Every recipe is a plain command, listed [below](#commands).

Missing Qt? That is the only real prerequisite — see [Installing Qt](#installing-qt).

## Layout

| | |
|---|---|
| `client/` | Qt6 + C++20 desktop client (Linux, Windows, macOS) |
| `server/` | Go backend, standard library only |
| `docs/` | [wire format](docs/wire-format.md), and design notes as they appear |

## Installing Qt

The client needs **Qt 6.8**, CMake 3.21+, Ninja and a C++20 compiler. The
server needs **Go 1.26+** and nothing else.

Qt comes from [aqtinstall](https://github.com/miurahr/aqtinstall), which
downloads the official prebuilt Qt binaries — the same ones the Qt Online
Installer ships, without the account or the GUI. One tool, one command shape,
every platform, and it finishes in minutes instead of compiling Qt from source.

<details>
<summary><b>Step 1 — get aqt</b></summary>

**Windows** — download [`aqt_x64.exe`](https://github.com/miurahr/aqtinstall/releases/latest)
from the latest release. It bundles its own Python, so nothing else is needed.

**macOS** — download [`aqt-macos`](https://github.com/miurahr/aqtinstall/releases/latest),
then `chmod +x aqt-macos`.

**Linux** — no standalone binary is published, but every distribution ships
Python:

```bash
pipx install aqtinstall     # or: pip install --user aqtinstall
```
</details>

<details>
<summary><b>Step 2 — install Qt</b></summary>

Pick the line for your platform. Only the last argument, the compiler ABI,
differs.

```bash
aqt install-qt linux   desktop 6.8.0 linux_gcc_64      -O ~/Qt   # Linux
aqt install-qt mac     desktop 6.8.0 clang_64          -O ~/Qt   # macOS
aqt install-qt windows desktop 6.8.0 win64_msvc2022_64 -O C:\Qt  # Windows
```

Roughly 1.5 GB and a few minutes. If your ABI is not in that table,
`aqt list-qt <host> desktop --arch 6.8.0` prints the valid names.

Windows also needs a compiler: Visual Studio 2022 with the *Desktop development
with C++* workload, and you build from a *Developer PowerShell for VS 2022*
prompt. Linux and macOS use the system compiler (`gcc-c++`, or Xcode command
line tools).

Your distribution's Qt works too, if it is 6.8 or newer — `qt6-qtbase-devel` on
Fedora, `qt6-base-dev` on Debian and Ubuntu, `qt6-base` on Arch. aqt is the
default here only because it gives everyone the identical Qt.
</details>

<details>
<summary><b>Step 3 — point CMake at it</b></summary>

```bash
cmake --preset default -DCMAKE_PREFIX_PATH=~/Qt/6.8.0/gcc_64
cmake --build build
./build/client/feather-chat
```

Tired of passing the path? Copy `CMakeUserPresets.json.example` to
`CMakeUserPresets.json` and set it once — that file is gitignored, so your
local path stays out of the repository. A distribution Qt needs no path at all.
</details>

## Commands

[just](https://github.com/casey/just) is a convenience, never a requirement:
`dnf install just`, `brew install just`, `pacman -S just`,
`winget install Casey.Just`, or `cargo install just`.

| | | plain equivalent |
|---|---|---|
| `just build` | configure and build the client | `cmake --preset default && cmake --build build` |
| `just client` | build it, then run it | the above, then `./build/client/feather-chat` |
| `just server` | run the server on `:4444` | `cd server && go run . -addr :4444` |
| `just test` | run the Go tests (none written yet) | `cd server && go test -race ./...` |
| `just check` | format, vet, test and build | all of the above |
| `just clean` | delete build output | `rm -rf build build-debug` |

`just server :5000` picks another port. `just build-debug` produces an
unoptimized build with symbols in `build-debug/`.

## Roadmap

In rough order:

- **Audio path.** Opus capture and playback over the UDP protocol in
  [`docs/wire-format.md`](docs/wire-format.md), and a server that forwards
  frames per room. Opus and libsodium will come in through CMake's
  `FetchContent` — small C libraries that build in seconds, so they need no
  installer.
- **Encryption.** Nothing ships to the public internet before this lands.
- **Jitter buffer**, so packet reordering stops turning into clicks.
- Accounts, persistent rooms, a device picker, push to talk.

## Security

Nothing here is hardened yet. Audio has no encryption and no authentication, so
anyone on the network path can record a call, and anyone who can reach the
server's UDP port can join a room whose name they can guess. Run it on a
trusted network until encryption lands.

Found something worse than that? Open an issue.

## License

[AGPL-3.0](LICENSE). Run it, modify it and self-host it freely. If you offer a
modified version to others over a network, you have to publish your changes
too.
