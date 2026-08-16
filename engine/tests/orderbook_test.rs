mod helpers;

use vertex_engine::orderbook::OrderBook;
use vertex_engine::types::{OrderType, Side};

use helpers::{
    UserId, make_generator, make_modify, make_order, make_order_with_id, make_user_id,
    make_user_id_fixed,
};

fn new_book() -> OrderBook {
    OrderBook::new()
}

fn uid_a() -> UserId {
    make_user_id_fixed(100)
}

fn uid_b() -> UserId {
    make_user_id_fixed(200)
}

// ========== Empty Book ==========

#[test]
fn new_has_size_zero() {
    let book = new_book();
    assert_eq!(book.size(), 0);
}

#[test]
fn has_order_false() {
    let book = new_book();
    assert!(!book.has_order(&999));
}

#[test]
fn get_order_type_none() {
    let book = new_book();
    assert!(book.get_order_type(&999).is_none());
}

#[test]
fn get_order_info_empty() {
    let book = new_book();
    let info = book.get_order_info();
    assert!(info.get_bids().is_empty());
    assert!(info.get_asks().is_empty());
}

#[test]
fn cancel_unknown_returns_none() {
    let mut book = new_book();
    assert!(book.cancel_order(&999).is_none());
}

// ========== GTC Placement ==========

#[test]
fn single_buy_rests() {
    let mut book = new_book();
    let mut generator = make_generator();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    assert_eq!(book.size(), 1);
    assert!(book.has_order(&order.get_order_id()));
}

#[test]
fn single_sell_rests() {
    let mut book = new_book();
    let mut generator = make_generator();
    let order = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_a());
    book.add_order(&order, &mut generator);
    assert_eq!(book.size(), 1);
}

#[test]
fn multiple_same_price_fifo() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();
    let uid2 = make_user_id();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 20, uid2);
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);

    let info = book.get_order_info();
    let bids = info.get_bids();
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0].quantity, 30);
}

#[test]
fn multiple_different_prices_btree_ordering() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid);
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let o3 = make_order(OrderType::GoodTillCancel, Side::Buy, 48000, 10, uid);
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);
    book.add_order(&o3, &mut generator);

    let info = book.get_order_info();
    let bids = info.get_bids();
    assert_eq!(bids.len(), 3);
    // push_front reverses BTreeMap ascending → result is descending
    assert_eq!(bids[0].price, 50000);
    assert_eq!(bids[1].price, 49000);
    assert_eq!(bids[2].price, 48000);
}

#[test]
fn buy_and_sell_no_cross_both_rest() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid_a());
    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&buy, &mut generator);
    book.add_order(&sell, &mut generator);

    assert_eq!(book.size(), 2);
}

#[test]
fn get_order_info_after_adds() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid_a());
    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 20, uid_b());
    book.add_order(&buy, &mut generator);
    book.add_order(&sell, &mut generator);

    let info = book.get_order_info();
    assert_eq!(info.get_bids().len(), 1);
    assert_eq!(info.get_bids()[0].price, 49000);
    assert_eq!(info.get_bids()[0].quantity, 10);
    assert_eq!(info.get_asks().len(), 1);
    assert_eq!(info.get_asks()[0].price, 51000);
    assert_eq!(info.get_asks()[0].quantity, 20);
}

// ========== GTC Matching ==========

#[test]
fn buy_crosses_best_ask_full_fill() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn sell_crosses_best_bid_full_fill() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    let trades = book.add_order(&sell, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn buy_crosses_multiple_ask_levels() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid_b());
    book.add_order(&sell1, &mut generator);
    book.add_order(&sell2, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 51000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 2);
    assert_eq!(book.size(), 0);
}

#[test]
fn partial_fill_aggressor_larger() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 1);
    assert!(book.has_order(&buy.get_order_id()));
}

#[test]
fn partial_fill_resting_larger() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 20, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 1);
    let info = book.get_order_info();
    assert_eq!(info.get_asks()[0].quantity, 10);
}

