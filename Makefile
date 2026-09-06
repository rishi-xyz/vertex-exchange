.PHONY: up down build logs test lint e2e seed

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
