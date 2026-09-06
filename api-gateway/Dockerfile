# Build context: api-gateway/
FROM golang:1.26-bookworm AS builder

WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -o /out/vertex-gateway ./cmd/server
RUN CGO_ENABLED=0 go build -o /out/vertex-adminctl ./cmd/adminctl

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/vertex-gateway /usr/local/bin/vertex-gateway
COPY --from=builder /out/vertex-adminctl /usr/local/bin/vertex-adminctl

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/vertex-gateway"]
