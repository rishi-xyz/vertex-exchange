#!/usr/bin/env bash
# End-to-end vertical slice: compose infra up -> engine+gateway boot -> register
# two users -> fund -> cross a GTC order pair -> assert fill over WS, DB and API.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY="$ROOT/api-gateway"
ENGINE="$ROOT/engine"

GATEWAY_PORT="${GATEWAY_PORT:-8080}"
ENGINE_PORT="${ENGINE_PORT:-5000}"
JWT_SECRET="${JWT_SECRET:-e2e-secret}"
DATABASE_URL="${DATABASE_URL:-postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable}"
REDIS_URL="${REDIS_URL:-redis://localhost:6379}"
BASE_URL="http://localhost:$GATEWAY_PORT"
ENGINE_GRPC="localhost:$ENGINE_PORT"

GW_BIN="$(mktemp -d)/vertex-gateway"
ADMIN_BIN="$(mktemp -d)/vertex-adminctl"
ENGINE_LOG="$(mktemp)"
GW_LOG="$(mktemp)"
WS_LOG="$(mktemp)"
ENGINE_PID=""
GW_PID=""

log()  { printf '[e2e] %s\n' "$*"; }
fail() { log "FAIL: $*"; exit 1; }

json() { python3 -c "import sys,json; d=json.load(sys.stdin); print(d$1)"; }