#[test]
fn exact_quantity_match_both_filled() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    let trade = &trades[0];
    assert_eq!(trade.get_bid_trade_info().get_quantity(), 10);
    assert_eq!(trade.get_ask_trade_info().get_quantity(), 10);
    assert_eq!(book.size(), 0);
}

#[test]
fn buy_at_exactly_best_ask() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
}

#[test]
fn sell_at_exactly_best_bid() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    let trades = book.add_order(&sell, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
}

#[test]
fn multiple_resting_fifo_at_level() {
    let mut book = new_book();
    let mut generator = make_generator();

    let uid1 = uid_a();
    let uid2 = make_user_id();
    let uid3 = make_user_id();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid1);
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid2);
    book.add_order(&sell1, &mut generator);
    book.add_order(&sell2, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 8, uid3);
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].get_ask_trade_info().get_quantity(), 5);
    assert_eq!(trades[1].get_ask_trade_info().get_quantity(), 3);
    let info = book.get_order_info();
    assert_eq!(info.get_asks()[0].quantity, 2);
}

#[test]
fn trade_price_is_resting_order_price() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 52000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades[0].get_ask_trade_info().get_price(), 51000);
    assert_eq!(trades[0].get_bid_trade_info().get_price(), 51000);
}

#[test]
fn partial_gtc_aggressor_rests_remainder() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);

    assert_eq!(book.size(), 1);
    assert!(book.has_order(&buy.get_order_id()));
}

// ========== Self-Trade Prevention ==========

#[test]
fn same_user_buy_sell_skipped() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert!(trades.is_empty());
    assert_eq!(book.size(), 2);
}

#[test]
fn all_self_trade_level_removed_and_reinserted() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid);
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid);
    book.add_order(&sell1, &mut generator);
    book.add_order(&sell2, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 52000, 10, uid);
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert!(trades.is_empty());

    assert_eq!(book.size(), 3);
    let info = book.get_order_info();
    assert_eq!(info.get_asks().len(), 2);
}

#[test]
fn mixed_self_and_other_fills_only_others() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell_other = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    let sell_self = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid);
    book.add_order(&sell_other, &mut generator);
    book.add_order(&sell_self, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    let info = book.get_order_info();
    assert_eq!(info.get_asks()[0].quantity, 5);
}

#[test]
fn self_trade_in_fak_returns_empty_trades() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    book.add_order(&sell, &mut generator);

    // FAK buy from same user — self-trade skipped, no fills, returns Some(empty)
    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_some());
    assert!(trades.unwrap().is_empty());
}

#[test]
fn self_trade_in_fok_excluded_from_liquidity() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

#[test]
fn self_trade_levels_reinserted_after_match() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid);
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid);
    book.add_order(&sell1, &mut generator);
    book.add_order(&sell2, &mut generator);

    let sell_other = make_order(OrderType::GoodTillCancel, Side::Sell, 52000, 5, uid_b());
    book.add_order(&sell_other, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 52000, 5, uid);
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);

    let info = book.get_order_info();
    assert_eq!(info.get_asks().len(), 2);
}

#[test]
fn aggressor_side_determines_rotation_direction() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    book.add_order(&buy, &mut generator);

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    let trades = book.add_order(&sell, &mut generator).unwrap();
    assert!(trades.is_empty());
    assert_eq!(book.size(), 2);
}

#[test]
fn self_trade_does_not_hide_other_users_buried_same_side_order() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid_x = uid_a();
    let uid_y = uid_b();

    // X's sell at 50000 — the only resting ask at the level
    let sell_x = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_x);
    book.add_order(&sell_x, &mut generator);

    // X's buy sits in front at the same price (FIFO)
    let buy_x = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 5, uid_x);
    book.add_order(&buy_x, &mut generator);

    // Y's buy is buried behind X's buy at the same price
    let buy_y = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 5, uid_y);
    book.add_order(&buy_y, &mut generator);

    // Y aggressively buys 5 at 50000. The front bid (X's) only has
    // self-trades at the level, but Y's buried bid can match X's sell.
    let buy_y2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 5, uid_y);
    let trades = book.add_order(&buy_y2, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].get_bid_trade_info().get_user_id(), uid_y);
    assert_eq!(trades[0].get_ask_trade_info().get_user_id(), uid_x);
    // Only X's unfillable buy remains; the level was not dropped.
    assert_eq!(book.size(), 1);
    assert!(book.has_order(&buy_x.get_order_id()));
}

