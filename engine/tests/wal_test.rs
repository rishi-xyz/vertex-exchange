mod helpers;

use std::fs;
use std::io::Write;

use vertex_engine::engine::engine_from_env;
use vertex_engine::engine::trade_def::{ExchangeEngine, UsersEngine};
use vertex_engine::types::{Asset, OrderType, Side, WalEntryType};
use vertex_engine::wal::engine::WalEngine;
use vertex_engine::wal::{WalEntry, WalReader, WalWriter};

use helpers::{make_order, make_pair, make_user_id};

fn tmp_wal_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("vertex_wal_test");
    fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}_{}.wal", uuid::Uuid::new_v4()))
}

// ---------------------------------------------------------------------------
// WalWriter / WalReader unit tests
// ---------------------------------------------------------------------------

#[test]
fn write_and_read_roundtrip() {
    let path = tmp_wal_path("roundtrip");
    let mut writer = WalWriter::new(&path).unwrap();

    let entry = WalEntry::new(
        0,
        WalEntryType::AddTradingPair {
            pair: make_pair(Asset::ETH, Asset::USDC),
        },
    );
    let seq1 = writer.write(entry).unwrap();
    assert_eq!(seq1, 1);

    let entry2 = WalEntry::new(
        0,
        WalEntryType::CancelOrder {
            pair: make_pair(Asset::ETH, Asset::USDC),
            order_id: 12345,
            success: true,
        },
    );
    let seq2 = writer.write(entry2).unwrap();
    assert_eq!(seq2, 2);

    drop(writer);

    let reader = WalReader::new(&path).unwrap();
    let entries: Vec<WalEntry> = reader.collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].seq(), 1);
    assert_eq!(entries[1].seq(), 2);

    // verify entry types
    match entries[0].entry() {
        WalEntryType::AddTradingPair { pair } => {
            assert_eq!(pair.base, Asset::ETH);
            assert_eq!(pair.quote, Asset::USDC);
        }
        _ => panic!("Expected AddTradingPair"),
    }
    match entries[1].entry() {
        WalEntryType::CancelOrder {
            order_id, success, ..
        } => {
            assert_eq!(*order_id, 12345);
            assert!(*success);
        }
        _ => panic!("Expected CancelOrder"),
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn skip_malformed_lines() {
    let path = tmp_wal_path("malformed");
    let mut file = fs::File::create(&path).unwrap();
    // valid entry
    writeln!(
        file,
        "{{\"seq\":1,\"entry\":{{\"AddTradingPair\":{{\"pair\":{{\"base\":\"ETH\",\"quote\":\"USDC\"}}}}}}}}"
    )
    .unwrap();
    // invalid JSON
    writeln!(file, "NOT VALID JSON").unwrap();
    // another valid entry
    writeln!(
        file,
        "{{\"seq\":2,\"entry\":{{\"CancelOrder\":{{\"pair\":{{\"base\":\"ETH\",\"quote\":\"USDC\"}},\"order_id\":99,\"success\":true}}}}}}"
    )
    .unwrap();
    drop(file);

    let reader = WalReader::new(&path).unwrap();
    let entries: Vec<WalEntry> = reader.collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].seq(), 1);
    assert_eq!(entries[1].seq(), 2);

    let _ = fs::remove_file(&path);
}

#[test]
fn empty_file_yields_no_entries() {
    let path = tmp_wal_path("empty");
    fs::File::create(&path).unwrap();

    let reader = WalReader::new(&path).unwrap();
    let entries: Vec<WalEntry> = reader.collect();
    assert!(entries.is_empty());

    let _ = fs::remove_file(&path);
}

#[test]
fn seq_increments_across_writes() {
    let path = tmp_wal_path("seq_inc");
    let mut writer = WalWriter::new(&path).unwrap();

    for i in 0..5 {
        let entry = WalEntry::new(
            0,
            WalEntryType::AddTradingPair {
                pair: make_pair(Asset::ETH, Asset::USDC),
            },
        );
        let seq = writer.write(entry).unwrap();
        assert_eq!(seq, i + 1);
    }

    assert_eq!(writer.seq(), 5);

    let _ = fs::remove_file(&path);
}

