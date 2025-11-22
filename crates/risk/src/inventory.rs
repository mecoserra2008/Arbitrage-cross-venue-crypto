use arbitrage_core::{Exchange, Position, Side, Symbol};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn};

/// Manages inventory across exchanges to prevent accumulation risk
pub struct InventoryManager {
    positions: Arc<DashMap<(Exchange, Symbol), Decimal>>,
    max_inventory: Decimal,
    target_inventory: Decimal,
    auto_hedge_enabled: bool,
}

impl InventoryManager {
    pub fn new(max_inventory: Decimal, auto_hedge_enabled: bool) -> Self {
        Self {
            positions: Arc::new(DashMap::new()),
            max_inventory,
            target_inventory: Decimal::ZERO,
            auto_hedge_enabled,
        }
    }

    pub fn institutional() -> Self {
        Self::new(
            Decimal::from(10), // Max 10 BTC equivalent
            true,              // Auto-hedge enabled
        )
    }

    /// Update position inventory
    pub fn update_position(&self, exchange: Exchange, symbol: Symbol, quantity: Decimal) {
        self.positions
            .insert((exchange, symbol), quantity);
    }

    /// Get net inventory across all exchanges for a symbol
    pub fn get_net_inventory(&self, symbol: &Symbol) -> Decimal {
        self.positions
            .iter()
            .filter(|entry| &entry.value().1 == symbol)
            .map(|entry| *entry.value())
            .sum()
    }

    /// Get inventory for specific exchange and symbol
    pub fn get_inventory(&self, exchange: Exchange, symbol: &Symbol) -> Decimal {
        self.positions
            .get(&(exchange, symbol.clone()))
            .map(|v| *v)
            .unwrap_or(Decimal::ZERO)
    }

    /// Check if trade would violate inventory limits
    pub fn can_trade(
        &self,
        buy_exchange: Exchange,
        sell_exchange: Exchange,
        symbol: &Symbol,
        quantity: Decimal,
    ) -> Result<(), InventoryError> {
        let buy_inventory = self.get_inventory(buy_exchange, symbol);
        let sell_inventory = self.get_inventory(sell_exchange, symbol);

        // After trade: buy side increases, sell side decreases
        let new_buy_inventory = buy_inventory + quantity;
        let new_sell_inventory = sell_inventory - quantity;

        // Check individual exchange limits
        if new_buy_inventory.abs() > self.max_inventory {
            return Err(InventoryError::ExceedsLimit {
                exchange: buy_exchange,
                current: buy_inventory,
                after_trade: new_buy_inventory,
                limit: self.max_inventory,
            });
        }

        if new_sell_inventory.abs() > self.max_inventory {
            return Err(InventoryError::ExceedsLimit {
                exchange: sell_exchange,
                current: sell_inventory,
                after_trade: new_sell_inventory,
                limit: self.max_inventory,
            });
        }

        // Check net inventory
        let current_net = self.get_net_inventory(symbol);
        let net_change = Decimal::ZERO; // Arbitrage is net-neutral
        let new_net = current_net + net_change;

        if new_net.abs() > self.max_inventory {
            return Err(InventoryError::NetInventoryExceeded {
                current: current_net,
                limit: self.max_inventory,
            });
        }

        Ok(())
    }

    /// Calculate required hedge to bring inventory to target
    pub fn calculate_hedge(&self, symbol: &Symbol) -> Option<HedgeRecommendation> {
        if !self.auto_hedge_enabled {
            return None;
        }

        let net_inventory = self.get_net_inventory(symbol);
        let deviation = net_inventory - self.target_inventory;

        // Only hedge if deviation is significant
        if deviation.abs() < Decimal::new(1, 2) {
            // Less than 0.01
            return None;
        }

        // Find exchange with most inventory to hedge from
        let mut max_inventory = Decimal::ZERO;
        let mut hedge_exchange = None;

        for entry in self.positions.iter() {
            let (exchange, sym) = entry.key();
            if sym == symbol {
                let qty = *entry.value();
                if qty.abs() > max_inventory.abs() {
                    max_inventory = qty;
                    hedge_exchange = Some(*exchange);
                }
            }
        }

        hedge_exchange.map(|exchange| {
            let side = if deviation > Decimal::ZERO {
                Side::Sell // We're long, need to sell
            } else {
                Side::Buy // We're short, need to buy
            };

            HedgeRecommendation {
                exchange,
                symbol: symbol.clone(),
                side,
                quantity: deviation.abs(),
                reason: format!("Net inventory {} exceeds target {}", net_inventory, self.target_inventory),
            }
        })
    }