#[test]
fn self_trade_does_not_hide_other_users_buried_same_side_order_sell() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid_x = uid_a();
    let uid_y = uid_b();

    let buy_x = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_x);
    book.add_order(&buy_x, &mut generator);

    let sell_x = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_x);
    book.add_order(&sell_x, &mut generator);

    let sell_y = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_y);
    book.add_order(&sell_y, &mut generator);

    let sell_y2 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_y);
    let trades = book.add_order(&sell_y2, &mut generator).unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].get_bid_trade_info().get_user_id(), uid_x);
    assert_eq!(trades[0].get_ask_trade_info().get_user_id(), uid_y);
    // Only X's unfillable sell remains; the level was not dropped.
    assert_eq!(book.size(), 1);
    assert!(book.has_order(&sell_x.get_order_id()));
}

// ========== FillAndKill (FAK) ==========

#[test]
fn fak_no_resting_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
    assert_eq!(book.size(), 0);
}

#[test]
fn fak_full_match_returns_trades() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn fak_partial_remainder_discarded() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn fak_does_not_enter_book() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);
    assert_eq!(book.size(), 0);
}

#[test]
fn fak_price_below_best_ask_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

#[test]
fn fak_matches_multiple_levels() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid_b());
    book.add_order(&sell1, &mut generator);
    book.add_order(&sell2, &mut generator);

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 51000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 2);
    assert_eq!(book.size(), 0);
}

#[test]
fn fak_self_trade_skipped() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    book.add_order(&sell, &mut generator);

    // FAK from same user — self-trade skipped, returns Some(empty)
    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_some());
    assert!(trades.unwrap().is_empty());
}

#[test]
fn fak_not_in_orders_map_after() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillAndKill, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);
    assert!(!book.has_order(&buy.get_order_id()));
}

// ========== FillOrKill (FOK) ==========

#[test]
fn fok_sufficient_liquidity_fills() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn fok_insufficient_liquidity_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
    assert_eq!(book.size(), 1);
}

#[test]
fn fok_exactly_enough_liquidity() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].get_bid_trade_info().get_quantity(), 10);
}

#[test]
fn fok_self_trade_excluded_from_check() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid);
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid);
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

#[test]
fn fok_partial_unfilled_returns_none_safety_net() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 20, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

#[test]
fn fok_rejected_leaves_resting_orders_untouched() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 20, uid_a());
    assert!(book.add_order(&buy, &mut generator).is_none());

    // Atomicity: the rejected FOK must not have partially consumed the
    // resting order or changed its remaining quantity.
    assert_eq!(book.size(), 1);
    let info = book.get_order_info();
    assert_eq!(info.get_asks().len(), 1);
    assert_eq!(info.get_asks()[0].quantity, 10);
}

#[test]
fn fok_rejected_leaves_multiple_levels_untouched() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell1 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell1, &mut generator);
    let sell2 = make_order(OrderType::GoodTillCancel, Side::Sell, 50100, 5, uid_b());
    book.add_order(&sell2, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50200, 15, uid_a());
    assert!(book.add_order(&buy, &mut generator).is_none());

    assert_eq!(book.size(), 2);
    let info = book.get_order_info();
    assert_eq!(info.get_asks().len(), 2);
    assert_eq!(info.get_asks()[0].quantity, 5);
    assert_eq!(info.get_asks()[1].quantity, 5);
}

#[test]
fn fok_does_not_enter_book() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);
    assert_eq!(book.size(), 0);
}

#[test]
fn fok_no_opposite_orders_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

#[test]
fn fok_enough_total_but_spread_gap_partial_fails() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::FillOrKill, Side::Buy, 50000, 10, uid_a());
    let trades = book.add_order(&buy, &mut generator);
    assert!(trades.is_none());
}

