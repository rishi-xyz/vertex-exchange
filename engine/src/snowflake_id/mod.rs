//! Snowflake ID generator — produces 64-bit, time-sortable, globally unique IDs.
//!
//! Used for order IDs and trade IDs. Snowflake IDs encode timestamp, datacenter,
//! machine, and sequence information into a single `u64`, making them:
//!
//! - **Time-sortable** — IDs generated later are always larger
//! - **Globally unique** — no collisions across datacenters or machines
//! - **Compact** — single `u64`, no UUID overhead
//!
//! # Bit Layout (64-bit)
///
/// ```text
/// 0 | 41-bit timestamp_ms | 5-bit datacenter_id | 5-bit machine_id | 12-bit sequence
/// ```
///
/// | Field | Bits | Range | Purpose |
/// |-------|------|-------|---------|
/// | timestamp_ms | 41 | ~69 years from epoch | Milliseconds since custom epoch |
/// | datacenter_id | 5 | 0–31 | Prevents cross-datacenter collisions |
/// | machine_id | 5 | 0–31 | Prevents same-datacenter collisions |
/// | sequence | 12 | 0–4095 | Per-millisecond counter for burst traffic |
///
/// # Clock Regression
///
/// If the system clock moves backwards, the generator **panics** rather than
/// producing duplicate IDs. In production, you'd handle this with a configurable
/// tolerance or NTP-safe fallback. For V1, a panic is acceptable.
///
/// # Sequence Overflow
///
/// If more than 4096 IDs are generated within a single millisecond, the generator
/// spins until the next millisecond. This handles burst traffic without collisions.
use std::time::{SystemTime, UNIX_EPOCH};

pub const MACHINE_BITS: u64 = 5;
pub const DATACENTER_BITS: u64 = 5;
pub const SEQUENCE_BITS: u64 = 12;

pub const MAX_MACHINE_ID: u64 = (1 << MACHINE_BITS) - 1;
pub const MAX_DATACENTER_ID: u64 = (1 << DATACENTER_BITS) - 1;
pub const MAX_SEQUENCE_ID: u64 = (1 << SEQUENCE_BITS) - 1;

pub const MACHINE_SHIFT: u64 = SEQUENCE_BITS;
pub const DATACENTER_SHIFT: u64 = MACHINE_SHIFT + MACHINE_BITS;
pub const TIMESTAMP_SHIFT: u64 = DATACENTER_SHIFT + DATACENTER_BITS;

/// Custom epoch (2024-01-01 00:00:00 UTC).
/// Unix timestamp: 1704067200000 ms.
/// All timestamps are stored as milliseconds since this fixed epoch.
const EPOCH: u64 = 1_704_067_200_000;

/// Generates unique 64-bit snowflake IDs.
///
/// Each generator instance is identified by a `(machine_id, datacenter_id)` pair.
/// In a single-instance deployment (V1), both are `1`. In a multi-instance setup,
/// each instance gets a unique pair to prevent ID collisions.
///
/// # Panics
///
/// [`next_id`](SnowflakeGenerator::next_id) panics if the system clock moves backwards.
///
/// # Examples
///
/// ```rust
/// let mut generator = vertex_engine::snowflake_id::SnowFlakeGenerator::new(1, 1);
/// let id1 = generator.next_id();
/// let id2 = generator.next_id();
/// assert!(id2 > id1);
/// ```
#[derive(Debug)]
pub struct SnowFlakeGenerator {
    machine_id: u64,
    datacenter_id: u64,
    sequence: u64,
    last_timestamp: u64,
}

impl SnowFlakeGenerator {
    /// Creates a new snowflake generator
    ///
    /// # Arguments
    ///
    /// * `machine_id` — Unique machine identifier (0–31). Panics if out of range.
    /// * `datacenter_id` — Unique datacenter identifier (0–31). Panics if out of range.
    ///
    /// # Panics
    ///
    /// Panics if `machine_id > 31` or `datacenter_id > 31`.
    /// # Example
    /// ```rust
    /// let mut generator = vertex_engine::snowflake_id::SnowFlakeGenerator::new(1, 1);
    /// ```
    pub fn new(machine_id: u64, datacenter_id: u64) -> Self {
        assert!(
            machine_id <= MAX_MACHINE_ID,
            "machine_id must be between 0 - {}",
            MAX_DATACENTER_ID
        );
        assert!(
            datacenter_id <= MAX_DATACENTER_ID,
            "datacenter_id must be between 0 - {}",
            MAX_DATACENTER_ID
        );
        SnowFlakeGenerator {
            machine_id,
            datacenter_id,
            sequence: (0),
            last_timestamp: (0),
        }
    }

    /// Generates the next unique snowflake ID.
    ///
    /// Handles two edge cases:
    /// - **Same millisecond** — increments the sequence counter. If the counter
    ///   overflows ( exceeds 4095), spins until the next millisecond.
    /// - **Clock regression** — panics immediately. A production system might
    ///   use a tolerance window or NTP-safe fallback.
    ///
    /// # Panics
    ///
    /// Panics if the system clock has moved backwards since the last call.
    ///
    /// # Returns
    ///
    /// A 64-bit snowflake ID encoding `(timestamp, datacenter, machine, sequence)`.
    ///
    /// # Note
    /// ### Masking and Collisions
    ///
    /// If multiple IDs are generated within the same millisecond, increment the
    /// sequence counter instead of advancing the timestamp.
    ///
    /// The sequence field is limited to 12 bits (0–4095). To keep it within this
    /// range, we increment and mask it with `MAX_SEQUENCE`.
    ///
    /// ```text
    /// sequence = (sequence + 1) & 0b111111111111
    /// ```
    ///
    /// This bitmask preserves only the lower 12 bits. When the sequence reaches
    /// 4095 (`0b111111111111`) and is incremented again, it wraps back to `0`.
    ///
    /// ```text
    /// 4094 -> 4095 -> 0 -> 1 -> ...
    /// ```
    ///
    /// If the sequence wraps to `0`, we've already generated every possible
    /// sequence value for the current millisecond (4096 IDs). Generating another
    /// ID immediately would reuse the same `(timestamp, sequence)` pair and
    /// produce a duplicate ID.
    ///
    /// To avoid collisions, we busy-wait until the system clock advances to the
    /// next millisecond. Once the timestamp changes, the sequence is reset to `0`
    /// and ID generation can safely continue.
    pub fn next_id(&mut self) -> u64 {
        let mut timestamp = Self::current_millis();
        assert!(
            timestamp >= self.last_timestamp,
            "Clock moved backwards. Refusing to generate id for {} milliseconds",
            self.last_timestamp - timestamp
        );

        if timestamp == self.last_timestamp {
            self.sequence = (self.sequence + 1) & MAX_SEQUENCE_ID;
            if self.sequence == 0 {
                // All 4096 sequence values have been used in the current millisecond.
                // Wait until the clock advances before generating another ID.
                while timestamp <= self.last_timestamp {
                    std::hint::spin_loop();
                    timestamp = Self::current_millis();
                }
            }
        } else {
            self.sequence = 0;
        }
        self.last_timestamp = timestamp;
        ((timestamp - EPOCH) << TIMESTAMP_SHIFT)
            | (self.datacenter_id << DATACENTER_SHIFT)
            | (self.machine_id << MACHINE_SHIFT)
            | (self.sequence)
    }

    /// Returns the current time in milliseconds since the Unix epoch.
    fn current_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime is before UNIX_EPOCH")
            .as_millis() as u64
    }
}
