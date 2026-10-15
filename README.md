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

With a C++ toolchain, CMake, Ninja, Go and Qt 6.8 in place:

```bash
git clone https://github.com/seagullhq/feather-chat
cd feather-chat

just server      # serves hello world on :4444
just client      # builds and opens a window that says hello world
```

They do not talk to each other yet — that is the
[audio path](#roadmap), and it is the next thing to build.

No `just`? Every recipe is a plain command, listed [below](#commands).

Qt is the only prerequisite that takes real work. On Linux and macOS see
[Installing Qt](#installing-qt--linux-and-macos); on Windows it comes from
vcpkg, see [Windows notes](#windows-notes).

## Layout

| | |
|---|---|
| `client/` | Qt6 + C++20 desktop client (Linux, Windows, macOS) |
| `server/` | Go backend, standard library only |
| `docs/` | [docs index](docs/README.md) — protocol, wire format, audio path, server design, security model, ADR |
| `triplets/` | vcpkg triplet used by the Windows build |
| `scripts/` | `msvc-env.ps1`, which puts the MSVC toolchain on `PATH` for the Windows build, and `windows-build.ps1`, which `just build` calls |

## Installing Qt — Linux and macOS

The client needs **Qt 6.8**, CMake 3.21+, Ninja and a C++20 compiler. The
server needs **Go 1.26+** and nothing else.

**On Windows, skip this section** — Qt comes from vcpkg there. See
[Windows notes](#windows-notes).

The quickest route is your package manager: `qt6-qtbase-devel` on Fedora,
`qt6-base-dev` on Debian and Ubuntu, `qt6-base` on Arch, `brew install qt` on
macOS. Anything 6.8 or newer works, and CMake finds it with no help.

If your distribution is behind, or you want the exact same Qt as everyone else,
[aqtinstall](https://github.com/miurahr/aqtinstall) downloads the official
prebuilt Qt binaries — the same ones the Qt Online Installer ships, without the
account or the GUI.

<details>
<summary><b>Step 1 — get aqt</b></summary>

**Linux**

```bash
pipx install aqtinstall     # or: pip install --user aqtinstall
```

**macOS** — either `pipx install aqtinstall`, or download
[`aqt-macos`](https://github.com/miurahr/aqtinstall/releases/latest) from the
latest release and `chmod +x` it, which needs no Python at all.
</details>

<details>
<summary><b>Step 2 — install Qt</b></summary>

Pick the line for your platform. Only the last argument, the compiler ABI,
differs.

```bash
aqt install-qt linux desktop 6.8.0 linux_gcc_64 -O ~/Qt   # Linux
aqt install-qt mac   desktop 6.8.0 clang_64     -O ~/Qt   # macOS
```

On Windows, skip all of this and read [Windows notes](#windows-notes) instead —
Qt comes from vcpkg there.

If your ABI is not in that table, `aqt list-qt <host> desktop --arch 6.8.0`
prints the valid names. Use the system compiler: `gcc-c++` on Linux, Xcode
command line tools on macOS.

Roughly 1.5 GB and a few minutes.
</details>

<details>
<summary><b>Step 3 — point CMake at it</b></summary>

```bash
cmake --preset default -DCMAKE_PREFIX_PATH=~/Qt/6.8.0/gcc_64
cmake --build --preset default
./dist/client/feather-chat
```

Tired of passing the path? Copy `CMakeUserPresets.json.example` to
`CMakeUserPresets.json` and set it once — that file is gitignored, so your
local path stays out of the repository. A distribution Qt needs no path at all.
</details>

## Windows notes

Windows is the one platform without a system package manager that ships Qt, so
it uses [vcpkg](https://github.com/microsoft/vcpkg) with MSVC — the same
combination Mumble, qBittorrent and KeePassXC document for their own Windows
builds. Linux and macOS keep the system Qt described above; there is no vcpkg
on those.

### Setup

1. **Visual Studio 2022** with the *Desktop development with C++* workload.
2. **vcpkg**, once per machine:

   ```powershell
   git clone https://github.com/microsoft/vcpkg $HOME\vcpkg
   & "$HOME\vcpkg\bootstrap-vcpkg.bat"
   setx VCPKG_ROOT "$HOME\vcpkg"
   ```

3. Build with `just`, from any PowerShell:

   ```powershell
   just build
   ```

   `just` loads the MSVC toolchain for you first, via
   `scripts/msvc-env.ps1`. This matters: CMake's Ninja generator picks
   whichever C++ driver it finds first on `PATH`, and LLVM's `bin` usually
   comes before MSVC's, so a bare `cmake --preset windows` in a normal shell
   quietly builds the client with `clang++` against an MSVC-built Qt. The root
   `CMakeLists.txt` now rejects a non-MSVC compiler with that explanation, so
   it fails immediately instead of much later.

   If you would rather drive CMake yourself, use a **Developer PowerShell for
   Visual Studio** instead:

   ```powershell
   cmake --preset windows
   cmake --build --preset windows
   ```

### One build directory per toolchain

The `windows` preset builds in `dist/client-windows`, not `dist/client`, which
is where the system-Qt presets build. The two cannot share: a build directory
remembers the compiler CMake picked the first time it ran, so configuring one
preset in it silently changes the toolchain every later run of the other uses,
which on Windows means a `clang++` from LLVM's `bin` meeting a Qt built by MSVC.
Each preset gets its own directory, as `dist/client-debug` already does.

What the two builds still share is where the binary lands:
`client/CMakeLists.txt` sends the release build to `dist/client`, so `just
client` finds it at the same path on every platform, and the debug build to
`dist/client-debug`, where it cannot overwrite the release one.

### The first build takes about an hour, and only once

`cmake --preset windows` compiles Qt from source. `scripts/windows-build.ps1`
points vcpkg at a [binary
cache](https://learn.microsoft.com/vcpkg/users/binarycaching) in
`F:\tools\vcpkg-binary-cache`, so that hour is paid once: every later build,
including one after `just clean` or a fresh clone of the repo, restores Qt from
that cache in seconds. `just clean-deps` deletes the cache too, which forces the
full rebuild again.

The cache is configured in the script rather than in the preset on purpose.
vcpkg reads its binary-cache list from the environment, and CMake hands a
`VCPKG_BINARY_SOURCES` cache variable to the toolchain as an ordinary entry that
vcpkg never reads — CMake even warns that it went unused. vcpkg then quietly
falls back to its own per-machine cache, so the configured cache stays empty and
every fresh build pays the hour again.

It also lives outside the working tree, next to vcpkg. It used to be
`.cache/vcpkg-binary` inside the repo, but a gitignored directory there is one
`git clean -xdf` or disk cleanup away from being deleted, and rebuilding it costs
an hour. Nothing in the repo depends on where it is: point `VCPKG_BINARY_SOURCES`
at any other directory, or at a shared drive or an `azblob`/`s3` URL for CI, and
the script leaves your setting alone.

The preset uses an overlay triplet, `triplets/x64-windows-static-release.cmake`,
which drops the debug build of every dependency and roughly halves both the
wait and the disk usage. You lose the ability to step inside Qt's own source,
not the ability to debug feather-chat.

Because the cache is a plain directory of zips, a team or a CI runner can share
one by pointing `VCPKG_BINARY_SOURCES` at it, no code change involved. That is
the difference between an hour and a minute: on a 16-core machine the full Qt
build measures 58 minutes, and restoring the same 36 packages from the cache
takes 42 seconds, which is what makes a Windows CI job practical. qBittorrent
instead sidesteps vcpkg entirely and pulls a prebuilt Qt.

### Icons need Qt's SVG module

The icons are SVG files compiled into the binary, and Qt 6 moved SVG out of
qtbase into a separate `qtsvg` module. Without it the build still succeeds and
the app still starts, but every icon is blank: `QIcon` has no image handler for
SVG and hands back a null pixmap. That is why `qtsvg` is a dependency in
`vcpkg.json`, and why removing it is not a simplification.

### Why static linking

`VCPKG_TARGET_TRIPLET` is `x64-windows-static-release`, so Qt is linked into the
executable. Mumble and qBittorrent both do this, and the reason is practical:
Windows has no rpath, so a dynamically linked `feather-chat.exe` started from
Explorer dies looking for `Qt6Core.dll` and needs `windeployqt` to scatter DLLs
beside it. A static binary just runs.

Qt is LGPL, which puts conditions on static linking — feather-chat is AGPL-3.0
and ships its source, which satisfies them. A closed-source fork could not link
Qt statically without a commercial Qt license.

### Do not mix toolchains

Qt built for MSVC and Qt built for MinGW are different ABIs. Linking one
against the other fails, or produces a binary that crashes at runtime. If you
prefer MSYS2 and MinGW, keep everything inside MSYS2 and do not use vcpkg —
that path works but nobody here builds it regularly.

## Commands

[just](https://github.com/casey/just) is a convenience, never a requirement:
`dnf install just`, `brew install just`, `pacman -S just`,
`winget install Casey.Just`, or `cargo install just`.

| | | plain equivalent |
|---|---|---|
| `just build` | configure and build the client | `cmake --preset default && cmake --build --preset default` |
| `just client` | build it, then run it | the above, then `./dist/client/feather-chat` |
| `just server` | build and run the server on `:7700` | `cd server && go build -o ../dist/server/feather-chat-server . && ../dist/server/feather-chat-server -tcp :7700` |
| `just test` | run the Go tests (none written yet) | `cd server && go test -race ./...` |
| `just check` | format, vet, test and build | all of the above |
| `just clean` | delete build output | `rm -rf dist/client dist/client-debug dist/client-windows dist/server` |
| `just clean-deps` | also drop the vcpkg binary cache, forcing a full Qt rebuild | `rm -rf F:/tools/vcpkg-binary-cache` |

`just server :5000` picks another port. `just build-debug` produces an
unoptimized build with symbols in `dist/client-debug/`.

On Windows `just build` and `just client` load the MSVC environment from
`scripts/msvc-env.ps1` before calling CMake; see
[Windows notes](#windows-notes).

`just fmt` and `just check` run [clang-format](https://clang.llvm.org/docs/ClangFormat.html)
over the C++ as well as `gofmt` over the Go. Install it with
`dnf install clang-tools-extra`, `brew install clang-format`, or
`pip install clang-format`. Style lives in `.clang-format`, so nobody has to
argue about braces.

## Roadmap

In rough order:

- **Audio path.** Opus capture and playback over the UDP protocol in
  [`docs/wire-format.md`](docs/wire-format.md), and a server that forwards
  frames per room. Opus will come in through CMake's `FetchContent` — a
  small C library that builds in seconds, so it needs no installer.
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
