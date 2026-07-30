pub mod trade_def;

use std::collections::HashMap;
use tracing;

use crate::{
    engine::trade_def::{ExchangeEngine, UsersEngine},
    level_info::OrderBookLevelInfo,
    order::Order,
    order_modify::OrderModify,
    orderbook::OrderBook,
    snowflake_id::SnowFlakeGenerator,
    trade::Trades,
    trading_pair::TradingPair,
    types::{Asset, OrderId, Quantity, UserId},
    user::User,
    wal::engine::WalEngine,
};

pub struct CoreEngine {
    orderbooks: HashMap<TradingPair, OrderBook>,
    users: HashMap<UserId, User>,
    generator: SnowFlakeGenerator,
}

impl CoreEngine {
    pub fn new(machine_id: u64, datacenter_id: u64) -> Self {
        CoreEngine {
            orderbooks: HashMap::new(),
            users: HashMap::new(),
            generator: SnowFlakeGenerator::new(machine_id, datacenter_id),
        }
    }

    pub fn next_id(&mut self) -> OrderId {
        self.generator.next_id()
    }
}

impl ExchangeEngine for CoreEngine {
    // create orderbook
    fn add_trading_pair(&mut self, pair: TradingPair) {
        tracing::debug!(%pair, "add_trading_pair");
        self.orderbooks.entry(pair).or_insert(OrderBook::new());
    }

    fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook> {
        tracing::debug!(%pair, "remove_trading_pair");
        self.orderbooks.remove(pair)
    }

    fn add_order(&mut self, pair: &TradingPair, order: &Order) -> Option<Trades> {
        tracing::debug!(%pair, order_id = order.get_order_id(), price = order.get_price(), quantity = order.get_remaining_quantity(), side = ?order.get_side(), "add_order");
        self.orderbooks
            .get_mut(pair)?
            .add_order(order, &mut self.generator)
    }

    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool {
        tracing::debug!(%pair, order_id, "cancel_order");
        if let Some(book) = self.orderbooks.get_mut(pair) {
            book.cancel_order(order_id);
            return true;
        };
        return false;
    }

    fn modify_order(&mut self, pair: &TradingPair, modify_order: OrderModify) -> Option<Trades> {
        tracing::debug!(%pair, order_id = modify_order.get_order_id(), "modify_order");
        self.orderbooks
            .get_mut(pair)?
            .modify_order(modify_order, &mut self.generator)
    }

    fn get_order_info(&self, pair: &TradingPair) -> Option<OrderBookLevelInfo> {
        self.orderbooks
            .get(pair)
            .map(|book: &OrderBook| book.get_order_info())
    }

    fn size(&self, pair: &TradingPair) -> Option<usize> {
        self.orderbooks.get(pair).map(|book| book.size())
    }
}

impl UsersEngine for CoreEngine {
    fn add_user(&mut self, user_id: UserId) {
        tracing::debug!(%user_id, "add_user");
        self.users
            .entry(user_id)
            .or_insert(User::new(Some(user_id)));
    }

    fn remove_user(&mut self, user_id: UserId) -> Result<HashMap<Asset, Quantity>, String> {
        tracing::debug!(%user_id, "remove_user");
        // cancel all orders for this user across all orderbooks
        for book in self.orderbooks.values_mut() {
            let cancelled_ids = book.cancel_orders_for_user(user_id);
            // unlock each cancelled order's locked funds
            if let Some(user) = self.users.get_mut(&user_id) {
                for order_id in cancelled_ids {
                    let _ = user.unlock_order(&order_id);
                }
            }
        }
        // Remove user and return final balances snapshots
        let user = self.users.remove(&user_id).ok_or("User not found")?;
        Ok(user.get_all_balances().clone())
    }

    fn deposit_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), String> {
        tracing::debug!(%user_id, ?asset, quantity, "deposit_balance");
        let user = self.users.get_mut(&user_id).ok_or("User not found")?;
        user.add_balance(asset, quantity);
        Ok(())
    }

    fn withdraw_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), String> {
        tracing::debug!(%user_id, ?asset, quantity, "withdraw_balance");
        let user = self.users.get_mut(&user_id).ok_or("User not found")?;
        user.substract_balance(asset, quantity)
    }

    fn get_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, String> {
        let user = self.users.get(&user_id).ok_or("User not found")?;
        Ok(user.get_available_balance(&asset))
    }
}