cleanup() {
  [ -n "$GW_PID" ] && kill "$GW_PID" 2>/dev/null
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

log "building gateway"
( cd "$GATEWAY" && go build -o "$GW_BIN" ./cmd/server ) || fail "build gateway"
( cd "$GATEWAY" && go build -o "$ADMIN_BIN" ./cmd/adminctl ) || fail "build adminctl"
log "building engine"
( cd "$ENGINE" && cargo build --quiet ) || fail "build engine"

log "starting infra"
docker compose -f "$ROOT/docker-compose.yml" up -d --wait || fail "compose up"
docker compose -f "$ROOT/docker-compose.yml" ps --format '{{.Name}} {{.Health}}' | sed 's/^/[e2e] infra /'

log "starting engine"
( cd "$ENGINE" && REDIS_URL="$REDIS_URL" PORT="$ENGINE_PORT" \
  ./target/debug/vertex-engine >"$ENGINE_LOG" 2>&1 ) &
ENGINE_PID=$!

log "starting gateway"
( cd "$GATEWAY" && JWT_SECRET="$JWT_SECRET" DATABASE_URL="$DATABASE_URL" \
  REDIS_URL="$REDIS_URL" GATEWAY_PORT="$GATEWAY_PORT" ENGINE_GRPC_ADDR="$ENGINE_GRPC" \
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
"$ADMIN_BIN" -addr "$ENGINE_GRPC" add-pair ETH-USDC >/dev/null || fail "add pair"

STAMP="$(date +%s)"
BUYER_EMAIL="buyer-$STAMP@example.com"
SELLER_EMAIL="seller-$STAMP@example.com"

log "registering buyer ($BUYER_EMAIL)"
BUYER=$(curl -s -m 5 -X POST "$BASE_URL/auth/register" -d "{\"email\":\"$BUYER_EMAIL\",\"password\":\"password123\"}") || fail "register buyer"
BUYER_ID=$(echo "$BUYER" | json "['user']['id']")
[ -n "$BUYER_ID" ] || fail "register buyer: $BUYER"

log "registering seller ($SELLER_EMAIL)"
SELLER=$(curl -s -m 5 -X POST "$BASE_URL/auth/register" -d "{\"email\":\"$SELLER_EMAIL\",\"password\":\"password123\"}")
SELLER_ID=$(echo "$SELLER" | json "['user']['id']")
[ -n "$SELLER_ID" ] || fail "register seller: $SELLER"

BUYER_TOKEN=$(curl -s -m 5 -X POST "$BASE_URL/auth/login" -d "{\"email\":\"$BUYER_EMAIL\",\"password\":\"password123\"}" | json "['token']")
SELLER_TOKEN=$(curl -s -m 5 -X POST "$BASE_URL/auth/login" -d "{\"email\":\"$SELLER_EMAIL\",\"password\":\"password123\"}" | json "['token']")
[ -n "$BUYER_TOKEN" ] && [ -n "$SELLER_TOKEN" ] || fail "login"

log "funding accounts"
curl -s -m 5 -X POST "$BASE_URL/users/$BUYER_ID/deposit" -d '{"asset":"USDC","quantity":10000}' >/dev/null || fail "fund buyer"
curl -s -m 5 -X POST "$BASE_URL/users/$SELLER_ID/deposit" -d '{"asset":"ETH","quantity":10}' >/dev/null || fail "fund seller"

log "connecting buyer websocket"
curl -s -m 20 -N --http1.1 -H "Authorization: Bearer $BUYER_TOKEN" \
  "ws://localhost:$GATEWAY_PORT/ws?pair=ETH-USDC" >"$WS_LOG" 2>&1 &
WS_PID=$!
sleep 2

log "placing bid (buyer): 1 ETH @ 3000"
BID=$(curl -s -m 5 -X POST -H "Authorization: Bearer $BUYER_TOKEN" "$BASE_URL/orders" \
  -d '{"pair":"ETH-USDC","side":"buy","type":"gtc","price":3000,"quantity":1}')
BID_ID=$(echo "$BID" | json "['order']['id']")
[ -n "$BID_ID" ] || fail "place bid: $BID"
sleep 1

log "placing ask (seller): 1 ETH @ 3000 — should cross"
ASK=$(curl -s -m 5 -X POST -H "Authorization: Bearer $SELLER_TOKEN" "$BASE_URL/orders" \
  -d '{"pair":"ETH-USDC","side":"sell","type":"gtc","price":3000,"quantity":1}')
ASK_ID=$(echo "$ASK" | json "['order']['id']")
[ -n "$ASK_ID" ] || fail "place ask: $ASK"

sleep 3
kill "$WS_PID" 2>/dev/null

log "asserting fill visible over websocket"
grep -q '"type":"trade"' "$WS_LOG" || fail "no trade event on websocket"
log "asserting order statuses"
BID_STATUS=$(curl -s -m 5 -H "Authorization: Bearer $BUYER_TOKEN" "$BASE_URL/orders/$BID_ID" | json "['order']['status']")
ASK_STATUS=$(curl -s -m 5 -H "Authorization: Bearer $SELLER_TOKEN" "$BASE_URL/orders/$ASK_ID" | json "['order']['status']")
[ "$BID_STATUS" = "Filled" ] || fail "buyer order status = $BID_STATUS, want Filled"
[ "$ASK_STATUS" = "Filled" ] || fail "seller order status = $ASK_STATUS, want Filled"

log "asserting trade persisted in postgres"
TRADE_COUNT=$(docker compose -f "$ROOT/docker-compose.yml" exec -T postgres \
  psql -U vertex -d vertex -tAc "SELECT count(*) FROM trades WHERE pair='ETH-USDC'" | tr -d '[:space:]')
[ "$TRADE_COUNT" -ge 1 ] || fail "no trades persisted, count=$TRADE_COUNT"

log "asserting book cleared after fill"
BOOK=$(curl -s -m 5 "$BASE_URL/orderbook/ETH-USDC")
[ "$(echo "$BOOK" | json "['bids']")" = "[]" ] || fail "bids not cleared: $BOOK"
[ "$(echo "$BOOK" | json "['asks']")" = "[]" ] || fail "asks not cleared: $BOOK"

log "PASS: end-to-end vertical slice green"
log "  trade_id: $(grep -o '"trade_id":[0-9]*' "$WS_LOG" | head -1)"
log "  trades in postgres: $TRADE_COUNT"
