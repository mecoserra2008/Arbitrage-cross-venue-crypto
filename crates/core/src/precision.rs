use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Exchange-specific trading rules and precision requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRules {
    /// Minimum order quantity
    pub min_quantity: Decimal,

    /// Maximum order quantity
    pub max_quantity: Decimal,

    /// Quantity step size (lot size)
    pub quantity_step: Decimal,

    /// Minimum notional value (price * quantity)
    pub min_notional: Decimal,

    /// Price tick size
    pub price_tick: Decimal,

    /// Number of decimal places for quantity
    pub quantity_precision: u32,

    /// Number of decimal places for price
    pub price_precision: u32,
}

impl ExchangeRules {
    /// Round quantity to exchange precision
    pub fn round_quantity(&self, quantity: Decimal) -> Decimal {
        let multiplier = Decimal::from(10u64.pow(self.quantity_precision));
        (quantity * multiplier).round() / multiplier
    }

    /// Round price to exchange precision
    pub fn round_price(&self, price: Decimal) -> Decimal {
        let multiplier = Decimal::from(10u64.pow(self.price_precision));
        (price * multiplier).round() / multiplier
    }

    /// Adjust quantity to lot size
    pub fn adjust_to_lot_size(&self, quantity: Decimal) -> Decimal {
        if self.quantity_step == Decimal::ZERO {
            return quantity;
        }

        let lots = (quantity / self.quantity_step).floor();
        let adjusted = lots * self.quantity_step;
        self.round_quantity(adjusted)
    }

    /// Validate if order meets exchange requirements
    pub fn validate_order(&self, price: Decimal, quantity: Decimal) -> Result<(), PrecisionError> {
        let adjusted_qty = self.adjust_to_lot_size(quantity);

        if adjusted_qty < self.min_quantity {
            return Err(PrecisionError::QuantityTooSmall {
                quantity: adjusted_qty,
                minimum: self.min_quantity,
            });
        }

        if adjusted_qty > self.max_quantity {
            return Err(PrecisionError::QuantityTooLarge {
                quantity: adjusted_qty,
                maximum: self.max_quantity,
            });
        }

        let notional = price * adjusted_qty;
        if notional < self.min_notional {
            return Err(PrecisionError::NotionalTooSmall {
                notional,
                minimum: self.min_notional,
            });
        }

        Ok(())
    }
}

/// Precision error types
#[derive(Debug, thiserror::Error)]
pub enum PrecisionError {
    #[error("Quantity {quantity} below minimum {minimum}")]
    QuantityTooSmall { quantity: Decimal, minimum: Decimal },

    #[error("Quantity {quantity} above maximum {maximum}")]
    QuantityTooLarge { quantity: Decimal, maximum: Decimal },

    #[error("Notional value ${notional} below minimum ${minimum}")]
    NotionalTooSmall { notional: Decimal, minimum: Decimal },

    #[error("Invalid lot size: {0}")]
    InvalidLotSize(String),
}

/// Manager for exchange-specific rules
pub struct ExchangeRulesManager {
    rules: HashMap<(String, String), ExchangeRules>, // (exchange, symbol) -> rules
}

impl ExchangeRulesManager {
    pub fn new() -> Self {
        let mut rules = HashMap::new();

        // Binance BTC/USDT Perpetual
        rules.insert(
            ("Binance".to_string(), "BTC/USDT".to_string()),
            ExchangeRules {
                min_quantity: Decimal::new(1, 3), // 0.001 BTC
                max_quantity: Decimal::new(1000, 0),
                quantity_step: Decimal::new(1, 3), // 0.001
                min_notional: Decimal::new(5, 0), // $5
                price_tick: Decimal::new(1, 1), // 0.1
                quantity_precision: 3,
                price_precision: 1,
            },
        );

        // Binance ETH/USDT Perpetual
        rules.insert(
            ("Binance".to_string(), "ETH/USDT".to_string()),
            ExchangeRules {
                min_quantity: Decimal::new(1, 3), // 0.001 ETH
                max_quantity: Decimal::new(10000, 0),
                quantity_step: Decimal::new(1, 3), // 0.001
                min_notional: Decimal::new(5, 0), // $5
                price_tick: Decimal::new(1, 2), // 0.01
                quantity_precision: 3,
                price_precision: 2,
            },
        );

        // Bybit BTC/USDT Perpetual
        rules.insert(
            ("Bybit".to_string(), "BTC/USDT".to_string()),
            ExchangeRules {
                min_quantity: Decimal::new(1, 4), // 0.0001 BTC
                max_quantity: Decimal::new(500, 0),
                quantity_step: Decimal::new(1, 4), // 0.0001
                min_notional: Decimal::new(10, 0), // $10
                price_tick: Decimal::new(5, 1), // 0.5
                quantity_precision: 4,
                price_precision: 1,
            },
        );

        // OKX BTC/USDT Perpetual
        rules.insert(
            ("OKX".to_string(), "BTC/USDT".to_string()),
            ExchangeRules {
                min_quantity: Decimal::new(1, 4), // 0.0001 BTC
                max_quantity: Decimal::new(1000, 0),
                quantity_step: Decimal::new(1, 4), // 0.0001
                min_notional: Decimal::new(1, 0), // $1
                price_tick: Decimal::new(1, 1), // 0.1
                quantity_precision: 4,
                price_precision: 1,
            },
        );

        Self { rules }
    }

    pub fn get_rules(&self, exchange: &str, symbol: &str) -> Option<&ExchangeRules> {
        self.rules.get(&(exchange.to_string(), symbol.to_string()))
    }

    pub fn adjust_quantity(&self, exchange: &str, symbol: &str, quantity: Decimal) -> Decimal {
        if let Some(rules) = self.get_rules(exchange, symbol) {
            rules.adjust_to_lot_size(quantity)
        } else {
            quantity
        }
    }

    pub fn validate_order(
        &self,
        exchange: &str,
        symbol: &str,
        price: Decimal,
        quantity: Decimal,
    ) -> Result<(Decimal, Decimal), PrecisionError> {
        let rules = self
            .get_rules(exchange, symbol)
            .ok_or_else(|| PrecisionError::InvalidLotSize("Unknown symbol".to_string()))?;

        let adjusted_price = rules.round_price(price);
        let adjusted_quantity = rules.adjust_to_lot_size(quantity);

        rules.validate_order(adjusted_price, adjusted_quantity)?;

        Ok((adjusted_price, adjusted_quantity))
    }
}

impl Default for ExchangeRulesManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_lot_size_adjustment() {
        let rules = ExchangeRules {
            min_quantity: dec!(0.001),
            max_quantity: dec!(1000),
            quantity_step: dec!(0.001),
            min_notional: dec!(5),
            price_tick: dec!(0.1),
            quantity_precision: 3,
            price_precision: 1,
        };

        // 0.0015 should round down to 0.001
        assert_eq!(rules.adjust_to_lot_size(dec!(0.0015)), dec!(0.001));

        // 0.0025 should round down to 0.002
        assert_eq!(rules.adjust_to_lot_size(dec!(0.0025)), dec!(0.002));
    }

    #[test]
    fn test_order_validation() {
        let manager = ExchangeRulesManager::new();

        // Valid order
        let result = manager.validate_order("Binance", "BTC/USDT", dec!(50000), dec!(0.01));
        assert!(result.is_ok());

        // Too small quantity
        let result = manager.validate_order("Binance", "BTC/USDT", dec!(50000), dec!(0.0001));
        assert!(result.is_err());
    }
}
