# Vertex

Centralized crypto exchange matching engine and trading infrastructure.

## Architecture

```
Clients → Go API Layer → gRPC → Rust Matching Engine
               ↓                        ↓
       Redis Streams ← ← ← ← Redis Pub/Sub (fills)
               ↓
        DB Filler / Notification Fan-out → Storage
```

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