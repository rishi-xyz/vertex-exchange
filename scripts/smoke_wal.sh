#!/usr/bin/env bash
# Stage 5.1 smoke test: deposits, a resting order and its balance lock must
# survive an engine crash+restart when WAL_ENABLED=true. The gateway is left
# running throughout — only the engine process is killed and restarted — to
# prove the gRPC client recovers once the engine comes back.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY="$ROOT/api-gateway"
ENGINE="$ROOT/engine"

GATEWAY_PORT="${GATEWAY_PORT:-8080}"
ENGINE_PORT="${ENGINE_PORT:-5000}"
JWT_SECRET="${JWT_SECRET:-smoke-secret}"
DATABASE_URL="${DATABASE_URL:-postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable}"
REDIS_URL="${REDIS_URL:-redis://localhost:6379}"
BASE_URL="http://localhost:$GATEWAY_PORT"

WORKDIR="$(mktemp -d)"
WAL_PATH="$WORKDIR/engine.wal"
GW_BIN="$WORKDIR/vertex-gateway"
ADMIN_BIN="$WORKDIR/vertex-adminctl"
ENGINE_LOG="$WORKDIR/engine.log"
GW_LOG="$WORKDIR/gateway.log"
ENGINE_PID=""
GW_PID=""

log()  { printf '[smoke-wal] %s\n' "$*"; }
fail() { log "FAIL: $*"; [ -f "$ENGINE_LOG" ] && { log "engine log:"; cat "$ENGINE_LOG"; }; exit 1; }
json() { python3 -c "import sys,json; d=json.load(sys.stdin); print(d$1)"; }

