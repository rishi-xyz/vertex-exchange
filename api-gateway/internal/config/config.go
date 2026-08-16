package config

import (
	"fmt"
	"os"
	"time"
)

type Config struct {
	GatewayPort    string
	EngineGRPCAddr string
	DatabaseURL    string
	RedisURL       string
	JWTSecret      string
	JWTTTL         time.Duration
}

func Load() (*Config, error) {
	ttl, err := time.ParseDuration(envOr("JWT_TTL", "24h"))
	if err != nil {
		return nil, fmt.Errorf("parse JWT_TTL: %w", err)
	}

	cfg := &Config{
		GatewayPort:    envOr("GATEWAY_PORT", "8080"),
		EngineGRPCAddr: envOr("ENGINE_GRPC_ADDR", "localhost:5000"),
		DatabaseURL:    envOr("DATABASE_URL", "postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable"),
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
