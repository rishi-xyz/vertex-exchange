package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math/rand"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/gorilla/websocket"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/grpcclient"
)

// errorTracker buckets non-success order placements by cause, so a spike in
// "errored" is diagnosable instead of an opaque count. Shared across all
// workers for one run.
type errorTracker struct {
	mu     sync.Mutex
	counts map[string]int64
}

func newErrorTracker() *errorTracker { return &errorTracker{counts: make(map[string]int64)} }

const maxErrorBuckets = 20

func (t *errorTracker) record(status int, err error) {
	key := errorKey(status, err)
	t.mu.Lock()
	defer t.mu.Unlock()
	if _, exists := t.counts[key]; !exists && len(t.counts) >= maxErrorBuckets {
		key = "other (bucket limit reached)"
	}
	t.counts[key]++
}

func (t *errorTracker) snapshot() map[string]int64 {
	t.mu.Lock()
	defer t.mu.Unlock()
	out := make(map[string]int64, len(t.counts))
	for k, v := range t.counts {
		out[k] = v
	}
	return out
}

func errorKey(status int, err error) string {
	if err == nil {
		return fmt.Sprintf("http %d", status)
	}
	msg := err.Error()
	switch {
	case strings.Contains(msg, "Client.Timeout") || strings.Contains(msg, "context deadline exceeded"):
		return "client timeout"
	case strings.Contains(msg, "connection refused"):
		return "connection refused"
	case strings.Contains(msg, "connection reset"):
		return "connection reset"
	case strings.Contains(msg, "too many open files"):
		return "too many open files (fd exhaustion)"
	case strings.Contains(msg, "EOF"):
		return "EOF (connection closed mid-request)"
	default:
		return "other transport error: " + msg
	}
}

var httpClient = &http.Client{
	Timeout: 10 * time.Second,
	Transport: &http.Transport{
		MaxIdleConnsPerHost: 256,
		IdleConnTimeout:     90 * time.Second,
	},
}

// orderRecord tracks one placed order from submission to its first
// observed fill (if any), for latency measurement and end-of-run cleanup.
type orderRecord struct {
	submitTime    time.Time
	engineOrderID string
	firstFillAt   time.Time // zero until a fill notification arrives
	terminal      bool      // Filled or Cancelled seen over WS
}

// worker simulates one exchange user: a REST placer loop and a WebSocket
// reader loop that correlates fill notifications back to orders this same
// worker placed. All mutable state here is either touched by exactly one
// goroutine (placed/ok/throttled/errored/placementLatencies, by the placer;
// wsReadErr, by the reader) or guarded by mu (pending, fillLatencies, both
// read and written by both goroutines).
type worker struct {
	id       int
	token    string
	excluded bool // set if this worker's WS connection failed; skipped in the run

	conn   *websocket.Conn
	closed atomic.Bool // set by closeWS before closing, so readLoop can tell a deliberate close from a real failure

	mu            sync.Mutex
	pending       map[string]*orderRecord
	fillLatencies []time.Duration

	placed, ok, throttled, errored int64
	placementLatencies             []time.Duration

	wsReadErr bool
}

func (w *worker) connectWS(baseURL string) error {
	u, err := url.Parse(baseURL)
	if err != nil {
		return fmt.Errorf("parse base url: %w", err)
	}
	switch u.Scheme {
	case "https":
		u.Scheme = "wss"
	default:
		u.Scheme = "ws"
	}
	u.Path = "/ws"
	// Deliberately no ?pair= here: a bare connection subscribes only to this
	// user's personal "user:<id>" channel (see server/ws.go handleWS). Also
	// subscribing to the pair channel would additionally deliver every
	// depth/trade/ticker broadcast from every other simulated user's
	// activity on that pair to every connection, multiplying per-connection
	// volume by -users and overflowing the hub's 32-message non-blocking
	// send buffer (ws/hub.go), silently disconnecting most workers early
	// and corrupting the fill-rate/fill-latency numbers.
	u.RawQuery = ""

	header := http.Header{"Authorization": {"Bearer " + w.token}}
	conn, _, err := websocket.DefaultDialer.Dial(u.String(), header)
	if err != nil {
		return err
	}
	w.conn = conn
	return nil
}

func (w *worker) closeWS() {
	w.closed.Store(true)
	if w.conn != nil {
		w.conn.Close()
	}
}

type wsOrderMessage struct {
	Type  string `json:"type"`
	Order struct {
		ID        string `json:"id"`
		Status    string `json:"status"`
		Remaining int64  `json:"remaining"`
	} `json:"order"`
}

func (w *worker) readLoop() {
	for {
		_, data, err := w.conn.ReadMessage()
		if err != nil {
			if !w.closed.Load() {
				w.wsReadErr = true
			}
			return
		}
		var msg wsOrderMessage
		if err := json.Unmarshal(data, &msg); err != nil || msg.Type != "order" {
			continue
		}
		w.mu.Lock()
		rec, ok := w.pending[msg.Order.ID]
		if ok {
			if (msg.Order.Status == "Filled" || msg.Order.Status == "PartiallyFilled") && rec.firstFillAt.IsZero() {
				rec.firstFillAt = time.Now()
				w.fillLatencies = append(w.fillLatencies, rec.firstFillAt.Sub(rec.submitTime))
			}
			if msg.Order.Status == "Filled" || msg.Order.Status == "Cancelled" {
				rec.terminal = true
			}
		}
		w.mu.Unlock()
	}
}

