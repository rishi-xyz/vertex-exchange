package ratelimit

import (
	"testing"
	"time"
)

func TestAllowBurstThenThrottle(t *testing.T) {
	l := New(1, 3) // 1 token/sec, burst of 3
	for i := 0; i < 3; i++ {
		if !l.Allow("k") {
			t.Fatalf("call %d: expected allowed within burst", i)
		}
	}
	if l.Allow("k") {
		t.Fatal("expected 4th call to be throttled")
	}
}

func TestAllowRefillsOverTime(t *testing.T) {
	l := New(1000, 1) // fast refill for a quick test
	if !l.Allow("k") {
		t.Fatal("expected first call to be allowed")
	}
	if l.Allow("k") {
		t.Fatal("expected immediate second call to be throttled")
	}
	time.Sleep(5 * time.Millisecond)
	if !l.Allow("k") {
		t.Fatal("expected call to be allowed after refill")
	}
}

func TestAllowKeysAreIndependent(t *testing.T) {
	l := New(1, 1)
	if !l.Allow("a") {
		t.Fatal("expected key a to be allowed")
	}
	if !l.Allow("b") {
		t.Fatal("expected key b to be allowed independently of key a")
	}
}
