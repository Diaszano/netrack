# syntax=docker/dockerfile:1
# Cross-compiles on the build host (any arch) for the target arch with zig as
# C compiler/linker, then ships a static musl binary in a scratch image.
FROM --platform=$BUILDPLATFORM rust:1.100-alpine AS builder
ARG TARGETARCH
RUN apk add --no-cache zig musl-dev ca-certificates \
 && rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo install cargo-zigbuild --locked
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src
COPY web ./web
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=netrack-target-$TARGETARCH \
    case "$TARGETARCH" in \
      arm64) T=aarch64-unknown-linux-musl ;; \
      amd64) T=x86_64-unknown-linux-musl ;; \
      *) echo "unsupported arch $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && cargo zigbuild --release --locked --target "$T" \
 && install -D "target/$T/release/netrack" /out/netrack \
 && install -d -o 65534 -g 65534 /out/data

FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /out/netrack /netrack
COPY --from=builder --chown=65534:65534 /out/data /data
ENV NETRACK_DATA_DIR=/data NETRACK_PORT=5216
VOLUME /data
EXPOSE 5216
USER 65534
ENTRYPOINT ["/netrack"]
