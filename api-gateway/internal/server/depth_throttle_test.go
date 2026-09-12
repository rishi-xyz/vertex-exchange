package server

import (
	"sync/atomic"
	"testing"
	"time"
)

func TestDepthThrottleFiresImmediatelyWhenIdle(t *testing.T) {
	var calls int64
	dt := newDepthThrottler(50*time.Millisecond, func(pair string) { atomic.AddInt64(&calls, 1) })

	dt.Request("ETH-USDC")
	time.Sleep(5 * time.Millisecond)
	if got := atomic.LoadInt64(&calls); got != 1 {
		t.Errorf("calls = %d, want 1 (leading-edge fire on an idle pair)", got)
	}
}

func TestDepthThrottleCoalescesBurst(t *testing.T) {
	var calls int64
	dt := newDepthThrottler(50*time.Millisecond, func(pair string) { atomic.AddInt64(&calls, 1) })

	for i := 0; i < 20; i++ {
		dt.Request("ETH-USDC")
	}
	time.Sleep(5 * time.Millisecond)
	if got := atomic.LoadInt64(&calls); got != 1 {
		t.Errorf("calls after burst = %d, want 1 (leading edge only; rest coalesced)", got)
	}

	time.Sleep(70 * time.Millisecond)
	if got := atomic.LoadInt64(&calls); got != 2 {
		t.Errorf("calls after window = %d, want 2 (one trailing-edge fire for the coalesced burst)", got)
	}
}

func TestDepthThrottleNoTrailingFireWithoutFollowupRequest(t *testing.T) {
	var calls int64
	dt := newDepthThrottler(20*time.Millisecond, func(pair string) { atomic.AddInt64(&calls, 1) })

	dt.Request("ETH-USDC")
	time.Sleep(50 * time.Millisecond)
	if got := atomic.LoadInt64(&calls); got != 1 {
		t.Errorf("calls = %d, want 1 (no trailing fire when nothing arrived during the window)", got)
	}
}

func TestDepthThrottlePairsAreIndependent(t *testing.T) {
	var calls int64
	dt := newDepthThrottler(50*time.Millisecond, func(pair string) { atomic.AddInt64(&calls, 1) })

	dt.Request("ETH-USDC")
	dt.Request("BTC-USDC")
	time.Sleep(5 * time.Millisecond)
	if got := atomic.LoadInt64(&calls); got != 2 {
		t.Errorf("calls = %d, want 2 (each pair gets its own leading-edge fire)", got)
	}
}
