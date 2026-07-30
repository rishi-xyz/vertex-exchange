#!/usr/bin/env bash
set -euo pipefail

PROTO_DIR="$(cd "$(dirname "$0")/../../proto" && pwd)"
GRPCURL="grpcurl -plaintext -import-path $PROTO_DIR -proto engine/vertex_engine.proto"
HOST="localhost:5000"
OUTFILE="output/test_output_$(date +%s).log"

exec > >(tee -a "$OUTFILE") 2>&1

echo "=============================================="
echo " Vertex Engine gRPC Test Suite"
echo " Server: $HOST"
echo " Proto:  $PROTO_DIR/engine/vertex_engine.proto"
echo " Output: $OUTFILE"
echo "=============================================="
echo ""

call() {
    local step=$1 endpoint=$2 data=$3
    echo "--- Step $step: $endpoint ---"
    echo "Request: $data"
    $GRPCURL -d "$data" $HOST $endpoint || echo "ERROR: ${endpoint##*/} failed"
    echo ""
}

# ─── Step 1-2: Create users ───────────────────────────────────
echo ">> Adding users..."

ALICE_ID=$($GRPCURL -d '{}' $HOST vertex_engine.EngineServices/AddUser | jq -r '.userId')
echo "--- Step 1: vertex_engine.EngineServices/AddUser ---"
echo "Alice ID: $ALICE_ID"
echo ""

BOB_ID=$($GRPCURL -d '{}' $HOST vertex_engine.EngineServices/AddUser | jq -r '.userId')
echo "--- Step 2: vertex_engine.EngineServices/AddUser ---"
echo "Bob ID: $BOB_ID"
echo ""

# ─── Step 3-6: Deposit balances ────────────────────────────────
call 3 vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$ALICE_ID\",\"asset\":\"USDC\",\"quantity\":1000000}"
call 4 vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$ALICE_ID\",\"asset\":\"SOL\",\"quantity\":1000}"
call 5 vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$BOB_ID\",\"asset\":\"USDC\",\"quantity\":1000000}"
call 6 vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$BOB_ID\",\"asset\":\"SOL\",\"quantity\":1000}"

# ─── Step 7: Add trading pair ──────────────────────────────────
call 7 vertex_engine.EngineServices/AddTradingPair \
    '{"pair":{"base":"SOL","quote":"USDC"}}'

# ─── Step 8: Get order book (empty) ────────────────────────────
call 8 vertex_engine.UserSerivces/GetOrderBook \
    '{"pair":{"base":"SOL","quote":"USDC"}}'

# ─── Step 9: Alice places a buy order ──────────────────────────
echo "--- Step 9: vertex_engine.UserSerivces/SubmitOrder (Alice buys 50 SOL @ 25 USDC) ---"
ALICE_ORDER=$($GRPCURL -d "{
  \"pair\":{\"base\":\"SOL\",\"quote\":\"USDC\"},
  \"orderType\":\"GoodTillCancel\",
  \"side\":\"Buy\",
  \"price\":25,
  \"quantity\":50,
  \"userId\":\"$ALICE_ID\"
}" $HOST vertex_engine.UserSerivces/SubmitOrder)
echo "Response: $ALICE_ORDER"
ALICE_ORDER_ID=$(echo "$ALICE_ORDER" | jq -r '.orderId')
echo "Alice order ID: $ALICE_ORDER_ID"
echo ""

# ─── Step 10: Get order book (should have bid) ─────────────────
call 10 vertex_engine.UserSerivces/GetOrderBook \
    '{"pair":{"base":"SOL","quote":"USDC"}}'

# ─── Step 11: Bob places a matching sell order ─────────────────
echo "--- Step 11: vertex_engine.UserSerivces/SubmitOrder (Bob sells 30 SOL @ 25 USDC) ---"
BOB_ORDER=$($GRPCURL -d "{
  \"pair\":{\"base\":\"SOL\",\"quote\":\"USDC\"},
  \"orderType\":\"GoodTillCancel\",
  \"side\":\"Sell\",
  \"price\":25,
  \"quantity\":30,
  \"userId\":\"$BOB_ID\"
}" $HOST vertex_engine.UserSerivces/SubmitOrder)
echo "Response: $BOB_ORDER"
BOB_ORDER_ID=$(echo "$BOB_ORDER" | jq -r '.orderId')
echo "Bob order ID: $BOB_ORDER_ID"
echo ""

# ─── Step 12: Get order book (should show trades, remaining) ───
call 12 vertex_engine.UserSerivces/GetOrderBook \
    '{"pair":{"base":"SOL","quote":"USDC"}}'

# ─── Step 13: Modify Alice's order ─────────────────────────────
call 13 vertex_engine.UserSerivces/ModifyOrder \
    "{
      \"pair\":{\"base\":\"SOL\",\"quote\":\"USDC\"},
      \"orderId\":$ALICE_ORDER_ID,
      \"price\":30,
      \"quantity\":50,
      \"side\":\"Buy\",
      \"userId\":\"$ALICE_ID\"
    }"

# ─── Step 14: Cancel Bob's remaining order ─────────────────────
call 14 vertex_engine.UserSerivces/CancelOrder \
    "{
      \"pair\":{\"base\":\"SOL\",\"quote\":\"USDC\"},
      \"orderId\":$BOB_ORDER_ID
    }"

# ─── Step 15: Withdraw from Alice ──────────────────────────────
call 15 vertex_engine.EngineServices/WithdrawBalance \
    "{\"userId\":\"$ALICE_ID\",\"asset\":\"USDC\",\"quantity\":500000}"

# ─── Step 16: Remove Bob ───────────────────────────────────────
call 16 vertex_engine.EngineServices/RemoveUser \
    "{\"userId\":\"$BOB_ID\"}"

# ─── Step 17: Overflow test (should NOT crash the engine) ──────
echo "--- Step 17: Overflow test (massive deposit x2) ---"
call 17 vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$ALICE_ID\",\"asset\":\"SOL\",\"quantity\":3270311877}"
call 17b vertex_engine.EngineServices/DepositBalance \
    "{\"userId\":\"$ALICE_ID\",\"asset\":\"SOL\",\"quantity\":2724320592}"
echo "(This second deposit should overflow u32; the engine should log an error and keep running)"
echo ""

# ─── Step 18: Verify engine is still alive ─────────────────────
call 18 vertex_engine.UserSerivces/GetOrderBook \
    '{"pair":{"base":"SOL","quote":"USDC"}}'

echo "=============================================="
echo " All tests completed."
echo " Full output saved to: $OUTFILE"
echo "=============================================="
