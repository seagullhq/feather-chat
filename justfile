# Recipes marked [unix] and [windows] have a per-platform twin; just picks the
# one matching the host. Unix recipes run under sh, Windows ones under
# PowerShell, so neither platform needs the other's shell installed.
[windows]
set shell := ["powershell.exe", "-NoLogo", "-Command"]

# Run `just` with no arguments to see this list.
default:
    @just --list

# Configure and build the client.
[unix]
build:
    cmake --preset default
    cmake --build --preset default

[windows]
build:
    cmake --preset windows
    cmake --build --preset windows

# Build the client with symbols, into dist/client/feather-chat-debug/.
build-debug:
    cmake --preset debug
    cmake --build --preset debug

# Build and run the client.
[unix]
client: build
    ./dist/client/feather-chat

[windows]
client: build
    .\dist\client\feather-chat.exe

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
test:
    @just test-server

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
[unix]
fmt: fmt-go
    clang-format -i client/src/*.cpp

[windows]
fmt: fmt-go
    Get-ChildItem client\src\*.cpp | ForEach-Object { clang-format -i $_.FullName }

[working-directory: 'server']
fmt-go:
    gofmt -w .

# Fail if anything is unformatted.
[unix]
fmt-check: fmt-check-go
    clang-format --style=file --dry-run -Werror client/src/*.cpp

[windows]
fmt-check: fmt-check-go
    Get-ChildItem client\src\*.cpp | ForEach-Object { clang-format --style=file --dry-run -Werror $_.FullName }

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
    cmake -E rm -rf dist/client dist/client-debug dist/server
