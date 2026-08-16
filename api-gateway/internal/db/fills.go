package db

import (
	"context"
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

// ApplyFillToOrders decrements the resting quantity of both matched orders and
// advances their status.
func (s *Store) ApplyFillToOrders(ctx context.Context, f Fill) error {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)

	for _, side := range []struct {
		orderID  int64
		quantity int64
	}{
		{f.BidOrderID, f.BidQuantity},
		{f.AskOrderID, f.AskQuantity},
	} {
		if _, err := tx.Exec(ctx,
			`UPDATE orders
			 SET remaining = remaining - $1,
			     status = CASE
			         WHEN remaining - $1 <= 0 THEN 'Filled'
			         WHEN status = 'Empty' THEN 'PartiallyFilled'
			         ELSE status
			     END
			 WHERE engine_order_id = $2 AND pair = $3`,
			side.quantity, side.orderID, f.Pair,
		); err != nil {
			return err
		}
	}

	return tx.Commit(ctx)
}
