# Recipes marked [unix] and [windows] have a per-platform twin; just picks the
# one matching the host. Unix recipes run under sh, Windows ones under
# PowerShell, so neither platform needs the other's shell installed.
#
# `just` runs each recipe line in a separate shell process, so a Windows recipe
# that needs state to carry between lines must keep it on a single line.
[windows]
set shell := ["powershell.exe", "-NoLogo", "-Command"]

# Cargo output directory (relative to client/, where the cargo recipes run).
client_target := "../dist/client"

# Run `just` with no arguments to see this list.
default:
    @just --list

# Build the client for shipping (small, fat LTO, slow) into dist/client/release/.
[working-directory('client')]
build:
    cargo build --release --target-dir {{ client_target }}

# Build the client with symbols into dist/client/debug/.
[working-directory('client')]
build-debug:
    cargo build --target-dir {{ client_target }}

# Fast optimized build (no LTO) into dist/client/fast/, for development.
[working-directory('client')]
build-fast:
    cargo build --profile fast --target-dir {{ client_target }}

# Build (fast profile) and run the client.
[working-directory('client')]
client:
    cargo run --profile fast --target-dir {{ client_target }}

# Install the client, desktop entry and icons for the current user.
[linux]
[working-directory('client')]
install-client: build
    install -Dm755 {{ client_target }}/release/feather-chat ~/.local/bin/feather-chat
    install -Dm644 assets/icon.png ~/.local/share/icons/hicolor/256x256/apps/feather-chat.png
    install -Dm644 assets/feather.svg ~/.local/share/icons/hicolor/scalable/apps/feather-chat.svg
    install -Dm644 ../packaging/linux/feather-chat.desktop ~/.local/share/applications/feather-chat.desktop
    gtk-update-icon-cache -q -t ~/.local/share/icons/hicolor 2>/dev/null || true

# Build a macOS .app bundle into dist/.
[macos]
bundle-client: build
    rm -rf "dist/Feather Chat.app"
    mkdir -p "dist/Feather Chat.app/Contents/MacOS" "dist/Feather Chat.app/Contents/Resources"
    install -m 755 dist/client/release/feather-chat "dist/Feather Chat.app/Contents/MacOS/feather-chat"
    install -m 644 packaging/macos/Info.plist "dist/Feather Chat.app/Contents/Info.plist"
    install -m 644 client/assets/icon.icns "dist/Feather Chat.app/Contents/Resources/icon.icns"

# Build the server binary into dist/server/.
[unix]
[working-directory('server')]
build-server:
    mkdir -p ../dist/server
    go build -o ../dist/server/feather-chat-server ./cmd/server

[windows]
[working-directory('server')]
build-server:
    if (!(Test-Path ../dist/server)) { New-Item -ItemType Directory -Path ../dist/server }
    go build -o ../dist/server/feather-chat-server.exe ./cmd/server

# Build and run the server.
[unix]
server port=":7700": build-server
    ./dist/server/feather-chat-server -tcp {{ port }}

[windows]
server port=":7700": build-server
    .\dist\server\feather-chat-server.exe -tcp {{ port }}

# Run every test in the repository.
test: test-server test-client

[working-directory('client')]
test-client:
    cargo test --target-dir {{ client_target }}

# Run the server's Go tests: verbose, no result cache.
# The -race flag needs cgo and a C compiler, which we don't require on Windows.
[unix]
[working-directory('server')]
test-server:
    go test -race -v ./...

[windows]
[working-directory('server')]
test-server:
    go test -v ./...

# Format everything that has a formatter.
fmt: fmt-go fmt-client

# Fail if anything is unformatted.
fmt-check: fmt-check-go fmt-check-client

[working-directory('client')]
fmt-client:
    cargo fmt

[working-directory('client')]
fmt-check-client:
    cargo fmt --check

[working-directory('server')]
fmt-go:
    gofmt -w .

[unix]
[working-directory('server')]
fmt-check-go:
    gofmt -l . | (! grep .)

[windows]
[working-directory('server')]
fmt-check-go:
    if (gofmt -l .) { Write-Error "Go files need gofmt"; exit 1 }

[working-directory('server')]
vet:
    go vet ./...

# Format, vet, test and build. Run before pushing.
check: fmt-check vet test build build-server

# Delete build output.
[working-directory('client')]
clean:
    cargo clean --target-dir {{ client_target }}