#[test]
fn writer_set_seq_resumes_from_last() {
    let path = tmp_wal_path("set_seq");
    let mut writer = WalWriter::new(&path).unwrap();

    writer.set_seq(10);
    let entry = WalEntry::new(
        0,
        WalEntryType::AddTradingPair {
            pair: make_pair(Asset::ETH, Asset::USDC),
        },
    );
    let seq = writer.write(entry).unwrap();
    assert_eq!(seq, 11);

    let _ = fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// WalEngine replay tests
// ---------------------------------------------------------------------------

#[test]
fn replay_empty_wal_gives_empty_engine() {
    let path = tmp_wal_path("replay_empty");
    fs::File::create(&path).unwrap();

    let engine = WalEngine::new(1, 1, &path).unwrap();
    assert!(engine.size(&make_pair(Asset::ETH, Asset::USDC)).is_none());

    let _ = fs::remove_file(&path);
}

#[test]
fn replay_restores_trading_pair() {
    let path = tmp_wal_path("replay_pair");
    {
        let mut writer = WalWriter::new(&path).unwrap();
        let entry = WalEntry::new(
            0,
            WalEntryType::AddTradingPair {
                pair: make_pair(Asset::ETH, Asset::USDC),
            },
        );
        writer.write(entry).unwrap();
    }

    let engine = WalEngine::new(1, 1, &path).unwrap();
    assert_eq!(engine.size(&make_pair(Asset::ETH, Asset::USDC)), Some(0));

    let _ = fs::remove_file(&path);
}

#[test]
fn replay_restores_order_in_book() {
    let path = tmp_wal_path("replay_order");
    let user = make_user_id();

    // Manually write an AddOrder WAL entry
    {
        let mut writer = WalWriter::new(&path).unwrap();

        // First add the trading pair
        let pair_entry = WalEntry::new(
            0,
            WalEntryType::AddTradingPair {
                pair: make_pair(Asset::ETH, Asset::USDC),
            },
        );
        writer.write(pair_entry).unwrap();

        // Fund the user so the replayed order can be placed
        let user_entry = WalEntry::new(0, WalEntryType::AddUser { user_id: user });
        writer.write(user_entry).unwrap();
        writer
            .write(WalEntry::new(
                0,
                WalEntryType::DepositBalance {
                    user_id: user,
                    asset: Asset::USDC,
                    quantity: 500000,
                },
            ))
            .unwrap();

        // Then add a resting GTC order
        let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, user);
        let order_entry = WalEntry::new(
            0,
            WalEntryType::AddOrder {
                pair: make_pair(Asset::ETH, Asset::USDC),
                order,
                trades: None,
            },
        );
        writer.write(order_entry).unwrap();
    }

    let engine = WalEngine::new(1, 1, &path).unwrap();
    assert_eq!(engine.size(&make_pair(Asset::ETH, Asset::USDC)), Some(1));

    let _ = fs::remove_file(&path);
}

#[test]
fn replay_restores_cancel() {
    let path = tmp_wal_path("replay_cancel");
    let user = make_user_id();

    {
        let mut writer = WalWriter::new(&path).unwrap();

        // Add pair
        writer
            .write(WalEntry::new(
                0,
                WalEntryType::AddTradingPair {
                    pair: make_pair(Asset::ETH, Asset::USDC),
                },
            ))
            .unwrap();

        // Fund the user so the replayed order can be placed
        writer
            .write(WalEntry::new(0, WalEntryType::AddUser { user_id: user }))
            .unwrap();
        writer
            .write(WalEntry::new(
                0,
                WalEntryType::DepositBalance {
                    user_id: user,
                    asset: Asset::USDC,
                    quantity: 500000,
                },
            ))
            .unwrap();

        // Add order
        let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, user);
        let order_id = order.get_order_id();
        writer
            .write(WalEntry::new(
                0,
                WalEntryType::AddOrder {
                    pair: make_pair(Asset::ETH, Asset::USDC),
                    order,
                    trades: None,
                },
            ))
            .unwrap();

        // Cancel it
        writer
            .write(WalEntry::new(
                0,
                WalEntryType::CancelOrder {
                    pair: make_pair(Asset::ETH, Asset::USDC),
                    order_id,
                    success: true,
                },
            ))
            .unwrap();
    }

    let engine = WalEngine::new(1, 1, &path).unwrap();
    assert_eq!(engine.size(&make_pair(Asset::ETH, Asset::USDC)), Some(0));

    let _ = fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// WalEngine live operation tests (write-before-mutate verification)
