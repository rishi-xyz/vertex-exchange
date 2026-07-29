use std::{collections::HashMap, path::Path};

use crate::{
    engine::{
        CoreEngine,
        trade_def::{ExchangeEngine, UsersEngine},
    },
    types::{Asset, OrderId, Quantity, WalEntryType},
    wal::{WalEntry, WalReader, WalWriter},
};

pub struct WalEngine {
    inner: CoreEngine,
    writer: WalWriter,
}

impl WalEngine {
    pub fn new(
        machine_id: u64,
        datacenter_id: u64,
        wal_path: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let path = wal_path.as_ref();
        // replay existing wal to recover state
        let mut engine = CoreEngine::new(machine_id, datacenter_id);
        let mut max_seq: u64 = 0;

        if path.exists() {
            let reader = WalReader::new(path)?;
            for entry in reader {
                Self::replay_entry(&mut engine, &entry);
                if entry.seq > max_seq {
                    max_seq = entry.seq
                }
            }
        }

        // open writer to continue writing logs
        let mut writer = WalWriter::new(path)?;
        writer.set_seq(max_seq);

        Ok(WalEngine {
            inner: engine,
            writer,
        })
    }

    pub fn core(&self) -> &CoreEngine {
        &self.inner
    }

    pub fn next_id(&mut self) -> OrderId {
        self.inner.next_id()
    }

    /// replay function
    fn replay_entry(engine: &mut CoreEngine, entry: &WalEntry) {
        match &entry.entry {
            WalEntryType::AddTradingPair { pair } => {
                engine.add_trading_pair(*pair);
            }
            WalEntryType::RemoveTradingPair { pair } => {
                engine.remove_trading_pair(pair);
            }
            WalEntryType::AddOrder {
                pair,
                order,
                trades: _,
            } => {
                engine.add_order(pair, order);
            }
            WalEntryType::CancelOrder {
                pair,
                order_id,
                success: _,
            } => {
                engine.cancel_order(pair, order_id);
            }
            WalEntryType::ModifyOrder {
                pair,
                modify,
                trades: _,
            } => {
                engine.modify_order(pair, modify.clone());
            }
            WalEntryType::AddUser { user_id } => {
                engine.add_user(*user_id);
            }
            WalEntryType::RemoveUser { user_id } => {
                let _ = engine.remove_user(*user_id);
            }
            WalEntryType::DepositBalance {
                user_id,
                asset,
                quantity,
            } => {
                let _ = engine.deposit_balance(*user_id, *asset, *quantity);
            }
            WalEntryType::WithdrawBalance {
                user_id,
                asset,
                quantity,
            } => {
                let _ = engine.withdraw_balance(*user_id, *asset, *quantity);
            }
        }
    }
}

impl ExchangeEngine for WalEngine {
    fn add_trading_pair(&mut self, pair: crate::trading_pair::TradingPair) {
        let entry = WalEntry::new(0, WalEntryType::AddTradingPair { pair });
        self.writer.write(entry).unwrap();
        self.inner.add_trading_pair(pair);
    }

    fn remove_trading_pair(
        &mut self,
        pair: &crate::trading_pair::TradingPair,
    ) -> Option<crate::orderbook::OrderBook> {
        let entry = WalEntry::new(0, WalEntryType::RemoveTradingPair { pair: *pair });
        self.writer.write(entry).unwrap();
        self.inner.remove_trading_pair(pair)
    }

    fn add_order(
        &mut self,
        pair: &crate::trading_pair::TradingPair,
        order: &crate::order::Order,
    ) -> Option<crate::trade::Trades> {
        let entry = WalEntry::new(
            0,
            WalEntryType::AddOrder {
                pair: *pair,
                order: *order,
                trades: None,
            },
        );
        self.writer.write(entry).unwrap();
        self.inner.add_order(pair, order)
    }

    fn cancel_order(
        &mut self,
        pair: &crate::trading_pair::TradingPair,
        order_id: &crate::types::OrderId,
    ) -> bool {
        let entry = WalEntry::new(
            0,
            WalEntryType::CancelOrder {
                pair: *pair,
                order_id: *order_id,
                success: true,
            },
        );
        self.writer.write(entry).unwrap();
        self.inner.cancel_order(pair, order_id)
    }

    fn modify_order(
        &mut self,
        pair: &crate::trading_pair::TradingPair,
        modify_order: crate::order_modify::OrderModify,
    ) -> Option<crate::trade::Trades> {
        let entry = WalEntry::new(
            0,
            WalEntryType::ModifyOrder {
                pair: *pair,
                modify: modify_order.clone(),
                trades: None,
            },
        );
        self.writer.write(entry).unwrap();
        self.inner.modify_order(pair, modify_order)
    }

    fn get_order_info(
        &self,
        pair: &crate::trading_pair::TradingPair,
    ) -> Option<crate::level_info::OrderBookLevelInfo> {
        self.inner.get_order_info(pair)
    }

    fn size(&self, pair: &crate::trading_pair::TradingPair) -> Option<usize> {
        self.inner.size(pair)
    }
}

impl UsersEngine for WalEngine {
    fn add_user(&mut self, user_id: crate::types::UserId) {
        let entry = WalEntry::new(
            0, // filler will be replaced by atual seq value by writer function
            WalEntryType::AddUser { user_id },
        );
        self.writer.write(entry).unwrap();
        self.inner.add_user(user_id);
    }
    fn remove_user(
        &mut self,
        user_id: crate::types::UserId,
    ) -> Result<HashMap<Asset, Quantity>, String> {
        let entry = WalEntry::new(0, WalEntryType::RemoveUser { user_id });
        self.writer.write(entry).unwrap();
        self.inner.remove_user(user_id)
    }

    fn deposit_balance(
        &mut self,
        user_id: crate::types::UserId,
        asset: crate::types::Asset,
        quantity: crate::types::Quantity,
    ) -> Result<(), String> {
        let entry = WalEntry::new(
            0,
            WalEntryType::DepositBalance {
                user_id,
                asset,
                quantity,
            },
        );
        self.writer.write(entry).unwrap();
        self.inner.deposit_balance(user_id, asset, quantity)
    }

    fn withdraw_balance(
        &mut self,
        user_id: crate::types::UserId,
        asset: crate::types::Asset,
        quantity: crate::types::Quantity,
    ) -> Result<(), String> {
        let entry = WalEntry::new(
            0,
            WalEntryType::WithdrawBalance {
                user_id,
                asset,
                quantity,
            },
        );
        self.writer.write(entry).unwrap();
        self.inner.withdraw_balance(user_id, asset, quantity)
    }

    fn get_balance(&self, user_id: crate::types::UserId, asset: Asset) -> Result<Quantity, String> {
        self.inner.get_balance(user_id, asset)
    }
}
