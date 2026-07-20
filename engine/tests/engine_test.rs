mod helpers;

use vertex_engine::engine::Engine;
use vertex_engine::trading_pair::TradingPair;
use vertex_engine::types::{Asset, OrderType, Side};

use helpers::{make_modify, make_order, make_pair, make_user_id};

fn new_engine() -> Engine {
    Engine::new(1, 1)
}

fn eth_usdc() -> TradingPair {
    make_pair(Asset::ETH, Asset::USDC)
}

fn btc_usdc() -> TradingPair {
    make_pair(Asset::BTC, Asset::USDC)
}

#[test]
fn new_creates_empty_engine() {
    let engine = new_engine();
    assert!(engine.size(&eth_usdc()).is_none());
}

#[test]
fn add_trading_pair_adds_new() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());
    assert_eq!(engine.size(&eth_usdc()), Some(0));
}

#[test]
fn add_trading_pair_duplicate_noop() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());
    engine.add_trading_pair(eth_usdc());
    assert_eq!(engine.size(&eth_usdc()), Some(0));
}

#[test]
fn remove_trading_pair_returns_some() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());
    let removed = engine.remove_trading_pair(&eth_usdc());
    assert!(removed.is_some());
    assert!(engine.size(&eth_usdc()).is_none());
}

#[test]
fn remove_nonexistent_returns_none() {
    let mut engine = new_engine();
    assert!(engine.remove_trading_pair(&eth_usdc()).is_none());
}

#[test]
fn add_order_nonexistent_pair_returns_none() {
    let mut engine = new_engine();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, make_user_id());
    let result = engine.add_order(&eth_usdc(), &order);
    assert!(result.is_none());
}

#[test]
fn add_order_existing_pair_delegates() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, make_user_id());
    let result = engine.add_order(&eth_usdc(), &order);
    assert!(result.is_some());
    assert_eq!(engine.size(&eth_usdc()), Some(1));
}

#[test]
fn cancel_order_existing_pair_returns_true() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());

    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, make_user_id());
    engine.add_order(&eth_usdc(), &order);
    let result = engine.cancel_order(&eth_usdc(), &order.get_order_id());
    assert!(result);
    assert_eq!(engine.size(&eth_usdc()), Some(0));
}

#[test]
fn cancel_order_nonexistent_pair_returns_false() {
    let mut engine = new_engine();
    let result = engine.cancel_order(&eth_usdc(), &999);
    assert!(!result);
}

#[test]
fn cancel_nonexistent_order_on_existing_pair_returns_true() {
    // Documents the cancel_order bug: returns true if pair exists, regardless of order
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());
    let result = engine.cancel_order(&eth_usdc(), &999);
    assert!(result);
}

#[test]
fn modify_order_nonexistent_pair_returns_none() {
    let mut engine = new_engine();
    let modify = make_modify(42, 50000, Side::Buy, 10, make_user_id());
    let result = engine.modify_order(&eth_usdc(), modify);
    assert!(result.is_none());
}

#[test]
fn get_order_info_nonexistent_pair_returns_none() {
    let engine = new_engine();
    assert!(engine.get_order_info(&eth_usdc()).is_none());
}

#[test]
fn size_nonexistent_pair_returns_none() {
    let engine = new_engine();
    assert!(engine.size(&eth_usdc()).is_none());
}

#[test]
fn multiple_pairs_isolated_books() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());
    engine.add_trading_pair(btc_usdc());

    let uid = make_user_id();
    let eth_order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let btc_order = make_order(OrderType::GoodTillCancel, Side::Buy, 100000, 5, uid);

    engine.add_order(&eth_usdc(), &eth_order);
    engine.add_order(&btc_usdc(), &btc_order);

    assert_eq!(engine.size(&eth_usdc()), Some(1));
    assert_eq!(engine.size(&btc_usdc()), Some(1));

    engine.cancel_order(&eth_usdc(), &eth_order.get_order_id());
    assert_eq!(engine.size(&eth_usdc()), Some(0));
    assert_eq!(engine.size(&btc_usdc()), Some(1));
}

#[test]
fn order_ids_are_unique() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());

    let uid = make_user_id();
    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    engine.add_order(&eth_usdc(), &o1);
    let id1 = o1.get_order_id();

    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 51000, 10, uid);
    engine.add_order(&eth_usdc(), &o2);
    let id2 = o2.get_order_id();

    assert_ne!(id1, id2);
}

#[test]
fn full_lifecycle_add_add_cancel_check_size() {
    let mut engine = new_engine();
    engine.add_trading_pair(eth_usdc());

    let uid = make_user_id();
    let o1 = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    let o2 = make_order(OrderType::GoodTillCancel, Side::Buy, 51000, 20, uid);

    engine.add_order(&eth_usdc(), &o1);
    engine.add_order(&eth_usdc(), &o2);
    assert_eq!(engine.size(&eth_usdc()), Some(2));

    engine.cancel_order(&eth_usdc(), &o1.get_order_id());
    assert_eq!(engine.size(&eth_usdc()), Some(1));

    engine.cancel_order(&eth_usdc(), &o2.get_order_id());
    assert_eq!(engine.size(&eth_usdc()), Some(0));
}
