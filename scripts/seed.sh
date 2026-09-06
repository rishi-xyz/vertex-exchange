#!/usr/bin/env bash
# Seeds a running stack with demo trading pairs and two funded demo users.
# Assumes engine + gateway are already up (e.g. `docker compose up -d` or
# `make up`), reachable at BASE_URL / ENGINE_GRPC.
set -eu

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY="$ROOT/api-gateway"

GATEWAY_PORT="${GATEWAY_PORT:-8080}"
ENGINE_PORT="${ENGINE_PORT:-5000}"
BASE_URL="${BASE_URL:-http://localhost:$GATEWAY_PORT}"
ENGINE_GRPC="${ENGINE_GRPC:-localhost:$ENGINE_PORT}"
DEMO_PASSWORD="${DEMO_PASSWORD:-password123}"

log() { printf '[seed] %s\n' "$*"; }
json() { python3 -c "import sys,json; d=json.load(sys.stdin); print(d$1)"; }

log "building adminctl"
ADMIN_BIN="$(mktemp -d)/vertex-adminctl"
( cd "$GATEWAY" && go build -o "$ADMIN_BIN" ./cmd/adminctl )

for pair in ETH-USDC BTC-USDC SOL-USDC; do
  log "adding trading pair $pair"
  "$ADMIN_BIN" -addr "$ENGINE_GRPC" add-pair "$pair" >/dev/null 2>&1 || log "  (already exists, skipping)"
done

register_and_fund() {
  local email="$1"; shift
  local deposits=("$@")

  log "registering $email"
  RESP=$(curl -s -m 5 -X POST "$BASE_URL/auth/register" \
    -d "{\"email\":\"$email\",\"password\":\"$DEMO_PASSWORD\"}")
  USER_ID=$(echo "$RESP" | json "['user']['id']" 2>/dev/null || true)
  if [ -z "$USER_ID" ]; then
    log "  already registered, logging in instead"
    RESP=$(curl -s -m 5 -X POST "$BASE_URL/auth/login" \
      -d "{\"email\":\"$email\",\"password\":\"$DEMO_PASSWORD\"}")
    USER_ID=$(echo "$RESP" | json "['user']['id']")
  fi
  TOKEN=$(curl -s -m 5 -X POST "$BASE_URL/auth/login" \
    -d "{\"email\":\"$email\",\"password\":\"$DEMO_PASSWORD\"}" | json "['token']")

  for entry in "${deposits[@]}"; do
    asset="${entry%%:*}"
    quantity="${entry##*:}"
    log "  depositing $quantity $asset"
    curl -s -m 5 -X POST -H "Authorization: Bearer $TOKEN" \
      "$BASE_URL/users/$USER_ID/deposit" -d "{\"asset\":\"$asset\",\"quantity\":$quantity}" >/dev/null
  done
}

register_and_fund "demo-buyer@vertex.local" "USDC:1000000" "BTC:10" "SOL:1000"
register_and_fund "demo-seller@vertex.local" "ETH:1000" "BTC:10" "USDC:1000000"

log "done — demo-buyer@vertex.local / demo-seller@vertex.local, password: $DEMO_PASSWORD"
