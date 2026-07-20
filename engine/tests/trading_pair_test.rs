use std::collections::HashMap;
use vertex_engine::trading_pair::TradingPair;
use vertex_engine::types::Asset;

#[test]
fn new_stores_base_and_quote() {
    let pair = TradingPair::new(Asset::ETH, Asset::USDC);
    assert_eq!(pair.base, Asset::ETH);
    assert_eq!(pair.quote, Asset::USDC);
}

#[test]
fn display_format() {
    let pair = TradingPair::new(Asset::ETH, Asset::USDC);
    assert_eq!(pair.to_string(), "ETH-USDC");

    let pair2 = TradingPair::new(Asset::BTC, Asset::USDT);
    assert_eq!(pair2.to_string(), "BTC-USDT");
}

#[test]
fn equality() {
    let a = TradingPair::new(Asset::ETH, Asset::USDC);
    let b = TradingPair::new(Asset::ETH, Asset::USDC);
    let c = TradingPair::new(Asset::BTC, Asset::USDC);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn hash_usable_as_hashmap_key() {
    let mut map = HashMap::new();
    let pair = TradingPair::new(Asset::ETH, Asset::USDC);
    map.insert(pair, 42);
    assert_eq!(map.get(&pair), Some(&42));
}

#[test]
fn different_pairs_distinct() {
    let a = TradingPair::new(Asset::ETH, Asset::USDC);
    let b = TradingPair::new(Asset::BTC, Asset::USDC);
    let c = TradingPair::new(Asset::ETH, Asset::USDT);
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
}
