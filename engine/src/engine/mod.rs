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
    types::{Asset, OrderError, OrderId, OrderType, Price, Quantity, Side, UserId},
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

    /// Validates the basic invariants of an order before it touches the book:
    /// positive price, positive quantity, and a notional (`price * quantity`)
    /// that fits in `Quantity` (u32).
    fn validate_order(price: Price, quantity: Quantity) -> Result<(), OrderError> {
        if price <= 0 || quantity == 0 {
            return Err(OrderError::InvalidOrder);
        }
        Self::notional(price, quantity)
            .map(|_| ())
            .ok_or(OrderError::InvalidOrder)
    }

    /// Computes `price * quantity` as a `Quantity`, rejecting overflow.
    fn notional(price: Price, quantity: Quantity) -> Option<Quantity> {
        let notional = (price as u64).checked_mul(quantity as u64)?;
        u32::try_from(notional).ok()
    }

    /// The asset and amount a user must have locked for a given side/price/quantity.
    fn lock_amount_for(
        pair: &TradingPair,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> Result<(Asset, Quantity), OrderError> {
        match side {
            Side::Buy => Ok((pair.quote, Self::notional(price, quantity).ok_or(OrderError::InvalidOrder)?)),
            Side::Sell => Ok((pair.base, quantity)),
        }
    }

    /// Settles a set of trades against the users' balances, both sides of each fill.
    ///
    /// The buyer pays `quantity * exec_price` of the quote asset and receives the
    /// base asset; the seller does the inverse. `Trade` records a single execution
    /// price per fill (the resting/maker price), so the notional is conserved.
    fn settle_trades(users: &mut HashMap<UserId, User>, pair: &TradingPair, trades: &Trades) {
        for trade in trades {
            let bid = trade.get_bid_trade_info();
            let ask = trade.get_ask_trade_info();
            let exec_price: Price = bid.get_price();
            let quantity: Quantity = bid.get_quantity();
            let notional: Quantity = match (exec_price as u64).checked_mul(quantity as u64).and_then(|v| u32::try_from(v).ok()) {
                Some(v) => v,
                None => {
                    tracing::error!(
                        %pair,
                        exec_price,
                        quantity,
                        "fill notional overflow during settlement; skipping"
                    );
                    continue;
                }
            };

            if let Some(Err(e)) = users
                .get_mut(&bid.get_user_id())
                .map(|u| u.apply_fill(&bid.get_order_id(), pair.quote, notional, pair.base, quantity))
            {
                tracing::warn!(%pair, user_id = %bid.get_user_id(), "buyer fill settlement failed: {e}");
            }
            if let Some(Err(e)) = users
                .get_mut(&ask.get_user_id())
                .map(|u| u.apply_fill(&ask.get_order_id(), pair.base, quantity, pair.quote, notional))
            {
                tracing::warn!(%pair, user_id = %ask.get_user_id(), "seller fill settlement failed: {e}");
            }
        }
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

    fn add_order(
        &mut self,
        pair: &TradingPair,
        order: &Order,
    ) -> Result<Option<Trades>, OrderError> {
        tracing::debug!(%pair, order_id = order.get_order_id(), price = order.get_price(), quantity = order.get_remaining_quantity(), side = ?order.get_side(), "add_order");
        Self::validate_order(order.get_price(), order.get_initial_quantity())?;

        let book = self.orderbooks.get_mut(pair).ok_or(OrderError::NoSuchPair)?;
        if book.has_order(&order.get_order_id()) {
            return Err(OrderError::InvalidOrder);
        }
        let (lock_asset, lock_amount) = Self::lock_amount_for(
            pair,
            order.get_side(),
            order.get_price(),
            order.get_initial_quantity(),
        )?;

        {
            let user = self
                .users
                .get_mut(&order.get_user_id())
                .ok_or(OrderError::NoSuchUser)?;
            user.lock(order.get_order_id(), lock_asset, lock_amount)
                .map_err(|_| OrderError::InsufficientBalance)?;
        }

        let trades = book.add_order(order, &mut self.generator);
        match trades {
            Some(trades) => {
                Self::settle_trades(&mut self.users, pair, &trades);
                // FAK remainder is discarded by the book — release the leftover lock
                if order.get_type() == OrderType::FillAndKill {
                    let _ = self
                        .users
                        .get_mut(&order.get_user_id())
                        .and_then(|u| u.unlock_order(&order.get_order_id()).ok());
                }
                Ok(Some(trades))
            }
            None => {
                // book rejected the order (FAK no-match, FOK partial, ...) — release the lock
                let _ = self
                    .users
                    .get_mut(&order.get_user_id())
                    .and_then(|u| u.unlock_order(&order.get_order_id()).ok());
                Ok(None)
            }
        }
    }

    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool {
        tracing::debug!(%pair, order_id, "cancel_order");
        let Some(book) = self.orderbooks.get_mut(pair) else {
            return false;
        };
        let Some(order) = book.cancel_order(order_id) else {
            return false;
        };
        if let Some(user) = self.users.get_mut(&order.get_user_id()) {
            let _ = user.unlock_order(order_id);
        }
        true
    }

    fn modify_order(
        &mut self,
        pair: &TradingPair,
        modify_order: OrderModify,
    ) -> Result<Option<Trades>, OrderError> {
        tracing::debug!(%pair, order_id = modify_order.get_order_id(), "modify_order");
        Self::validate_order(modify_order.get_price(), modify_order.get_quantity())?;

        let book = self.orderbooks.get_mut(pair).ok_or(OrderError::NoSuchPair)?;
        let old = book
            .get_order(&modify_order.get_order_id())
            .ok_or(OrderError::InvalidOrder)?;
        if modify_order.get_user_id() != old.get_user_id()
            || modify_order.get_side() != old.get_side()
        {
            return Err(OrderError::InvalidOrder);
        }
        let (new_asset, new_amount) = Self::lock_amount_for(
            pair,
            modify_order.get_side(),
            modify_order.get_price(),
            modify_order.get_quantity(),
        )?;

        // Pre-check balance before mutating the book so a failed modify
        // leaves state untouched. The old order's remaining lock becomes
        // available again, so it counts towards the new lock.
        {
            let user = self
                .users
                .get_mut(&old.get_user_id())
                .ok_or(OrderError::NoSuchUser)?;
            let old_locked = user
                .get_all_locked_balances()
                .get(&modify_order.get_order_id())
                .map(|e| e.amount)
                .unwrap_or(0);
            let available_after_unlock =
                user.get_available_balance(&new_asset) as u64 + old_locked as u64;
            if new_amount as u64 > available_after_unlock {
                return Err(OrderError::InsufficientBalance);
            }
        }

        book.cancel_order(&modify_order.get_order_id());
        {
            let user = self
                .users
                .get_mut(&old.get_user_id())
                .ok_or(OrderError::NoSuchUser)?;
            let _ = user.unlock_order(&modify_order.get_order_id());
            user.lock(modify_order.get_order_id(), new_asset, new_amount)
                .map_err(|_| OrderError::InsufficientBalance)?;
        }

        let new_order = Order::new(
            modify_order.get_order_id(),
            old.get_type(),
            modify_order.get_side(),
            modify_order.get_status(),
            modify_order.get_price(),
            modify_order.get_quantity(),
            modify_order.get_user_id(),
        );

        let trades = book.add_order(&new_order, &mut self.generator);
        match trades {
            Some(trades) => {
                Self::settle_trades(&mut self.users, pair, &trades);
                if new_order.get_type() == OrderType::FillAndKill {
                    let _ = self
                        .users
                        .get_mut(&new_order.get_user_id())
                        .and_then(|u| u.unlock_order(&new_order.get_order_id()).ok());
                }
                Ok(Some(trades))
            }
            None => {
                let _ = self
                    .users
                    .get_mut(&new_order.get_user_id())
                    .and_then(|u| u.unlock_order(&new_order.get_order_id()).ok());
                Ok(None)
            }
        }
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

    fn get_total_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, String> {
        let user = self.users.get(&user_id).ok_or("User not found")?;
        Ok(user.get_balance(&asset))
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

    fn add_order(
        &mut self,
        pair: &TradingPair,
        order: &Order,
    ) -> Result<Option<Trades>, OrderError> {
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

    fn modify_order(
        &mut self,
        pair: &TradingPair,
        modify_order: OrderModify,
    ) -> Result<Option<Trades>, OrderError> {
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

    fn get_total_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, String> {
        match self {
            EngineWrapper::Core(e) => e.get_total_balance(user_id, asset),
            EngineWrapper::Wal(e) => e.get_total_balance(user_id, asset),
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
