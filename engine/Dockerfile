# Build context: repo root (build.rs reads ../proto relative to this crate).
FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY proto ./proto
COPY engine ./engine
WORKDIR /build/engine
RUN cargo build --release --bin vertex-engine

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/engine/target/release/vertex-engine /usr/local/bin/vertex-engine

EXPOSE 5000
ENTRYPOINT ["/usr/local/bin/vertex-engine"]