// randomOrder draws both sides' prices from the same distribution around
// midPrice, so crosses happen naturally as the book's spread narrows over
// the run rather than needing a hand-tuned crossing probability: a buy at or
// above the current best ask crosses immediately, as does a sell at or below
// the current best bid.
func randomOrder(rnd *rand.Rand, midPrice int32, jitterBps int, minQty, maxQty uint32) (side string, price int32, qty uint32) {
	jitter := int32(int64(midPrice) * int64(jitterBps) / 10000)
	if jitter < 1 {
		jitter = 1
	}
	price = midPrice + rnd.Int31n(2*jitter+1) - jitter
	if price < 1 {
		price = 1
	}
	if rnd.Intn(2) == 0 {
		side = "buy"
	} else {
		side = "sell"
	}
	qty = minQty
	if maxQty > minQty {
		qty += uint32(rnd.Intn(int(maxQty - minQty + 1)))
	}
	return side, price, qty
}

func (w *worker) placeLoop(ctx context.Context, rc runConfig) {
	rnd := rand.New(rand.NewSource(rc.seed + int64(w.id)))
	count := 0
	for {
		if ctx.Err() != nil {
			return
		}
		if rc.orderCount > 0 && count >= rc.orderCount {
			return
		}
		count++

		side, price, qty := randomOrder(rnd, rc.midPrice, rc.jitterBps, rc.minQty, rc.maxQty)
		t0 := time.Now()
		orderID, engineOrderID, status, err := submitOrder(ctx, rc.baseURL, w.token, rc.pairStr, side, price, qty)
		w.placed++
		switch {
		case status == http.StatusTooManyRequests:
			w.throttled++
			backoff := 100*time.Millisecond + time.Duration(rnd.Intn(50))*time.Millisecond
			select {
			case <-time.After(backoff):
			case <-ctx.Done():
				return
			}
		case err != nil || status != http.StatusCreated:
			w.errored++
			rc.errTracker.record(status, err)
		default:
			w.ok++
			w.placementLatencies = append(w.placementLatencies, time.Since(t0))
			w.mu.Lock()
			w.pending[orderID] = &orderRecord{submitTime: t0, engineOrderID: engineOrderID}
			w.mu.Unlock()
		}
	}
}

// cleanup cancels every order this worker placed that never reached a
// terminal state, via direct engine gRPC (bypassing the gateway/rate
// limiter, matching how setup bypasses REST), so repeated benchmark runs
// don't leave the book growing unbounded. Best-effort: an order that filled
// or was already cancelled between the read loop's last update and now
// simply fails to cancel and is ignored.
func (w *worker) cleanup(ctx context.Context, engineClient *grpcclient.Client, pair *engine.TradingPair) int {
	w.mu.Lock()
	var ids []uint64
	for _, rec := range w.pending {
		if rec.terminal {
			continue
		}
		id, err := strconv.ParseUint(rec.engineOrderID, 10, 64)
		if err == nil {
			ids = append(ids, id)
		}
	}
	w.mu.Unlock()

	for _, id := range ids {
		engineClient.Users.CancelOrder(ctx, &engine.CancelOrderRequest{Pair: pair, OrderId: id})
	}
	return len(ids)
}

// drainAndClose fully reads and closes resp.Body. Go's transport can only
// return a connection to the keep-alive pool if the body was read to EOF
// before Close; closing early (as this tool originally did on every
// non-201/non-200 response) forces a brand new TCP connection — and thus a
// new local ephemeral port — for every single throttled/error response.
// Under sustained load with tens of thousands of 429s, that reliably
// exhausts the local ephemeral port range within its ~60s TIME_WAIT window,
// surfacing as "dial tcp ...: cannot assign requested address" — a
// benchmark-client artifact, not a server-side failure.
func drainAndClose(resp *http.Response) {
	io.Copy(io.Discard, resp.Body)
	resp.Body.Close()
}

// checkAuth performs a side-effect-free authenticated GET, returning the
// HTTP status so callers can distinguish a valid token (200) from an
// invalid one (401) without placing any state-changing request.
func checkAuth(ctx context.Context, baseURL, token string) (int, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, strings.TrimRight(baseURL, "/")+"/balances", nil)
	if err != nil {
		return 0, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	resp, err := httpClient.Do(req)
	if err != nil {
		return 0, err
	}
	defer drainAndClose(resp)
	return resp.StatusCode, nil
}

// submitOrder places one GTC order and classifies the response. status is
// always returned (even on a body-decode failure, as 0) so callers can
// distinguish "rate limited" from "everything else" without inspecting err.
func submitOrder(ctx context.Context, baseURL, token, pairStr, side string, price int32, qty uint32) (orderID, engineOrderID string, status int, err error) {
	body, _ := json.Marshal(map[string]any{
		"pair": pairStr, "side": side, "type": "gtc", "price": price, "quantity": qty,
	})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(baseURL, "/")+"/orders", bytes.NewReader(body))
	if err != nil {
		return "", "", 0, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := httpClient.Do(req)
	if err != nil {
		return "", "", 0, err
	}
	defer drainAndClose(resp)

	if resp.StatusCode != http.StatusCreated {
		return "", "", resp.StatusCode, nil
	}
	var out struct {
		Order struct {
			ID            string `json:"id"`
			EngineOrderID string `json:"engine_order_id"`
		} `json:"order"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return "", "", resp.StatusCode, err
	}
	return out.Order.ID, out.Order.EngineOrderID, resp.StatusCode, nil
}
