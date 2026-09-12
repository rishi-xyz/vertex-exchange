package db

import (
	"context"
	"fmt"
	"strings"
	"time"

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

// PersistFillBatch durably records a batch of fills in one transaction: a
// bulk trade insert plus a single bulk order update. This is the durability
// path only — notification is handled separately and does not wait on this
// (see internal/liveorders and Server.OnFill), so this function's only job
// is "don't lose the batch," not "notify anyone."
//
// Quantities are pre-aggregated per (engine_order_id, pair) across the whole
// batch before the bulk UPDATE, because a single order can appear on either
// side of more than one fill within one batch (e.g. a large taker crossing
// several resting orders, or one resting order hit by several takers in
// quick succession) — a naive UPDATE...FROM(VALUES...) with duplicate keys
// in the VALUES list would apply only one of them, silently dropping the
// rest.
func (s *Store) PersistFillBatch(ctx context.Context, fills []Fill) error {
	if len(fills) == 0 {
		return nil
	}

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)

	if err := insertTradesBatch(ctx, tx, fills); err != nil {
		return fmt.Errorf("insert trades: %w", err)
	}
	if err := updateOrdersBatch(ctx, tx, fills); err != nil {
		return fmt.Errorf("update orders: %w", err)
	}

	return tx.Commit(ctx)
}

func insertTradesBatch(ctx context.Context, tx pgx.Tx, fills []Fill) error {
	var sb strings.Builder
	args := make([]any, 0, len(fills)*9)
	for i, f := range fills {
		if i > 0 {
			sb.WriteString(", ")
		}
		n := i * 9
		fmt.Fprintf(&sb, "($%d,$%d,$%d,$%d,$%d,$%d,$%d,$%d,to_timestamp($%d/1000000000.0))",
			n+1, n+2, n+3, n+4, n+5, n+6, n+7, n+8, n+9)
		args = append(args, f.TradeID, f.Pair, f.BidPrice, f.BidQuantity,
			f.BidOrderID, f.AskOrderID, f.BidUserID, f.AskUserID, f.Timestamp)
	}
	query := `INSERT INTO trades (trade_id, pair, price, quantity, bid_order_id, ask_order_id, bid_user_id, ask_user_id, executed_at)
		VALUES ` + sb.String() + ` ON CONFLICT (trade_id) DO NOTHING`
	_, err := tx.Exec(ctx, query, args...)
	return err
}

func updateOrdersBatch(ctx context.Context, tx pgx.Tx, fills []Fill) error {
	type orderKey struct {
		engineOrderID int64
		pair          string
	}
	deltas := make(map[orderKey]int64, len(fills)*2)
	for _, f := range fills {
		deltas[orderKey{f.BidOrderID, f.Pair}] += f.BidQuantity
		deltas[orderKey{f.AskOrderID, f.Pair}] += f.AskQuantity
	}

	var sb strings.Builder
	args := make([]any, 0, len(deltas)*3)
	i := 0
	for k, qty := range deltas {
		if i > 0 {
			sb.WriteString(", ")
		}
		n := i * 3
		fmt.Fprintf(&sb, "($%d::bigint,$%d::text,$%d::bigint)", n+1, n+2, n+3)
		args = append(args, k.engineOrderID, k.pair, qty)
		i++
	}

	query := `UPDATE orders AS o
		SET remaining = o.remaining - v.qty,
		    status = CASE
		        WHEN o.remaining - v.qty <= 0 THEN 'Filled'
		        WHEN o.status = 'Empty' THEN 'PartiallyFilled'
		        ELSE o.status
		    END
		FROM (VALUES ` + sb.String() + `) AS v(engine_order_id, pair, qty)
		WHERE o.engine_order_id = v.engine_order_id AND o.pair = v.pair`
	_, err := tx.Exec(ctx, query, args...)
	return err
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
