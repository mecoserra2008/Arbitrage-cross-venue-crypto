use arbitrage_core::{Exchange, MetricsCollector, OrderBookManager, Symbol};
use arbitrage_exchanges::FeeManager;
use crate::opportunity::{ArbitrageConfig, ArbitrageOpportunity};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{debug, info};

/// Arbitrage opportunity detector
pub struct ArbitrageDetector {
    config: Arc<RwLock<ArbitrageConfig>>,
    orderbook_manager: Arc<OrderBookManager>,
}

impl ArbitrageDetector {
    pub fn new(config: ArbitrageConfig, orderbook_manager: Arc<OrderBookManager>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            orderbook_manager,
        }
    }

    /// Scan for arbitrage opportunities across all exchange pairs
    pub fn scan_opportunities(&self, symbol: &Symbol) -> Vec<ArbitrageOpportunity> {
        let config = self.config.read();
        let mut opportunities = Vec::new();

        // Compare all exchange pairs
        for (i, &buy_exchange) in config.enabled_exchanges.iter().enumerate() {
            for &sell_exchange in config.enabled_exchanges.iter().skip(i + 1) {
                // Check both directions
                if let Some(opp) = self.check_pair(buy_exchange, sell_exchange, symbol, &config) {
                    opportunities.push(opp);
                }
                if let Some(opp) = self.check_pair(sell_exchange, buy_exchange, symbol, &config) {
                    opportunities.push(opp);
                }
            }
        }

        opportunities
    }

    /// Check arbitrage opportunity between two exchanges
    fn check_pair(
        &self,
        buy_exchange: Exchange,
        sell_exchange: Exchange,
        symbol: &Symbol,
        config: &ArbitrageConfig,
    ) -> Option<ArbitrageOpportunity> {
        // Get order books
        let buy_snapshot = self.orderbook_manager.get_snapshot(buy_exchange, symbol)?;
        let sell_snapshot = self.orderbook_manager.get_snapshot(sell_exchange, symbol)?;

        // Get best prices
        let best_ask = buy_snapshot.best_ask()?;
        let best_bid = sell_snapshot.best_bid()?;

        // Check if there's a price discrepancy
        if best_bid.price <= best_ask.price {
            return None;
        }

        // Calculate maximum tradeable quantity
        let max_quantity = best_ask.quantity.min(best_bid.quantity);
        let quantity = max_quantity.min(config.max_position_size / best_ask.price);

        if quantity <= Decimal::ZERO {
            return None;
        }

        // Calculate fees
        let buy_fee = FeeManager::calculate_fee(
            buy_exchange,
            false, // taker
            quantity,
            best_ask.price,
        );

        let sell_fee = FeeManager::calculate_fee(
            sell_exchange,
            false, // taker
            quantity,
            best_bid.price,
        );

        // Estimate slippage
        let buy_orderbook = self.orderbook_manager
            .get_snapshot(buy_exchange, symbol)?;
        let sell_orderbook = self.orderbook_manager
            .get_snapshot(sell_exchange, symbol)?;

        let buy_slippage = estimate_slippage(&buy_orderbook, quantity);
        let sell_slippage = estimate_slippage(&sell_orderbook, quantity);

        // Check if slippage is acceptable
        let max_slippage = config.max_slippage_bps / Decimal::from(10000);
        if buy_slippage > max_slippage || sell_slippage > max_slippage {
            debug!(
                "Slippage too high: buy={}, sell={}, max={}",
                buy_slippage, sell_slippage, max_slippage
            );
            return None;
        }

        // Create opportunity
        let opportunity = ArbitrageOpportunity::new(
            symbol.clone(),
            buy_exchange,
            sell_exchange,
            best_ask.price,
            best_bid.price,
            quantity,
            buy_fee,
            sell_fee,
            buy_slippage,
            sell_slippage,
        );

        // Check if profitable
        if !opportunity.is_profitable(config.min_profit_bps) {
            return None;
        }

        info!(
            "Found opportunity: {} {} -> {} @ {} bps profit",
            symbol,
            buy_exchange,
            sell_exchange,
            opportunity.profit_bps
        );

        // Record metrics
        MetricsCollector::record_arbitrage_opportunity(
            &buy_exchange.to_string(),
            &sell_exchange.to_string(),
            opportunity.profit_bps.to_string().parse().unwrap_or(0.0),
        );

        Some(opportunity)
    }

    pub fn update_config(&self, config: ArbitrageConfig) {
        *self.config.write() = config;
    }

    pub fn get_config(&self) -> ArbitrageConfig {
        self.config.read().clone()
    }
}

/// Estimate slippage based on order book depth
fn estimate_slippage(snapshot: &arbitrage_core::OrderBookSnapshot, quantity: Decimal) -> Decimal {
    let best_price = match snapshot.best_ask() {
        Some(ask) => ask.price,
        None => return Decimal::ZERO,
    };

    let vwap = match calculate_vwap(&snapshot.asks, quantity) {
        Some(vwap) => vwap,
        None => return Decimal::ZERO,
    };

    (vwap - best_price).abs() / best_price
}

/// Calculate volume-weighted average price
fn calculate_vwap(levels: &[arbitrage_core::PriceLevel], quantity: Decimal) -> Option<Decimal> {
    let mut remaining = quantity;
    let mut total_cost = Decimal::ZERO;
    let mut total_qty = Decimal::ZERO;

    for level in levels {
        if remaining <= Decimal::ZERO {
            break;
        }

        let qty = remaining.min(level.quantity);
        total_cost += qty * level.price;
        total_qty += qty;
        remaining -= qty;
    }

    if total_qty > Decimal::ZERO {
        Some(total_cost / total_qty)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrage_core::{OrderBookSnapshot, PriceLevel};
    use rust_decimal_macros::dec;

    #[test]
    fn test_vwap_calculation() {
        let levels = vec![
            PriceLevel::new(dec!(50000), dec!(1.0)),
            PriceLevel::new(dec!(50001), dec!(2.0)),
            PriceLevel::new(dec!(50002), dec!(3.0)),
        ];

        let vwap = calculate_vwap(&levels, dec!(2.0)).unwrap();
        // (50000 * 1.0 + 50001 * 1.0) / 2.0 = 50000.5
        assert_eq!(vwap, dec!(50000.5));
    }
}
