package db

import (
	"context"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// Fill mirrors one engine trade as published to the Redis fills stream.
// Timestamp is engine epoch nanoseconds.
type Fill struct {
	TradeID     int64
	Timestamp   int64
	Pair        string
	BidOrderID  int64
	BidUserID   string
	BidPrice    int64
	BidQuantity int64
	AskOrderID  int64
	AskUserID   string
	AskPrice    int64
	AskQuantity int64
}

// AddTrade inserts a fill into the trades ledger, ignoring duplicates.
func (s *Store) AddTrade(ctx context.Context, f Fill) error {
	_, err := s.pool.Exec(ctx,
		`INSERT INTO trades (trade_id, pair, price, quantity, bid_order_id, ask_order_id, bid_user_id, ask_user_id, executed_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, to_timestamp($9 / 1000000000.0))
		 ON CONFLICT (trade_id) DO NOTHING`,
		f.TradeID, f.Pair, f.BidPrice, f.BidQuantity,
		f.BidOrderID, f.AskOrderID, f.BidUserID, f.AskUserID, f.Timestamp,
	)
	return err
}

// FillOrderUpdate is the post-fill state of one order side, returned so
// callers can notify the owning account without a second round trip.
type FillOrderUpdate struct {
	OrderID   uuid.UUID
	UserID    uuid.UUID
	Pair      string
	Status    string
	Remaining int64
}

// ApplyFillToOrders decrements the resting quantity of both matched orders,
// advances their status, and returns the resulting state of each.
func (s *Store) ApplyFillToOrders(ctx context.Context, f Fill) ([]FillOrderUpdate, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback(ctx)

	var updates []FillOrderUpdate
	for _, side := range []struct {
		orderID  int64
		quantity int64
	}{
		{f.BidOrderID, f.BidQuantity},
		{f.AskOrderID, f.AskQuantity},
	} {
		var u FillOrderUpdate
		err := tx.QueryRow(ctx,
			`UPDATE orders
			 SET remaining = remaining - $1,
			     status = CASE
			         WHEN remaining - $1 <= 0 THEN 'Filled'
			         WHEN status = 'Empty' THEN 'PartiallyFilled'
			         ELSE status
			     END
			 WHERE engine_order_id = $2 AND pair = $3
			 RETURNING id, user_id, pair, status, remaining`,
			side.quantity, side.orderID, f.Pair,
		).Scan(&u.OrderID, &u.UserID, &u.Pair, &u.Status, &u.Remaining)
		if err == pgx.ErrNoRows {
			continue
		}
		if err != nil {
			return nil, err
		}
		updates = append(updates, u)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	return updates, nil
}

// TradeRecord mirrors one row of the trades ledger.
type TradeRecord struct {
	TradeID    int64
	Pair       string
	Price      int64
	Quantity   int64
	BidOrderID int64
	AskOrderID int64
	BidUserID  string
	AskUserID  string
	ExecutedAt time.Time
}

// ListTradesByUser returns trades involving engineUserID (either side),
// most recent first, optionally filtered by pair.
func (s *Store) ListTradesByUser(ctx context.Context, engineUserID, pair string, limit, offset int) ([]*TradeRecord, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT trade_id, pair, price, quantity, bid_order_id, ask_order_id, bid_user_id, ask_user_id, executed_at
		 FROM trades
		 WHERE (bid_user_id = $1 OR ask_user_id = $1) AND ($2 = '' OR pair = $2)
		 ORDER BY id DESC
		 LIMIT $3 OFFSET $4`,
		engineUserID, pair, limit, offset,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var trades []*TradeRecord
	for rows.Next() {
		var t TradeRecord
		if err := rows.Scan(&t.TradeID, &t.Pair, &t.Price, &t.Quantity, &t.BidOrderID, &t.AskOrderID, &t.BidUserID, &t.AskUserID, &t.ExecutedAt); err != nil {
			return nil, err
		}
		trades = append(trades, &t)
	}
	return trades, rows.Err()
}

// Ticker summarizes recent trading activity for a pair.
type Ticker struct {
	Pair          string
	LastPrice     int64
	LastQuantity  int64
	LastTradeAt   *time.Time
	Volume24h     int64
	TradeCount24h int64
}

// GetTicker returns the last trade and trailing-24h stats for pair. A pair
// with no trades yet returns zero values rather than an error.
func (s *Store) GetTicker(ctx context.Context, pair string) (*Ticker, error) {
	t := &Ticker{Pair: pair}
	err := s.pool.QueryRow(ctx,
		`SELECT price, quantity, executed_at FROM trades WHERE pair = $1 ORDER BY id DESC LIMIT 1`, pair,
	).Scan(&t.LastPrice, &t.LastQuantity, &t.LastTradeAt)
	if err != nil && err != pgx.ErrNoRows {
		return nil, err
	}

	if err := s.pool.QueryRow(ctx,
		`SELECT COALESCE(SUM(quantity), 0), COUNT(*) FROM trades WHERE pair = $1 AND executed_at > now() - interval '24 hours'`,
		pair,
	).Scan(&t.Volume24h, &t.TradeCount24h); err != nil {
		return nil, err
	}
	return t, nil
}