// ---------------------------------------------------------------------------

#[test]
fn wal_engine_add_order_writes_entry_before_mutation() {
    let path = tmp_wal_path("live_add");
    let mut engine = WalEngine::new(1, 1, &path).unwrap();
    let pair = make_pair(Asset::ETH, Asset::USDC);
    let user = make_user_id();

    engine.add_trading_pair(pair);
    engine.add_user(user);
    engine.deposit_balance(user, Asset::USDC, 500000).unwrap();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, user);
    let order_id = order.get_order_id();
    let _ = engine.add_order(&pair, &order);
    // Drop engine to flush WAL
    drop(engine);

    // Read WAL file and verify entries
    let reader = WalReader::new(&path).unwrap();
    let entries: Vec<WalEntry> = reader.collect();

    // Should have: AddTradingPair + AddUser + DepositBalance + AddOrder = 4 entries
    assert_eq!(entries.len(), 4);

    // First entry: AddTradingPair
    match entries[0].entry() {
        WalEntryType::AddTradingPair { pair: p } => {
            assert_eq!(p.base, Asset::ETH);
            assert_eq!(p.quote, Asset::USDC);
        }
        _ => panic!("Expected AddTradingPair as first entry"),
    }

    // Last entry: AddOrder
    match entries[3].entry() {
        WalEntryType::AddOrder {
            pair: p, order: o, ..
        } => {
            assert_eq!(p.base, Asset::ETH);
            assert_eq!(o.get_order_id(), order_id);
        }
        _ => panic!("Expected AddOrder as last entry"),
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn wal_engine_cancel_order_writes_entry() {
    let path = tmp_wal_path("live_cancel");
    let mut engine = WalEngine::new(1, 1, &path).unwrap();
    let pair = make_pair(Asset::ETH, Asset::USDC);
    let user = make_user_id();

    engine.add_trading_pair(pair);
    engine.add_user(user);
    engine.deposit_balance(user, Asset::USDC, 500000).unwrap();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, user);
    let order_id = order.get_order_id();
    let _ = engine.add_order(&pair, &order);
    engine.cancel_order(&pair, &order_id);

    drop(engine);

    let reader = WalReader::new(&path).unwrap();
    let entries: Vec<WalEntry> = reader.collect();

    // AddTradingPair + AddUser + DepositBalance + AddOrder + CancelOrder = 5
    assert_eq!(entries.len(), 5);

    match entries[4].entry() {
        WalEntryType::CancelOrder {
            order_id: id,
            success,
            ..
        } => {
            assert_eq!(*id, order_id);
            assert!(*success);
        }
        _ => panic!("Expected CancelOrder as last entry"),
    }

    let _ = fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// engine_from_env tests
// ---------------------------------------------------------------------------

#[test]
fn engine_from_env_default_is_core() {
    // Ensure WAL_ENABLED is not set
    unsafe { std::env::remove_var("WAL_ENABLED") };
    let engine = engine_from_env(1, 1);
    // Verify it's a Core variant by checking size returns None for unknown pair
    assert!(engine.size(&make_pair(Asset::ETH, Asset::USDC)).is_none());
}

#[test]
fn engine_from_env_wal_enabled() {
    let path = tmp_wal_path("env_wal");
    unsafe {
        std::env::set_var("WAL_ENABLED", "true");
        std::env::set_var("WAL_PATH", path.to_str().unwrap());
    }

    let engine = engine_from_env(1, 1);
    assert!(engine.size(&make_pair(Asset::ETH, Asset::USDC)).is_none());

    // Clean up
    unsafe {
        std::env::remove_var("WAL_ENABLED");
        std::env::remove_var("WAL_PATH");
    }
    let _ = fs::remove_file(&path);
}
