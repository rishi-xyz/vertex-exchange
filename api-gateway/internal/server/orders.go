package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"sync"

	"github.com/go-chi/chi/v5"
	"github.com/google/uuid"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/liveorders"
)

func (s *Server) handleDeposit(w http.ResponseWriter, r *http.Request) {
	caller := userFromContext(r.Context())
	if caller == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	userID, err := uuid.Parse(chi.URLParam(r, "id"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_user_id", "invalid user id")
		return
	}
	if userID != caller.ID {
		// Hide existence of other accounts rather than leaking a 403.
		writeError(w, http.StatusNotFound, "not_found", "user not found")
		return
	}
	var req struct {
		Asset    string `json:"asset"`
		Quantity uint32 `json:"quantity"`
	}
	if err := decodeJSON(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	asset, ok := assetByName(req.Asset)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid_asset", fmt.Sprintf("unsupported asset %q", req.Asset))
		return
	}
	if req.Quantity == 0 {
		writeError(w, http.StatusBadRequest, "invalid_quantity", "quantity must be positive")
		return
	}
	user, err := s.store.GetUserByID(r.Context(), userID)
	if errors.Is(err, db.ErrNotFound) {
		writeError(w, http.StatusNotFound, "not_found", "user not found")
		return
	}
	if err != nil {
		s.internalError(w, "get user", err)
		return
	}
	if _, err := s.engine.Engine.DepositBalance(r.Context(), &engine.DepositBalanceRequest{
		UserId:   user.EngineUserID,
		Asset:    asset,
		Quantity: req.Quantity,
	}); err != nil {
		code, ecode := grpcToHTTP(err)
		writeError(w, code, ecode, grpcMessage(err))
		return
	}
	if s.balCache != nil {
		s.balCache.Invalidate(r.Context(), user.EngineUserID, assetName(asset))
	}
	writeJSON(w, http.StatusOK, map[string]any{"deposited": req.Quantity, "asset": assetName(asset)})
}

// balanceEntry is one asset's available/locked/total balance.
type balanceEntry struct {
	asset     string
	available uint32
	total     uint32
	err       error
}

func (s *Server) handleBalances(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}

	assets := allAssets()
	entries := make([]balanceEntry, len(assets))
	var wg sync.WaitGroup
	for i, asset := range assets {
		wg.Add(1)
		go func(i int, asset engine.Asset) {
			defer wg.Done()
			e := balanceEntry{asset: assetName(asset)}
			avail, err := s.engine.Engine.GetBalance(r.Context(), &engine.GetBalanceRequest{UserId: user.EngineUserID, Asset: asset})
			if err != nil {
				e.err = err
				entries[i] = e
				return
			}
			total, err := s.engine.Engine.GetTotalBalance(r.Context(), &engine.GetTotalBalanceRequest{UserId: user.EngineUserID, Asset: asset})
			if err != nil {
				e.err = err
				entries[i] = e
				return
			}
			e.available, e.total = avail.Quantity, total.Quantity
			entries[i] = e
		}(i, asset)
	}
	wg.Wait()

	balances := map[string]any{}
	for _, e := range entries {
		if e.err != nil {
			code, ecode := grpcToHTTP(e.err)
			writeError(w, code, ecode, grpcMessage(e.err))
			return
		}
		if s.balCache != nil {
			s.balCache.Set(r.Context(), user.EngineUserID, e.asset, e.available)
		}
		balances[e.asset] = map[string]any{
			"available": e.available,
			"locked":    e.total - e.available,
			"total":     e.total,
		}
	}
	writeJSON(w, http.StatusOK, map[string]any{"balances": balances})
}

