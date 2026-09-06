// Package ratelimit implements a small in-memory per-key token bucket, used
// to guard the gateway's gRPC-fronting endpoints from abusive callers.
package ratelimit

import (
	"sync"
	"time"
)

type bucket struct {
	tokens float64
	last   time.Time
}

// Limiter is a per-key token bucket: each key accrues `rate` tokens per
// second up to `burst`, and Allow consumes one token per call.
type Limiter struct {
	mu      sync.Mutex
	buckets map[string]*bucket
	rate    float64
	burst   float64
}

// New creates a Limiter and starts a background sweep that evicts buckets
// idle for more than 10 minutes, so the map does not grow unbounded.
func New(rate, burst float64) *Limiter {
	l := &Limiter{buckets: make(map[string]*bucket), rate: rate, burst: burst}
	go l.sweep()
	return l
}

// Allow reports whether the call under key should proceed, consuming a
// token if so.
func (l *Limiter) Allow(key string) bool {
	l.mu.Lock()
	defer l.mu.Unlock()

	now := time.Now()
	b, ok := l.buckets[key]
	if !ok {
		l.buckets[key] = &bucket{tokens: l.burst - 1, last: now}
		return true
	}

	elapsed := now.Sub(b.last).Seconds()
	b.tokens = min(l.burst, b.tokens+elapsed*l.rate)
	b.last = now
	if b.tokens < 1 {
		return false
	}
	b.tokens--
	return true
}

func (l *Limiter) sweep() {
	for range time.Tick(10 * time.Minute) {
		cutoff := time.Now().Add(-10 * time.Minute)
		l.mu.Lock()
		for k, b := range l.buckets {
			if b.last.Before(cutoff) {
				delete(l.buckets, k)
			}
		}
		l.mu.Unlock()
	}
}
