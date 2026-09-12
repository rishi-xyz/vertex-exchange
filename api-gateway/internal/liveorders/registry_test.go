package liveorders

import (
	"testing"

	"github.com/google/uuid"
)

func TestApplyFillPartialThenFull(t *testing.T) {
	r := New()
	orderID, userID := uuid.New(), uuid.New()
	r.Put(1, State{OrderID: orderID, UserID: userID, Pair: "ETH-USDC", Remaining: 10, Status: "Empty"})

	s, ok := r.ApplyFill(1, 4)
	if !ok {
		t.Fatal("expected order 1 to be tracked")
	}
	if s.Status != "PartiallyFilled" || s.Remaining != 6 {
		t.Errorf("after partial fill: status=%q remaining=%d, want PartiallyFilled, 6", s.Status, s.Remaining)
	}

	s, ok = r.ApplyFill(1, 6)
	if !ok {
		t.Fatal("expected order 1 to still be tracked")
	}
	if s.Status != "Filled" || s.Remaining != 0 {
		t.Errorf("after full fill: status=%q remaining=%d, want Filled, 0", s.Status, s.Remaining)
	}
}

func TestApplyFillOverfillStaysFilled(t *testing.T) {
	r := New()
	r.Put(1, State{Remaining: 5, Status: "Empty"})
	s, ok := r.ApplyFill(1, 8) // more than remaining, e.g. a rounding/race edge case
	if !ok {
		t.Fatal("expected order 1 to be tracked")
	}
	if s.Status != "Filled" {
		t.Errorf("status = %q, want Filled even when overfilled", s.Status)
	}
	if s.Remaining != -3 {
		t.Errorf("remaining = %d, want -3 (not clamped, matches the SQL side's behavior)", s.Remaining)
	}
}

func TestApplyFillUntrackedOrder(t *testing.T) {
	r := New()
	if _, ok := r.ApplyFill(999, 1); ok {
		t.Error("expected untracked order to return ok=false")
	}
}

func TestDeleteRemovesEntry(t *testing.T) {
	r := New()
	r.Put(1, State{Remaining: 10, Status: "Empty"})
	r.Delete(1)
	if _, ok := r.ApplyFill(1, 1); ok {
		t.Error("expected deleted order to be untracked")
	}
}

func TestPutOverwritesExistingEntry(t *testing.T) {
	r := New()
	r.Put(1, State{Remaining: 10, Status: "Empty"})
	r.ApplyFill(1, 10) // now Filled, remaining 0
	// Simulates a modify (cancel-replace): same engine_order_id, fresh state.
	r.Put(1, State{Remaining: 3, Status: "Empty"})
	s, ok := r.ApplyFill(1, 1)
	if !ok {
		t.Fatal("expected order 1 to be tracked after re-Put")
	}
	if s.Status != "PartiallyFilled" || s.Remaining != 2 {
		t.Errorf("after re-Put + fill: status=%q remaining=%d, want PartiallyFilled, 2", s.Status, s.Remaining)
	}
}
