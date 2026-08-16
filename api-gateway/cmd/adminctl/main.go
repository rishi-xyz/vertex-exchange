package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
)

// adminctl is a dev-only tool for engine operations the gateway deliberately
// does not expose, e.g. registering trading pairs.
func main() {
	addr := flag.String("addr", envOr("ENGINE_GRPC_ADDR", "localhost:5000"), "engine gRPC address")
	flag.Parse()

	conn, err := grpc.NewClient(*addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("dial engine: %v", err)
	}
	defer conn.Close()
	client := engine.NewEngineServicesClient(conn)

	args := flag.Args()
	if len(args) == 0 {
		log.Fatal("usage: adminctl <add-pair BASE-QUOTE|add-user ID>")
	}
	ctx := context.Background()

	switch args[0] {
	case "add-pair":
		if len(args) != 2 {
			log.Fatal("usage: adminctl add-pair BASE-QUOTE")
		}
		pair, err := parsePair(args[1])
		if err != nil {
			log.Fatalf("pair: %v", err)
		}
		if _, err := client.AddTradingPair(ctx, &engine.AddTradingPairRequest{Pair: pair}); err != nil {
			log.Fatalf("add pair: %v", err)
		}
		fmt.Printf("added pair %s\n", strings.ToUpper(args[1]))
	case "add-user":
		if len(args) != 2 {
			log.Fatal("usage: adminctl add-user ID")
		}
		if _, err := client.AddUser(ctx, &engine.AddUserRequest{UserId: args[1]}); err != nil {
			log.Fatalf("add user: %v", err)
		}
		fmt.Printf("added user %s\n", args[1])
	default:
		log.Fatalf("unknown command %q", args[0])
	}
}

func parsePair(s string) (*engine.TradingPair, error) {
	parts := strings.Split(strings.ToUpper(s), "-")
	if len(parts) != 2 {
		return nil, fmt.Errorf("pair must be BASE-QUOTE")
	}
	base, ok := assetByName(parts[0])
	if !ok {
		return nil, fmt.Errorf("unsupported base asset %q", parts[0])
	}
	quote, ok := assetByName(parts[1])
	if !ok {
		return nil, fmt.Errorf("unsupported quote asset %q", parts[1])
	}
	return &engine.TradingPair{Base: base, Quote: quote}, nil
}

func assetByName(s string) (engine.Asset, bool) {
	switch strings.ToUpper(s) {
	case "ETH":
		return engine.Asset_ETH, true
	case "SOL":
		return engine.Asset_SOL, true
	case "BTC":
		return engine.Asset_BTC, true
	case "USDC":
		return engine.Asset_USDC, true
	case "USDT":
		return engine.Asset_USDT, true
	}
	return 0, false
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}
