#!/usr/bin/env bash
# Regenerates Go gRPC code from proto/engine/vertex_engine.proto.
# Requires protoc, protoc-gen-go and protoc-gen-go-grpc on PATH.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GEN_DIR="$REPO_ROOT/api-gateway/gen"
MODULE="github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"

protoc \
  --proto_path="$REPO_ROOT/proto" \
  --go_out="$GEN_DIR" \
  --go_opt=paths=source_relative \
  --go_opt=Mengine/vertex_engine.proto="$MODULE" \
  --go-grpc_out="$GEN_DIR" \
  --go-grpc_opt=paths=source_relative \
  --go-grpc_opt=Mengine/vertex_engine.proto="$MODULE" \
  engine/vertex_engine.proto

echo "regenerated proto code into $GEN_DIR"
