package ws

import (
	"sync"
)

// Hub fans out messages to every client subscribed to a topic. A topic is
// either a pair (depth/trade/ticker events) or "user:<id>" (personal order
// updates).
type Hub struct {
	mu   sync.RWMutex
	subs map[string]map[*Client]struct{}
}

func NewHub() *Hub {
	return &Hub{subs: make(map[string]map[*Client]struct{})}
}

func (h *Hub) Subscribe(topic string, c *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.subs[topic] == nil {
		h.subs[topic] = make(map[*Client]struct{})
	}
	h.subs[topic][c] = struct{}{}
}

// UnsubscribeAll drops the client from every topic it subscribed to.
func (h *Hub) UnsubscribeAll(c *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, topic := range c.topics {
		delete(h.subs[topic], c)
	}
	close(c.send)
}

// Broadcast sends msg to every client subscribed to topic. Non-blocking: a
// slow client is dropped rather than blocking the hub.
func (h *Hub) Broadcast(topic string, msg []byte) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.subs[topic] {
		select {
		case c.send <- msg:
		default:
			delete(h.subs[topic], c)
			close(c.send)
		}
	}
}
