use dashmap::DashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Deduplicates arbitrage opportunities to prevent duplicate executions
pub struct OpportunityDeduplicator {
    seen_opportunities: Arc<DashMap<OpportunityFingerprint, Instant>>,
    cooldown_duration: Duration,
}

impl OpportunityDeduplicator {
    pub fn new(cooldown_ms: u64) -> Self {
        Self {
            seen_opportunities: Arc::new(DashMap::new()),
            cooldown_duration: Duration::from_millis(cooldown_ms),
        }
    }

    pub fn institutional() -> Self {
        Self::new(5000) // 5 second cooldown
    }

    /// Check if opportunity is duplicate, returns true if it's new/cooled down
    pub fn check_and_mark(&self, fingerprint: OpportunityFingerprint) -> bool {
        let now = Instant::now();

        // Clean up expired entries periodically
        if self.seen_opportunities.len() > 1000 {
            self.cleanup_expired(now);
        }

        match self.seen_opportunities.get(&fingerprint) {
            Some(last_seen) => {
                if now.duration_since(*last_seen) > self.cooldown_duration {
                    // Cooldown period expired, allow execution
                    self.seen_opportunities.insert(fingerprint, now);
                    true
                } else {
                    // Still in cooldown
                    false
                }
            }
            None => {
                // New opportunity
                self.seen_opportunities.insert(fingerprint, now);
                true
            }
        }
    }

    fn cleanup_expired(&self, now: Instant) {
        self.seen_opportunities.retain(|_, last_seen| {
            now.duration_since(*last_seen) <= self.cooldown_duration * 2
        });
    }

    pub fn clear(&self) {
        self.seen_opportunities.clear();
    }

    pub fn size(&self) -> usize {
        self.seen_opportunities.len()
    }
}

/// Unique identifier for an arbitrage opportunity
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpportunityFingerprint {
    buy_exchange: String,
    sell_exchange: String,
    symbol: String,
    // Round prices to avoid minor fluctuations creating new opportunities
    buy_price_rounded: i64,
    sell_price_rounded: i64,
}

impl OpportunityFingerprint {
    pub fn new(
        buy_exchange: &str,
        sell_exchange: &str,
        symbol: &str,
        buy_price: rust_decimal::Decimal,
        sell_price: rust_decimal::Decimal,
    ) -> Self {
        // Round to nearest dollar to group similar opportunities
        let buy_price_rounded = (buy_price.round().to_string().parse::<f64>().unwrap_or(0.0) as i64);
        let sell_price_rounded = (sell_price.round().to_string().parse::<f64>().unwrap_or(0.0) as i64);

        Self {
            buy_exchange: buy_exchange.to_string(),
            sell_exchange: sell_exchange.to_string(),
            symbol: symbol.to_string(),
            buy_price_rounded,
            sell_price_rounded,
        }
    }
}

/// Rate limiter using token bucket algorithm
pub struct TokenBucketRateLimiter {
    capacity: u32,
    refill_rate: u32, // tokens per second
    tokens: Arc<parking_lot::Mutex<f64>>,
    last_refill: Arc<parking_lot::Mutex<Instant>>,
}

impl TokenBucketRateLimiter {
    pub fn new(capacity: u32, refill_rate: u32) -> Self {
        Self {
            capacity,
            refill_rate,
            tokens: Arc::new(parking_lot::Mutex::new(capacity as f64)),
            last_refill: Arc::new(parking_lot::Mutex::new(Instant::now())),
        }
    }

    /// Try to consume tokens, returns true if successful
    pub fn try_acquire(&self, tokens: u32) -> bool {
        self.refill();

        let mut token_count = self.tokens.lock();
        if *token_count >= tokens as f64 {
            *token_count -= tokens as f64;
            true
        } else {
            false
        }
    }

    fn refill(&self) {
        let now = Instant::now();
        let mut last_refill = self.last_refill.lock();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();

        if elapsed > 0.0 {
            let mut tokens = self.tokens.lock();
            let new_tokens = elapsed * self.refill_rate as f64;
            *tokens = (*tokens + new_tokens).min(self.capacity as f64);
            *last_refill = now;
        }
    }

    pub fn available_tokens(&self) -> u32 {
        self.refill();
        self.tokens.lock().floor() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::thread;

    #[test]
    fn test_deduplication() {
        let dedup = OpportunityDeduplicator::new(1000);

        let fp = OpportunityFingerprint::new(
            "Binance",
            "Bybit",
            "BTC/USDT",
            dec!(50000),
            dec!(50100),
        );

        // First check should pass
        assert!(dedup.check_and_mark(fp.clone()));

        // Immediate duplicate should fail
        assert!(!dedup.check_and_mark(fp.clone()));

        // After cooldown, should pass again
        thread::sleep(Duration::from_millis(1100));
        assert!(dedup.check_and_mark(fp));
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = TokenBucketRateLimiter::new(10, 5);

        // Should be able to consume up to capacity
        for _ in 0..10 {
            assert!(limiter.try_acquire(1));
        }

        // Should fail after exhausting tokens
        assert!(!limiter.try_acquire(1));

        // Wait for refill
        thread::sleep(Duration::from_millis(500));

        // Should have ~2-3 tokens refilled
        assert!(limiter.available_tokens() >= 2);
    }
}