// ========== Cancel ==========

#[test]
fn cancel_existing_returns_order_decrements_size() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    assert_eq!(book.size(), 1);

    let cancelled = book.cancel_order(&order.get_order_id());
    assert!(cancelled.is_some());
    assert_eq!(book.size(), 0);
}

#[test]
fn cancel_nonexistent_returns_none() {
    let mut book = new_book();
    assert!(book.cancel_order(&999).is_none());
}

#[test]
fn cancel_one_of_many_at_level() {
    let mut book = new_book();
    let mut generator = make_generator();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 20, uid_b());
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);

    book.cancel_order(&o1.get_order_id());
    assert_eq!(book.size(), 1);
    let info = book.get_order_info();
    assert_eq!(info.get_bids()[0].quantity, 20);
}

#[test]
fn cancel_last_at_level_removes_level() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);

    book.cancel_order(&order.get_order_id());
    let info = book.get_order_info();
    assert!(info.get_bids().is_empty());
}

#[test]
fn cancel_from_bids_and_asks() {
    let mut book = new_book();
    let mut generator = make_generator();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&buy, &mut generator);
    book.add_order(&sell, &mut generator);

    book.cancel_order(&buy.get_order_id());
    assert_eq!(book.size(), 1);
    book.cancel_order(&sell.get_order_id());
    assert_eq!(book.size(), 0);
}

#[test]
fn cancel_then_readd_same_id_succeeds() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let id = order.get_order_id();
    book.add_order(&order, &mut generator);
    book.cancel_order(&id);

    let order2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let result = book.add_order(&order2, &mut generator);
    assert!(result.is_some());
}

#[test]
fn cancel_partially_filled_order() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let cancelled = book.cancel_order(&buy.get_order_id());
    assert!(cancelled.is_some());
    assert_eq!(book.size(), 0);
}

// ========== Modify ==========

#[test]
fn modify_existing_old_removed_new_added() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let id = order.get_order_id();
    book.add_order(&order, &mut generator);

    let modify = make_modify(id, 51000, Side::Buy, 20, uid_a());
    book.modify_order(modify, &mut generator);

    assert!(book.has_order(&id));
    let info = book.get_order_info();
    assert_eq!(info.get_bids()[0].price, 51000);
    assert_eq!(info.get_bids()[0].quantity, 20);
}

#[test]
fn modify_to_crossing_price_triggers_fill() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let modify = make_modify(buy.get_order_id(), 50000, Side::Buy, 10, uid_a());
    let trades = book.modify_order(modify, &mut generator).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(book.size(), 0);
}

#[test]
fn modify_nonexistent_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let modify = make_modify(999, 50000, Side::Buy, 10, uid_a());
    let result = book.modify_order(modify, &mut generator);
    assert!(result.is_none());
}

#[test]
fn modify_preserves_order_type() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let id = order.get_order_id();
    book.add_order(&order, &mut generator);

    let modify = make_modify(id, 51000, Side::Buy, 20, uid_a());
    book.modify_order(modify, &mut generator);

    assert_eq!(book.get_order_type(&id), Some(OrderType::GoodTillCancel));
}

#[test]
fn modify_changes_price_and_quantity() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let id = order.get_order_id();
    book.add_order(&order, &mut generator);

    let modify = make_modify(id, 48000, Side::Buy, 25, uid_a());
    book.modify_order(modify, &mut generator);

    let info = book.get_order_info();
    assert_eq!(info.get_bids().len(), 1);
    assert_eq!(info.get_bids()[0].price, 48000);
    assert_eq!(info.get_bids()[0].quantity, 25);
}

#[test]
fn modify_partially_filled_order() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 5, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let modify = make_modify(buy.get_order_id(), 49000, Side::Buy, 20, uid_a());
    book.modify_order(modify, &mut generator);

    let info = book.get_order_info();
    assert_eq!(info.get_bids().len(), 1);
    assert_eq!(info.get_bids()[0].price, 49000);
    assert_eq!(info.get_bids()[0].quantity, 20);
}

