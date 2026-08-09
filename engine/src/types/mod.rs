use core::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{order::Order, order_modify::OrderModify, trade::Trades, trading_pair::TradingPair};

/// Price of an order in the smallest quote-unit (e.g. cents for USDC).
///
/// Using `i32` allows negative prices to be rejected at the type level
/// while keeping arithmetic simple. Max representable price: ~2.1 billion.
pub type Price = i32;

/// Quantity of an asset being traded, in the smallest base-unit.
///
/// Using `u32` — quantities are always non-negative. Max: ~4.2 billion units.
pub type Quantity = u32;

/// Universally unique identifier for a user account.
///
/// Uses UUID v4 (random). Generated client-side or by the Go API layer;
/// the engine does not create user IDs.
pub type UserId = Uuid;

/// Unique identifier for an order, assigned by the engine via snowflake generation.
///
/// Will be  SnowFlakeId format using [`SnowFlakeGenerator`](crate::snowflake_id::SnowFlakeGenerator)
pub type OrderId = u64;

/// Unique identifier for a trade (fill), assigned by the engine via snowflake generation.
///
/// Trade IDs follow the same SnowFlakeId format as order IDs
/// using [`SnowFlakeGenerator`](crate::snowflake_id::SnowFlakeGenerator),
/// ensuring global uniqueness
/// and time-sortability across distributed engine instances.
pub type TradeId = u64;

/// Errors that can occur when placing or modifying an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderError {
    /// The trading pair has no orderbook in the engine.
    NoSuchPair,
    /// The order references a user that does not exist in the engine.
    NoSuchUser,
    /// The user does not have enough available balance to cover the order.
    InsufficientBalance,
    /// The order itself is invalid (non-positive price/quantity, notional overflow,
    /// duplicate order id, or unsupported side/user change on modify).
    InvalidOrder,
}

/// Determines how long an order lives and how it interacts with the book.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderType {
    /// Remains in the book until explicitly cancelled or fully filled.
    /// The default order type for limit orders.
    GoodTillCancel,
    /// Remains in the book until end of the trading day, then auto-cancelled.
    /// (V1: treated identically to GTC — day-end expiry is not yet implemented.)
    GoodForDay,
    /// Immediate-or-cancel: matches as much as possible at the limit price,
    /// then any unfilled remainder is discarded. Nothing rests in the book.
    FillAndKill,
    /// Fill-or-kill: must be filled entirely in one match, or the entire
    /// order is cancelled. (V1: not yet implemented — behaves like FAK.)
    FillOrKill,
}

/// Which side of the trade the order is on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Side {
    /// Buyer — wants to purchase the base asset with quote currency.
    Buy,
    /// Seller — wants to sell the base asset for quote currency.
    Sell,
}

/// Tracks the lifecycle state of an order.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Order was explicitly cancelled by the user or the engine (e.g. FAK expiry).
    Cancelled,
    /// Some quantity has been matched, but `remaining_quantity > 0`.
    PartiallyFilled,
    /// All quantity has been matched (`remaining_quantity == 0`).
    Filled,
    /// Initial state before any matching. Used as a default when constructing
    /// TODO / DOC: add how we modify order
    Empty,
}

/// Supported crypto assets in the exchange.
///
/// V1 supports a fixed set. A production system would use a string or
/// database-backed asset registry, but an enum keeps things simple for
/// the matching engine prototype.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub enum Asset {
    /// Ethereum
    ETH,
    /// Solana
    SOL,
    /// Bitcoin
    BTC,
    /// USD Coin (stablecoin)
    USDC,
    /// Tether (stablecoin)
    USDT,
}

#[derive(Serialize, Deserialize)]
pub enum WalEntryType {
    // engine wal entry
    AddTradingPair {
        pair: TradingPair,
    },
    RemoveTradingPair {
        pair: TradingPair,
    },
    AddOrder {
        pair: TradingPair,
        order: Order,
        trades: Option<Trades>,
    },
    CancelOrder {
        pair: TradingPair,
        order_id: OrderId,
        success: bool,
    },
    ModifyOrder {
        pair: TradingPair,
        modify: OrderModify,
        trades: Option<Trades>,
    },
    // user wal entry
    AddUser {
        user_id: UserId,
    },
    RemoveUser {
        user_id: UserId,
    },
    DepositBalance {
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    },
    WithdrawBalance {
        user_id: UserId,
        asset: Asset,
        quantity: Quantity,
    },
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Asset::ETH => write!(f, "ETH"),
            Asset::SOL => write!(f, "SOL"),
            Asset::BTC => write!(f, "BTC"),
            Asset::USDC => write!(f, "USDC"),
            Asset::USDT => write!(f, "USDT"),
        }
    }
}
