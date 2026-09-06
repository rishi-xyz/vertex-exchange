package main

import (
	"context"
	"fmt"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/auth"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/grpcclient"
)

type setupConfig struct {
	databaseURL      string
	engineAddr       string
	jwtSecret        string
	jwtTTL           time.Duration
	users            int
	pair             *engine.TradingPair
	pairStr          string
	depositBase      uint32
	depositQuote     uint32
	setupConcurrency int
}

// setupWorkers provisions cfg.users accounts entirely by direct Postgres
// insert and engine gRPC calls, minting JWTs in-process — no REST call is
// ever made, so the gateway's auth rate limiter never sees this traffic. A
// per-user failure is logged and drops that slot rather than aborting the
// whole run.
func setupWorkers(ctx context.Context, cfg setupConfig) ([]*worker, *grpcclient.Client, *pgxpool.Pool, error) {
	// A modest pool: setup is a short burst of inserts bounded by
	// setupConcurrency, not sustained load.
	pool, err := db.Open(ctx, cfg.databaseURL, 10)
	if err != nil {
		return nil, nil, nil, fmt.Errorf("connect postgres: %w", err)
	}
	store := db.NewStore(pool)

	engineClient, err := grpcclient.New(ctx, cfg.engineAddr)
	if err != nil {
		pool.Close()
		return nil, nil, nil, fmt.Errorf("connect engine: %w", err)
	}

	// Idempotent on the engine side (adding an already-existing pair is a
	// harmless no-op there), so a repeat run doesn't need `make seed` to
	// have created the pair first — errors here aren't worth failing setup
	// over since order placement will surface a clearer one if it matters.
	_, _ = engineClient.Engine.AddTradingPair(ctx, &engine.AddTradingPairRequest{Pair: cfg.pair})

	// bcrypt is deliberately called exactly once: login is never exercised
	// by this tool (tokens are minted in-process), so every benchmark
	// account safely shares one fixed password hash.
	hash, err := auth.HashPassword("benchmark-password-unused")
	if err != nil {
		engineClient.Close()
		pool.Close()
		return nil, nil, nil, fmt.Errorf("hash password: %w", err)
	}
	authMgr := auth.NewManager(cfg.jwtSecret, cfg.jwtTTL)
	runID := time.Now().UnixNano()

	workers := make([]*worker, cfg.users)
	var wg sync.WaitGroup
	sem := make(chan struct{}, cfg.setupConcurrency)
	var failedMu sync.Mutex
	var failed int

	for i := 0; i < cfg.users; i++ {
		wg.Add(1)
		sem <- struct{}{}
		go func(i int) {
			defer wg.Done()
			defer func() { <-sem }()

			w, err := provisionUser(ctx, store, engineClient, authMgr, hash, cfg, runID, i)
			if err != nil {
				failedMu.Lock()
				failed++
				failedMu.Unlock()
				return
			}
			workers[i] = w
		}(i)
	}
	wg.Wait()

	// Compact out failed slots.
	out := workers[:0]
	for _, w := range workers {
		if w != nil {
			out = append(out, w)
		}
	}
	return out, engineClient, pool, nil
}

func provisionUser(ctx context.Context, store *db.Store, engineClient *grpcclient.Client, authMgr *auth.Manager,
	passwordHash string, cfg setupConfig, runID int64, i int) (*worker, error) {
	engineUID := uuid.NewString()
	if _, err := engineClient.Engine.AddUser(ctx, &engine.AddUserRequest{UserId: engineUID}); err != nil {
		return nil, fmt.Errorf("add user: %w", err)
	}

	email := fmt.Sprintf("bench-%d-%d@vertex.local", runID, i)
	gatewayUserID, err := store.CreateUser(ctx, email, passwordHash, engineUID)
	if err != nil {
		return nil, fmt.Errorf("create user row: %w", err)
	}

	if _, err := engineClient.Engine.DepositBalance(ctx, &engine.DepositBalanceRequest{
		UserId: engineUID, Asset: cfg.pair.Quote, Quantity: cfg.depositQuote,
	}); err != nil {
		return nil, fmt.Errorf("deposit quote asset: %w", err)
	}
	if _, err := engineClient.Engine.DepositBalance(ctx, &engine.DepositBalanceRequest{
		UserId: engineUID, Asset: cfg.pair.Base, Quantity: cfg.depositBase,
	}); err != nil {
		return nil, fmt.Errorf("deposit base asset: %w", err)
	}

	token, _, err := authMgr.Issue(gatewayUserID.String(), engineUID)
	if err != nil {
		return nil, fmt.Errorf("issue token: %w", err)
	}

	return &worker{
		id:      i,
		token:   token,
		pending: make(map[string]*orderRecord),
	}, nil
}