#[test]
fn modify_triggers_self_trade_prevention() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid);
    book.add_order(&buy, &mut generator);

    let modify = make_modify(buy.get_order_id(), 50000, Side::Buy, 10, uid);
    let trades = book.modify_order(modify, &mut generator);
    assert!(trades.is_some());
}

// ========== Duplicate Order Rejection ==========

#[test]
fn add_order_duplicate_id_returns_none() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let id = order.get_order_id();
    book.add_order(&order, &mut generator);

    let order2 = make_order_with_id(id, OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let result = book.add_order(&order2, &mut generator);
    assert!(result.is_none());
}

#[test]
fn add_order_after_cancel_same_id_succeeds() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    book.cancel_order(&order.get_order_id());

    let order2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let result = book.add_order(&order2, &mut generator);
    assert!(result.is_some());
}

// ========== OrderBook Level Info ==========

#[test]
fn get_order_info_reflects_remaining_not_initial() {
    let mut book = new_book();
    let mut generator = make_generator();

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 20, uid_b());
    book.add_order(&sell, &mut generator);

    let buy = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&buy, &mut generator);

    let info = book.get_order_info();
    assert_eq!(info.get_asks()[0].quantity, 10);
}

#[test]
fn get_order_info_bids_descending_best_first() {
    let mut book = new_book();
    let mut generator = make_generator();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 48000, 10, uid_a());
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let o3 = make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 10, uid_a());
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);
    book.add_order(&o3, &mut generator);

    let info = book.get_order_info();
    let bids = info.get_bids();
    // push_front reverses BTreeMap ascending → descending, best bid first
    assert_eq!(bids[0].price, 50000);
    assert_eq!(bids[1].price, 49000);
    assert_eq!(bids[2].price, 48000);
}

#[test]
fn get_order_info_asks_ascending_best_first() {
    let mut book = new_book();
    let mut generator = make_generator();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Sell, 52000, 10, uid_b());
    let o2 = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 10, uid_b());
    let o3 = make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 10, uid_b());
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);
    book.add_order(&o3, &mut generator);

    let info = book.get_order_info();
    let asks = info.get_asks();
    // Asks are sorted ascending by price — best ask first
    assert_eq!(asks[0].price, 50000);
    assert_eq!(asks[1].price, 51000);
    assert_eq!(asks[2].price, 52000);
}

#[test]
fn get_order_info_after_cancel_removes_level() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    book.cancel_order(&order.get_order_id());

    let info = book.get_order_info();
    assert!(info.get_bids().is_empty());
}

#[test]
fn get_order_info_aggregates_same_price_orders() {
    let mut book = new_book();
    let mut generator = make_generator();

    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 20, uid_b());
    let o3 = make_order(
        OrderType::GoodTillCancel,
        Side::Buy,
        50000,
        30,
        make_user_id(),
    );
    book.add_order(&o1, &mut generator);
    book.add_order(&o2, &mut generator);
    book.add_order(&o3, &mut generator);

    let info = book.get_order_info();
    assert_eq!(info.get_bids().len(), 1);
    assert_eq!(info.get_bids()[0].quantity, 60);
}

// ========== Duplicate ID Rejection ==========

#[test]
fn duplicate_gtc_buy_id_rejected() {
    let mut book = new_book();
    let mut generator = make_generator();

    let order = make_order_with_id(42, OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    let result1 = book.add_order(&order, &mut generator);
    assert!(result1.is_some());

    let order2 = make_order_with_id(42, OrderType::GoodTillCancel, Side::Buy, 51000, 5, uid_a());
    let result2 = book.add_order(&order2, &mut generator);
    assert!(result2.is_none());
}

#[test]
fn duplicate_fak_id_not_rejected_because_never_rests() {
    let mut book = new_book();
    let mut generator = make_generator();

    // Resting sell with enough qty for two FAK fills
    let resting = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 20, uid_b());
    book.add_order(&resting, &mut generator);

    // First FAK matches 5
    let fak = make_order_with_id(42, OrderType::FillAndKill, Side::Buy, 50000, 5, uid_a());
    let t1 = book.add_order(&fak, &mut generator).unwrap();
    assert_eq!(t1.len(), 1);

    // Second FAK with same ID succeeds because FAK never enters orders_map
    let fak2 = make_order_with_id(42, OrderType::FillAndKill, Side::Buy, 50000, 5, uid_a());
    let t2 = book.add_order(&fak2, &mut generator).unwrap();
    assert_eq!(t2.len(), 1);
}

