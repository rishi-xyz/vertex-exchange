mod helpers;

use vertex_engine::types::Asset;
use vertex_engine::user::User;

use helpers::make_user_id;

#[test]
fn new_with_none_generates_random_uuid() {
    let user = User::new(None);
    let user2 = User::new(None);
    assert_ne!(user.get_id(), user2.get_id());
}

#[test]
fn new_with_some_uses_provided_uuid() {
    let uid = make_user_id();
    let user = User::new(Some(uid));
    assert_eq!(user.get_id(), uid);
}

#[test]
fn add_balance_credits() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    assert_eq!(user.get_balance(&Asset::USDC), 10000);
}

#[test]
fn add_balance_accumulates() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 1000);
    user.add_balance(Asset::USDC, 2000);
    assert_eq!(user.get_balance(&Asset::USDC), 3000);
}

#[test]
fn get_balance_returns_total() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    assert_eq!(user.get_balance(&Asset::USDC), 5000);
}

#[test]
fn available_balance_no_locks_equals_total() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    assert_eq!(user.get_available_balance(&Asset::USDC), 5000);
}

#[test]
fn available_balance_after_lock_reduces() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    user.lock(1, Asset::USDC, 2000).unwrap();
    assert_eq!(user.get_available_balance(&Asset::USDC), 3000);
    assert_eq!(user.get_balance(&Asset::USDC), 5000);
}

#[test]
fn available_balance_multiple_locks_accumulate() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 2000).unwrap();
    user.lock(2, Asset::USDC, 3000).unwrap();
    assert_eq!(user.get_available_balance(&Asset::USDC), 5000);
}

#[test]
fn available_balance_unknown_asset_returns_zero() {
    let user = User::new(None);
    assert_eq!(user.get_available_balance(&Asset::ETH), 0);
}

#[test]
fn lock_success() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    assert!(user.lock(1, Asset::USDC, 2000).is_ok());
    assert_eq!(user.get_locked_balance(Asset::USDC), 2000);
}

#[test]
fn lock_insufficient_balance_errs() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 100);
    let result = user.lock(1, Asset::USDC, 200);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Insufficient balance");
}

#[test]
fn lock_exact_available_succeeds() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    assert!(user.lock(1, Asset::USDC, 5000).is_ok());
    assert_eq!(user.get_available_balance(&Asset::USDC), 0);
}

#[test]
fn lock_zero_amount() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    assert!(user.lock(1, Asset::USDC, 0).is_ok());
    assert_eq!(user.get_available_balance(&Asset::USDC), 5000);
}

#[test]
fn lock_same_order_id_overwrites() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 2000).unwrap();
    user.lock(1, Asset::USDC, 3000).unwrap();
    assert_eq!(user.get_locked_balance(Asset::USDC), 3000);
}

#[test]
fn unlock_order_success_restores_balance() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    user.lock(1, Asset::USDC, 2000).unwrap();
    assert_eq!(user.get_available_balance(&Asset::USDC), 3000);
    user.unlock_order(&1).unwrap();
    assert_eq!(user.get_available_balance(&Asset::USDC), 5000);
    assert_eq!(user.get_locked_balance(Asset::USDC), 0);
}

#[test]
fn unlock_order_not_found_errs() {
    let mut user = User::new(None);
    let result = user.unlock_order(&999);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Order not Locked");
}

#[test]
fn get_locked_balance_sums_all_locks() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 1000).unwrap();
    user.lock(2, Asset::USDC, 2000).unwrap();
    user.lock(3, Asset::USDC, 3000).unwrap();
    assert_eq!(user.get_locked_balance(Asset::USDC), 6000);
}

#[test]
fn get_locked_balance_no_locks_zero() {
    let user = User::new(None);
    assert_eq!(user.get_locked_balance(Asset::USDC), 0);
}

#[test]
fn apply_fill_partial_reduces_lock_credits_received() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.add_balance(Asset::ETH, 0);
    user.lock(1, Asset::USDC, 5000).unwrap();
    user.apply_fill(&1, Asset::USDC, 2000, Asset::ETH, 1)
        .unwrap();
    assert_eq!(user.get_locked_balance(Asset::USDC), 3000);
    assert_eq!(user.get_balance(&Asset::ETH), 1);
}

#[test]
fn apply_fill_full_removes_lock_entry() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.add_balance(Asset::ETH, 0);
    user.lock(1, Asset::USDC, 5000).unwrap();
    user.apply_fill(&1, Asset::USDC, 5000, Asset::ETH, 5)
        .unwrap();
    assert_eq!(user.get_locked_balance(Asset::USDC), 0);
    assert_eq!(user.get_balance(&Asset::ETH), 5);
}

#[test]
fn apply_fill_wrong_asset_errs() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 5000).unwrap();
    let result = user.apply_fill(&1, Asset::ETH, 2000, Asset::USDC, 1);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Locked asset mismatch");
}

#[test]
fn apply_fill_insufficient_locked_errs() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 1000).unwrap();
    let result = user.apply_fill(&1, Asset::USDC, 2000, Asset::ETH, 1);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Locked amount insufficient for fill");
}

#[test]
fn substract_balance_success() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.substract_balance(Asset::USDC, 3000).unwrap();
    assert_eq!(user.get_balance(&Asset::USDC), 7000);
}

#[test]
fn substract_balance_insufficient_errs() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 1000);
    let result = user.substract_balance(Asset::USDC, 2000);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Insufficient Balance");
}

#[test]
fn substract_balance_exact() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 5000);
    user.substract_balance(Asset::USDC, 5000).unwrap();
    assert_eq!(user.get_balance(&Asset::USDC), 0);
}

#[test]
fn substract_balance_respects_locked_funds() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 4000).unwrap();
    let result = user.substract_balance(Asset::USDC, 8000);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Insufficient Balance");
}

#[test]
fn get_all_balances_returns_all() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.add_balance(Asset::ETH, 5);
    user.add_balance(Asset::BTC, 1);
    let balances = user.get_all_balances();
    assert_eq!(balances.get(&Asset::USDC), Some(&10000));
    assert_eq!(balances.get(&Asset::ETH), Some(&5));
    assert_eq!(balances.get(&Asset::BTC), Some(&1));
}

#[test]
fn get_all_balances_includes_locked() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 3000).unwrap();
    let balances = user.get_all_balances();
    assert_eq!(balances.get(&Asset::USDC), Some(&10000));
}

#[test]
fn get_all_locked_balances_returns_locks() {
    let mut user = User::new(None);
    user.add_balance(Asset::USDC, 10000);
    user.lock(1, Asset::USDC, 2000).unwrap();
    user.lock(2, Asset::USDC, 3000).unwrap();
    let locks = user.get_all_locked_balances();
    assert_eq!(locks.len(), 2);
    assert!(locks.contains_key(&1));
    assert!(locks.contains_key(&2));
}
