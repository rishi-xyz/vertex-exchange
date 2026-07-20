use vertex_engine::level_info::{LevelInfo, OrderBookLevelInfo};

#[test]
fn level_info_new() {
    let level = LevelInfo::new(50000, 100);
    assert_eq!(level.price, 50000);
    assert_eq!(level.quantity, 100);
}

#[test]
fn orderbook_level_info_stores_bids_and_asks() {
    let bids = vec![LevelInfo::new(49000, 50), LevelInfo::new(48000, 100)];
    let asks = vec![LevelInfo::new(51000, 30), LevelInfo::new(52000, 60)];
    let info = OrderBookLevelInfo::new(bids.into(), asks.into());
    assert_eq!(info.get_bids().len(), 2);
    assert_eq!(info.get_asks().len(), 2);
}

#[test]
fn empty_bids_and_asks() {
    let info = OrderBookLevelInfo::new(Vec::new().into(), Vec::new().into());
    assert!(info.get_bids().is_empty());
    assert!(info.get_asks().is_empty());
}

#[test]
fn multiple_levels_sorted_correctly() {
    let bids = vec![
        LevelInfo::new(48000, 10),
        LevelInfo::new(49000, 20),
        LevelInfo::new(50000, 30),
    ];
    let asks = vec![
        LevelInfo::new(51000, 10),
        LevelInfo::new(52000, 20),
        LevelInfo::new(53000, 30),
    ];
    let info = OrderBookLevelInfo::new(bids.into(), asks.into());
    let bids_ref = info.get_bids();
    assert_eq!(bids_ref[0].price, 48000);
    assert_eq!(bids_ref[1].price, 49000);
    assert_eq!(bids_ref[2].price, 50000);
    let asks_ref = info.get_asks();
    assert_eq!(asks_ref[0].price, 51000);
    assert_eq!(asks_ref[1].price, 52000);
    assert_eq!(asks_ref[2].price, 53000);
}
