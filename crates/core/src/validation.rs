use rust_decimal::Decimal;
use std::time::Duration;

/// Validates orderbook data freshness and quality
pub struct OrderBookValidator {
    max_age_ms: u64,
    min_depth: usize,
    max_spread_bps: Decimal,
}

impl OrderBookValidator {
    pub fn new(max_age_ms: u64, min_depth: usize, max_spread_bps: Decimal) -> Self {
        Self {
            max_age_ms,
            min_depth,
            max_spread_bps,
        }
    }

    pub fn institutional() -> Self {
        Self {
            max_age_ms: 1000, // 1 second max staleness
            min_depth: 5,     // At least 5 price levels
            max_spread_bps: 100, // Max 1% spread
        }
    }

    pub fn is_fresh(&self, timestamp_ns: u64, now_ns: u64) -> bool {
        let age_ms = (now_ns.saturating_sub(timestamp_ns)) / 1_000_000;
        age_ms <= self.max_age_ms
    }

    pub fn has_sufficient_depth(&self, bid_levels: usize, ask_levels: usize) -> bool {
        bid_levels >= self.min_depth && ask_levels >= self.min_depth
    }

    pub fn is_spread_acceptable(&self, spread: Decimal, mid_price: Decimal) -> bool {
        if mid_price == Decimal::ZERO {
            return false;
        }
        let spread_bps = (spread / mid_price) * Decimal::from(10000);
        spread_bps <= self.max_spread_bps
    }
}

/// Validates liquidity depth for trade execution
pub struct LiquidityValidator {
    min_depth_levels: usize,
    min_total_liquidity: Decimal,
    max_slippage_bps: Decimal,
}

impl LiquidityValidator {
    pub fn new(
        min_depth_levels: usize,
        min_total_liquidity: Decimal,
        max_slippage_bps: Decimal,
    ) -> Self {
        Self {
            min_depth_levels,
            min_total_liquidity,
            max_slippage_bps,
        }
    }

    pub fn institutional() -> Self {
        Self {
            min_depth_levels: 10,
            min_total_liquidity: Decimal::from(100000), // $100k
            max_slippage_bps: Decimal::from(20), // 20 bps = 0.2%
        }
    }

    pub fn validate_depth(
        &self,
        levels: &[crate::types::PriceLevel],
        required_quantity: Decimal,
    ) -> Result<Decimal, LiquidityError> {
        if levels.len() < self.min_depth_levels {
            return Err(LiquidityError::InsufficientDepth {
                available: levels.len(),
                required: self.min_depth_levels,
            });
        }

        let mut remaining = required_quantity;
        let mut total_value = Decimal::ZERO;

        for level in levels.iter().take(self.min_depth_levels) {
            let qty = remaining.min(level.quantity);
            total_value += qty * level.price;
            remaining -= qty;

            if remaining <= Decimal::ZERO {
                break;
            }
        }

        if remaining > Decimal::ZERO {
            return Err(LiquidityError::InsufficientQuantity {
                required: required_quantity,
                available: required_quantity - remaining,
            });
        }

        if total_value < self.min_total_liquidity {
            return Err(LiquidityError::InsufficientValue {
                available: total_value,
                required: self.min_total_liquidity,
            });
        }

        Ok(total_value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LiquidityError {
    #[error("Insufficient orderbook depth: {available} levels, need {required}")]
    InsufficientDepth { available: usize, required: usize },

    #[error("Insufficient quantity: need {required}, available {available}")]
    InsufficientQuantity {
        required: Decimal,
        available: Decimal,
    },

    #[error("Insufficient liquidity value: ${available}, need ${required}")]
    InsufficientValue {
        available: Decimal,
        required: Decimal,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PriceLevel;
    use rust_decimal_macros::dec;

    #[test]
    fn test_freshness_validation() {
        let validator = OrderBookValidator::institutional();
        let now = 1000_000_000_000u64; // 1 billion nanoseconds

        assert!(validator.is_fresh(now - 500_000_000, now)); // 500ms old - fresh
        assert!(!validator.is_fresh(now - 2_000_000_000, now)); // 2s old - stale
    }

    #[test]
    fn test_liquidity_validation() {
        let validator = LiquidityValidator::institutional();
        let levels = vec![
            PriceLevel::new(dec!(50000), dec!(1.0)),
            PriceLevel::new(dec!(50001), dec!(2.0)),
            PriceLevel::new(dec!(50002), dec!(3.0)),
        ];

        // Not enough depth
        assert!(validator.validate_depth(&levels, dec!(1.0)).is_err());
    }
}