#[test]
fn duplicate_fok_id_not_rejected_because_never_rests() {
    let mut book = new_book();
    let mut generator = make_generator();

    let resting = make_order(OrderType::GoodTillCancel, Side::Sell, 50000, 20, uid_b());
    book.add_order(&resting, &mut generator);

    let fok = make_order_with_id(42, OrderType::FillOrKill, Side::Buy, 50000, 5, uid_a());
    let t1 = book.add_order(&fok, &mut generator).unwrap();
    assert_eq!(t1.len(), 1);

    let fok2 = make_order_with_id(42, OrderType::FillOrKill, Side::Buy, 50000, 5, uid_a());
    let t2 = book.add_order(&fok2, &mut generator).unwrap();
    assert_eq!(t2.len(), 1);
}

// ========== Multi-Level Cascading ==========

#[test]
fn many_bids_one_sell_cascading_fill() {
    let mut book = new_book();
    let mut generator = make_generator();

    for price in 45000..55000 {
        let buy = make_order(OrderType::GoodTillCancel, Side::Buy, price, 5, uid_a());
        book.add_order(&buy, &mut generator);
    }

    let sell = make_order(OrderType::GoodTillCancel, Side::Sell, 45000, 50, uid_b());
    let trades = book.add_order(&sell, &mut generator).unwrap();

    assert_eq!(trades.len(), 10);
    assert_eq!(book.size(), 9990);
}

#[test]
fn empty_book_lifecycle() {
    let mut book = new_book();
    let mut generator = make_generator();

    assert_eq!(book.size(), 0);

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    assert_eq!(book.size(), 1);

    book.cancel_order(&order.get_order_id());
    assert_eq!(book.size(), 0);
}

#[test]
fn cancel_orders_for_user_none_found() {
    let mut book = new_book();
    let mut generator = make_generator();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a());
    book.add_order(&order, &mut generator);
    let removed = book.cancel_orders_for_user(make_user_id());
    assert!(removed.is_empty());
    assert_eq!(book.size(), 1);
}

#[test]
fn cancel_orders_for_user_removes_all() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 8, uid_b()),
        &mut generator,
    );
    assert_eq!(book.size(), 3);
    let removed = book.cancel_orders_for_user(uid);
    assert_eq!(removed.len(), 2);
    assert_eq!(book.size(), 1);
}

#[test]
fn cancel_orders_for_user_at_multiple_price_levels() {
    let mut book = new_book();
    let mut generator = make_generator();
    let uid = uid_a();
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 51000, 15, uid),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Sell, 52000, 20, uid),
        &mut generator,
    );
    assert_eq!(book.size(), 3);
    let removed = book.cancel_orders_for_user(uid);
    assert_eq!(removed.len(), 3);
    assert_eq!(book.size(), 0);
}

#[test]
fn cancel_orders_for_user_preserves_other_users_orders() {
    let mut book = new_book();
    let mut generator = make_generator();
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid_a()),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Sell, 51000, 5, uid_b()),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Buy, 49000, 8, uid_a()),
        &mut generator,
    );
    book.add_order(
        &make_order(OrderType::GoodTillCancel, Side::Sell, 52000, 6, uid_b()),
        &mut generator,
    );
    assert_eq!(book.size(), 4);
    let removed = book.cancel_orders_for_user(uid_a());
    assert_eq!(removed.len(), 2);
    assert_eq!(book.size(), 2);
    let removed = book.cancel_orders_for_user(uid_b());
    assert_eq!(removed.len(), 2);
    assert_eq!(book.size(), 0);
}
