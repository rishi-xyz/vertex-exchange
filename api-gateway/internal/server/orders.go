package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"

	"github.com/go-chi/chi/v5"
	"github.com/google/uuid"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
)

func (s *Server) handleDeposit(w http.ResponseWriter, r *http.Request) {
	userID, err := uuid.Parse(chi.URLParam(r, "id"))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_user_id", "invalid user id")
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
	writeJSON(w, http.StatusOK, map[string]any{"deposited": req.Quantity, "asset": assetName(asset)})
}

func (s *Server) handleBalances(w http.ResponseWriter, r *http.Request) {
	user := userFromContext(r.Context())
	if user == nil {
		writeError(w, http.StatusUnauthorized, "unauthorized", "missing or invalid token")
		return
	}
	balances := map[string]any{}
	for _, asset := range allAssets() {
		resp, err := s.engine.Engine.GetBalance(r.Context(), &engine.GetBalanceRequest{
			UserId: user.EngineUserID,
			Asset:  asset,
		})
		if err != nil {
			code, ecode := grpcToHTTP(err)
			writeError(w, code, ecode, grpcMessage(err))
			return
		}
		balances[assetName(asset)] = resp.Quantity
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
