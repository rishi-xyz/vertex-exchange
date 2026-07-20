mod helpers;

use vertex_engine::trade::{Trade, TradeInfo, Trades};

use helpers::make_user_id;

#[test]
fn trade_info_new_stores_fields() {
    let uid = make_user_id();
    let info = TradeInfo::new(42, 50000, 10, uid);
    assert_eq!(info.get_order_id(), 42);
    assert_eq!(info.get_price(), 50000);
    assert_eq!(info.get_quantity(), 10);
    assert_eq!(info.get_user_id(), uid);
}

#[test]
fn trade_new_sets_bid_ask_and_timestamp() {
    let uid1 = make_user_id();
    let uid2 = make_user_id();
    let bid = TradeInfo::new(1, 50000, 10, uid1);
    let ask = TradeInfo::new(2, 50000, 10, uid2);
    let trade = Trade::new(100, bid, ask);
    assert_eq!(trade.get_trade_id(), 100);
    assert!(trade.get_timestamp() > 0);
    assert_eq!(trade.get_bid_trade_info().get_order_id(), 1);
    assert_eq!(trade.get_ask_trade_info().get_order_id(), 2);
}

#[test]
fn set_trade_id_overwrites() {
    let uid1 = make_user_id();
    let uid2 = make_user_id();
    let mut trade = Trade::new(
        0,
        TradeInfo::new(1, 50000, 10, uid1),
        TradeInfo::new(2, 50000, 10, uid2),
    );
    assert_eq!(trade.get_trade_id(), 0);
    trade.set_trade_id(999);
    assert_eq!(trade.get_trade_id(), 999);
}

#[test]
fn set_timestamp_overwrites() {
    let uid1 = make_user_id();
    let uid2 = make_user_id();
    let mut trade = Trade::new(
        0,
        TradeInfo::new(1, 50000, 10, uid1),
        TradeInfo::new(2, 50000, 10, uid2),
    );
    let original_ts = trade.get_timestamp();
    trade.set_timestamp(12345);
    assert_eq!(trade.get_timestamp(), 12345);
    assert_ne!(trade.get_timestamp(), original_ts);
}

#[test]
fn trades_fifo_ordering() {
    let uid1 = make_user_id();
    let uid2 = make_user_id();
    let mut trades: Trades = Trades::new();
    let t1 = Trade::new(
        1,
        TradeInfo::new(1, 50000, 10, uid1),
        TradeInfo::new(2, 50000, 10, uid2),
    );
    let t2 = Trade::new(
        2,
        TradeInfo::new(3, 51000, 5, uid1),
        TradeInfo::new(4, 51000, 5, uid2),
    );
    trades.push_back(t1);
    trades.push_back(t2);
    assert_eq!(trades.pop_front().unwrap().get_trade_id(), 1);
    assert_eq!(trades.pop_front().unwrap().get_trade_id(), 2);
    assert!(trades.is_empty());
}
