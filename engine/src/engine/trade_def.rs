use std::collections::HashMap;

use crate::{
    level_info::OrderBookLevelInfo,
    order::Order,
    order_modify::OrderModify,
    orderbook::OrderBook,
    trade::Trades,
    trading_pair::TradingPair,
    types::{Asset, OrderError, OrderId, Quantity, UserError, UserId},
};

pub trait ExchangeEngine {
    fn add_trading_pair(&mut self, pair: TradingPair);
    fn remove_trading_pair(&mut self, pair: &TradingPair) -> Option<OrderBook>;
    fn add_order(
        &mut self,
        pair: &TradingPair,
        order: &Order,
    ) -> Result<Option<Trades>, OrderError>;
    fn cancel_order(&mut self, pair: &TradingPair, order_id: &OrderId) -> bool;
    fn modify_order(
        &mut self,
        pair: &TradingPair,
        modify_order: OrderModify,
    ) -> Result<Option<Trades>, OrderError>;
    fn get_order_info(&self, pair: &TradingPair) -> Option<OrderBookLevelInfo>;
    fn size(&self, pair: &TradingPair) -> Option<usize>;
}

pub trait UsersEngine {
    fn add_user(&mut self, user_id: UserId);
    fn remove_user(&mut self, user_id: UserId) -> Result<HashMap<Asset, Quantity>, UserError>;
    fn deposit_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), UserError>;
    fn withdraw_balance(
        &mut self,
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    ) -> Result<(), UserError>;
    fn get_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, UserError>;
    fn get_total_balance(&self, user_id: UserId, asset: Asset) -> Result<Quantity, UserError>;
}