    /// Get inventory summary
    pub fn get_summary(&self) -> InventorySummary {
        let mut by_symbol: DashMap<Symbol, Decimal> = DashMap::new();
        let mut by_exchange: DashMap<Exchange, Decimal> = DashMap::new();

        for entry in self.positions.iter() {
            let ((exchange, symbol), quantity) = entry.pair();

            by_symbol
                .entry(symbol.clone())
                .and_modify(|q| *q += *quantity)
                .or_insert(*quantity);

            by_exchange
                .entry(*exchange)
                .and_modify(|q| *q += quantity.abs())
                .or_insert(quantity.abs());
        }

        InventorySummary {
            total_positions: self.positions.len(),
            net_by_symbol: by_symbol.iter().map(|e| (e.key().clone(), *e.value())).collect(),
            total_by_exchange: by_exchange.iter().map(|e| (*e.key(), *e.value())).collect(),
        }
    }

    /// Reset all inventory tracking
    pub fn reset(&self) {
        self.positions.clear();
    }

    /// Set auto-hedge enabled/disabled
    pub fn set_auto_hedge(&mut self, enabled: bool) {
        self.auto_hedge_enabled = enabled;
        info!("Auto-hedge {}", if enabled { "enabled" } else { "disabled" });
    }
}

#[derive(Debug, Clone)]
pub struct HedgeRecommendation {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub side: Side,
    pub quantity: Decimal,
    pub reason: String,
}

#[derive(Debug)]
pub struct InventorySummary {
    pub total_positions: usize,
    pub net_by_symbol: Vec<(Symbol, Decimal)>,
    pub total_by_exchange: Vec<(Exchange, Decimal)>,
}

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Inventory limit exceeded on {exchange}: current={current}, after_trade={after_trade}, limit={limit}")]
    ExceedsLimit {
        exchange: Exchange,
        current: Decimal,
        after_trade: Decimal,
        limit: Decimal,
    },

    #[error("Net inventory would exceed limit: current={current}, limit={limit}")]
    NetInventoryExceeded { current: Decimal, limit: Decimal },
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrage_core::InstrumentType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_inventory_limits() {
        let manager = InventoryManager::new(dec!(5.0), false);
        let symbol = Symbol::perpetual("BTC", "USDT");

        // Set initial inventory
        manager.update_position(Exchange::Binance, symbol.clone(), dec!(3.0));
        manager.update_position(Exchange::Bybit, symbol.clone(), dec!(-2.0));

        // This trade would push Binance to 5.0 - should pass
        assert!(manager
            .can_trade(Exchange::Binance, Exchange::Bybit, &symbol, dec!(2.0))
            .is_ok());

        // This trade would push Binance to 6.0 - should fail
        assert!(manager
            .can_trade(Exchange::Binance, Exchange::Bybit, &symbol, dec!(3.0))
            .is_err());
    }

    #[test]
    fn test_hedge_calculation() {
        let manager = InventoryManager::new(dec!(10.0), true);
        let symbol = Symbol::perpetual("BTC", "USDT");

        manager.update_position(Exchange::Binance, symbol.clone(), dec!(5.0));
        manager.update_position(Exchange::Bybit, symbol.clone(), dec!(3.0));

        let hedge = manager.calculate_hedge(&symbol);
        assert!(hedge.is_some());

        let hedge = hedge.unwrap();
        assert_eq!(hedge.side, Side::Sell);
        assert_eq!(hedge.quantity, dec!(8.0)); // Net is 8.0, target is 0
    }
}
