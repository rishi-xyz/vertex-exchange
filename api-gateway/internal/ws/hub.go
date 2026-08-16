package ws

import (
	"sync"
)

// Hub fans out trade and depth events to every client subscribed to a pair.
type Hub struct {
	mu   sync.RWMutex
	subs map[string]map[*Client]struct{}
}

func NewHub() *Hub {
	return &Hub{subs: make(map[string]map[*Client]struct{})}
}

func (h *Hub) Subscribe(pair string, c *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.subs[pair] == nil {
		h.subs[pair] = make(map[*Client]struct{})
	}
	h.subs[pair][c] = struct{}{}
}

// UnsubscribeAll drops the client from every pair it subscribed to.
func (h *Hub) UnsubscribeAll(c *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if c.pair != "" {
		delete(h.subs[c.pair], c)
	}
	close(c.send)
}

// Broadcast sends msg to every client subscribed to pair. Non-blocking: a slow
// client is dropped rather than blocking the hub.
func (h *Hub) Broadcast(pair string, msg []byte) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.subs[pair] {
		select {
		case c.send <- msg:
		default:
			delete(h.subs[pair], c)
			close(c.send)
		}
	}
}
