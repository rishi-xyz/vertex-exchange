package grpcclient

import (
	"context"
	"fmt"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
)

// Client wraps the two engine gRPC services.
type Client struct {
	Users  engine.UserServicesClient
	Engine engine.EngineServicesClient

	conn *grpc.ClientConn
}

func New(ctx context.Context, addr string) (*Client, error) {
	conn, err := grpc.NewClient(
		addr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		return nil, fmt.Errorf("dial engine %q: %w", addr, err)
	}

	c := &Client{
		Users:  engine.NewUserServicesClient(conn),
		Engine: engine.NewEngineServicesClient(conn),
		conn:   conn,
	}

	dialCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := c.Ping(dialCtx); err != nil {
		conn.Close()
		return nil, fmt.Errorf("engine %q not reachable: %w", addr, err)
	}

	return c, nil
}

// Ping checks engine connectivity. The probe intentionally targets a pair that
// usually does not exist: any server-produced status (e.g. NotFound) proves the
// engine is reachable and speaking our proto, whereas a transport failure does not.
func (c *Client) Ping(ctx context.Context) error {
	_, err := c.Users.GetOrderBook(ctx, &engine.GetOrderBookRequest{
		Pair: &engine.TradingPair{Base: engine.Asset_ETH, Quote: engine.Asset_USDC},
	})
	if err == nil {
		return nil
	}
	code := status.Code(err)
	if code == codes.Unavailable || code == codes.DeadlineExceeded {
		return err
	}
	return nil
}

func (c *Client) Close() error {
	return c.conn.Close()
}
