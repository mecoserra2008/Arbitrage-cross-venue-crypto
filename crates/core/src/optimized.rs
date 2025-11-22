/// Ultra-low latency optimizations for critical paths
use crate::types::{OrderBookSnapshot, PriceLevel};
use arrayvec::ArrayVec;
use rust_decimal::Decimal;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Maximum price levels to process (stack-allocated)
const MAX_LEVELS: usize = 32;

/// Fast orderbook snapshot with stack allocation
#[repr(align(64))]  // Cache line alignment
pub struct FastOrderBookSnapshot {
    pub bids: ArrayVec<PriceLevel, MAX_LEVELS>,
    pub asks: ArrayVec<PriceLevel, MAX_LEVELS>,
    pub timestamp_ns: u64,
    pub sequence: u64,
}

impl FastOrderBookSnapshot {
    #[inline(always)]
    pub fn from_snapshot(snapshot: &OrderBookSnapshot) -> Self {
        let mut bids = ArrayVec::new();
        let mut asks = ArrayVec::new();

        // Take only top levels (most important for arbitrage)
        for level in snapshot.bids.iter().take(MAX_LEVELS) {
            if bids.is_full() { break; }
            bids.push(*level);
        }

        for level in snapshot.asks.iter().take(MAX_LEVELS) {
            if asks.is_full() { break; }
            asks.push(*level);
        }

        Self {
            bids,
            asks,
            timestamp_ns: snapshot.timestamp_ns,
            sequence: snapshot.sequence,
        }
    }

    #[inline(always)]
    pub fn best_bid(&self) -> Option<&PriceLevel> {
        self.bids.first()
    }

    #[inline(always)]
    pub fn best_ask(&self) -> Option<&PriceLevel> {
        self.asks.first()
    }

    #[inline(always)]
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    /// Fast VWAP calculation using array iteration (cache-friendly)
    #[inline(always)]
    pub fn vwap(&self, side: crate::types::Side, mut quantity: Decimal) -> Option<Decimal> {
        let levels = match side {
            crate::types::Side::Buy => &self.asks,
            crate::types::Side::Sell => &self.bids,
        };

        let mut total_cost = Decimal::ZERO;
        let mut total_qty = Decimal::ZERO;

        for level in levels.iter() {
            if quantity <= Decimal::ZERO {
                break;
            }

            let qty = quantity.min(level.quantity);
            total_cost += qty * level.price;
            total_qty += qty;
            quantity -= qty;
        }

        if total_qty > Decimal::ZERO {
            Some(total_cost / total_qty)
        } else {
            None
        }
    }
}

/// Lock-free trading state using atomics
pub struct FastTradingState {
    enabled: AtomicBool,
    opportunities_detected: AtomicU64,
    opportunities_executed: AtomicU64,
    last_execution_ns: AtomicU64,
}

impl FastTradingState {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            opportunities_detected: AtomicU64::new(0),
            opportunities_executed: AtomicU64::new(0),
            last_execution_ns: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[inline(always)]
    pub fn record_opportunity(&self) {
        self.opportunities_detected.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_execution(&self, timestamp_ns: u64) {
        self.opportunities_executed.fetch_add(1, Ordering::Relaxed);
        self.last_execution_ns.store(timestamp_ns, Ordering::Release);
    }

    #[inline(always)]
    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.opportunities_detected.load(Ordering::Relaxed),
            self.opportunities_executed.load(Ordering::Relaxed),
            self.last_execution_ns.load(Ordering::Relaxed),
        )
    }
}

impl Default for FastTradingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Fast validation checks using early returns
pub struct FastValidator;

impl FastValidator {
    /// Quick freshness check (branchless where possible)
    #[inline(always)]
    pub fn is_fresh(timestamp_ns: u64, now_ns: u64, max_age_ns: u64) -> bool {
        now_ns.saturating_sub(timestamp_ns) <= max_age_ns
    }

    /// Quick depth check
    #[inline(always)]
    pub fn has_depth(bid_count: usize, ask_count: usize, min_depth: usize) -> bool {
        bid_count >= min_depth && ask_count >= min_depth
    }

    /// Quick spread check (most common rejection case)
    #[inline(always)]
    pub fn is_spread_valid(bid: Decimal, ask: Decimal) -> bool {
        ask > bid  // Fast path: price discrepancy exists
    }

    /// Combined fast check (returns immediately on first failure)
    #[inline(always)]
    pub fn quick_validate(
        bid: Option<&PriceLevel>,
        ask: Option<&PriceLevel>,
        timestamp_ns: u64,
        now_ns: u64,
    ) -> bool {
        // Fast rejection path
        let (bid, ask) = match (bid, ask) {
            (Some(b), Some(a)) => (b, a),
            _ => return false,
        };

        // Most common rejection: no price discrepancy
        if ask.price <= bid.price {
            return false;
        }

        // Check freshness (1 second = 1,000,000,000 ns)
        if !Self::is_fresh(timestamp_ns, now_ns, 1_000_000_000) {
            return false;
        }

        true
    }
}

/// Pre-allocated buffer pool for zero-copy operations
pub struct BufferPool {
    buffers: crossbeam::queue::ArrayQueue<Vec<u8>>,
}

impl BufferPool {
    pub fn new(capacity: usize, buffer_size: usize) -> Self {
        let buffers = crossbeam::queue::ArrayQueue::new(capacity);

        for _ in 0..capacity {
            let _ = buffers.push(Vec::with_capacity(buffer_size));
        }

        Self { buffers }
    }

    #[inline(always)]
    pub fn acquire(&self) -> Option<Vec<u8>> {
        self.buffers.pop()
    }

    #[inline(always)]
    pub fn release(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        let _ = self.buffers.push(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Exchange, InstrumentType, Symbol};
    use rust_decimal_macros::dec;

    #[test]
    fn test_fast_snapshot() {
        let snapshot = OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: Symbol::perpetual("BTC", "USDT"),
            bids: vec![
                PriceLevel::new(dec!(50000), dec!(1.0)),
                PriceLevel::new(dec!(49999), dec!(2.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50001), dec!(1.0)),
                PriceLevel::new(dec!(50002), dec!(2.0)),
            ],
            timestamp_ns: 1000,
            sequence: 1,
        };

        let fast = FastOrderBookSnapshot::from_snapshot(&snapshot);

        assert_eq!(fast.best_bid().unwrap().price, dec!(50000));
        assert_eq!(fast.best_ask().unwrap().price, dec!(50001));
        assert_eq!(fast.spread().unwrap(), dec!(1));
    }

    #[test]
    fn test_fast_validator() {
        let now = 1_000_000_000u64;
        let old = 500_000_000u64;

        assert!(FastValidator::is_fresh(now, now, 1_000_000_000));
        assert!(!FastValidator::is_fresh(old, now, 1_000_000_000));
    }

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new(10, 4096);

        let buf1 = pool.acquire().unwrap();
        assert_eq!(buf1.capacity(), 4096);

        pool.release(buf1);

        let buf2 = pool.acquire().unwrap();
        assert_eq!(buf2.len(), 0);  // Should be cleared
    }
}