func (s *Server) handleSubmitOrder(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	var req struct {
		Pair     string `json:"pair"`
		Side     string `json:"side"`
		Type     string `json:"type"`
		Price    int32  `json:"price"`
		Quantity uint32 `json:"quantity"`
	}
	if err := decodeJSON(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	pair, err := parsePair(req.Pair)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
		return
	}
	orderSide, ok := parseSide(req.Side)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid_side", "side must be buy or sell")
		return
	}
	orderType, ok := parseOrderType(req.Type)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid_order_type", "type must be gtc, gfd, fak or fok")
		return
	}
	if req.Quantity == 0 {
		writeError(w, http.StatusBadRequest, "invalid_quantity", "quantity must be positive")
		return
	}
	if req.Price <= 0 {
		writeError(w, http.StatusBadRequest, "invalid_price", "price must be positive")
		return
	}

	if insufficient, asset := s.balanceInsufficientCached(r.Context(), user.EngineUserID, pair, orderSide, req.Price, req.Quantity); insufficient {
		writeError(w, http.StatusConflict, "insufficient_balance", fmt.Sprintf("insufficient %s balance", asset))
		return
	}

	resp, err := s.engine.Users.SubmitOrder(r.Context(), &engine.SubmitOrderRequest{
		Pair:      pair,
		OrderType: orderType,
		Side:      orderSide,
		Price:     req.Price,
		Quantity:  req.Quantity,
		UserId:    user.EngineUserID,
	})
	if err != nil {
		code, ecode := grpcToHTTP(err)
		writeError(w, code, ecode, grpcMessage(err))
		return
	}

	orderID, err := s.store.CreateOrder(r.Context(), user.ID, pairName(pair), sideName(orderSide), orderTypeName(orderType),
		req.Price, req.Quantity, int64(resp.OrderId))
	if err != nil {
		s.internalError(w, "persist order", err)
		return
	}
	s.registry.Put(int64(resp.OrderId), liveorders.State{
		OrderID: orderID, UserID: user.ID, Pair: pairName(pair), Remaining: int64(req.Quantity), Status: "Empty",
	})
	if s.balCache != nil {
		base, quote, _ := splitPair(pairName(pair))
		if orderSide == engine.Side_Buy {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, quote)
		} else {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, base)
		}
	}
	go s.broadcastDepth(context.Background(), pairName(pair))

	writeJSON(w, http.StatusCreated, map[string]any{
		"order": map[string]any{
			"id":              orderID.String(),
			"pair":            pairName(pair),
			"side":            sideName(orderSide),
			"type":            orderTypeName(orderType),
			"price":           req.Price,
			"quantity":        req.Quantity,
			"remaining":       req.Quantity,
			"status":          "Empty",
			"engine_order_id": strconv.FormatUint(resp.OrderId, 10),
		},
	})
}

func (s *Server) handleGetOrder(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	orderID, err := uuid.Parse(chi.URLParam(r, "id"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_order_id", "invalid order id")
		return
	}
	order, err := s.store.GetOrderByID(r.Context(), orderID)
	if errors.Is(err, db.ErrNotFound) {
		writeError(w, http.StatusNotFound, "not_found", "order not found")
		return
	}
	if err != nil {
		s.internalError(w, "get order", err)
		return
	}
	if order.UserID != user.ID {
		writeError(w, http.StatusNotFound, "not_found", "order not found")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"order": orderJSON(order)})
}

// loadOwnOpenOrder fetches an order by id, enforcing ownership (IDOR-safe:
// a mismatched owner reads as not-found) and that it is still open.
func (s *Server) loadOwnOpenOrder(w http.ResponseWriter, r *http.Request, userID uuid.UUID) (*db.Order, bool) {
	orderID, err := uuid.Parse(chi.URLParam(r, "id"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_order_id", "invalid order id")
		return nil, false
	}
	order, err := s.store.GetOrderByID(r.Context(), orderID)
	if errors.Is(err, db.ErrNotFound) {
		writeError(w, http.StatusNotFound, "not_found", "order not found")
		return nil, false
	}
	if err != nil {
		s.internalError(w, "get order", err)
		return nil, false
	}
	if order.UserID != userID {
		writeError(w, http.StatusNotFound, "not_found", "order not found")
		return nil, false
	}
	if order.Status == "Filled" || order.Status == "Cancelled" {
		writeError(w, http.StatusConflict, "order_closed", fmt.Sprintf("order is already %s", order.Status))
		return nil, false
	}
	return order, true
}

func (s *Server) handleCancelOrder(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	order, ok := s.loadOwnOpenOrder(w, r, user.ID)
	if !ok {
		return
	}
	pair, err := parsePair(order.Pair)
	if err != nil {
		s.internalError(w, "parse stored pair", err)
		return
	}
	resp, err := s.engine.Users.CancelOrder(r.Context(), &engine.CancelOrderRequest{
		Pair:    pair,
		OrderId: uint64(order.EngineOrderID),
	})
	if err != nil {
		code, ecode := grpcToHTTP(err)
		writeError(w, code, ecode, grpcMessage(err))
		return
	}
	if !resp.Success {
		writeError(w, http.StatusConflict, "cancel_failed", "order could not be cancelled")
		return
	}
	if err := s.store.CancelOrder(r.Context(), order.ID); err != nil {
		s.internalError(w, "persist cancel", err)
		return
	}
	s.registry.Delete(order.EngineOrderID)
	if s.balCache != nil {
		side, _ := sideFromStored(order.Side)
		base, quote, _ := splitPair(order.Pair)
		if side == engine.Side_Buy {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, quote)
		} else {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, base)
		}
	}
	go s.broadcastDepth(context.Background(), order.Pair)
	writeJSON(w, http.StatusOK, map[string]any{"cancelled": true, "order_id": order.ID.String()})
}

