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
)

// FillHandler is invoked for every newly persisted fill.
type FillHandler func(db.Fill)

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
func (c *Consumer) Run(ctx context.Context) {
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
			Count:    16,
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

		for _, stream := range resp {
			for _, msg := range stream.Messages {
				if fill, ok := parseFill(msg.Values); ok {
					if err := c.store.AddTrade(context.Background(), fill); err != nil {
						log.Printf("fills: add trade: %v", err)
					}
					if err := c.store.ApplyFillToOrders(context.Background(), fill); err != nil {
						log.Printf("fills: apply fill: %v", err)
					}
					c.onFill(fill)
				}
				c.rdb.XAck(context.Background(), Stream, Group, msg.ID)
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
