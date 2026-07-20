use std::collections::HashSet;
use vertex_engine::types::{Asset, OrderStatus, OrderType, Side};

#[test]
fn asset_display_all_variants() {
    assert_eq!(Asset::ETH.to_string(), "ETH");
    assert_eq!(Asset::SOL.to_string(), "SOL");
    assert_eq!(Asset::BTC.to_string(), "BTC");
    assert_eq!(Asset::USDC.to_string(), "USDC");
    assert_eq!(Asset::USDT.to_string(), "USDT");
}

#[test]
fn order_type_equality_and_copy() {
    let a = OrderType::GoodTillCancel;
    let b = a;
    assert_eq!(a, b);
    assert_ne!(OrderType::FillAndKill, OrderType::FillOrKill);
    assert_ne!(OrderType::GoodTillCancel, OrderType::GoodForDay);
}

#[test]
fn side_equality_and_copy() {
    let a = Side::Buy;
    let b = a;
    assert_eq!(a, b);
    assert_ne!(Side::Buy, Side::Sell);
}

#[test]
fn order_status_all_distinct() {
    let statuses = [
        OrderStatus::Cancelled,
        OrderStatus::PartiallyFilled,
        OrderStatus::Filled,
        OrderStatus::Empty,
    ];
    for i in 0..statuses.len() {
        for j in (i + 1)..statuses.len() {
            assert_ne!(statuses[i], statuses[j]);
        }
    }
}

#[test]
fn asset_hash_consistency() {
    let mut set = HashSet::new();
    set.insert(Asset::ETH);
    set.insert(Asset::ETH);
    set.insert(Asset::BTC);
    assert_eq!(set.len(), 2);
    assert!(set.contains(&Asset::ETH));
    assert!(set.contains(&Asset::BTC));
    assert!(!set.contains(&Asset::SOL));
}
