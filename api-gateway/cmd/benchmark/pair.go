package main

import (
	"fmt"
	"strings"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
)

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

// parsePair parses a "BASE-QUOTE" symbol, e.g. "ETH-USDC".
func parsePair(s string) (*engine.TradingPair, error) {
	parts := strings.Split(strings.ToUpper(s), "-")
	if len(parts) != 2 {
		return nil, fmt.Errorf("pair must be in BASE-QUOTE form, e.g. ETH-USDC")
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
