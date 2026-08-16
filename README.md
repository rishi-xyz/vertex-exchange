# Vertex

Centralized crypto exchange matching engine and trading infrastructure.

## Architecture

![Architecture Image](assets/vertex.png)

## Stack

| Layer | Language | Role |
|-------|----------|------|
| API Gateway | Go | REST + WebSocket, auth, rate limiting, balance cache |
| Matching Engine | Rust | Orderbook, matching, WAL, fills distribution |
| Storage | Postgres / Redis | Accounts, trade history, cache, streams |

## Quick Start

```bash
docker compose up -d
```

## Project Layout

```
engine/              # Rust matching engine (tonic gRPC server)
api-gateway/         # Go REST + WebSocket service
proto/               # Shared protobuf definitions
docker-compose.yml   # All services
discussions/         # Architecture docs
```