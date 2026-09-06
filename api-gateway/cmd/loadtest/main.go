// loadtest fires concurrent order placements at a running gateway and
// reports latency percentiles, as a Stage 5.2 sanity check that the
// single-threaded engine keeps up with modest V1 concurrency. It is not a
// stress test: every order is a unique-price GTC that never crosses, so it
// measures the REST -> gRPC -> Postgres round trip without exercising the
// matching/fills path.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"math/rand"
	"net/http"
	"os"
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

func main() {
	baseURL := flag.String("base-url", "http://localhost:8080", "gateway base URL")
	pair := flag.String("pair", "ETH-USDC", "trading pair to place orders on")
	concurrency := flag.Int("concurrency", 10, "number of concurrent workers (each gets its own account)")
	total := flag.Int("requests", 100, "total number of orders to place across all workers")
	flag.Parse()

	client := &http.Client{Timeout: 10 * time.Second}
	perWorker := *total / *concurrency
	actualTotal := perWorker * *concurrency
	if perWorker > 10 {
		log.Printf("warning: %d orders/worker exceeds the gateway's per-user rate-limit burst (10); "+
			"expect some 429s past the burst rather than a clean throughput signal", perWorker)
	}

	// One account per worker: the gateway rate-limits per user (Stage 3.3),
	// so a single shared account would measure the rate limiter, not engine
	// throughput. Many small accounts is also the more realistic shape for a
	// concurrency sanity check anyway.
	//
	// /auth/* is itself rate-limited per source IP, and every worker's setup
	// calls come from this one process's IP, so this phase is sequential and
	// retries through 429s rather than firing all registrations at once.
	log.Printf("registering %d worker accounts (paced by the gateway's per-IP auth rate limit)", *concurrency)
	tokens := make([]string, *concurrency)
	for w := 0; w < *concurrency; w++ {
		email := fmt.Sprintf("loadtest-%d-%d@example.com", time.Now().UnixNano(), w)
		password := "password123"
		userID := mustRegisterRetry(client, *baseURL, email, password)
		token := mustLoginRetry(client, *baseURL, email, password)
		mustDeposit(client, *baseURL, token, userID, "USDC", 1_000_000_000)
		tokens[w] = token
	}

	log.Printf("placing %d orders on %s across %d workers (%d each)", actualTotal, *pair, *concurrency, perWorker)

	var wg sync.WaitGroup
	var okCount, errCount int64
	latencies := make([]time.Duration, actualTotal)

	start := time.Now()
	for w := 0; w < *concurrency; w++ {
		wg.Add(1)
		go func(worker int) {
			defer wg.Done()
			rnd := rand.New(rand.NewSource(time.Now().UnixNano() + int64(worker)))
			token := tokens[worker]
			for j := 0; j < perWorker; j++ {
				i := worker*perWorker + j
				// A unique, wide-spread price per order keeps every order
				// resting (never crosses), isolating placement latency from
				// matching/fills.
				price := 1 + rnd.Intn(1_000_000)
				t0 := time.Now()
				err := placeOrder(client, *baseURL, token, *pair, price)
				latencies[i] = time.Since(t0)
				if err != nil {
					atomic.AddInt64(&errCount, 1)
				} else {
					atomic.AddInt64(&okCount, 1)
				}
			}
		}(w)
	}
	wg.Wait()
	elapsed := time.Since(start)

	sort.Slice(latencies, func(i, j int) bool { return latencies[i] < latencies[j] })
	pct := func(p float64) time.Duration {
		idx := int(p * float64(len(latencies)-1))
		return latencies[idx]
	}

	fmt.Println()
	fmt.Printf("orders:      %d ok, %d failed\n", okCount, errCount)
	fmt.Printf("throughput:  %.1f orders/sec (%s wall time)\n", float64(*total)/elapsed.Seconds(), elapsed)
	fmt.Printf("latency:     p50=%s  p95=%s  p99=%s  max=%s\n",
		pct(0.50), pct(0.95), pct(0.99), latencies[len(latencies)-1])

	if errCount > 0 {
		os.Exit(1)
	}
}

func mustRegisterRetry(client *http.Client, baseURL, email, password string) string {
	resp := doJSONRetry(client, "POST", baseURL+"/auth/register", "", map[string]any{"email": email, "password": password})
	user, ok := resp["user"].(map[string]any)
	if !ok {
		log.Fatalf("register: unexpected response: %v", resp)
	}
	id, _ := user["id"].(string)
	if id == "" {
		log.Fatalf("register: missing user id: %v", resp)
	}
	return id
}

func mustLoginRetry(client *http.Client, baseURL, email, password string) string {
	resp := doJSONRetry(client, "POST", baseURL+"/auth/login", "", map[string]any{"email": email, "password": password})
	token, _ := resp["token"].(string)
	if token == "" {
		log.Fatalf("login: missing token: %v", resp)
	}
	return token
}

// doJSONRetry retries on 429 (the gateway's per-IP auth rate limit) with a
// short backoff, up to 2 minutes total, so setup self-paces against
// whatever the server's limiter allows rather than assuming its constants.
func doJSONRetry(client *http.Client, method, url, token string, payload map[string]any) map[string]any {
	deadline := time.Now().Add(2 * time.Minute)
	for {
		out, status := tryJSON(client, method, url, token, payload)
		if status != http.StatusTooManyRequests {
			return out
		}
		if time.Now().After(deadline) {
			log.Fatalf("%s %s: still rate-limited after 2m of retries", method, url)
		}
		time.Sleep(time.Second)
	}
}

func mustDeposit(client *http.Client, baseURL, token, userID, asset string, quantity int) {
	doJSON(client, "POST", baseURL+"/users/"+userID+"/deposit", token, map[string]any{"asset": asset, "quantity": quantity})
}

func placeOrder(client *http.Client, baseURL, token, pair string, price int) error {
	body, _ := json.Marshal(map[string]any{
		"pair": pair, "side": "buy", "type": "gtc", "price": price, "quantity": 1,
	})
	req, err := http.NewRequest("POST", baseURL+"/orders", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)
	resp, err := client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusCreated {
		return fmt.Errorf("unexpected status %d", resp.StatusCode)
	}
	return nil
}

func doJSON(client *http.Client, method, url, token string, payload map[string]any) map[string]any {
	out, status := tryJSON(client, method, url, token, payload)
	if status >= 300 {
		log.Fatalf("%s %s: status %d: %v", method, url, status, out)
	}
	return out
}

// tryJSON performs one request and returns the decoded body and status,
// without treating a non-2xx as fatal (the caller decides).
func tryJSON(client *http.Client, method, url, token string, payload map[string]any) (map[string]any, int) {
	body, _ := json.Marshal(payload)
	req, err := http.NewRequest(method, url, bytes.NewReader(body))
	if err != nil {
		log.Fatalf("%s %s: %v", method, url, err)
	}
	req.Header.Set("Content-Type", "application/json")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := client.Do(req)
	if err != nil {
		log.Fatalf("%s %s: %v", method, url, err)
	}
	defer resp.Body.Close()
	var out map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		log.Fatalf("%s %s: decode response: %v", method, url, err)
	}
	return out, resp.StatusCode
}
