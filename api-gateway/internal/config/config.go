package config

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

type Config struct {
	GatewayPort    string
	EngineGRPCAddr string
	DatabaseURL    string
	DBMaxConns     int32
	RedisURL       string
	JWTSecret      string
	JWTTTL         time.Duration
}

func Load() (*Config, error) {
	ttl, err := time.ParseDuration(envOr("JWT_TTL", "24h"))
	if err != nil {
		return nil, fmt.Errorf("parse JWT_TTL: %w", err)
	}
	// pgxpool defaults to 4 connections if unset, which starves under any
	// real concurrency: every authenticated request does at least one query
	// (auth middleware's GetUserByID) plus whatever the handler itself needs,
	// and the fills consumer shares the same pool.
	maxConns, err := strconv.ParseInt(envOr("DB_MAX_CONNS", "20"), 10, 32)
	if err != nil {
		return nil, fmt.Errorf("parse DB_MAX_CONNS: %w", err)
	}

	cfg := &Config{
		GatewayPort:    envOr("GATEWAY_PORT", "8080"),
		EngineGRPCAddr: envOr("ENGINE_GRPC_ADDR", "localhost:5000"),
		DatabaseURL:    envOr("DATABASE_URL", "postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable"),
		DBMaxConns:     int32(maxConns),
		RedisURL:       envOr("REDIS_URL", "redis://localhost:6379"),
		JWTSecret:      envOr("JWT_SECRET", ""),
		JWTTTL:         ttl,
	}

	if cfg.JWTSecret == "" {
		return nil, fmt.Errorf("JWT_SECRET is required")
	}

	return cfg, nil
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}
