//! Price-time priority order book for a single trading pair.
//!
//! The [`OrderBook`](crate::orderbook::OrderBook) maintains two sides — bids (buy orders) and asks (sell orders) —
//! each organized as a `BTreeMap<Price, VecDeque<Order>>`. This gives us:
//!
//! - **Price priority** — `BTreeMap` keeps levels sorted; best bid is the max key,
//!   best ask is the min key.
//! - **Time priority** — `VecDeque` at each level is FIFO; oldest order matches first.
//!
//! # Matching Algorithm
//!
//! When an incoming order arrives via [`add_order`](crate::orderbook::OrderBook::add_order):
//!
//! 1. Check if the order can match (price is at or past the best opposite price).
//! 2. Walk the opposite side from the best price inward.
//! 3. At each level, match orders front-to-back (time priority).
//! 4. Fill quantity = `min(incoming_remaining, resting_remaining)`.
//! 5. Continue until the incoming order is fully filled or no more resting orders match.
//!
//! [`FillAndKill`](crate::types::OrderType::FillAndKill) orders are matched against
//! resting orders without entering the book; any unfilled remainder is discarded.
//!
//! # Thread Safety
//!
//! Individual orders are `Arc<Mutex<Order>>`. The book itself is **not** `Sync` —
//! it is owned by the [`CoreEngine`](crate::engine::CoreEngine) which is wrapped
//! in a `tokio::sync::RwLock` at the gRPC boundary.

use crate::level_info::{LevelInfo, OrderBookLevelInfo};
use crate::order_modify::OrderModify;
use crate::snowflake_id::SnowFlakeGenerator;
use crate::trade::{Trade, TradeInfo};
use crate::types::OrderType;
use crate::{
    order::{Order, OrderPointers},
    trade::Trades,
    types::{OrderId, Price, Quantity, Side, UserId},
};
use std::cmp::min;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// A price-time priority order book for a single trading pair.
///
/// Contains both bid (buy) and ask (sell) sides, plus a flat lookup table
/// (`orders_map: HashMap<OrderId, Order>`) for O(1) order existence checks.
///
/// # Data Structures
///
/// - `bids_map` — `BTreeMap<Price, VecDeque<Order>>` sorted ascending. Best bid is the **last** key.
/// - `asks_map` — `BTreeMap<Price, VecDeque<Order>>` sorted ascending. Best ask is the **first** key.
/// - `orders_map` — `HashMap<OrderId, Order>` for O(1) lookups by order ID (private).
#[derive(Debug)]
pub struct OrderBook {
    /// Buy orders keyed by price, sorted ascending (best bid = last key)
    bids_map: BTreeMap<Price, OrderPointers>,
    /// Sell orders keyed by price, sorted ascending (best ask = first key)
    asks_map: BTreeMap<Price, OrderPointers>,
    /// Flat lookup of all orders by ID (for existence checks and type lookups)
    orders_map: HashMap<OrderId, Order>,
}

