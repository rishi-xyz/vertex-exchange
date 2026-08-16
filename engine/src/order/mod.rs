//! The [`Order`](crate::order::Order) struct — a single limit order in the exchange.
//!
//! An order represents a intent to buy or sell a specific quantity of an asset
//! at a specific price. Orders live inside an [`OrderBook`](crate::orderbook::OrderBook)
//! and are matched on a price-time priority basis.
//!
//! # Order Lifecycle
//!
//! ```text
//! new() → Engine assigns snowflake ID → enters book → matched (partial/full) → Filled
//!                                        or
//! new() → Engine assigns snowflake ID → enters book → cancelled → removed
//! ```
//!
//! The `order_id` passed to [`Order::new`](crate::order::Order::new) is always a placeholder (`0`).
//! The [`CoreEngine`](crate::engine::CoreEngine) calls [`Order::set_order_id`](crate::order::Order::set_order_id)
//! to stamp a real snowflake ID before the order enters the book.

use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tracing;

use crate::types::{OrderId, OrderStatus, OrderType, Price, Quantity, Side, UserId};

/// A single limit order in the order book.
///
/// Orders are wrapped in `Arc<Mutex<Order>>` ([`OrderPointer`]) so they can be
/// shared between the book's price-level deques and the `orders_map` lookup table.
///
/// # Invariants
///
/// - `remaining_quantity <= initial_quantity` always holds.
/// - `remaining_quantity == 0` implies `status == Filled`.
/// - `timestamp` is set once at construction and never changes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Order {
    /// Snowflake ID assigned by the engine. Placeholder `0` until [`set_order_id`](Order::set_order_id) is called.
    order_id: OrderId,
    /// How long the order lives (GTC, FAK, etc.)
    order_type: OrderType,
    /// Buy or sell
    side: Side,
    /// Current lifecycle state
    status: OrderStatus,
    /// Limit price in quote units
    price: Price,
    /// Original quantity when the order was created
    initial_quantity: Quantity,
    /// Quantity yet to be matched. Decreases on each fill.
    remaining_quantity: Quantity,
    /// Nanosecond timestamp of when the order was created (epoch)
    timestamp: u64,
    /// UUID of the user who placed this order
    user_id: UserId,
}

impl Order {
    /// Creates a new order.
    ///
    /// # Arguments
    ///
    /// * `order_id` — ID get's assign by the engine.
    /// * `order_type` — How long the order lives ([`GoodTillCancel`](OrderType::GoodTillCancel), [`FillAndKill`](OrderType::FillAndKill), etc.)
    /// * `side` — Buy or sell
    /// * `status` — Initial status (typically [`Empty`](OrderStatus::Empty))
    /// * `price` — Limit price in quote units
    /// * `quantity` — Number of base units to trade
    /// * `user_id` — UUID of the placing user
    ///
    /// # Examples
    ///
    /// ```rust
    /// use vertex_engine::order::Order;
    /// use vertex_engine::types::{OrderType, Side, OrderStatus};
    /// use uuid::Uuid;
    ///
    /// let user_id = Uuid::new_v4();
    /// let order = Order::new(
    ///     0, // example — engine assigns real ID
    ///     OrderType::GoodTillCancel,
    ///     Side::Buy,
    ///     OrderStatus::Empty,
    ///     50000,  // price
    ///     10,     // quantity
    ///     user_id,
    /// );
    /// assert_eq!(order.get_remaining_quantity(), 10);
    /// ```
    pub fn new(
        order_id: OrderId,
        order_type: OrderType,
        side: Side,
        status: OrderStatus,
        price: Price,
        quantity: Quantity,
        user_id: UserId,
    ) -> Self {
        let ts: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Order {
            order_id,
            order_type,
            side,
            status,
            price,
            initial_quantity: quantity,
            remaining_quantity: quantity,
            timestamp: ts,
            user_id,
        }
    }

    /// Returns the snowflake order ID.
    pub fn get_order_id(&self) -> OrderId {
        self.order_id
    }

    /// Returns the order type ([`GoodTillCancel`](OrderType::GoodTillCancel), [`FillAndKill`](OrderType::FillAndKill), etc.)
    pub fn get_type(&self) -> OrderType {
        self.order_type
    }

    /// Returns which side of the trade this order is on.
    pub fn get_side(&self) -> Side {
        self.side
    }

    /// Returns the current lifecycle status of the order.
    pub fn get_status(&self) -> OrderStatus {
        self.status
    }

    /// Returns the limit price in quote units.
    pub fn get_price(&self) -> Price {
        self.price
    }

    /// Returns the original quantity when the order was created.
    pub fn get_initial_quantity(&self) -> Quantity {
        self.initial_quantity
    }

    /// Returns the quantity yet to be matched.
    ///
    /// Decreases as fills occur. When this reaches `0`, the order is fully filled.
    pub fn get_remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    /// Returns the quantity that has been matched so far.
    ///
    /// Equivalent to `initial_quantity - remaining_quantity`.
    pub fn get_filled_quantity(&self) -> Quantity {
        self.initial_quantity - self.remaining_quantity
    }

    /// Returns `true` if order has been fully filled (`remaining_quantity == 0`).
    pub fn is_filled(&self) -> bool {
        self.remaining_quantity == 0
    }

    /// Returns the nanosecond epoch timestamp of when this order was created.
    pub fn get_timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Returns the UUID of the user who placed this order.
    pub fn get_user_id(&self) -> UserId {
        self.user_id
    }

    /// Applies a fill to this order, reducing `remaining_quantity`.
    ///
    /// Updates the order's status to [`Filled`](OrderStatus::Filled) or
    /// [`PartiallyFilled`](OrderStatus::PartiallyFilled) accordingly.
    ///
    /// # Arguments
    ///
    /// * `quantity` — Number of units to fill. Must be `<= remaining_quantity`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `quantity > remaining_quantity` (overfill attempt).
    ///
    /// # Examples
    ///
    /// ```
    /// use vertex_engine::order::Order;
    /// use vertex_engine::types::{OrderType, Side, OrderStatus};
    /// use uuid::Uuid;
    ///
    /// let user_id = Uuid::new_v4();
    /// let mut order = Order::new(
    ///     0,
    ///     OrderType::GoodTillCancel,
    ///     Side::Buy,
    ///     OrderStatus::Empty,
    ///     50000,
    ///     10,
    ///     user_id,
    /// );
    /// order.fills(5).unwrap();
    /// assert_eq!(order.get_remaining_quantity(), 5);
    /// assert_eq!(order.get_status(), OrderStatus::PartiallyFilled);
    /// ```
    pub fn fills(&mut self, quantity: Quantity) -> Result<(), String> {
        if quantity == 0 {
            return Ok(());
        }
        if quantity > self.remaining_quantity {
            tracing::warn!(
                order_id = self.get_order_id(),
                remaining = self.remaining_quantity,
                filled = quantity,
                "Order cannot be filled for more than its remaining quantity"
            );
            return Err(format!(
                "Order {} cannot be filled for more than its remaining quantity",
                self.get_order_id()
            ));
        }
        self.remaining_quantity -= quantity;
        if self.remaining_quantity == 0 {
            self.status = OrderStatus::Filled;
        } else {
            self.status = OrderStatus::PartiallyFilled;
        };
        tracing::debug!(
            order_id = self.get_order_id(),
            remaining = self.remaining_quantity,
            status = ?self.status,
            "order filled"
        );
        Ok(())
    }
}

/// A deque of orders at a single price level, ordered by time of arrival (FIFO).
pub type OrderPointers = VecDeque<Order>;
