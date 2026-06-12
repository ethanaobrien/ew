# syntax=docker/dockerfile:1


FROM --platform=$BUILDPLATFORM rust:bookworm AS builder

ARG TARGETARCH

WORKDIR /ew

RUN apt-get update \
 && case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-gnu > /rust-target ;; \
      arm64) apt-get install -y --no-install-recommends \
                 gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
             && rustup target add aarch64-unknown-linux-gnu \
             && echo aarch64-unknown-linux-gnu > /rust-target ;; \
      *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && rm -rf /var/lib/apt/lists/*

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar

COPY . .

# Cache
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/ew/target,id=ew-target-${TARGETARCH},sharing=locked \
    cargo build --release --target "$(cat /rust-target)" \
 && cp "target/$(cat /rust-target)/release/ew" /usr/local/bin/ew

FROM docker.io/library/debian:bookworm-slim

COPY --from=builder --chmod=755 /usr/local/bin/ew /root/ew/ew
COPY --chmod=755 ./docker/start.sh /root/ew/start.sh

ENTRYPOINT ["/root/ew/start.sh"]
