//! Trade and trade-info types — represent a matched fill between two orders.
//!
//! When an incoming order matches a resting order in the book, a [`Trade`](crate::trade::Trade) is created.
//! Each [`Trade`](crate::trade::Trade) contains two [`TradeInfo`](crate::trade::TradeInfo) snapshots: one for the buy side and one
//! for the sell side. The engine stamps the trade with a snowflake ID and timestamp
//! after the orderbook produces it.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::types::{OrderId, Price, Quantity, TradeId, UserId};

/// A snapshot of one side of a matched trade.
///
/// Captures the order ID, price, quantity, and user of either the bid or ask
/// side at the moment of the fill. Stored as an immutable `Arc` inside [`Trade`].
///
/// # Examples
///
/// ```ignore
/// let info = TradeInfo::new(order_id, 50000, 10, user_id);
/// assert_eq!(info.get_price(), 50000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeInfo {
    /// ID of the order that was filled on this side
    order_id: OrderId,
    /// Price at which the fill occurred (the resting order's price)
    price: Price,
    /// Quantity matched in this fill
    quantity: Quantity,
    /// User who owns this order
    user_id: UserId,
}

impl TradeInfo {
    /// Creates a new trade info snapshot.
    ///
    /// # Arguments
    ///
    /// * `order_id` — ID of the filled order
    /// * `price` — Execution price (from the resting order)
    /// * `quantity` — Number of base units matched
    /// * `user_id` — UUID of the order's owner
    pub fn new(order_id: OrderId, price: Price, quantity: Quantity, user_id: UserId) -> Self {
        TradeInfo {
            order_id,
            price,
            quantity,
            user_id,
        }
    }
}

pub trait GetTradeInfo {
    /// Returns the ID of the filled order.
    fn get_order_id(&self) -> OrderId;
    /// Returns the execution price of this fill.
    fn get_price(&self) -> Price;
    /// Returns the number of base units matched.
    fn get_quantity(&self) -> Quantity;
    /// Returns the UUID of the order's owner.
    fn get_user_id(&self) -> UserId;
}

impl GetTradeInfo for TradeInfo {
    fn get_order_id(&self) -> OrderId {
        self.order_id
    }

    fn get_price(&self) -> Price {
        self.price
    }

    fn get_quantity(&self) -> Quantity {
        self.quantity
    }

    fn get_user_id(&self) -> UserId {
        self.user_id
    }
}

/// A single trade (fill) between two opposing orders.
///
/// Created by the [`OrderBook`](crate::orderbook::OrderBook) during matching.
/// Initially constructed with placeholder `trade_id = 0` and `timestamp = 0`;
/// the [`CoreEngine`](crate::engine::CoreEngine) stamps real values via
/// [`set_trade_id`](Trade::set_trade_id) and [`set_timestamp`](Trade::set_timestamp).
///
/// # Fields
///
/// - `bid_trade` — snapshot of the buy order that was filled
/// - `ask_trade` — snapshot of the sell order that was filled
///
/// The trade always records the **resting order's price** as the execution price,
/// regardless of which side was the aggressor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    /// Snowflake trade ID. Placeholder `0` until stamped by the engine.
    trade_id: TradeId,
    /// Nanosecond epoch timestamp. Placeholder `0` until stamped by the engine.
    timestamp: u64,
    /// Buy side of the trade
    bid_trade: TradeInfo,
    /// Sell side of the trade
    ask_trade: TradeInfo,
}

impl Trade {
    /// Creates a new trade with the given bid and ask info.
    ///
    /// In practice, `trade_id` and `timestamp` are placeholders (`0`) because
    /// the orderbook does not have access to the snowflake generator.
    /// The engine stamps real values after receiving the trade.
    ///
    /// # Arguments
    ///
    /// * `trade_id` — Placeholder (typically `0`). Overwritten by the engine.
    /// * `timestamp` — Placeholder (typically `0`). Overwritten by the engine.
    /// * `bid_trade` — Trade info for the buy side
    /// * `ask_trade` — Trade info for the sell side
    pub fn new(
        trade_id: TradeId,
        timestamp: u64,
        bid_trade: TradeInfo,
        ask_trade: TradeInfo,
    ) -> Self {
        Trade {
            trade_id,
            timestamp,
            bid_trade,
            ask_trade,
        }
    }
}

pub trait GetTrade {
    fn get_trade_id(&self) -> TradeId;
    fn get_timestamp(&self) -> u64;
    fn get_bid_trade_info(&self) -> &TradeInfo;
    fn get_ask_trade_info(&self) -> &TradeInfo;
}

pub trait SetTrade {
    /// Overwrites the trade ID. Called by the engine to stamp the real snowflake ID.
    fn set_trade_id(&mut self, id: TradeId);
    /// Overwrites the timestamp. Called by the engine to stamp the real fill time.
    fn set_timestamp(&mut self, ts: u64);
}

impl GetTrade for Trade {
    fn get_trade_id(&self) -> TradeId {
        self.trade_id
    }

    fn get_timestamp(&self) -> u64 {
        self.timestamp
    }

    fn get_ask_trade_info(&self) -> &TradeInfo {
        &self.ask_trade
    }

    fn get_bid_trade_info(&self) -> &TradeInfo {
        &self.bid_trade
    }
}

impl SetTrade for Trade {
    fn set_trade_id(&mut self, id: TradeId) {
        self.trade_id = id;
    }

    fn set_timestamp(&mut self, ts: u64) {
        self.timestamp = ts;
    }
}

pub type Trades = VecDeque<Trade>;