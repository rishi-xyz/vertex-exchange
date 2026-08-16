mod helpers;

use vertex_engine::types::{OrderStatus, OrderType, Side};

use helpers::{make_order, make_user_id};

#[test]
fn new_sets_fields_correctly() {
    let uid = make_user_id();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 50000, 10, uid);
    assert_eq!(order.get_type(), OrderType::GoodTillCancel);
    assert_eq!(order.get_side(), Side::Buy);
    assert_eq!(order.get_price(), 50000);
    assert_eq!(order.get_initial_quantity(), 10);
    assert_eq!(order.get_remaining_quantity(), 10);
    assert_eq!(order.get_user_id(), uid);
    assert_eq!(order.get_status(), OrderStatus::Empty);
}

#[test]
fn initial_equals_remaining_quantity() {
    let uid = make_user_id();
    let order = make_order(OrderType::GoodTillCancel, Side::Sell, 100, 500, uid);
    assert_eq!(order.get_initial_quantity(), order.get_remaining_quantity());
}

#[test]
fn filled_quantity_zero_for_new_order() {
    let uid = make_user_id();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 50, uid);
    assert_eq!(order.get_filled_quantity(), 0);
}

#[test]
fn is_filled_false_for_new_order() {
    let uid = make_user_id();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 50, uid);
    assert!(!order.is_filled());
}

#[test]
fn partial_fill_reduces_remaining_sets_partially_filled() {
    let uid = make_user_id();
    let mut order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    order.fills(3).unwrap();
    assert_eq!(order.get_remaining_quantity(), 7);
    assert_eq!(order.get_filled_quantity(), 3);
    assert_eq!(order.get_status(), OrderStatus::PartiallyFilled);
}

#[test]
fn exact_fill_sets_filled_status() {
    let uid = make_user_id();
    let mut order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    order.fills(10).unwrap();
    assert_eq!(order.get_remaining_quantity(), 0);
    assert!(order.is_filled());
    assert_eq!(order.get_status(), OrderStatus::Filled);
}

#[test]
fn overfill_returns_err() {
    let uid = make_user_id();
    let mut order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    let result = order.fills(11);
    assert!(result.is_err());
}

#[test]
fn zero_fill_succeeds_remaining_unchanged() {
    let uid = make_user_id();
    let mut order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    order.fills(0).unwrap();
    assert_eq!(order.get_remaining_quantity(), 10);
    // A zero fill is a no-op: status must not be marked as filled/partial
    assert_eq!(order.get_status(), OrderStatus::Empty);
}

#[test]
fn sequential_fills_accumulate() {
    let uid = make_user_id();
    let mut order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    order.fills(3).unwrap();
    assert_eq!(order.get_remaining_quantity(), 7);
    assert_eq!(order.get_status(), OrderStatus::PartiallyFilled);
    order.fills(7).unwrap();
    assert_eq!(order.get_remaining_quantity(), 0);
    assert_eq!(order.get_status(), OrderStatus::Filled);
}

#[test]
fn timestamp_is_nonzero() {
    let uid = make_user_id();
    let order = make_order(OrderType::GoodTillCancel, Side::Buy, 100, 10, uid);
    assert!(order.get_timestamp() > 0);
}