func (s *Server) handleModifyOrder(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	order, ok := s.loadOwnOpenOrder(w, r, user.ID)
	if !ok {
		return
	}
	var req struct {
		Price    int32  `json:"price"`
		Quantity uint32 `json:"quantity"`
	}
	if err := decodeJSON(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}
	if req.Quantity == 0 {
		writeError(w, http.StatusBadRequest, "invalid_quantity", "quantity must be positive")
		return
	}
	if req.Price <= 0 {
		writeError(w, http.StatusBadRequest, "invalid_price", "price must be positive")
		return
	}
	pair, err := parsePair(order.Pair)
	if err != nil {
		s.internalError(w, "parse stored pair", err)
		return
	}
	side, ok := sideFromStored(order.Side)
	if !ok {
		s.internalError(w, "parse stored side", fmt.Errorf("unknown side %q", order.Side))
		return
	}

	if _, err := s.engine.Users.ModifyOrder(r.Context(), &engine.ModifyOrderRequest{
		Pair:     pair,
		OrderId:  uint64(order.EngineOrderID),
		Price:    req.Price,
		Quantity: req.Quantity,
		Side:     side,
		UserId:   user.EngineUserID,
	}); err != nil {
		code, ecode := grpcToHTTP(err)
		writeError(w, code, ecode, grpcMessage(err))
		return
	}
	if err := s.store.ModifyOrder(r.Context(), order.ID, req.Price, req.Quantity); err != nil {
		s.internalError(w, "persist modify", err)
		return
	}
	// Cancel-replace keeps the same engine_order_id, so this overwrites the
	// existing registry entry with the fresh resting order's state.
	s.registry.Put(order.EngineOrderID, liveorders.State{
		OrderID: order.ID, UserID: user.ID, Pair: order.Pair, Remaining: int64(req.Quantity), Status: "Empty",
	})
	if s.balCache != nil {
		base, quote, _ := splitPair(order.Pair)
		if side == engine.Side_Buy {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, quote)
		} else {
			s.balCache.Invalidate(r.Context(), user.EngineUserID, base)
		}
	}
	go s.broadcastDepth(context.Background(), order.Pair)

	updated, err := s.store.GetOrderByID(r.Context(), order.ID)
	if err != nil {
		s.internalError(w, "reload order", err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"order": orderJSON(updated)})
}

// pagination reads and clamps limit/offset query params.
func pagination(r *http.Request) (limit, offset int) {
	limit, offset = 50, 0
	if v, err := strconv.Atoi(r.URL.Query().Get("limit")); err == nil && v > 0 {
		limit = v
	}
	if limit > 200 {
		limit = 200
	}
	if v, err := strconv.Atoi(r.URL.Query().Get("offset")); err == nil && v >= 0 {
		offset = v
	}
	return limit, offset
}

var validOrderStatuses = map[string]bool{"Empty": true, "PartiallyFilled": true, "Filled": true, "Cancelled": true}

func (s *Server) handleListOrders(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	pairFilter := ""
	if raw := r.URL.Query().Get("pair"); raw != "" {
		pair, err := parsePair(raw)
		if err != nil {
			writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
			return
		}
		pairFilter = pairName(pair)
	}
	statusFilter := r.URL.Query().Get("status")
	if statusFilter != "" && !validOrderStatuses[statusFilter] {
		writeError(w, http.StatusBadRequest, "invalid_status", "status must be Empty, PartiallyFilled, Filled or Cancelled")
		return
	}
	limit, offset := pagination(r)

	orders, err := s.store.ListOrders(r.Context(), user.ID, pairFilter, statusFilter, limit, offset)
	if err != nil {
		s.internalError(w, "list orders", err)
		return
	}
	out := make([]map[string]any, 0, len(orders))
	for _, o := range orders {
		out = append(out, orderJSON(o))
	}
	writeJSON(w, http.StatusOK, map[string]any{"orders": out})
}

