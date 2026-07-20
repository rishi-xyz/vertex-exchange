mod helpers;

use vertex_engine::order_modify::OrderModify;
use vertex_engine::types::{OrderStatus, OrderType, Side};

use helpers::{make_modify, make_user_id};

#[test]
fn new_stores_all_fields() {
    let uid = make_user_id();
    let modify = make_modify(42, 55000, Side::Buy, 20, uid);
    assert_eq!(modify.get_order_id(), 42);
    assert_eq!(modify.get_price(), 55000);
    assert_eq!(modify.get_side(), Side::Buy);
    assert_eq!(modify.get_quantity(), 20);
    assert_eq!(modify.get_status(), OrderStatus::Empty);
    assert_eq!(modify.get_user_id(), uid);
}

#[test]
fn to_order_pointer_creates_correct_order() {
    let uid = make_user_id();
    let modify = make_modify(42, 55000, Side::Sell, 20, uid);
    let order = modify.to_order_pointer(OrderType::FillAndKill);
    assert_eq!(order.get_order_id(), 42);
    assert_eq!(order.get_type(), OrderType::FillAndKill);
    assert_eq!(order.get_side(), Side::Sell);
    assert_eq!(order.get_price(), 55000);
    assert_eq!(order.get_initial_quantity(), 20);
    assert_eq!(order.get_user_id(), uid);
}

#[test]
fn serde_roundtrip() {
    let uid = make_user_id();
    let modify = make_modify(42, 55000, Side::Buy, 20, uid);
    let json = serde_json::to_string(&modify).unwrap();
    let deserialized: OrderModify = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.get_order_id(), modify.get_order_id());
    assert_eq!(deserialized.get_price(), modify.get_price());
    assert_eq!(deserialized.get_side(), modify.get_side());
    assert_eq!(deserialized.get_quantity(), modify.get_quantity());
    assert_eq!(deserialized.get_user_id(), modify.get_user_id());
}
