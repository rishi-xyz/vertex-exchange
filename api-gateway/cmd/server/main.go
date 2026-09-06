package main

import (
	"context"
	"log"
	"net/http"
	"os/signal"
	"syscall"
	"time"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/balancecache"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/config"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/fills"
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

	store := db.NewStore(pool)

	balCache, err := balancecache.New(cfg.RedisURL, 2*time.Second)
	if err != nil {
		log.Fatalf("balance cache: %v", err)
	}
	defer balCache.Close()

	srv := server.New(cfg, engine, store, balCache)

	consumer, err := fills.New(cfg.RedisURL, store, srv.OnFill)
	if err != nil {
		log.Fatalf("fills consumer: %v", err)
	}
	defer consumer.Close()
	go consumer.Run(ctx)
	log.Printf("fills consumer started")

	httpSrv := &http.Server{
		Addr:    ":" + cfg.GatewayPort,
		Handler: srv.Router(),
	}

	go func() {
		log.Printf("gateway listening on :%s", cfg.GatewayPort)
		if err := httpSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("http server: %v", err)
		}
	}()

	<-ctx.Done()
	log.Println("shutting down")
	httpSrv.Shutdown(context.Background())
}
