#!/usr/bin/env bash
# Pressure/capacity ramp test: runs the full-stack benchmark
# (api-gateway/cmd/benchmark) at a sequence of increasing concurrent-user
# counts against an already-running stack, and prints a table showing how
# throughput, latency, and error/fill rates trend as load increases — so you
# can see concretely where "how many users can this handle" starts to bend.
#
# Usage:
#   JWT_SECRET=dev-secret-change-me ./scripts/pressure_test.sh
#   JWT_SECRET=... USER_STEPS="10 50 100 250 500 1000" STEP_DURATION=30s ./scripts/pressure_test.sh
#
# Assumes a stack is already up (e.g. `make up`) and JWT_SECRET matches it.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY="$ROOT/api-gateway"

BASE_URL="${BASE_URL:-http://localhost:8080}"
ENGINE_ADDR="${ENGINE_ADDR:-localhost:5000}"
DATABASE_URL="${DATABASE_URL:-postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable}"
JWT_SECRET="${JWT_SECRET:-}"
PAIR="${PAIR:-ETH-USDC}"
STEP_DURATION="${STEP_DURATION:-20s}"
COOLDOWN="${COOLDOWN:-10}"
USER_STEPS="${USER_STEPS:-10 25 50 100 200 400}"

log()  { printf '[pressure] %s\n' "$*"; }
fail() { log "FAIL: $*"; exit 1; }

[ -n "$JWT_SECRET" ] || fail "JWT_SECRET is required and must match the target gateway's JWT_SECRET"

BIN_DIR="$(mktemp -d)"
BENCH_BIN="$BIN_DIR/vertex-benchmark"
log "building benchmark tool"
( cd "$GATEWAY" && go build -o "$BENCH_BIN" ./cmd/benchmark ) || fail "build benchmark tool"

log "checking gateway is reachable at $BASE_URL"
code=$(curl -s -m 5 -o /dev/null -w '%{http_code}' "$BASE_URL/healthz" 2>/dev/null)
[ "$code" = "200" ] || fail "gateway not healthy at $BASE_URL (got HTTP $code) — is the stack up? (make up)"

RESULTS_FILE="$BIN_DIR/results.tsv"
: > "$RESULTS_FILE"

# extract_metrics parses one benchmark run's stdout (passed as $1, a file
# path) into a single tab-separated row: users, ok_per_sec, p99_ms,
# error_count, throttled_count, fill_rate_pct, fill_p99_ms
extract_metrics() {
	python3 - "$1" <<'PYEOF'
import re, sys

with open(sys.argv[1]) as f:
    text = f.read()

def num(pattern, default="n/a"):
    m = re.search(pattern, text)
    return m.group(1) if m else default

ok_per_sec = num(r"\(([\d.]+) ok/sec\)")
errored    = num(r"(\d+) errored", "0")
throttled  = num(r"(\d+) throttled")
placement_block = re.search(r"placement latency.*?\n\s*p50=\S+\s+p95=\S+\s+p99=([\d.]+)(m?s)", text, re.S)
if placement_block:
    val, unit = placement_block.groups()
    p99_ms = float(val) * (1000 if unit == "s" else 1)
else:
    p99_ms = "n/a"
fill_rate = num(r"\(([\d.]+)% fill rate\)")
fill_block = re.search(r"fill latency.*?\n\s*p50=\S+\s+p95=\S+\s+p99=([\d.]+)(m?s)", text, re.S)
if fill_block:
    fval, funit = fill_block.groups()
    fill_p99_ms = float(fval) * (1000 if funit == "s" else 1)
else:
    fill_p99_ms = "n/a"

print(f"{ok_per_sec}\t{p99_ms}\t{errored}\t{throttled}\t{fill_rate}\t{fill_p99_ms}")
PYEOF
}

for users in $USER_STEPS; do
	log "=== step: $users users, ${STEP_DURATION} ==="
	OUT_FILE="$BIN_DIR/out-$users.log"
	"$BENCH_BIN" \
		-base-url "$BASE_URL" -engine-addr "$ENGINE_ADDR" -database-url "$DATABASE_URL" \
		-jwt-secret "$JWT_SECRET" -pair "$PAIR" \
		-users "$users" -duration "$STEP_DURATION" \
		-progress-interval 0 \
		> "$OUT_FILE" 2>&1
	tail -20 "$OUT_FILE" | sed 's/^/[pressure]   /'

	metrics=$(extract_metrics "$OUT_FILE")
	printf '%s\t%s\n' "$users" "$metrics" >> "$RESULTS_FILE"

	log "cooling down ${COOLDOWN}s before the next step..."
	sleep "$COOLDOWN"
done

echo
echo "=== Pressure Test Summary ($PAIR, ${STEP_DURATION}/step) ==="
printf '%-8s %-12s %-10s %-10s %-12s %-12s %-12s\n' "users" "ok/sec" "p99(ms)" "errored" "throttled" "fill-rate%" "fill-p99(ms)"
while IFS=$'\t' read -r users ok_per_sec p99_ms errored throttled fill_rate fill_p99_ms; do
	printf '%-8s %-12s %-10s %-10s %-12s %-12s %-12s\n' "$users" "$ok_per_sec" "$p99_ms" "$errored" "$throttled" "$fill_rate" "$fill_p99_ms"
done < "$RESULTS_FILE"

echo
log "raw per-step output kept in $BIN_DIR (not auto-deleted; inspect or rm -rf when done)"
log "reminder: this ramp doesn't reset the book between steps beyond each run's own cleanup —"
log "if fill-p99 grows across steps while ok/sec stays flat, that's the fills pipeline queueing, not order placement saturating."
