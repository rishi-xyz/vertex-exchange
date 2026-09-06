// benchmark is a full-stack load/scoring tool: it answers "how many users
// and how much load can the whole exchange handle, and how well" by driving
// real traffic through the deployed REST/gRPC/Postgres/Redis/WebSocket
// stack.
//
// Account setup (registration, funding) bypasses REST entirely — it mints
// JWTs in-process and provisions accounts via direct engine gRPC calls and a
// direct Postgres insert — since the gateway's per-IP auth rate limiter
// would make provisioning many simulated users from one process impractical.
// Order placement is NOT bypassed: it goes through the real REST endpoint
// and is subject to the real per-user order rate limiter, so scaling
// -users up to push aggregate throughput higher is itself a legitimate way
// to explore "how many users can this handle".
package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
)

type runConfig struct {
	baseURL    string
	pair       *engine.TradingPair
	pairStr    string
	midPrice   int32
	jitterBps  int
	minQty     uint32
	maxQty     uint32
	seed       int64
	duration   time.Duration
	orderCount int // per-worker fixed count; 0 means duration-based
	errTracker *errorTracker
}

func main() {
	baseURL := flag.String("base-url", "http://localhost:8080", "gateway base URL (REST + WS)")
	engineAddr := flag.String("engine-addr", "localhost:5000", "engine gRPC address, for direct account setup/cleanup")
	databaseURL := flag.String("database-url", "postgres://vertex:vertex@localhost:5432/vertex?sslmode=disable", "gateway's Postgres URL, for direct account setup")
	jwtSecret := flag.String("jwt-secret", "", "MUST match the target gateway's JWT_SECRET (required)")
	jwtTTL := flag.Duration("jwt-ttl", time.Hour, "lifetime of minted tokens; must exceed duration+fill-grace-period+cleanup")
	pairStr := flag.String("pair", "ETH-USDC", "trading pair to load")
	users := flag.Int("users", 50, "number of simulated users (each is one goroutine pair + one WS connection)")
	duration := flag.Duration("duration", 30*time.Second, "how long to place orders for (ignored if -orders-per-user > 0)")
	ordersPerUser := flag.Int("orders-per-user", 0, "if > 0, each user places exactly this many orders instead of running for -duration")
	midPrice := flag.Int("mid-price", 10000, "shared mid-price both sides jitter around")
	jitterBps := flag.Int("jitter-bps", 500, "half-width of the price distribution around mid-price, in basis points")
	minQty := flag.Int("min-qty", 1, "minimum order quantity")
	maxQty := flag.Int("max-qty", 1, "maximum order quantity")
	// Sizing: worst case a user spends is roughly
	// duration_seconds * 5 orders/sec * (mid_price+jitter) * max_qty (quote
	// side) or * max_qty (base side) — the defaults below have generous
	// headroom over the tool's own defaults; scale them up if you raise
	// -duration, -mid-price, -max-qty, or -orders-per-user substantially.
	depositQuote := flag.Int("deposit-quote", 1_000_000_000, "starting balance in the pair's quote asset, per user")
	depositBase := flag.Int("deposit-base", 1_000_000_000, "starting balance in the pair's base asset, per user")
	fillGrace := flag.Duration("fill-grace-period", 5*time.Second, "how long to keep WS connections open after placing stops, to catch trailing fills")
	cleanup := flag.Bool("cleanup", true, "cancel each worker's still-resting orders (via direct engine gRPC) after the run")
	setupConcurrency := flag.Int("setup-concurrency", 20, "concurrent account-provisioning workers")
	seed := flag.Int64("seed", 0, "RNG seed; 0 picks one from the current time")
	progressInterval := flag.Duration("progress-interval", 5*time.Second, "how often to log a progress line; 0 disables")
	flag.Parse()

	if *jwtSecret == "" {
		log.Fatal("-jwt-secret is required and must match the target gateway's JWT_SECRET")
	}
	if *ordersPerUser > 0 && *duration != 30*time.Second {
		log.Fatal("-orders-per-user and -duration are mutually exclusive; pick one")
	}
	minGraceForTokens := *duration + *fillGrace + 10*time.Second
	if *ordersPerUser == 0 && *jwtTTL <= minGraceForTokens {
		log.Printf("warning: -jwt-ttl (%s) is not much longer than the run (duration+grace ~%s); tokens may expire mid-run", *jwtTTL, minGraceForTokens)
	}
	if *maxQty < *minQty {
		log.Fatal("-max-qty must be >= -min-qty")
	}

	pair, err := parsePair(*pairStr)
	if err != nil {
		log.Fatalf("-pair: %v", err)
	}
	rngSeed := *seed
	if rngSeed == 0 {
		rngSeed = time.Now().UnixNano()
	}

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	log.Printf("provisioning %d users (setup bypasses REST: direct Postgres insert + engine gRPC)", *users)
	sc := setupConfig{
		databaseURL:      *databaseURL,
		engineAddr:       *engineAddr,
		jwtSecret:        *jwtSecret,
		jwtTTL:           *jwtTTL,
		users:            *users,
		pair:             pair,
		pairStr:          *pairStr,
		depositBase:      uint32(*depositBase),
		depositQuote:     uint32(*depositQuote),
		setupConcurrency: *setupConcurrency,
	}
	workers, engineClient, pool, err := setupWorkers(ctx, sc)
	if err != nil {
		log.Fatalf("setup: %v", err)
	}
	defer engineClient.Close()
	defer pool.Close()

	failedSetup := *users - len(workers)
	if len(workers) == 0 {
		log.Fatal("every worker failed setup — nothing to run")
	}
	log.Printf("provisioned %d/%d users", len(workers), *users)

	if err := preflightCheck(ctx, *baseURL, workers[0]); err != nil {
		log.Fatalf("preflight check failed (likely a -jwt-secret mismatch with the target gateway): %v", err)
	}

	rc := runConfig{
		baseURL:    *baseURL,
		pair:       pair,
		pairStr:    *pairStr,
		midPrice:   int32(*midPrice),
		jitterBps:  *jitterBps,
		minQty:     uint32(*minQty),
		maxQty:     uint32(*maxQty),
		seed:       rngSeed,
		duration:   *duration,
		orderCount: *ordersPerUser,
		errTracker: newErrorTracker(),
	}

	for _, w := range workers {
		if err := w.connectWS(*baseURL); err != nil {
			log.Printf("worker %d: ws connect failed: %v (excluded from this run)", w.id, err)
			w.excluded = true
			continue
		}
		go w.readLoop()
	}

	var stopProgress chan struct{}
	if *progressInterval > 0 {
		stopProgress = make(chan struct{})
		go progressLoop(workers, *progressInterval, stopProgress)
	}

	log.Printf("placing orders on %s for %s...", *pairStr, describeRunLength(rc))
	runStart := time.Now()

	placeCtx := ctx
	var cancelPlace context.CancelFunc
	if rc.orderCount == 0 {
		placeCtx, cancelPlace = context.WithTimeout(ctx, rc.duration)
		defer cancelPlace()
	}

	var wg sync.WaitGroup
	for _, w := range workers {
		if w.excluded {
			continue
		}
		wg.Add(1)
		go func(w *worker) {
			defer wg.Done()
			w.placeLoop(placeCtx, rc)
		}(w)
	}
	wg.Wait()
	runElapsed := time.Since(runStart)

	if stopProgress != nil {
		close(stopProgress)
	}

	log.Printf("run complete, waiting %s grace period for trailing fills...", *fillGrace)
	select {
	case <-time.After(*fillGrace):
	case <-ctx.Done():
	}

	for _, w := range workers {
		if !w.excluded {
			w.closeWS()
		}
	}

	cleanupStart := time.Now()
	cancelled := 0
	if *cleanup {
		log.Printf("cleaning up resting orders...")
		for _, w := range workers {
			cancelled += w.cleanup(context.Background(), engineClient, pair)
		}
	}
	cleanupElapsed := time.Since(cleanupStart)

	r := buildReport(workers, rc, failedSetup, runElapsed, *fillGrace, cleanupElapsed, cancelled)
	fmt.Println()
	r.Print()
}

