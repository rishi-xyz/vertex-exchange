use std::collections::HashMap;

use crate::{
    level_info::OrderBookLevelInfo, order::Order, order_modify::OrderModify, orderbook::OrderBook,
    snowflake_id::SnowFlakeGenerator, trade::Trades, trading_pair::TradingPair, types::OrderId,
};

pub struct Engine {
    orderbooks: HashMap<TradingPair, OrderBook>,
    generator: SnowFlakeGenerator,
}

impl Engine {
    pub fn new(machine_id: u64, datacenter_id: u64) -> Self {
        Engine {
            orderbooks: HashMap::new(),
            generator: SnowFlakeGenerator::new(machine_id, datacenter_id),
        }
    }
    // create orderbook
    pub fn add_trading_pair(&mut self, pair: TradingPair) {
        self.orderbooks.entry(pair).or_insert(OrderBook::new());
    }

    pub fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook> {
        self.orderbooks.remove(pair)
    }

    pub fn add_order(&mut self, pair: &TradingPair, order: &Order) -> Option<Trades> {
        self.orderbooks
            .get_mut(pair)?
            .add_order(order, &mut self.generator)
    }

    pub fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool {
        if let Some(book) = self.orderbooks.get_mut(pair) {
            book.cancel_order(order_id);
            return true;
        };
        return false;
    }

    pub fn modify_order(
        &mut self,
        pair: &TradingPair,
        modify_order: OrderModify,
    ) -> Option<Trades> {
        self.orderbooks
            .get_mut(pair)?
            .modify_order(modify_order, &mut self.generator)
    }

    pub fn get_order_info(&self, pair: &TradingPair) -> Option<OrderBookLevelInfo> {
        self.orderbooks
            .get(pair)
            .map(|book: &OrderBook| book.get_order_info())
    }

    pub fn size(&self, pair: &TradingPair) -> Option<usize> {
        self.orderbooks.get(pair).map(|book| book.size())
    }
}
