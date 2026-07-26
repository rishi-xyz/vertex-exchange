pub mod trade_def;

use std::collections::HashMap;

use crate::{
    engine::trade_def::ExchangeEngine, level_info::OrderBookLevelInfo, order::Order,
    order_modify::OrderModify, orderbook::OrderBook, snowflake_id::SnowFlakeGenerator,
    trade::Trades, trading_pair::TradingPair, types::OrderId, wal::engine::WalEngine,
};

pub struct CoreEngine {
    orderbooks: HashMap<TradingPair, OrderBook>,
    generator: SnowFlakeGenerator,
}

impl CoreEngine {
    pub fn new(machine_id: u64, datacenter_id: u64) -> Self {
        CoreEngine {
            orderbooks: HashMap::new(),
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
        self.orderbooks.entry(pair).or_insert(OrderBook::new());
    }

    fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook> {
        self.orderbooks.remove(pair)
    }

    fn add_order(&mut self, pair: &TradingPair, order: &Order) -> Option<Trades> {
        self.orderbooks
            .get_mut(pair)?
            .add_order(order, &mut self.generator)
    }

    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool {
        if let Some(book) = self.orderbooks.get_mut(pair) {
            book.cancel_order(order_id);
            return true;
        };
        return false;
    }

    fn modify_order(&mut self, pair: &TradingPair, modify_order: OrderModify) -> Option<Trades> {
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

pub fn engine_from_env(machine_id: u64 , datacenter_id: u64) -> EngineWrapper {
    let wal_enabled = std::env::var("WAL_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false);
    if wal_enabled {
        let path = std::env::var("WAL_PATH").unwrap_or_else(|_| "engine.wal".into());
        EngineWrapper::Wal(WalEngine::new(machine_id, datacenter_id, &path).expect("Failed to initialize WAL engine"))
    } else {
        EngineWrapper::Core(CoreEngine::new(1, 1))
    }
}
