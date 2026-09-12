// Package liveorders is the gateway's hot-path view of open order state,
// used to notify WebSocket clients of fills instantly without waiting on a
// Postgres round trip. Postgres remains the durable source of truth for
// history/audit; this registry is a synchronous, in-memory shadow of the
// "remaining/status" fields of currently-open orders, updated by the same
// handlers that write to Postgres (see internal/server/orders.go) and by
// the fills consumer as fills land.
//
// V1-scoped limitation: this registry is per-process and in-memory. It is
// correct for the current single-gateway-instance deployment. If the
// gateway is ever horizontally scaled, this needs to move to a shared store
// (e.g. Redis) — every replica would otherwise only know about orders
// placed through itself.
package liveorders

import (
	"sync"

	"github.com/google/uuid"
)

// State is the hot-path view of one open order.
type State struct {
	OrderID   uuid.UUID
	UserID    uuid.UUID
	Pair      string
	Remaining int64
	Status    string
}

// Registry maps engine_order_id (globally unique snowflake IDs, so no need
// to additionally key by pair) to the current State.
type Registry struct {
	mu     sync.RWMutex
	states map[int64]State
}

func New() *Registry {
	return &Registry{states: make(map[int64]State)}
}

// Put (re)sets the live state for an order — called on submit and on a
// successful modify (which keeps the same engine_order_id via cancel-replace
// but resets remaining/status to a fresh resting order).
func (r *Registry) Put(engineOrderID int64, s State) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.states[engineOrderID] = s
}

// Delete removes an order once no further updates are expected for it
// (cancelled, or fully filled).
func (r *Registry) Delete(engineOrderID int64) {
	r.mu.Lock()
	defer r.mu.Unlock()
	delete(r.states, engineOrderID)
}

// ApplyFill decrements remaining by quantity and advances status, mirroring
// the same CASE logic as the Postgres side (internal/db/fills.go) so the two
// stay in agreement. ok is false if the order isn't tracked (already
// evicted, or the registry hasn't been hydrated with it yet).
func (r *Registry) ApplyFill(engineOrderID, quantity int64) (State, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	s, ok := r.states[engineOrderID]
	if !ok {
		return State{}, false
	}
	s.Remaining -= quantity
	switch {
	case s.Remaining <= 0:
		s.Status = "Filled"
	case s.Status == "Empty":
		s.Status = "PartiallyFilled"
	}
	r.states[engineOrderID] = s
	return s, true
}
