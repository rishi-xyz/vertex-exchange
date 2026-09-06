package server

import (
	"testing"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/gen/engine"
)

func TestParsePair(t *testing.T) {
	tests := []struct {
		in    string
		want  *engine.TradingPair
		valid bool
	}{
		{"ETH-USDC", &engine.TradingPair{Base: engine.Asset_ETH, Quote: engine.Asset_USDC}, true},
		{"btc-usdt", &engine.TradingPair{Base: engine.Asset_BTC, Quote: engine.Asset_USDT}, true},
		{"ETH", nil, false},
		{"ETH-USDC-BTC", nil, false},
		{"DOGE-USDC", nil, false},
		{"USDC-ETH", &engine.TradingPair{Base: engine.Asset_USDC, Quote: engine.Asset_ETH}, true},
	}
	for _, tc := range tests {
		got, err := parsePair(tc.in)
		if tc.valid {
			if err != nil {
				t.Errorf("parsePair(%q): unexpected error %v", tc.in, err)
				continue
			}
			if got.Base != tc.want.Base || got.Quote != tc.want.Quote {
				t.Errorf("parsePair(%q) = %+v, want %+v", tc.in, got, tc.want)
			}
			continue
		}
		if err == nil {
			t.Errorf("parsePair(%q): expected error, got %+v", tc.in, got)
		}
	}
}

func TestPairName(t *testing.T) {
	pair := &engine.TradingPair{Base: engine.Asset_ETH, Quote: engine.Asset_USDC}
	if got := pairName(pair); got != "ETH-USDC" {
		t.Errorf("pairName = %q, want ETH-USDC", got)
	}
}

func TestParseSideAndType(t *testing.T) {
	if s, ok := parseSide("buy"); !ok || s != engine.Side_Buy {
		t.Errorf("parseSide(buy) = %v, %v", s, ok)
	}
	if s, ok := parseSide("sell"); !ok || s != engine.Side_Sell {
		t.Errorf("parseSide(sell) = %v, %v", s, ok)
	}
	if _, ok := parseSide("hold"); ok {
		t.Error("parseSide(hold) should fail")
	}
	if t_, ok := parseOrderType("fok"); !ok || t_ != engine.OrderType_FillOrKill {
		t.Errorf("parseOrderType(fok) = %v, %v", t_, ok)
	}
	if t_, ok := parseOrderType("fak"); !ok || t_ != engine.OrderType_FillAndKill {
		t.Errorf("parseOrderType(fak) = %v, %v", t_, ok)
	}
	if _, ok := parseOrderType("limit"); ok {
		t.Error("parseOrderType(limit) should fail")
	}
}

func TestSplitPair(t *testing.T) {
	base, quote, ok := splitPair("ETH-USDC")
	if !ok || base != "ETH" || quote != "USDC" {
		t.Errorf("splitPair(ETH-USDC) = %q, %q, %v", base, quote, ok)
	}
	if _, _, ok := splitPair("ETH"); ok {
		t.Error("splitPair(ETH) should fail")
	}
	if _, _, ok := splitPair("ETH-USDC-BTC"); ok {
		t.Error("splitPair(ETH-USDC-BTC) should fail")
	}
}

func TestSideFromStored(t *testing.T) {
	if s, ok := sideFromStored("Buy"); !ok || s != engine.Side_Buy {
		t.Errorf("sideFromStored(Buy) = %v, %v", s, ok)
	}
	if s, ok := sideFromStored("Sell"); !ok || s != engine.Side_Sell {
		t.Errorf("sideFromStored(Sell) = %v, %v", s, ok)
	}
	if _, ok := sideFromStored("buy"); ok {
		t.Error("sideFromStored(buy) should fail: only exact stored casing is valid")
	}
}

func TestValidEmail(t *testing.T) {
	valid := []string{"a@b.co", "user@example.com", "x.y+tag@sub.example.org"}
	for _, e := range valid {
		if !validEmail(e) {
			t.Errorf("validEmail(%q) = false, want true", e)
		}
	}
	invalid := []string{"", "a", "a@", "@b", "a@b", "no-at-sign", "a b@c.com"}
	for _, e := range invalid {
		if validEmail(e) {
			t.Errorf("validEmail(%q) = true, want false", e)
		}
	}
}
