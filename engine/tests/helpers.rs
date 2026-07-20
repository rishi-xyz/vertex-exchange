use std::sync::atomic::{AtomicU64, Ordering};

pub use uuid::Uuid;
pub use vertex_engine::order::Order;
pub use vertex_engine::order_modify::OrderModify;
pub use vertex_engine::snowflake_id::SnowFlakeGenerator;
pub use vertex_engine::trading_pair::TradingPair;
pub use vertex_engine::types::{
    Asset, OrderId, OrderStatus, OrderType, Price, Quantity, Side, UserId,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[allow(dead_code)]
pub fn next_order_id() -> OrderId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn make_user_id() -> UserId {
    Uuid::new_v4()
}

#[allow(dead_code)]
pub fn make_user_id_fixed(seed: u64) -> UserId {
    Uuid::from_u128(seed as u128)
}

pub fn make_order(
    order_type: OrderType,
    side: Side,
    price: Price,
    quantity: Quantity,
    user_id: UserId,
) -> Order {
    Order::new(
        next_order_id(),
        order_type,
        side,
        OrderStatus::Empty,
        price,
        quantity,
        user_id,
    )
}

pub fn make_order_with_id(
    order_id: OrderId,
    order_type: OrderType,
    side: Side,
    price: Price,
    quantity: Quantity,
    user_id: UserId,
) -> Order {
    Order::new(
        order_id,
        order_type,
        side,
        OrderStatus::Empty,
        price,
        quantity,
        user_id,
    )
}

#[allow(dead_code)]
pub fn make_modify(
    order_id: OrderId,
    price: Price,
    side: Side,
    quantity: Quantity,
    user_id: UserId,
) -> OrderModify {
    OrderModify::new(order_id, price, side, quantity, OrderStatus::Empty, user_id)
}

#[allow(dead_code)]
pub fn make_pair(base: Asset, quote: Asset) -> TradingPair {
    TradingPair::new(base, quote)
}

pub fn make_generator() -> SnowFlakeGenerator {
    SnowFlakeGenerator::new(1, 1)
}
