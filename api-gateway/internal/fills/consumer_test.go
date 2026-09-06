package fills

import (
	"testing"

	"github.com/rishi-xyz/vertex-exchange/api-gateway/internal/db"
)

func TestParseFill(t *testing.T) {
	vals := map[string]any{
		"trade_id":     "123",
		"timestamp":    "1786898117551000000",
		"pair":         "ETH-USDC",
		"bid_order_id": "10",
		"bid_user_id":  "u-bid",
		"bid_price":    "3000",
		"bid_quantity": "2",
		"ask_order_id": "11",
		"ask_user_id":  "u-ask",
		"ask_price":    "3000",
		"ask_quantity": "2",
	}
	fill, ok := parseFill(vals)
	if !ok {
		t.Fatal("parseFill returned ok=false")
	}
	want := db.Fill{
		TradeID: 123, Timestamp: 1786898117551000000, Pair: "ETH-USDC",
		BidOrderID: 10, BidUserID: "u-bid", BidPrice: 3000, BidQuantity: 2,
		AskOrderID: 11, AskUserID: "u-ask", AskPrice: 3000, AskQuantity: 2,
	}
	if fill != want {
		t.Errorf("parseFill = %+v, want %+v", fill, want)
	}
}

func TestParseFillMissingField(t *testing.T) {
	vals := map[string]any{
		"trade_id":     "123",
		"timestamp":    "1786898117551000000",
		"pair":         "ETH-USDC",
		"bid_order_id": "10",
		"bid_user_id":  "u-bid",
		"bid_price":    "3000",
		"bid_quantity": "2",
		"ask_order_id": "11",
		"ask_user_id":  "u-ask",
		"ask_price":    "3000",
	}
	// ask_quantity missing
	if _, ok := parseFill(vals); ok {
		t.Fatal("parseFill should fail on missing field")
	}
}

func TestParseFillNonNumeric(t *testing.T) {
	vals := map[string]any{
		"trade_id":     "abc",
		"timestamp":    "1786898117551000000",
		"pair":         "ETH-USDC",
		"bid_order_id": "10",
		"bid_user_id":  "u-bid",
		"bid_price":    "3000",
		"bid_quantity": "2",
		"ask_order_id": "11",
		"ask_user_id":  "u-ask",
		"ask_price":    "3000",
		"ask_quantity": "2",
	}
	if _, ok := parseFill(vals); ok {
		t.Fatal("parseFill should fail on non-numeric trade_id")
	}
}
