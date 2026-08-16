//! Cancel-replace order modification request.
//!
//! An [`OrderModify`](crate::order_modify::OrderModify) represents a request to replace an existing order with a new
//! one at a different price, quantity, or both. The engine cancels the old order
/// and creates a new one with a fresh snowflake ID.
use serde::{Deserialize, Serialize};

use crate::{
    order::Order,
    types::{OrderId, OrderStatus, OrderType, Price, Quantity, Side, UserId},
};

/// A request to modify (cancel-replace) an existing order.
///
/// The engine will:
/// 1. Cancel the order identified by `order_id`
/// 2. Create a new order with a fresh snowflake ID using the fields from this struct
/// 3. Attempt to match the new order against the book
///
/// The old order's ID is retired; the new order gets a new snowflake ID.
///
/// # Examples
///
/// ```
/// use vertex_engine::order_modify::OrderModify;
/// use vertex_engine::types::{OrderStatus, Side};
/// use uuid::Uuid;
///
/// let user_id = Uuid::new_v4();
/// let modify = OrderModify::new(
///     12345,          // order_id of the order to replace
///     51000,          // new price
///     Side::Buy,
///     5,              // new quantity
///     OrderStatus::Empty,
///     user_id,
/// );
/// assert_eq!(modify.get_order_id(), 12345);
/// assert_eq!(modify.get_price(), 51000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderModify {
    /// ID of the existing order to cancel and replace
    order_id: OrderId,
    /// New limit price
    price: Price,
    /// New side (must match the original — changing side is not supported)
    side: Side,
    /// New quantity
    quantity: Quantity,
    /// Initial status for the replacement order (typically [`Empty`](OrderStatus::Empty))
    status: OrderStatus,
    /// User who owns the order (must match the original)
    user_id: UserId,
}

impl OrderModify {
    /// Creates a new order modification request.
    ///
    /// # Arguments
    ///
    /// * `order_id` — ID of the order to replace
    /// * `price` — New limit price
    /// * `side` — New side (should match the original)
    /// * `quantity` — New quantity
    /// * `status` — Initial status for the replacement (typically [`Empty`](OrderStatus::Empty))
    /// * `user_id` — Owner of the order (should match the original)
    pub fn new(
        order_id: OrderId,
        price: Price,
        side: Side,
        quantity: Quantity,
        status: OrderStatus,
        user_id: UserId,
    ) -> Self {
        OrderModify {
            order_id,
            price,
            side,
            quantity,
            status,
            user_id,
        }
    }

    /// Returns the ID of the order to be replaced.
    pub fn get_order_id(&self) -> OrderId {
        self.order_id
    }

    /// Returns the new limit price.
    pub fn get_price(&self) -> Price {
        self.price
    }

    /// Returns the new side.
    pub fn get_side(&self) -> Side {
        self.side
    }

    /// Returns the new quantity.
    pub fn get_quantity(&self) -> Quantity {
        self.quantity
    }

    /// Returns the initial status for the replacement order.
    pub fn get_status(&self) -> OrderStatus {
        self.status
    }

    /// Returns the user who owns this order.
    pub fn get_user_id(&self) -> UserId {
        self.user_id
    }

    /// Converts this modification request into a new [`Order`] pointer.
    ///
    /// Used internally by [`OrderBook::modify_orders`](crate::orderbook::OrderBook::modify_orders)
    /// to construct the replacement order after cancelling the original.
    ///
    /// # Arguments
    ///
    /// * `order_type` — The order type from the original order (carried forward)
    pub fn to_order_pointer(&self, order_type: OrderType) -> Order {
        Order::new(
            self.order_id,
            order_type,
            self.side,
            self.status,
            self.price,
            self.quantity,
            self.user_id,
        )
    }
}
