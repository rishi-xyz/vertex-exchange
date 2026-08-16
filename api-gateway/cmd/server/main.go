package main

import (
	"context"
	"log"
	"net/http"
	"os/signal"
	"syscall"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/config"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/grpcclient"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/server"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("config: %v", err)
	}

	pool, err := db.Open(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("database: %v", err)
	}
	defer pool.Close()
	log.Printf("connected to postgres")

	engine, err := grpcclient.New(ctx, cfg.EngineGRPCAddr)
	if err != nil {
		log.Fatalf("connect engine: %v", err)
	}
	defer engine.Close()
	log.Printf("connected to engine at %s", cfg.EngineGRPCAddr)

	srv := &http.Server{
		Addr:    ":" + cfg.GatewayPort,
		Handler: server.New(cfg, engine, db.NewStore(pool)).Router(),
	}

	go func() {
		log.Printf("gateway listening on :%s", cfg.GatewayPort)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("http server: %v", err)
		}
	}()

	<-ctx.Done()
	log.Println("shutting down")
	srv.Shutdown(context.Background())
}
