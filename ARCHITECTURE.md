# VERTEX Exchange — Architecture

**Stack:** Primary backend = Go. WebSocket layer = Go. Matching engine = Rust.

---

## 1. High-Level Flow

```
Clients → Go API Layer (Primary Backend) → gRPC → Rust Core (Matching Engine)
                ↓                                        ↓
        Redis Streams (fills/events) ← ← ← ← ← Redis Pub/Sub (fills → Go backend)
                ↓
   DB Filler / Notification Fan-out → Infrastructure/Storage Layer
```

Three tiers: **Clients**, **Go API Layer**, **Rust Core**. A fourth tier, **Infrastructure/Storage**, sits underneath.

---

## 2. Clients Tier

- **Web/Mobile App** — HTTPS
- **WebSocket Client** — WSS (real-time market data, order updates)
- **Market Maker (API Client)** — HTTPS/WSS

All enter through the same **API Gateway / Load Balancer** in the Go layer.

---

## 3. Go API Layer ("Primary Backend")

### 3.1 Entry point
- **API Gateway / Load Balancer** — single entry, fans out to REST and WebSocket services.

### 3.2 Internal services
- **REST API Service** — request/response (place/cancel/modify order, balances, history)
- **WebSocket Server** — real-time push (`depth.<pair>`, `trade.<pair>`, `ticker.<pair>`)
- **Auth Service (JWT/OAuth)** — deferred to V2
- **Balance Service** — user balance queries with Redis cache pre-check (fast-fail before gRPC)

### 3.3 Order critical path
```
REST/WS → Rate Limiter / Risk Guard → gRPC Client → Rust Engine
```

### 3.4 Fills ingestion
- **Redis Streams** (fills/events) — Go-side consumer for guaranteed delivery
- **DB Filler Service** — writes engine output to persistent storage
- **Notification Fan-out** — pushes fill/order-status updates to WebSocket Server

### 3.5 Cross-cutting write path
- Go writes trade history to TimescaleDB / Postgres (consolidated via DB Filler)

---

## 4. Rust Core (Matching Engine)

### 4.1 RPC entry
- **gRPC Server (tonic)** — receives submit/cancel/replace calls from Go

### 4.2 Engine architecture

```
VertexEngine (trait)
    ├── CoreEngine               ← pure business logic, 
    └── WalEngine                ← wraps CoreEngine, adds WAL + ID generation + redis pub

engine_from_env() → Box<dyn VertexEngine>
    WAL_ENABLED=true  → WalEngine
    otherwise         → CoreEngine
```

- **`ExchangeEngine` trait** — public API for the engine; callers use the trait to stay decoupled from the implementation
- **`CoreEngine`** — owns orderbooks, users, and the snowflake ID generator; performs matching, balance locking, fill settlement; no WAL awareness
- **`WalEngine`** — wraps CoreEngine; writes WAL entries before each mutation, generates snowflake IDs for orders, handles replay on construction

### 4.3 Engine internals
- `BTreeMap<Price, VecDeque<Order>>` — price-time priority FIFO
- Self-trade prevention
- Partial fills / cancel-replace (modify)
- Snowflake Order IDs (assigned by engine before persistence)

### 4.4 Durability
- **WAL (local disk)** — disaster recovery only (append JSON lines pre-mutation, replay on startup)
- **Event Log → Kafka** — primary HA replay source (deferred to V2; V1 uses WAL/AOL only)

### 4.5 High availability
- **Passive Engine (Hot Standby)** — replays event log in lockstep (V2)
- V1: single engine, WAL recovery on restart

### 4.6 Environment configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `WAL_ENABLED` | `false` | Enable WAL-backed engine |
| `WAL_PATH` | `engine.wal` | Path to the WAL file |
| `TRACING_ENABLED` | `true` | Enable structured logging |
| `RUST_LOG` | `info,engine=debug` | Log level filter |

### 4.7 Fills distribution
- **Redis Pub/Sub** — engine publishes fills → consumed by Go's Redis Streams

---

## 5. Infrastructure / Storage Layer

| Component | V1 | V2 |
|-----------|----|----|
| **Postgres** | Accounts + trade history | Accounts / KYC |
| **Redis** | Cache + streams | Cache + streams |
| **TimescaleDB** | — | Trade history (time-series) |
| **Kafka** | — | Event log / audit |
| **Prometheus + Grafana** | — | Metrics / observability |
| **WAL Disk DR** | ✓ (local file) | ✓ |

---

## 6. Key Properties

1. **Engine is single source of truth** for balances and order IDs. Redis/cache is never authoritative.
2. **Balance pre-check** against Redis cache first, hard check inside engine.
3. **Fills path** uses Redis Streams (guaranteed delivery), not plain pub/sub.
4. **WAL = DR only.** HA path = passive engine via event log (V2).
5. **Engine is single-threaded** by default. Multi-threaded future uses message-passing (actor/shard), not shared-memory locking.
6. **Order IDs** (Snowflake) assigned by engine before persistence.

