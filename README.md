# feather-chat

Self-hosted voice chat: a Go backend and a native Qt desktop client.

Voice chat should not cost a gigabyte of RAM and a browser engine to hold a
conversation. feather-chat is an attempt at the same thing Discord does for
voice, without the weight, and on infrastructure you own.

> **Status: early.** Both sides are a hello world today. The build wiring and
> the wire format are what exist so far. See [Roadmap](#roadmap).

- `client/` — Qt6 + C++20 desktop client (Linux, Windows, macOS)
- `server/` — Go backend
- `docs/` — the [wire format](docs/wire-format.md), and design notes as they appear

## Building the client

You need CMake 3.21+, Ninja, a C++20 compiler, and Qt 6.8.

Qt comes from [aqtinstall](https://github.com/miurahr/aqtinstall), which
downloads the official prebuilt Qt binaries — the same ones the Qt Online
Installer ships, without the account or the GUI. Same tool and same command on
every platform, and it takes a couple of minutes rather than compiling Qt from
source.

### 1. Get aqt

**Windows** — download [`aqt_x64.exe`](https://github.com/miurahr/aqtinstall/releases/latest)
from the latest release. It bundles its own Python, so nothing else is needed.

**macOS** — download [`aqt-macos`](https://github.com/miurahr/aqtinstall/releases/latest),
then `chmod +x aqt-macos`.

**Linux** — no standalone binary is published, but every distribution ships
Python:

```bash
pipx install aqtinstall     # or: pip install --user aqtinstall
```

### 2. Install Qt

Pick the line for your platform. The last argument is the compiler ABI, which
is the only thing that differs.

```bash
aqt install-qt linux   desktop 6.8.0 linux_gcc_64    -O ~/Qt   # Linux
aqt install-qt mac     desktop 6.8.0 clang_64        -O ~/Qt   # macOS
aqt install-qt windows desktop 6.8.0 win64_msvc2022_64 -O C:\Qt # Windows
```

Roughly 1.5 GB and a few minutes. `aqt list-qt <host> desktop --arch 6.8.0`
prints the valid ABI names if yours is not in the table.

Windows also needs a compiler: Visual Studio 2022 with the "Desktop
development with C++" workload, and you build from a *Developer PowerShell for
VS 2022* prompt. Linux and macOS use the system compiler (`gcc-c++` / Xcode
command line tools).

### 3. Build

```bash
cmake --preset default -DCMAKE_PREFIX_PATH=~/Qt/6.8.0/gcc_64
cmake --build build
./build/client/feather-chat
```

Or, with [just](https://github.com/casey/just) installed, `just client`. Run
`just` on its own to see every recipe.

Tired of passing the path? Copy `CMakeUserPresets.json.example` to
`CMakeUserPresets.json` and set it once. That file is gitignored, so your local
path stays out of the repository.

Use `--preset debug` for an unoptimized build with symbols, in `build-debug/`.

If your distribution already packages Qt 6.8 (`qt6-qtbase-devel`, `qt6-base-dev`,
`qt6-base`) that works too — drop `CMAKE_PREFIX_PATH` and CMake will find it.
aqt is the default because it gives everyone the same Qt regardless of platform.

## Building the server

Go 1.26+, no other dependencies:

```bash
cd server
go build ./...
go test ./...
./server -addr :4444
```

Or `just server`.

## Roadmap

In rough order:

- **Audio path.** Opus capture and playback over the UDP protocol in
  [`docs/wire-format.md`](docs/wire-format.md), and a server that forwards frames per room.
  Opus and libsodium will come in through CMake's `FetchContent` — they are
  small C libraries that build in seconds, so they need no installer.
- **Encryption.** Nothing ships to the public internet before this lands.
- **Jitter buffer**, so packet reordering does not turn into clicks.
- Accounts, persistent rooms, device picker, push to talk.

## Common commands

Every command below has a plain equivalent, so `just` is a convenience, never a
requirement. Install it with `dnf install just`, `brew install just`,
`pacman -S just`, `winget install Casey.Just`, or `cargo install just`.

| | |
|---|---|
| `just build` | configure and build the client |
| `just client` | build it, then run it |
| `just server` | run the server on `:4444` (`just server :5000` for another port) |
| `just test` | run the test suite |
| `just check` | format, vet, test and build — what CI does |
| `just clean` | delete build output |

## Security

Nothing here is hardened yet. Audio has no encryption and no authentication, so
anyone on the network path can record a call and anyone who can reach the
server's UDP port can join a room whose name they can guess. Run it on a
trusted network until encryption lands.

Found something worse than that? Open an issue.

## License

[AGPL-3.0](LICENSE). You can run, modify and self-host this freely. If you
offer a modified version to others over a network, you have to publish your
changes too.