func describeRunLength(rc runConfig) string {
	if rc.orderCount > 0 {
		return fmt.Sprintf("%d orders/user", rc.orderCount)
	}
	return rc.duration.String()
}

func progressLoop(workers []*worker, interval time.Duration, stop chan struct{}) {
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-stop:
			return
		case <-ticker.C:
			var placed, ok, throttled int64
			for _, w := range workers {
				placed += w.placed
				ok += w.ok
				throttled += w.throttled
			}
			log.Printf("progress: %d placed, %d ok, %d throttled", placed, ok, throttled)
		}
	}
}

// preflightCheck hits a side-effect-free authenticated endpoint with the
// first worker's token to catch a -jwt-secret mismatch immediately, instead
// of discovering it only after the full run produced nothing but errors. It
// deliberately does not place an order: an order placed here wouldn't be
// tracked in any worker's pending map, so it would never be cancelled by
// cleanup and would leak into the book on every run.
func preflightCheck(ctx context.Context, baseURL string, w *worker) error {
	status, err := checkAuth(ctx, baseURL, w.token)
	if err != nil {
		return err
	}
	if status == 401 {
		return fmt.Errorf("balances check returned 401 unauthorized")
	}
	if status != 200 {
		return fmt.Errorf("balances check returned unexpected status %d", status)
	}
	return nil
}
