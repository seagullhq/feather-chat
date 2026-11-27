# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM golang:1.26-alpine AS build
ARG TARGETOS TARGETARCH
# x86-64 level: v2 runs on any CPU since ~2009; v3 (AVX2) is faster but crashes
# on older VPS CPUs. Build with --build-arg GOAMD64=v3 if the host supports it.
ARG GOAMD64=v2
WORKDIR /src
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod go mod download
COPY . .
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=$TARGETOS GOARCH=$TARGETARCH GOAMD64=$GOAMD64 \
    go build -trimpath -buildvcs=false -ldflags="-s -w" -o /out/server ./cmd/server
# scratch has no shell: prepare the certificate directory here.
RUN mkdir -m 0700 /out/tls

FROM scratch
COPY --from=build /out/server /server
# The TLS key must survive redeploys: clients pin its fingerprint. A new volume
# mounted here inherits this ownership, so the server can write to it.
COPY --from=build --chown=65532:65532 /out/tls /tls
ENV FEATHER_TLS_DIR=/tls
USER 65532:65532
VOLUME ["/tls"]
EXPOSE 7700/tcp 7701/udp
ENTRYPOINT ["/server"]
