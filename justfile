# Recipes run under `sh`, which Git for Windows provides on Windows.

# Run `just` with no arguments to see this list.
default:
    @just --list

# Configure and build the client.
build:
    cmake --preset default
    cmake --build build

# Build the client with symbols, into build-debug/.
build-debug:
    cmake --preset debug
    cmake --build build-debug

# Build and run the client.
client: build
    ./build/client/feather-chat

# Run the server.
[working-directory: 'server']
server port=":4444":
    go run . -addr {{port}}

# Run every test in the repository.
test:
    @just test-server

[working-directory: 'server']
test-server:
    go test -race ./...

# Format everything that has a formatter.
fmt:
    gofmt -w server

# What CI runs. Do this before opening a pull request.
check: fmt-check vet test build

[working-directory: 'server']
fmt-check:
    gofmt -l . | (! grep .)

[working-directory: 'server']
vet:
    go vet ./...

# Delete build output.
clean:
    cmake -E rm -rf build build-debug