cleanup() {
  [ -n "$GW_PID" ] && kill "$GW_PID" 2>/dev/null
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

start_engine() {
  ( cd "$ENGINE" && REDIS_URL="$REDIS_URL" PORT="$ENGINE_PORT" \
    WAL_ENABLED=true WAL_PATH="$WAL_PATH" \
    ./target/debug/vertex-engine >>"$ENGINE_LOG" 2>&1 ) &
  ENGINE_PID=$!
}

wait_engine_port() {
  for i in $(seq 1 30); do
    if (exec 3<>"/dev/tcp/localhost/$ENGINE_PORT") 2>/dev/null; then
      exec 3>&- 3<&-
      return 0
    fi
    sleep 0.5
  done
  return 1
}

log "building gateway + engine"
( cd "$GATEWAY" && go build -o "$GW_BIN" ./cmd/server ) || fail "build gateway"
( cd "$GATEWAY" && go build -o "$ADMIN_BIN" ./cmd/adminctl ) || fail "build adminctl"
( cd "$ENGINE" && cargo build --quiet ) || fail "build engine"

log "starting infra (postgres, redis)"
docker compose -f "$ROOT/docker-compose.yml" up -d --wait postgres redis || fail "compose up"

log "starting engine (WAL_ENABLED=true, WAL_PATH=$WAL_PATH)"
start_engine
wait_engine_port || fail "engine never listened on $ENGINE_PORT"

log "starting gateway"
( cd "$GATEWAY" && JWT_SECRET="$JWT_SECRET" DATABASE_URL="$DATABASE_URL" \
  REDIS_URL="$REDIS_URL" GATEWAY_PORT="$GATEWAY_PORT" ENGINE_GRPC_ADDR="localhost:$ENGINE_PORT" \
  "$GW_BIN" >"$GW_LOG" 2>&1 ) &
GW_PID=$!

log "waiting for gateway healthz"
for i in $(seq 1 30); do
  code=$(curl -s -m 2 -o /dev/null -w '%{http_code}' "$BASE_URL/healthz" 2>/dev/null)
  [ "$code" = "200" ] && break
  sleep 1
done
[ "$code" = "200" ] || { log "gateway log:"; cat "$GW_LOG"; fail "gateway never healthy"; }

log "adding trading pair"
"$ADMIN_BIN" -addr "localhost:$ENGINE_PORT" add-pair BTC-USDC >/dev/null || fail "add pair"

STAMP="$(date +%s)"
EMAIL="wal-smoke-$STAMP@example.com"
log "registering $EMAIL"
REG=$(curl -s -m 5 -X POST "$BASE_URL/auth/register" -d "{\"email\":\"$EMAIL\",\"password\":\"password123\"}")
USER_ID=$(echo "$REG" | json "['user']['id']")
[ -n "$USER_ID" ] || fail "register: $REG"
TOKEN=$(curl -s -m 5 -X POST "$BASE_URL/auth/login" -d "{\"email\":\"$EMAIL\",\"password\":\"password123\"}" | json "['token']")

log "depositing 50000 USDC"
curl -s -m 5 -X POST -H "Authorization: Bearer $TOKEN" "$BASE_URL/users/$USER_ID/deposit" -d '{"asset":"USDC","quantity":50000}' >/dev/null || fail "deposit"

log "placing a resting (non-crossing) bid: 1 BTC @ 10000"
ORDER=$(curl -s -m 5 -X POST -H "Authorization: Bearer $TOKEN" "$BASE_URL/orders" \
  -d '{"pair":"BTC-USDC","side":"buy","type":"gtc","price":10000,"quantity":1}')
ORDER_ID=$(echo "$ORDER" | json "['order']['id']")
[ -n "$ORDER_ID" ] || fail "place order: $ORDER"

log "recording pre-restart state"
BAL_BEFORE=$(curl -s -m 5 -H "Authorization: Bearer $TOKEN" "$BASE_URL/balances")
AVAIL_BEFORE=$(echo "$BAL_BEFORE" | json "['balances']['USDC']['available']")
LOCKED_BEFORE=$(echo "$BAL_BEFORE" | json "['balances']['USDC']['locked']")
BOOK_BEFORE=$(curl -s -m 5 "$BASE_URL/orderbook/BTC-USDC")
[ "$LOCKED_BEFORE" = "10000" ] || fail "expected 10000 USDC locked before restart, got $LOCKED_BEFORE"
echo "$BOOK_BEFORE" | grep -q '"price":10000' || fail "resting order not in book before restart: $BOOK_BEFORE"

log "killing engine (simulating a crash)"
kill -9 "$ENGINE_PID" 2>/dev/null
wait "$ENGINE_PID" 2>/dev/null
ENGINE_PID=""
for i in $(seq 1 20); do
  (exec 3<>"/dev/tcp/localhost/$ENGINE_PORT") 2>/dev/null && { exec 3>&- 3<&-; sleep 0.5; continue; }
  break
done

log "restarting engine from the same WAL file"
start_engine
wait_engine_port || fail "engine never came back up after restart"
sleep 1 # let the gateway's gRPC client reconnect

log "asserting balances survived the restart"
BAL_AFTER=$(curl -s -m 5 -H "Authorization: Bearer $TOKEN" "$BASE_URL/balances")
AVAIL_AFTER=$(echo "$BAL_AFTER" | json "['balances']['USDC']['available']")
LOCKED_AFTER=$(echo "$BAL_AFTER" | json "['balances']['USDC']['locked']")
[ "$AVAIL_AFTER" = "$AVAIL_BEFORE" ] || fail "available USDC changed across restart: $AVAIL_BEFORE -> $AVAIL_AFTER"
[ "$LOCKED_AFTER" = "$LOCKED_BEFORE" ] || fail "locked USDC changed across restart: $LOCKED_BEFORE -> $LOCKED_AFTER"

log "asserting the resting order survived the restart"
BOOK_AFTER=$(curl -s -m 5 "$BASE_URL/orderbook/BTC-USDC")
echo "$BOOK_AFTER" | grep -q '"price":10000' || fail "resting order missing from book after restart: $BOOK_AFTER"

log "asserting the restored order can still be cancelled through the gateway"
curl -s -m 5 -X POST -H "Authorization: Bearer $TOKEN" "$BASE_URL/orders/$ORDER_ID/cancel" | json "['cancelled']" | grep -q True \
  || fail "could not cancel restored order"

log "PASS: WAL crash-recovery smoke test green"
