use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::types::{Price, Quantity};

/// A single price level with its aggregated quantity.
///
/// Used in orderbook depth snapshots. `quantity` is the sum of
/// `remaining_quantity` across all orders at this price level.
///
/// # Examples
///
/// ```ignore
/// let level = LevelInfo::new(50000, 120);
/// assert_eq!(level.price, 50000);
/// assert_eq!(level.quantity, 120);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelInfo {
    /// Price in quote units
    pub price: Price,
    /// Total remaining quantity across all orders at this price
    pub quantity: Quantity,
}

impl LevelInfo {
    /// Creates a new price level snapshot.
    pub fn new(price: Price, quantity: Quantity) -> Self {
        LevelInfo { price, quantity }
    }
}

// shared slice of price levels.
pub type LevelInfos = VecDeque<LevelInfo>;

/// A full orderbook depth snapshot — bids and asks at each price level.
///
/// Bids are sorted ascending by price (best bid last); asks are sorted
/// ascending by price (best ask first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevelInfo {
    /// Bid levels, sorted ascending by price (best bid is the last element)
    bids: LevelInfos,
    /// Ask levels, sorted ascending by price (best ask is the first element)
    asks: LevelInfos,
}

impl OrderBookLevelInfo {
    /// Creates a new orderbook level info snapshot.
    ///
    /// # Arguments
    ///
    /// * `bids_` — Bid price levels (ascending by price)
    /// * `asks_` — Ask price levels (ascending by price)
    pub fn new(bids: LevelInfos, asks: LevelInfos) -> Self {
        OrderBookLevelInfo { bids, asks }
    }
}

pub trait getOrderBookLevelInfos {
    /// Returns the bid levels reference (ascending by price; best bid is last).
    fn get_bids(&self) -> &LevelInfos;
    /// Returns the ask levels reference (ascending by price; best ask is first).
    fn get_asks(&self) -> &LevelInfos;
}

impl getOrderBookLevelInfos for OrderBookLevelInfo {
    fn get_asks(&self) -> &LevelInfos {
        return &self.asks;
    }

    fn get_bids(&self) -> &LevelInfos {
        return &self.bids;
    }
}
