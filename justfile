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

# Run every test in the repository (none yet).
test:
    @just test-server

[working-directory: 'server']
test-server:
    go test -race ./...

# Format everything that has a formatter.
fmt:
    gofmt -w server
    clang-format -i client/src/*.cpp

# Format, vet, test and build. Run before pushing.
check: fmt-check vet test build

fmt-check: fmt-check-go
    clang-format --style=file --dry-run -Werror client/src/*.cpp

[working-directory: 'server']
fmt-check-go:
    gofmt -l . | (! grep .)

[working-directory: 'server']
vet:
    go vet ./...

# Delete build output.
clean:
    cmake -E rm -rf build build-debug
