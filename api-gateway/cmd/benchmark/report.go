package main

import (
	"fmt"
	"sort"
	"time"
)

type report struct {
	users, usersFailedSetup, wsConnected, wsDisconnected int
	pairStr                                              string
	midPrice                                             int32
	jitterBps                                            int
	minQty, maxQty                                       uint32

	placed, ok, throttled, errored int64
	placementLatencies             []time.Duration

	filled        int64
	fillLatencies []time.Duration

	cancelled int

	runElapsed, gracePeriod, cleanupElapsed time.Duration
}

func buildReport(workers []*worker, rc runConfig, failedSetup int, runElapsed, gracePeriod, cleanupElapsed time.Duration, cancelled int) *report {
	r := &report{
		users:            len(workers) + failedSetup,
		usersFailedSetup: failedSetup,
		pairStr:          rc.pairStr,
		midPrice:         rc.midPrice,
		jitterBps:        rc.jitterBps,
		minQty:           rc.minQty,
		maxQty:           rc.maxQty,
		runElapsed:       runElapsed,
		gracePeriod:      gracePeriod,
		cleanupElapsed:   cleanupElapsed,
		cancelled:        cancelled,
	}
	for _, w := range workers {
		if w.excluded {
			r.wsDisconnected++
			continue
		}
		if w.wsReadErr {
			r.wsDisconnected++
		} else {
			r.wsConnected++
		}
		r.placed += w.placed
		r.ok += w.ok
		r.throttled += w.throttled
		r.errored += w.errored
		r.placementLatencies = append(r.placementLatencies, w.placementLatencies...)

		w.mu.Lock()
		r.fillLatencies = append(r.fillLatencies, w.fillLatencies...)
		w.mu.Unlock()
	}
	r.filled = int64(len(r.fillLatencies))
	sort.Slice(r.placementLatencies, func(i, j int) bool { return r.placementLatencies[i] < r.placementLatencies[j] })
	sort.Slice(r.fillLatencies, func(i, j int) bool { return r.fillLatencies[i] < r.fillLatencies[j] })
	return r
}

func percentile(sorted []time.Duration, p float64) time.Duration {
	if len(sorted) == 0 {
		return 0
	}
	idx := int(p * float64(len(sorted)-1))
	return sorted[idx]
}

func formatLatencies(sorted []time.Duration) string {
	if len(sorted) == 0 {
		return "n/a (no samples)"
	}
	return fmt.Sprintf("p50=%s  p95=%s  p99=%s  max=%s",
		percentile(sorted, 0.50), percentile(sorted, 0.95), percentile(sorted, 0.99), sorted[len(sorted)-1])
}

func (r *report) Print() {
	fmt.Println("=== Vertex Benchmark ===")
	fmt.Printf("users:        %d requested (%d provisioned, %d setup failures)   pair: %s\n",
		r.users, r.users-r.usersFailedSetup, r.usersFailedSetup, r.pairStr)
	fmt.Printf("mid-price:    %d   jitter: ±%.2f%% (bps=%d)   qty: %d-%d\n",
		r.midPrice, float64(r.jitterBps)/100, r.jitterBps, r.minQty, r.maxQty)
	fmt.Println()

	fmt.Printf("orders:       %d total  (%d ok, %d throttled/429, %d errored)\n", r.placed, r.ok, r.throttled, r.errored)
	if r.runElapsed > 0 {
		fmt.Printf("throughput:   %.1f orders/sec placed  (%.1f ok/sec)\n",
			float64(r.placed)/r.runElapsed.Seconds(), float64(r.ok)/r.runElapsed.Seconds())
	}
	fmt.Printf("placement latency (ok orders only):\n              %s\n", formatLatencies(r.placementLatencies))
	fmt.Println()

	fillRate := 0.0
	if r.ok > 0 {
		fillRate = 100 * float64(r.filled) / float64(r.ok)
	}
	fmt.Printf("fills:        %d / %d ok orders received >=1 fill within the grace period (%.1f%% fill rate)\n", r.filled, r.ok, fillRate)
	fmt.Printf("fill latency (REST submit -> WS fill notification, filled orders only):\n              %s\n", formatLatencies(r.fillLatencies))
	fmt.Println()

	fmt.Printf("ws:           %d/%d connections stayed open for the whole run (%d disconnected/failed)\n",
		r.wsConnected, r.wsConnected+r.wsDisconnected, r.wsDisconnected)
	fmt.Println()

	fmt.Printf("cleanup:      cancelled %d resting orders in %s\n", r.cancelled, r.cleanupElapsed.Round(time.Millisecond))
	total := r.runElapsed + r.gracePeriod + r.cleanupElapsed
	fmt.Printf("wall time:    %s run + %s grace + %s cleanup = %s total\n",
		r.runElapsed.Round(time.Millisecond), r.gracePeriod.Round(time.Millisecond), r.cleanupElapsed.Round(time.Millisecond), total.Round(time.Millisecond))
}
