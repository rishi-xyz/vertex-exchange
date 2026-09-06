// Package balancecache is a Redis-backed, best-effort cache of per-user,
// per-asset available balances. It exists purely to fast-fail obviously
// undersized orders before paying a gRPC round trip; the engine remains the
// single source of truth and always makes the authoritative check. Any
// Redis error is treated as a cache miss rather than surfaced to callers.
package balancecache

import (
	"context"
	"fmt"
	"strconv"
	"time"

	"github.com/redis/go-redis/v9"
)

type Cache struct {
	rdb *redis.Client
	ttl time.Duration
}

// New builds a cache backed by url. ttl bounds how stale a cached value can
// be before it is dropped and re-fetched from the engine.
func New(url string, ttl time.Duration) (*Cache, error) {
	opts, err := redis.ParseURL(url)
	if err != nil {
		return nil, err
	}
	return &Cache{rdb: redis.NewClient(opts), ttl: ttl}, nil
}

func (c *Cache) Close() error {
	return c.rdb.Close()
}

func key(userID, asset string) string {
	return fmt.Sprintf("bal:%s:%s", userID, asset)
}

// Get returns the cached available balance, or ok=false on a miss (including
// any Redis error) so callers fall through to the engine.
func (c *Cache) Get(ctx context.Context, userID, asset string) (uint32, bool) {
	v, err := c.rdb.Get(ctx, key(userID, asset)).Result()
	if err != nil {
		return 0, false
	}
	n, err := strconv.ParseUint(v, 10, 32)
	if err != nil {
		return 0, false
	}
	return uint32(n), true
}

// Set caches an available balance fetched from the engine.
func (c *Cache) Set(ctx context.Context, userID, asset string, quantity uint32) {
	c.rdb.Set(ctx, key(userID, asset), quantity, c.ttl)
}

// Invalidate drops a cached balance because it may now be stale (deposit,
// fill, cancel, modify).
func (c *Cache) Invalidate(ctx context.Context, userID, asset string) {
	c.rdb.Del(ctx, key(userID, asset))
}