pub enum EngineWrapper {
    Core(CoreEngine),
    Wal(WalEngine),
}

impl EngineWrapper {
    pub fn next_id(&mut self) -> OrderId {
        match self {
            EngineWrapper::Core(e) => e.next_id(),
            EngineWrapper::Wal(e) => e.next_id(),
        }
    }
}

impl ExchangeEngine for EngineWrapper {
    fn add_trading_pair(&mut self, pair: TradingPair) {
        match self {
            EngineWrapper::Core(e) => e.add_trading_pair(pair),
            EngineWrapper::Wal(e) => e.add_trading_pair(pair),
        }
    }

    fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook> {
        match self {
            EngineWrapper::Core(e) => e.remove_trading_pair(pair),
            EngineWrapper::Wal(e) => e.remove_trading_pair(pair),
        }
    }

    fn add_order(&mut self, pair: &TradingPair, order: &Order) -> Option<Trades> {
        match self {
            EngineWrapper::Core(e) => e.add_order(pair, order),
            EngineWrapper::Wal(e) => e.add_order(pair, order),
        }
    }

    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool {
        match self {
            EngineWrapper::Core(e) => e.cancel_order(pair, order_id),
            EngineWrapper::Wal(e) => e.cancel_order(pair, order_id),
        }
    }

    fn modify_order(&mut self, pair: &TradingPair, modify_order: OrderModify) -> Option<Trades> {
        match self {
            EngineWrapper::Core(e) => e.modify_order(pair, modify_order),
            EngineWrapper::Wal(e) => e.modify_order(pair, modify_order),
        }
    }

    fn get_order_info(&self, pair: &TradingPair) -> Option<OrderBookLevelInfo> {
        match self {
            EngineWrapper::Core(e) => e.get_order_info(pair),
            EngineWrapper::Wal(e) => e.get_order_info(pair),
        }
    }

    fn size(&self, pair: &TradingPair) -> Option<usize> {
        match self {
            EngineWrapper::Core(e) => e.size(pair),
            EngineWrapper::Wal(e) => e.size(pair),
        }
    }
}

impl UsersEngine for EngineWrapper {
    fn add_user(&mut self, user_id: UserId) {
        match self {
            EngineWrapper::Core(e) => e.add_user(user_id),
            EngineWrapper::Wal(e) => e.add_user(user_id),
        }
    }

    fn remove_user(&mut self, user_id: UserId) -> Result<HashMap<Asset, Quantity>, String> {
        match self {
            EngineWrapper::Core(e) => e.remove_user(user_id),
            EngineWrapper::Wal(e) => e.remove_user(user_id),
        }
    }

    fn deposit_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), String> {
        match self {
            EngineWrapper::Core(e) => e.deposit_balance(user_id, asset, quantity),
            EngineWrapper::Wal(e) => e.deposit_balance(user_id, asset, quantity),
        }
    }

    fn withdraw_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), String> {
        match self {
            EngineWrapper::Core(e) => e.withdraw_balance(user_id, asset, quantity),
            EngineWrapper::Wal(e) => e.withdraw_balance(user_id, asset, quantity),
        }
    }

    fn get_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, String> {
        match self {
            EngineWrapper::Core(e) => e.get_balance(user_id, asset),
            EngineWrapper::Wal(e) => e.get_balance(user_id, asset),
        }
    }
}

pub fn engine_from_env(machine_id: u64, datacenter_id: u64) -> EngineWrapper {
    let wal_enabled = std::env::var("WAL_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false);
    if wal_enabled {
        let path = std::env::var("WAL_PATH").unwrap_or_else(|_| "engine.wal".into());
        EngineWrapper::Wal(
            WalEngine::new(machine_id, datacenter_id, &path)
                .expect("Failed to initialize WAL engine"),
        )
    } else {
        EngineWrapper::Core(CoreEngine::new(1, 1))
    }
}
