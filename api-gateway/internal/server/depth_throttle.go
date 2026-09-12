package server

import (
	"sync"
	"time"
)

// depthThrottler coalesces bursts of depth-refresh requests for the same
// pair into a leading-edge-then-trailing-edge pair of actual calls, instead
// of one gRPC GetOrderBook round trip per order/cancel/modify/fill. Before
// this, every one of those events queued its own GetOrderBook call into the
// engine's single serialized command queue — the same queue every order
// placement goes through — so under concurrent load, depth calls piled up
// behind placement traffic and delayed whichever fill notification happened
// to be waiting behind them (fills are notified sequentially within a
// batch; see internal/fills.Consumer.Run).
//
// Semantics per pair: the first request after an idle period fires
// immediately (so a quiet pair still feels instant). Any further requests
// within `interval` are coalesced into a single trailing-edge fire at the
// end of the window, capturing the latest state without one call per event.
type depthThrottler struct {
	mu       sync.Mutex
	states   map[string]*pairDepthState
	interval time.Duration
	fn       func(pair string)
}

type pairDepthState struct {
	lastFired time.Time
	timer     *time.Timer
	dirty     bool
}

func newDepthThrottler(interval time.Duration, fn func(pair string)) *depthThrottler {
	return &depthThrottler{
		states:   make(map[string]*pairDepthState),
		interval: interval,
		fn:       fn,
	}
}

// Request marks pair as needing a depth refresh. Non-blocking: the actual
// work always happens on its own goroutine.
func (t *depthThrottler) Request(pair string) {
	t.mu.Lock()
	defer t.mu.Unlock()

	st, ok := t.states[pair]
	if !ok {
		st = &pairDepthState{}
		t.states[pair] = st
	}

	now := time.Now()
	if st.timer == nil && now.Sub(st.lastFired) >= t.interval {
		st.lastFired = now
		go t.fn(pair)
		return
	}

	st.dirty = true
	if st.timer == nil {
		remaining := t.interval - now.Sub(st.lastFired)
		if remaining < 0 {
			remaining = 0
		}
		st.timer = time.AfterFunc(remaining, func() {
			t.mu.Lock()
			st.timer = nil
			fire := st.dirty
			st.dirty = false
			st.lastFired = time.Now()
			t.mu.Unlock()
			if fire {
				t.fn(pair)
			}
		})
	}
}