impl OrderBook {
    /// Creates an empty order book.
    pub fn new() -> Self {
        let bids_map: BTreeMap<Price, OrderPointers> = BTreeMap::new();
        let asks_map: BTreeMap<Price, OrderPointers> = BTreeMap::new();
        let orders_map: HashMap<OrderId, Order> = HashMap::new();
        OrderBook {
            bids_map,
            asks_map,
            orders_map,
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderBook {
    /// Checks whether an incoming order can potentially match at the given price.
    ///
    /// - **Buy**: can match if `price >= best_ask`
    /// - **Sell**: can match if `price <= best_bid`
    ///
    /// Returns `false` if the opposite side is empty.
    fn can_match(&self, side: Side, price: Price) -> bool {
        match side {
            Side::Buy => {
                if let Some((&best_ask, _)) = self.asks_map.first_key_value() {
                    price >= best_ask
                } else {
                    false
                }
            }
            Side::Sell => {
                if let Some((&best_bid, _)) = self.bids_map.last_key_value() {
                    price <= best_bid
                } else {
                    false
                }
            }
        }
    }

    /// Matches resting GTC orders against each other after a new GTC order enters the book.
    ///
    /// Walks both sides from best price inward, filling at each level.
    /// Returns all trades produced. Only called for GTC orders — FAK/FOK orders
    /// use [`match_against_opposite`](Self::match_against_opposite) instead.
    ///
    /// Self-trade prevention: if all resting orders at a price level belong to
    /// the same user as the aggressor, that level is temporarily removed from the
    /// map and re-inserted after matching completes. This avoids both self-matches
    /// and infinite loops that would occur if the level were left in place.
    fn match_order(&mut self, aggressor_side: Side, generator: &mut SnowFlakeGenerator) -> Trades {
        let mut trades: Trades = Trades::new();
        trades.reserve(self.orders_map.len() / 2);
        // Levels where all resting orders are self-trades, removed temporarily
        let mut self_trade_levels: Vec<(Price, OrderPointers)> = Vec::new();

        loop {
            if self.bids_map.is_empty() || self.asks_map.is_empty() {
                break;
            }

            let bid_price: Price = *self.bids_map.last_key_value().unwrap().0;
            let ask_price: Price = *self.asks_map.first_key_value().unwrap().0;

            if bid_price < ask_price {
                break;
            }

            // Scope borrows so we can remove levels from the map after matching
            let all_self_trade = {
                let bids: &mut VecDeque<Order> = self.bids_map.get_mut(&bid_price).unwrap();
                let asks: &mut VecDeque<Order> = self.asks_map.get_mut(&ask_price).unwrap();

                let initial_bids_len = bids.len();
                let initial_asks_len = asks.len();
                let mut skipped = 0;
                let mut all_self_trade = false;

                while bids.len() != 0 && asks.len() != 0 {
                    let bid = bids.front_mut().unwrap();
                    let ask = asks.front_mut().unwrap();

                    // Self-trade: rotate the resting order to the back and skip
                    if bid.get_user_id() == ask.get_user_id() {
                        match aggressor_side {
                            Side::Buy => {
                                let resting_order = asks.pop_front().unwrap();
                                asks.push_back(resting_order);
                                skipped += 1;
                                if skipped >= initial_asks_len {
                                    all_self_trade = true;
                                    break;
                                }
                            }
                            Side::Sell => {
                                let resting_order = bids.pop_front().unwrap();
                                bids.push_back(resting_order);
                                skipped += 1;
                                if skipped >= initial_bids_len {
                                    all_self_trade = true;
                                    break;
                                }
                            }
                        }
                        continue;
                    }

                    let quantity: Quantity =
                        min(bid.get_remaining_quantity(), ask.get_remaining_quantity());
                    let _ = bid.fills(quantity);
                    let _ = ask.fills(quantity);

                    let bid_id: OrderId = bid.get_order_id();
                    let ask_id: OrderId = ask.get_order_id();
                    let exec_price: Price = match aggressor_side {
                        Side::Buy => ask_price,
                        Side::Sell => bid_price,
                    };
                    let bid_user_id: UserId = bid.get_user_id();
                    let ask_user_id: UserId = ask.get_user_id();

                    if bid.is_filled() {
                        bids.pop_front();
                        self.orders_map.remove(&bid_id);
                    }
                    if ask.is_filled() {
                        asks.pop_front();
                        self.orders_map.remove(&ask_id);
                    }

                    let trade_id = generator.next_id();
                    trades.push_back(Trade::new(
                        trade_id,
                        TradeInfo::new(bid_id, exec_price, quantity, bid_user_id),
                        TradeInfo::new(ask_id, exec_price, quantity, ask_user_id),
                    ));
                }

                all_self_trade
            }; // borrows of bids/asks end here

            // All orders at this level are self-trades — remove temporarily
            // so the outer loop advances to the next price level
            if all_self_trade {
                match aggressor_side {
                    Side::Buy => {
                        if let Some(orders) = self.asks_map.remove(&ask_price) {
                            self_trade_levels.push((ask_price, orders));
                        }
                    }
                    Side::Sell => {
                        if let Some(orders) = self.bids_map.remove(&bid_price) {
                            self_trade_levels.push((bid_price, orders));
                        }
                    }
                }
                continue;
            }

            // Remove empty price levels
            if self
                .bids_map
                .get(&bid_price)
                .map_or(false, |q| q.is_empty())
            {
                self.bids_map.remove(&bid_price);
            }
            if self
                .asks_map
                .get(&ask_price)
                .map_or(false, |q| q.is_empty())
            {
                self.asks_map.remove(&ask_price);
            }
        }

        // Re-insert self-trade levels — their orders are still resting in the book
        for (price, orders) in self_trade_levels {
            match aggressor_side {
                Side::Buy => {
                    self.asks_map.insert(price, orders);
                }
                Side::Sell => {
                    self.bids_map.insert(price, orders);
                }
            }
        }

        trades
    }

    /// Matches a non-resting aggressor order against resting orders on the opposite side.
    ///
    /// The aggressor is **never inserted into the book**. Walks the opposite side
    /// from the best price inward, filling at each level. Self-trade prevention
    /// skips resting orders from the same user.
    ///
    /// When `require_full_fill` is `true` (FOK), a pre-check verifies that enough
    /// liquidity exists before any mutations. Returns `None` if the full quantity
    /// cannot be filled. When `false` (FAK), partial fills are accepted and any
    /// unfilled remainder is discarded.
    fn match_against_opposite(
        &mut self,
        aggressor: &Order,
        require_full_fill: bool,
        generator: &mut SnowFlakeGenerator,
    ) -> Option<Trades> {
        let order_side = aggressor.get_side();
        let order_id = aggressor.get_order_id();
        let order_price = aggressor.get_price();
        let order_user_id = aggressor.get_user_id();
        let required_qty: Quantity = aggressor.get_remaining_quantity();

        // FOK pre-check: sum available liquidity (read-only) before mutating.
        // Accumulate in u64 — a u32 sum can overflow with deep books, which would
        // corrupt the pre-check and let the safety net below fire mid-mutation.
        if require_full_fill {
            let mut available: u64 = 0;
            match order_side {
                Side::Buy => {
                    for (price, orders) in self.asks_map.iter() {
                        if order_price < *price {
                            break;
                        }
                        for o in orders.iter() {
                            if o.get_user_id() != order_user_id {
                                available += o.get_remaining_quantity() as u64;
                            }
                        }
                    }
                }
                Side::Sell => {
                    for (price, orders) in self.bids_map.iter().rev() {
                        if order_price > *price {
                            break;
                        }
                        for o in orders.iter() {
                            if o.get_user_id() != order_user_id {
                                available += o.get_remaining_quantity() as u64;
                            }
                        }
                    }
                }
            }
            if available < required_qty as u64 {
                return None;
            }
        }

        // matching loop — shared by FAK and FOK
        let mut trades: Trades = Trades::new();
        let mut remaining_qty: Quantity = required_qty;
        // Levels where all resting orders are self-trades, removed temporarily
        let mut self_trade_levels: Vec<(Price, OrderPointers)> = Vec::new();

        loop {
            if remaining_qty == 0 {
                break;
            }

            // get best price on the opposite side
            let resting_price: Price = match order_side {
                Side::Buy => {
                    if let Some((&price, _)) = self.asks_map.first_key_value() {
                        price
                    } else {
                        break;
                    }
                }
                Side::Sell => {
                    if let Some((&price, _)) = self.bids_map.last_key_value() {
                        price
                    } else {
                        break;
                    }
                }
            };

            // check if aggressor price crosses the resting price
            let crosses = match order_side {
                Side::Buy => order_price >= resting_price,
                Side::Sell => order_price <= resting_price,
            };
            if !crosses {
                break;
            }

            // match at this price level, scoped so resting_orders borrow ends before remove
            let (level_empty, all_self_trade) = {
                let resting_orders: &mut VecDeque<Order> = match order_side {
                    Side::Buy => self.asks_map.get_mut(&resting_price).unwrap(),
                    Side::Sell => self.bids_map.get_mut(&resting_price).unwrap(),
                };

                let initial_len = resting_orders.len();
                let mut skipped = 0;
                let mut all_self_trade = false;

                while remaining_qty > 0 && !resting_orders.is_empty() {
                    let resting_order = resting_orders.front_mut().unwrap();

                    // self-trade prevention: skip resting orders from the same user
                    if resting_order.get_user_id() == order_user_id {
                        let skipped_order = resting_orders.pop_front().unwrap();
                        resting_orders.push_back(skipped_order);
                        skipped += 1;
                        if skipped >= initial_len {
                            all_self_trade = true;
                            break;
                        }
                        continue;
                    }

                    let fill_qty = min(remaining_qty, resting_order.get_remaining_quantity());
                    remaining_qty -= fill_qty;
                    let _ = resting_order.fills(fill_qty);

                    let resting_id = resting_order.get_order_id();
                    let resting_price_val = resting_order.get_price();
                    let resting_user_id = resting_order.get_user_id();

                    if resting_order.is_filled() {
                        resting_orders.pop_front();
                        self.orders_map.remove(&resting_id);
                    }

                    // create trade with correct bid/ask sides
                    let trade_id = generator.next_id();
                    let (bid_info, ask_info) = match order_side {
                        Side::Buy => (
                            TradeInfo::new(order_id, resting_price_val, fill_qty, order_user_id),
                            TradeInfo::new(
                                resting_id,
                                resting_price_val,
                                fill_qty,
                                resting_user_id,
                            ),
                        ),
                        Side::Sell => (
                            TradeInfo::new(
                                resting_id,
                                resting_price_val,
                                fill_qty,
                                resting_user_id,
                            ),
                            TradeInfo::new(order_id, resting_price_val, fill_qty, order_user_id),
                        ),
                    };
                    trades.push_back(Trade::new(trade_id, bid_info, ask_info));
                }

                (resting_orders.is_empty(), all_self_trade)
            }; // borrow of resting_orders ends here

            // All orders at this level are self-trades — remove temporarily
            // so the outer loop advances to the next price level
            if all_self_trade {
                match order_side {
                    Side::Buy => {
                        if let Some(orders) = self.asks_map.remove(&resting_price) {
                            self_trade_levels.push((resting_price, orders));
                        }
                    }
                    Side::Sell => {
                        if let Some(orders) = self.bids_map.remove(&resting_price) {
                            self_trade_levels.push((resting_price, orders));
                        }
                    }
                }
                continue;
            }

            // remove empty price level (safe: resting_orders borrow has ended)
            if level_empty {
                match order_side {
                    Side::Buy => {
                        self.asks_map.remove(&resting_price);
                    }
                    Side::Sell => {
                        self.bids_map.remove(&resting_price);
                    }
                };
            }
        }

        // Re-insert self-trade levels — their orders are still resting in the book
        for (price, orders) in self_trade_levels {
            match order_side {
                Side::Buy => {
                    self.asks_map.insert(price, orders);
                }
                Side::Sell => {
                    self.bids_map.insert(price, orders);
                }
            }
        }

        // FOK safety net: with an exact pre-check this is unreachable — if it
        // ever fires, resting orders were partially filled without a full fill,
        // the book is corrupt, and continuing would be worse than failing fast.
        if require_full_fill && remaining_qty > 0 {
            panic!(
                "FOK safety net fired: order {} not fully filled ({remaining_qty} remaining) after mutation; book state corrupt",
                order_id
            );
        }

        Some(trades)
    }

    /// Cancels a resting order by ID.
    ///
    /// Removes the order from both the price-level deque and the `orders_map`.
    /// Returns the cancelled order, or `None` if not found.
    pub fn cancel_order(&mut self, order_id: &OrderId) -> Option<Order> {
        let order: Order = self.orders_map.remove(order_id)?;
        let price: Price = order.get_price();
        let side: Side = order.get_side();
        // get the level of order
        let level: Option<&mut VecDeque<Order>> = match side {
            Side::Buy => self.bids_map.get_mut(&price),
            Side::Sell => self.asks_map.get_mut(&price),
        };

        if let Some(orders) = level {
            orders.retain(|order| order.get_order_id() != *order_id);
            if orders.is_empty() {
                match side {
                    Side::Buy => self.bids_map.remove(&price),
                    Side::Sell => self.asks_map.remove(&price),
                };
            }
        }
        Some(order)
    }

    /// Adds an order to the book and attempts to match it.
    ///
    /// This is the main entry point for order placement. The flow:
    ///
    /// 1. Reject duplicate order IDs
    /// 2. **FAK / FOK orders**: match against resting orders without entering the book.
    ///    FAK accepts partial fills (remainder discarded). FOK requires full fill or rejects.
    /// 3. **GTC / other orders**: insert into the book, then match via [`match_order`].
    pub fn add_order(
        &mut self,
        order: &Order,
        generator: &mut SnowFlakeGenerator,
    ) -> Option<Trades> {
        let order_side = order.get_side();
        let order_id = order.get_order_id();
        let order_price = order.get_price();

        if self.orders_map.contains_key(&order_id) {
            return None;
        }

        // FAK: never enter the book — match and kill the remainder
        if order.get_type() == OrderType::FillAndKill {
            if !self.can_match(order_side, order_price) {
                return None;
            }
            return self.match_against_opposite(order, false, generator);
        }

        // FOK: never enter the book — must fill entirely or reject
        if order.get_type() == OrderType::FillOrKill {
            if !self.can_match(order_side, order_price) {
                return None;
            }
            return self.match_against_opposite(order, true, generator);
        }

        // GTC and other resting order types: insert into book, then match
        self.orders_map.insert(order_id, *order);
        let level = match order_side {
            Side::Buy => self.bids_map.entry(order_price).or_default(),
            Side::Sell => self.asks_map.entry(order_price).or_default(),
        };
        level.push_back(*order);
        Some(self.match_order(order_side, generator))
    }

    /// Modifies an existing order by cancel-replace.
    ///
    /// Cancels the old order and creates a new one with the parameters from
    /// `order_modify`. The new order then goes through the full matching flow.
    pub fn modify_order(
        &mut self,
        order_modify: OrderModify,
        generator: &mut SnowFlakeGenerator,
    ) -> Option<Trades> {
        let order_id = order_modify.get_order_id();
        // if order doesn't exits return
        if !self.orders_map.contains_key(&order_id) {
            return None;
        }
        let order_type = {
            let existing = self.orders_map.get(&order_id).unwrap();
            existing.get_type()
        };
        let new_order = Order::new(
            order_id,
            order_type,
            order_modify.get_side(),
            order_modify.get_status(),
            order_modify.get_price(),
            order_modify.get_quantity(),
            order_modify.get_user_id(),
        );
        let _ = self.cancel_order(&order_id);
        self.add_order(&new_order, generator)
    }

    /// Returns the total number of resting orders in the book.
    pub fn size(&self) -> usize {
        return self.orders_map.len();
    }

    /// Returns `true` if an order with the given ID exists in the book.
    pub fn has_order(&self, order_id: &OrderId) -> bool {
        self.orders_map.contains_key(order_id)
    }

    /// Returns a copy of the order with the given ID, if it exists.
    pub fn get_order(&self, order_id: &OrderId) -> Option<Order> {
        self.orders_map.get(order_id).copied()
    }

    /// Returns copies of all resting orders in the book.
    pub fn get_all_orders(&self) -> Vec<Order> {
        self.orders_map.values().copied().collect()
    }

    /// Returns the order type for the given order ID, if it exists.
    pub fn get_order_type(&self, order_id: &OrderId) -> Option<OrderType> {
        self.orders_map.get(order_id).map(|o| o.get_type())
    }

    /// Returns a depth snapshot of the order book.
    pub fn get_order_info(&self) -> OrderBookLevelInfo {
        let mut bids_info = VecDeque::new();
        let mut asks_info = VecDeque::new();
        for (price, orders) in self.bids_map.iter() {
            let total_quantity = orders
                .iter()
                .map(|order| order.get_remaining_quantity())
                .sum();
            bids_info.push_front(LevelInfo::new(*price, total_quantity));
        }
        for (price, orders) in self.asks_map.iter() {
            let total_quantity = orders
                .iter()
                .map(|order| order.get_remaining_quantity())
                .sum();
            asks_info.push_front(LevelInfo::new(*price, total_quantity));
        }
        OrderBookLevelInfo::new(bids_info, asks_info)
    }

    pub fn cancel_orders_for_user(&mut self, user_id: UserId) -> Vec<OrderId> {
        // collect order ids belonging to this user
        let to_remove: Vec<OrderId> = self
            .orders_map
            .iter()
            .filter(|(_, order)| order.get_user_id() == user_id)
            .map(|(id, _)| *id)
            .collect();

        // remove from price-level deques
        for orders in self.bids_map.values_mut() {
            orders.retain(|o| o.get_user_id() != user_id);
        }
        for orders in self.asks_map.values_mut() {
            orders.retain(|o| o.get_user_id() != user_id);
        }

        // remove from flat orders_map
        for id in &to_remove {
            self.orders_map.remove(id);
        }

        // clean up empty price levels
        self.bids_map.retain(|_, orders| !orders.is_empty());
        self.asks_map.retain(|_, orders| !orders.is_empty());

        to_remove
    }
}