func (s *Server) handleListTrades(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	pairFilter := ""
	if raw := r.URL.Query().Get("pair"); raw != "" {
		pair, err := parsePair(raw)
		if err != nil {
			writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
			return
		}
		pairFilter = pairName(pair)
	}
	limit, offset := pagination(r)

	trades, err := s.store.ListTradesByUser(r.Context(), user.EngineUserID, pairFilter, limit, offset)
	if err != nil {
		s.internalError(w, "list trades", err)
		return
	}
	out := make([]map[string]any, 0, len(trades))
	for _, t := range trades {
		side := "sell"
		if t.BidUserID == user.EngineUserID {
			side = "buy"
		}
		out = append(out, map[string]any{
			"trade_id":    t.TradeID,
			"pair":        t.Pair,
			"price":       t.Price,
			"quantity":    t.Quantity,
			"side":        side,
			"executed_at": t.ExecutedAt.UTC(),
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"trades": out})
}

func (s *Server) handleTicker(w http.ResponseWriter, r *http.Request) {
	pair, err := parsePair(chi.URLParam(r, "pair"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
		return
	}
	t, err := s.store.GetTicker(r.Context(), pairName(pair))
	if err != nil {
		s.internalError(w, "get ticker", err)
		return
	}
	var lastTradeAt any
	if t.LastTradeAt != nil {
		lastTradeAt = t.LastTradeAt.UTC()
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"pair":            t.Pair,
		"last_price":      t.LastPrice,
		"last_quantity":   t.LastQuantity,
		"last_trade_at":   lastTradeAt,
		"volume_24h":      t.Volume24h,
		"trade_count_24h": t.TradeCount24h,
	})
}

// balanceInsufficientCached is a best-effort fast-fail using the balance
// pre-check cache: it only ever says "insufficient" from a cache hit, and
// always defers to the engine on a miss, so a stale cache can only cost an
// extra round trip, never a wrongful rejection beyond the cache's TTL.
func (s *Server) balanceInsufficientCached(ctx context.Context, engineUserID string, pair *engine.TradingPair, side engine.Side, price int32, quantity uint32) (insufficient bool, asset string) {
	if s.balCache == nil {
		return false, ""
	}
	var need uint64
	var a string
	if side == engine.Side_Buy {
		a = assetName(pair.Quote)
		need = uint64(price) * uint64(quantity)
	} else {
		a = assetName(pair.Base)
		need = uint64(quantity)
	}
	avail, ok := s.balCache.Get(ctx, engineUserID, a)
	if !ok {
		return false, ""
	}
	if uint64(avail) < need {
		return true, a
	}
	return false, ""
}

func (s *Server) handleGetOrderBook(w http.ResponseWriter, r *http.Request) {
	pair, err := parsePair(chi.URLParam(r, "pair"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_pair", err.Error())
		return
	}
	resp, err := s.engine.Users.GetOrderBook(r.Context(), &engine.GetOrderBookRequest{Pair: pair})
	if err != nil {
		code, ecode := grpcToHTTP(err)
		writeError(w, code, ecode, grpcMessage(err))
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"pair": pairName(pair),
		"bids": levelsJSON(resp.Bids),
		"asks": levelsJSON(resp.Asks),
	})
}

func levelsJSON(levels []*engine.LevelInfo) []map[string]any {
	out := make([]map[string]any, 0, len(levels))
	for _, l := range levels {
		out = append(out, map[string]any{"price": l.Price, "quantity": l.Quantity})
	}
	return out
}

func orderJSON(o *db.Order) map[string]any {
	return map[string]any{
		"id":              o.ID.String(),
		"user_id":         o.UserID.String(),
		"pair":            o.Pair,
		"side":            o.Side,
		"type":            o.Type,
		"price":           o.Price,
		"quantity":        o.Quantity,
		"remaining":       o.Remaining,
		"status":          o.Status,
		"engine_order_id": strconv.FormatInt(o.EngineOrderID, 10),
		"created_at":      o.CreatedAt.UTC(),
	}
}

func parseSide(s string) (engine.Side, bool) {
	switch s {
	case "buy":
		return engine.Side_Buy, true
	case "sell":
		return engine.Side_Sell, true
	}
	return 0, false
}

func sideName(s engine.Side) string {
	return s.String()
}

// sideFromStored parses the side string as persisted by sideName (the
// engine proto enum's name, e.g. "Buy"/"Sell").
func sideFromStored(s string) (engine.Side, bool) {
	switch s {
	case "Buy":
		return engine.Side_Buy, true
	case "Sell":
		return engine.Side_Sell, true
	}
	return 0, false
}

func parseOrderType(s string) (engine.OrderType, bool) {
	switch s {
	case "gtc":
		return engine.OrderType_GoodTillCancel, true
	case "gfd":
		return engine.OrderType_GoodForDay, true
	case "fak":
		return engine.OrderType_FillAndKill, true
	case "fok":
		return engine.OrderType_FillOrKill, true
	}
	return 0, false
}

func orderTypeName(t engine.OrderType) string {
	return t.String()
}

func allAssets() []engine.Asset {
	return []engine.Asset{engine.Asset_ETH, engine.Asset_SOL, engine.Asset_BTC, engine.Asset_USDC, engine.Asset_USDT}
}

func decodeJSON(r *http.Request, v any) error {
	if r.Body == nil {
		return fmt.Errorf("empty request body")
	}
	if err := json.NewDecoder(r.Body).Decode(v); err != nil {
		return fmt.Errorf("malformed JSON body")
	}
	return nil
}
