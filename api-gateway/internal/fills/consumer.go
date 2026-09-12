package fills

import (
	"context"
	"log"
	"strconv"
	"strings"
	"time"

	"github.com/redis/go-redis/v9"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
)

const (
	Stream = "vertex:fills"
	Group  = "gateway-group"

	// batchCount bounds how many fills one XReadGroup call returns and thus
	// one persist transaction covers. Raised from an earlier per-message
	// design (Count: 16, one transaction per message) since transaction
	// round-trip overhead, not query work, was the throughput ceiling.
	batchCount = 100

	// persistQueueDepth bounds how many read batches can be waiting for the
	// persist worker before Run's XReadGroup loop blocks handing off the
	// next one. This is the backpressure valve: if Postgres falls behind,
	// the notify path still runs at full speed (nothing is lost — Redis
	// keeps unacked messages), but the read loop stalls once this many
	// batches are queued, rather than buffering unboundedly in memory.
	persistQueueDepth = 256
)

// FillHandler is invoked for every fill the instant it's read off the
// stream — before any Postgres write. It must not block on I/O beyond a
// quick in-process lookup (see internal/liveorders) and a WebSocket
// broadcast; the durability write happens separately, batched, in this
// consumer's own persist worker.
type FillHandler func(db.Fill)

type fillMsg struct {
	id   string
	fill db.Fill
}

type Consumer struct {
	rdb    *redis.Client
	store  *db.Store
	onFill FillHandler
}

func New(url string, store *db.Store, onFill FillHandler) (*Consumer, error) {
	opts, err := redis.ParseURL(url)
	if err != nil {
		return nil, err
	}
	return &Consumer{rdb: redis.NewClient(opts), store: store, onFill: onFill}, nil
}

func (c *Consumer) Close() error {
	return c.rdb.Close()
}

// Run consumes the fills stream until ctx is cancelled. The consumer group is
// created lazily on first connect so the gateway tolerates Redis being down at
// boot.
//
// Two stages run concurrently: this loop notifies on every fill the instant
// it's read (the hot path — see FillHandler), then hands the batch to a
// separate persistWorker goroutine for a single batched Postgres transaction
// and ack. The notify path never waits on the persist path.
func (c *Consumer) Run(ctx context.Context) {
	persistCh := make(chan []fillMsg, persistQueueDepth)
	go c.persistWorker(ctx, persistCh)

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		if err := c.ensureGroup(ctx); err != nil {
			log.Printf("fills: ensure group: %v", err)
			time.Sleep(time.Second)
			continue
		}

		resp, err := c.rdb.XReadGroup(ctx, &redis.XReadGroupArgs{
			Group:    Group,
			Consumer: "gateway-1",
			Streams:  []string{Stream, ">"},
			Count:    batchCount,
			Block:    5 * time.Second,
		}).Result()
		if err == redis.Nil {
			continue
		}
		if err != nil {
			log.Printf("fills: read: %v", err)
			time.Sleep(time.Second)
			continue
		}

		var batch []fillMsg
		for _, stream := range resp {
			for _, msg := range stream.Messages {
				fill, ok := parseFill(msg.Values)
				if !ok {
					continue
				}
				c.onFill(fill) // hot path: no Postgres, no waiting
				batch = append(batch, fillMsg{id: msg.ID, fill: fill})
			}
		}
		if len(batch) == 0 {
			continue
		}

		select {
		case persistCh <- batch:
		case <-ctx.Done():
			return
		}
	}
}

// persistWorker durably writes batches and acks them only on success, so a
// batch that fails to persist is redelivered rather than silently lost
// (the previous per-message version acked unconditionally, even on a DB
// error). Note: a transient failure here isn't automatically retried beyond
// Redis's normal redelivery-on-restart — proper retry of an in-flight
// pending batch would need periodic XPENDING/XAUTOCLAIM housekeeping, which
// is a reasonable follow-up but out of scope for this pass.
func (c *Consumer) persistWorker(ctx context.Context, persistCh <-chan []fillMsg) {
	for {
		select {
		case <-ctx.Done():
			return
		case batch := <-persistCh:
			fills := make([]db.Fill, len(batch))
			ids := make([]string, len(batch))
			for i, m := range batch {
				fills[i] = m.fill
				ids[i] = m.id
			}
			if err := c.store.PersistFillBatch(context.Background(), fills); err != nil {
				log.Printf("fills: persist batch of %d: %v", len(batch), err)
				continue
			}
			if err := c.rdb.XAck(context.Background(), Stream, Group, ids...).Err(); err != nil {
				log.Printf("fills: ack batch of %d: %v", len(batch), err)
			}
		}
	}
}

func (c *Consumer) ensureGroup(ctx context.Context) error {
	err := c.rdb.XGroupCreateMkStream(ctx, Stream, Group, "$").Err()
	if err == nil || strings.Contains(err.Error(), "BUSYGROUP") {
		return nil
	}
	return err
}

func parseFill(vals map[string]any) (db.Fill, bool) {
	var f db.Fill
	var ok bool
	if f.TradeID, ok = i64(vals, "trade_id"); !ok {
		return f, false
	}
	if f.Timestamp, ok = i64(vals, "timestamp"); !ok {
		return f, false
	}
	if f.Pair, ok = str(vals, "pair"); !ok {
		return f, false
	}
	if f.BidOrderID, ok = i64(vals, "bid_order_id"); !ok {
		return f, false
	}
	if f.BidUserID, ok = str(vals, "bid_user_id"); !ok {
		return f, false
	}
	if f.BidPrice, ok = i64(vals, "bid_price"); !ok {
		return f, false
	}
	if f.BidQuantity, ok = i64(vals, "bid_quantity"); !ok {
		return f, false
	}
	if f.AskOrderID, ok = i64(vals, "ask_order_id"); !ok {
		return f, false
	}
	if f.AskUserID, ok = str(vals, "ask_user_id"); !ok {
		return f, false
	}
	if f.AskPrice, ok = i64(vals, "ask_price"); !ok {
		return f, false
	}
	if f.AskQuantity, ok = i64(vals, "ask_quantity"); !ok {
		return f, false
	}
	return f, true
}

func str(vals map[string]any, key string) (string, bool) {
	v, ok := vals[key].(string)
	return v, ok
}

func i64(vals map[string]any, key string) (int64, bool) {
	v, ok := str(vals, key)
	if !ok {
		return 0, false
	}
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil {
		return 0, false
	}
	return n, true
}
