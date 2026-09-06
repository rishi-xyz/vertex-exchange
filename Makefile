.PHONY: up down build logs test lint e2e seed bench pressure

JWT_SECRET     ?= dev-secret-change-me
BENCH_USERS    ?= 50
BENCH_DURATION ?= 30s
BENCH_PAIR     ?= ETH-USDC
USER_STEPS     ?= 10 25 50 100 200 400
STEP_DURATION  ?= 20s

## Bring up the full stack (postgres, redis, engine, gateway) in containers.
up:
	docker compose up -d --build

down:
	docker compose down -v

build:
	docker compose build

logs:
	docker compose logs -f

## Run the Rust and Go test suites.
test:
	cd engine && cargo test
	cd api-gateway && go test ./...

## Run formatting/lint checks for both services (mirrors CI).
lint:
	cd engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings
	cd api-gateway && test -z "$$(gofmt -l .)" && go vet ./...

## Scripted end-to-end vertical slice: builds engine+gateway, brings up
## docker-compose infra, and asserts a full register->deposit->cross->fill
## flow. Requires nothing already running.
e2e:
	./scripts/e2e.sh

## Seed a running stack (e.g. after `make up`) with demo pairs and users.
seed:
	./scripts/seed.sh

## Full-stack load/scoring benchmark against a running stack (e.g. after
## `make up`). Account setup bypasses REST (mints JWTs in-process, provisions
## accounts via direct engine gRPC) so it isn't limited by the auth rate
## limiter; order placement goes through the real REST/gRPC/DB/Redis/WS
## stack and respects the real per-user order rate limiter. JWT_SECRET MUST
## match the target gateway's JWT_SECRET.
##   make bench JWT_SECRET=dev-secret-change-me BENCH_USERS=200 BENCH_DURATION=60s
bench:
	cd api-gateway && go run ./cmd/benchmark \
		-jwt-secret "$(JWT_SECRET)" \
		-users $(BENCH_USERS) \
		-duration $(BENCH_DURATION) \
		-pair $(BENCH_PAIR)

## Capacity ramp test: runs `bench` at increasing user counts (USER_STEPS)
## and prints a table of throughput/latency/fill-rate per step, so you can
## see concretely where capacity bends rather than picking one user count
## and guessing. JWT_SECRET MUST match the target gateway's JWT_SECRET.
##   make pressure JWT_SECRET=dev-secret-change-me USER_STEPS="10 50 200 500"
pressure:
	JWT_SECRET="$(JWT_SECRET)" PAIR="$(BENCH_PAIR)" USER_STEPS="$(USER_STEPS)" STEP_DURATION="$(STEP_DURATION)" \
		./scripts/pressure_test.sh
