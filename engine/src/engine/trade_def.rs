use crate::{
    level_info::OrderBookLevelInfo, order::Order, order_modify::OrderModify, orderbook::OrderBook,
    trade::Trades, trading_pair::TradingPair, types::OrderId,
};

pub trait ExchangeEngine {
    fn add_trading_pair(&mut self, pair: TradingPair);
    fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook>;
    fn add_order(&mut self, pair: &TradingPair, order: &Order) -> Option<Trades>;
    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool;
    fn modify_order(&mut self, pair: &TradingPair, modify_order: OrderModify) -> Option<Trades>;
    fn get_order_info(&self, pair: &TradingPair) -> Option<OrderBookLevelInfo>;
    fn size(&self, pair: &TradingPair) -> Option<usize>;
}
