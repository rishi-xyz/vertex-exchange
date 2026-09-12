package server

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/gorilla/websocket"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/liveorders"
)

var upgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
	CheckOrigin:     func(r *http.Request) bool { return true },
}

// authenticateWS accepts the bearer token via the Authorization header or the
// ?token= query parameter (browsers cannot set headers on ws connections).
func (s *Server) authenticateWS(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		token := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if token == r.Header.Get("Authorization") {
			token = r.URL.Query().Get("token")
		}
		if token == "" {
			writeError(w, http.StatusUnauthorized, "unauthorized", "missing bearer token")
			return
		}
		claims, err := s.auth.Parse(token)
		if err != nil {
			writeError(w, http.StatusUnauthorized, "unauthorized", "invalid or expired token")
			return
		}
		userID, err := uuid.Parse(claims.Subject)
		if err != nil {
			writeError(w, http.StatusUnauthorized, "unauthorized", "invalid token subject")
			return
		}
		user, err := s.store.GetUserByID(r.Context(), userID)
		if errors.Is(err, db.ErrNotFound) {
			writeError(w, http.StatusUnauthorized, "unauthorized", "account no longer exists")
			return
		}
		if err != nil {
			s.internalError(w, "get user by id", err)
			return
		}
		next(w, r.WithContext(context.WithValue(r.Context(), userKey, user)))
	}
}

// handleWS accepts an authenticated connection and always subscribes it to
// the caller's personal order channel ("user:<id>"); a pair query parameter
// (optional) additionally subscribes it to that pair's depth/trade/ticker
// events and sends an initial depth snapshot.
func (s *Server) handleWS(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}

	pairStr := strings.ToUpper(r.URL.Query().Get("pair"))
	if pairStr != "" {
		if _, err := parsePair(pairStr); err != nil {
			writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
			return
		}
	}

	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		return
	}

	if pairStr != "" {
		// Send a depth snapshot so clients can render the book immediately.
		if msg, ok := s.depthMessage(r.Context(), pairStr); ok {
			if err := conn.WriteMessage(websocket.TextMessage, msg); err != nil {
				conn.Close()
				return
			}
		}
	}

	topics := []string{userTopic(user.ID)}
	if pairStr != "" {
		topics = append(topics, pairStr)
	}
	s.hub.Serve(conn, topics)
}

func userTopic(id uuid.UUID) string {
	return "user:" + id.String()
}

// broadcastDepth re-fetches the orderbook for pair and pushes it to subscribers.
func (s *Server) broadcastDepth(ctx context.Context, pairStr string) {
	msg, ok := s.depthMessage(ctx, pairStr)
	if !ok {
		return
	}
	s.hub.Broadcast(pairStr, msg)
}

func (s *Server) depthMessage(ctx context.Context, pairStr string) ([]byte, bool) {
	pair, err := parsePair(pairStr)
	if err != nil {
		return nil, false
	}
	cctx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()
	resp, err := s.engine.Users.GetOrderBook(cctx, &engine.GetOrderBookRequest{Pair: pair})
	if err != nil {
		return nil, false
	}
	msg, err := json.Marshal(map[string]any{
		"type": "depth",
		"pair": pairStr,
		"bids": levelsJSON(resp.Bids),
		"asks": levelsJSON(resp.Asks),
	})
	if err != nil {
		return nil, false
	}
	return msg, true
}

// broadcastTrade pushes a fill to subscribers of the pair.
func (s *Server) broadcastTrade(pairStr string, trade map[string]any) {
	msg, err := json.Marshal(map[string]any{
		"type":  "trade",
		"pair":  pairStr,
		"trade": trade,
	})
	if err != nil {
		return
	}
	s.hub.Broadcast(pairStr, msg)
}

// broadcastTicker pushes a last-price update to subscribers of the pair.
func (s *Server) broadcastTicker(pairStr string, price, quantity, timestampMs int64) {
	msg, err := json.Marshal(map[string]any{
		"type":      "ticker",
		"pair":      pairStr,
		"price":     price,
		"quantity":  quantity,
		"timestamp": timestampMs,
	})
	if err != nil {
		return
	}
	s.hub.Broadcast(pairStr, msg)
}

// broadcastOrderUpdate pushes an order status change to its owner's personal
// channel.
func (s *Server) broadcastOrderUpdate(u liveorders.State) {
	msg, err := json.Marshal(map[string]any{
		"type": "order",
		"order": map[string]any{
			"id":        u.OrderID.String(),
			"pair":      u.Pair,
			"status":    u.Status,
			"remaining": u.Remaining,
		},
	})
	if err != nil {
		return
	}
	s.hub.Broadcast(userTopic(u.UserID), msg)
}

// OnFill is the fills consumer's hot-path callback: it fires the instant a
// fill is read off the Redis stream, before any Postgres write happens (see
// internal/fills.Consumer.Run and internal/liveorders). Nothing here touches
// the database — order status comes from the in-memory live-order registry,
// trade/ticker data comes straight from the fill event, and depth comes from
// the engine via gRPC — so none of this waits on the batched, asynchronous
// persistence path.
func (s *Server) OnFill(f db.Fill) {
	s.broadcastTrade(f.Pair, map[string]any{
		"trade_id":  f.TradeID,
		"timestamp": f.Timestamp / 1_000_000,
		"price":     f.BidPrice,
		"quantity":  f.BidQuantity,
		"bid": map[string]any{
			"order_id": f.BidOrderID,
			"user_id":  f.BidUserID,
			"price":    f.BidPrice,
			"quantity": f.BidQuantity,
		},
		"ask": map[string]any{
			"order_id": f.AskOrderID,
			"user_id":  f.AskUserID,
			"price":    f.AskPrice,
			"quantity": f.AskQuantity,
		},
	})
	s.broadcastTicker(f.Pair, f.BidPrice, f.BidQuantity, f.Timestamp/1_000_000)

	for _, side := range []struct {
		orderID  int64
		quantity int64
	}{
		{f.BidOrderID, f.BidQuantity},
		{f.AskOrderID, f.AskQuantity},
	} {
		state, ok := s.registry.ApplyFill(side.orderID, side.quantity)
		if !ok {
			continue // not tracked (already terminal/evicted, or a stale process) — Postgres remains authoritative regardless
		}
		s.broadcastOrderUpdate(state)
		if state.Status == "Filled" {
			s.registry.Delete(side.orderID)
		}
	}

	if s.balCache != nil {
		if base, quote, ok := splitPair(f.Pair); ok {
			ctx := context.Background()
			s.balCache.Invalidate(ctx, f.BidUserID, base)
			s.balCache.Invalidate(ctx, f.BidUserID, quote)
			s.balCache.Invalidate(ctx, f.AskUserID, base)
			s.balCache.Invalidate(ctx, f.AskUserID, quote)
		}
	}
	s.broadcastDepth(context.Background(), f.Pair)
}
