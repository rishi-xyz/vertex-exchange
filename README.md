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
cp .env.example .env   # adjust JWT_SECRET etc.
make up                 # postgres, redis, engine, gateway — all in containers
make seed                # optional: demo trading pairs + funded demo users
make e2e                 # scripted register -> deposit -> cross -> fill smoke test
```

See the [`Makefile`](Makefile) for the full list of targets (`test`, `lint`, `logs`, `down`).

## Project Layout

```
engine/                    # Rust matching engine (tonic gRPC server)
  discussions/prd.md       # original product/design notes
api-gateway/                # Go REST + WebSocket service
proto/                      # Shared protobuf definitions
scripts/e2e.sh               # scripted end-to-end vertical-slice test
scripts/seed.sh               # demo trading pairs + funded demo users
docker-compose.yml            # postgres, redis, engine, gateway
```