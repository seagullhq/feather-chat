# Recipes marked [unix] and [windows] have a per-platform twin; just picks the
# one matching the host. Unix recipes run under sh, Windows ones under
# PowerShell, so neither platform needs the other's shell installed.
#
# `just` runs each recipe line in a separate shell process, so a Windows recipe
# that needs state to carry between lines 
[windows]
set shell := ["powershell.exe", "-NoLogo", "-Command"]

# Run `just` with no arguments to see this list.
default:
    @just --list

# Build the client (release).
[working-directory: 'client']
build:
    cargo build --release

# Build the client with symbols.
[working-directory: 'client']
build-debug:
    cargo build

# Build and run the client.
[working-directory: 'client']
client:
    cargo run --release

# Build the server binary into dist/server/.
[unix]
[working-directory: 'server']
build-server:
    mkdir -p ../dist/server
    go build -o ../dist/server/feather-chat-server .

[windows]
[working-directory: 'server']
build-server:
    if (!(Test-Path ../dist/server)) { New-Item -ItemType Directory -Path ../dist/server }
    go build -o ../dist/server/feather-chat-server.exe .

# Build and run the server.
[unix]
server port=":7700": build-server
    ./dist/server/feather-chat-server -tcp {{port}}

[windows]
server port=":7700": build-server
    .\dist\server\feather-chat-server.exe -tcp {{port}}

# Run every test in the repository.
test: test-server test-client

[working-directory: 'client']
test-client:
    cargo test

# Run the server's Go tests: verbose, no result cache.
# The -race flag needs cgo and a C compiler, which we don't require on Windows.
[unix]
[working-directory: 'server']
test-server:
    go test -race -v ./...

[windows]
[working-directory: 'server']
test-server:
    go test -v ./...

# Format everything that has a formatter.
fmt: fmt-go fmt-client

# Fail if anything is unformatted.
fmt-check: fmt-check-go fmt-check-client

[working-directory: 'client']
fmt-client:
    cargo fmt

[working-directory: 'client']
fmt-check-client:
    cargo fmt --check

[working-directory: 'server']
fmt-go:
    gofmt -w .

[unix]
[working-directory: 'server']
fmt-check-go:
    gofmt -l . | (! grep .)

[windows]
[working-directory: 'server']
fmt-check-go:
    if (gofmt -l .) { Write-Error "Go files need gofmt"; exit 1 }

[working-directory: 'server']
vet:
    go vet ./...

# Format, vet, test and build. Run before pushing.
check: fmt-check vet test build


# Delete build output.
clean:
    cargo clean --manifest-path client/Cargo.toml
