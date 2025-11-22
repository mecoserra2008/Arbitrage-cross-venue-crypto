use arbitrage_core::{
    Exchange, LiquidityValidator, MetricsCollector, OpportunityDeduplicator,
    OpportunityFingerprint, OrderBookManager, OrderBookValidator, Symbol, Timestamp,
};
use arbitrage_exchanges::FeeManager;
use arbitrage_risk::InventoryManager;
use crate::opportunity::{ArbitrageConfig, ArbitrageOpportunity};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Enhanced arbitrage detector with comprehensive validation
pub struct EnhancedArbitrageDetector {
    config: Arc<RwLock<ArbitrageConfig>>,
    orderbook_manager: Arc<OrderBookManager>,
    orderbook_validator: OrderBookValidator,
    liquidity_validator: LiquidityValidator,
    deduplicator: OpportunityDeduplicator,
    inventory_manager: Option<Arc<InventoryManager>>,
}

impl EnhancedArbitrageDetector {
    pub fn new(
        config: ArbitrageConfig,
        orderbook_manager: Arc<OrderBookManager>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            orderbook_manager,
            orderbook_validator: OrderBookValidator::institutional(),
            liquidity_validator: LiquidityValidator::institutional(),
            deduplicator: OpportunityDeduplicator::institutional(),
            inventory_manager: None,
        }
    }

    pub fn with_inventory_manager(mut self, inventory_manager: Arc<InventoryManager>) -> Self {
        self.inventory_manager = Some(inventory_manager);
        self
    }

    /// Scan for arbitrage opportunities with comprehensive validation
    pub fn scan_opportunities(&self, symbol: &Symbol) -> Vec<ArbitrageOpportunity> {
        let config = self.config.read();
        let mut opportunities = Vec::new();
        let now_ns = Timestamp::now().as_nanos();

        // Compare all exchange pairs
        for (i, &buy_exchange) in config.enabled_exchanges.iter().enumerate() {
            for &sell_exchange in config.enabled_exchanges.iter().skip(i + 1) {
                // Check both directions
                if let Some(opp) = self.check_pair_with_validation(
                    buy_exchange,
                    sell_exchange,
                    symbol,
                    &config,
                    now_ns,
                ) {
                    opportunities.push(opp);
                }

                if let Some(opp) = self.check_pair_with_validation(
                    sell_exchange,
                    buy_exchange,
                    symbol,
                    &config,
                    now_ns,
                ) {
                    opportunities.push(opp);
                }
            }
        }

        // Sort by profitability
        opportunities.sort_by(|a, b| b.profit_bps.cmp(&a.profit_bps));

        opportunities
    }

    fn check_pair_with_validation(
        &self,
        buy_exchange: Exchange,
        sell_exchange: Exchange,
        symbol: &Symbol,
        config: &ArbitrageConfig,
        now_ns: u64,
    ) -> Option<ArbitrageOpportunity> {
        // Get order books
        let buy_snapshot = self.orderbook_manager.get_snapshot(buy_exchange, symbol)?;
        let sell_snapshot = self.orderbook_manager.get_snapshot(sell_exchange, symbol)?;

        // VALIDATION 1: Check orderbook freshness
        if !self.orderbook_validator.is_fresh(buy_snapshot.timestamp_ns, now_ns) {
            debug!(
                "Stale orderbook from {} for {}",
                buy_exchange, symbol
            );
            MetricsCollector::record_error("detector", "stale_orderbook");
            return None;
        }

        if !self.orderbook_validator.is_fresh(sell_snapshot.timestamp_ns, now_ns) {
            debug!(
                "Stale orderbook from {} for {}",
                sell_exchange, symbol
            );
            MetricsCollector::record_error("detector", "stale_orderbook");
            return None;
        }

        // VALIDATION 2: Check orderbook depth
        if !self.orderbook_validator.has_sufficient_depth(
            buy_snapshot.bids.len(),
            buy_snapshot.asks.len(),
        ) {
            debug!(
                "Insufficient depth on {} for {}",
                buy_exchange, symbol
            );
            return None;
        }

        if !self.orderbook_validator.has_sufficient_depth(
            sell_snapshot.bids.len(),
            sell_snapshot.asks.len(),
        ) {
            debug!(
                "Insufficient depth on {} for {}",
                sell_exchange, symbol
            );
            return None;
        }

        // Get best prices
        let best_ask = buy_snapshot.best_ask()?;
        let best_bid = sell_snapshot.best_bid()?;

        // VALIDATION 3: Check if there's a price discrepancy
        if best_bid.price <= best_ask.price {
            return None;
        }

        // VALIDATION 4: Check spread sanity
        if let Some(buy_spread) = buy_snapshot.spread() {
            if let Some(buy_mid) = buy_snapshot.mid_price() {
                if !self.orderbook_validator.is_spread_acceptable(buy_spread, buy_mid) {
                    debug!(
                        "Abnormal spread on {} for {}: {} bps",
                        buy_exchange,
                        symbol,
                        (buy_spread / buy_mid) * Decimal::from(10000)
                    );
                    return None;
                }
            }
        }

        // Calculate maximum tradeable quantity
        let max_quantity = best_ask.quantity.min(best_bid.quantity);
        let quantity = max_quantity.min(config.max_position_size / best_ask.price);

        if quantity <= Decimal::ZERO {
            return None;
        }

        // VALIDATION 5: Validate liquidity depth
        if let Err(e) = self.liquidity_validator.validate_depth(&buy_snapshot.asks, quantity) {
            debug!(
                "Insufficient liquidity on {} for {}: {}",
                buy_exchange, symbol, e
            );
            return None;
        }

        if let Err(e) = self.liquidity_validator.validate_depth(&sell_snapshot.bids, quantity) {
            debug!(
                "Insufficient liquidity on {} for {}: {}",
                sell_exchange, symbol, e
            );
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
        let buy_slippage = estimate_slippage(&buy_snapshot, quantity);
        let sell_slippage = estimate_slippage(&sell_snapshot, quantity);

        // VALIDATION 6: Check if slippage is acceptable
        let max_slippage = config.max_slippage_bps / Decimal::from(10000);
        if buy_slippage > max_slippage || sell_slippage > max_slippage {
            debug!(
                "Slippage too high: buy={}, sell={}, max={}",
                buy_slippage, sell_slippage, max_slippage
            );
            return None;
        }

        // Create opportunity
        let mut opportunity = ArbitrageOpportunity::new(
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

        // Calculate latency
        let detection_latency = now_ns.saturating_sub(
            buy_snapshot.timestamp_ns.max(sell_snapshot.timestamp_ns)
        );
        opportunity = opportunity.with_latency(detection_latency);

        // VALIDATION 7: Check if profitable after all costs
        if !opportunity.is_profitable(config.min_profit_bps) {
            return None;
        }

        // VALIDATION 8: Check deduplication
        let fingerprint = OpportunityFingerprint::new(
            &buy_exchange.to_string(),
            &sell_exchange.to_string(),
            &symbol.to_string(),
            opportunity.buy_price,
            opportunity.sell_price,
        );

        if !self.deduplicator.check_and_mark(fingerprint) {
            debug!(
                "Duplicate opportunity filtered: {} {} -> {}",
                symbol, buy_exchange, sell_exchange
            );
            return None;
        }

        // VALIDATION 9: Check inventory constraints
        if let Some(inventory_mgr) = &self.inventory_manager {
            if let Err(e) = inventory_mgr.can_trade(
                buy_exchange,
                sell_exchange,
                symbol,
                quantity,
            ) {
                warn!(
                    "Opportunity rejected due to inventory constraints: {}",
                    e
                );
                return None;
            }
        }

        info!(
            "Valid opportunity: {} {} -> {} @ {} bps profit (latency: {}μs)",
            symbol,
            buy_exchange,
            sell_exchange,
            opportunity.profit_bps,
            detection_latency / 1000
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

    pub fn get_deduplicator_stats(&self) -> usize {
        self.deduplicator.size()
    }

    pub fn clear_deduplicator(&self) {
        self.deduplicator.clear();
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
    fn test_enhanced_detection_with_validation() {
        let orderbook_mgr = Arc::new(OrderBookManager::new());
        let config = ArbitrageConfig::default();
        let detector = EnhancedArbitrageDetector::new(config, orderbook_mgr.clone());

        let symbol = Symbol::perpetual("BTC", "USDT");
        let now = Timestamp::now().as_nanos();

        // Create valid orderbook snapshots
        let binance = OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: symbol.clone(),
            bids: vec![
                PriceLevel::new(dec!(50000), dec!(1.0)),
                PriceLevel::new(dec!(49999), dec!(2.0)),
                PriceLevel::new(dec!(49998), dec!(3.0)),
                PriceLevel::new(dec!(49997), dec!(4.0)),
                PriceLevel::new(dec!(49996), dec!(5.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50001), dec!(1.0)),
                PriceLevel::new(dec!(50002), dec!(2.0)),
                PriceLevel::new(dec!(50003), dec!(3.0)),
                PriceLevel::new(dec!(50004), dec!(4.0)),
                PriceLevel::new(dec!(50005), dec!(5.0)),
            ],
            timestamp_ns: now,
            sequence: 1,
        };

        let bybit = OrderBookSnapshot {
            exchange: Exchange::Bybit,
            symbol: symbol.clone(),
            bids: vec![
                PriceLevel::new(dec!(50010), dec!(1.0)), // Higher bid - opportunity!
                PriceLevel::new(dec!(50009), dec!(2.0)),
                PriceLevel::new(dec!(50008), dec!(3.0)),
                PriceLevel::new(dec!(50007), dec!(4.0)),
                PriceLevel::new(dec!(50006), dec!(5.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50011), dec!(1.0)),
                PriceLevel::new(dec!(50012), dec!(2.0)),
                PriceLevel::new(dec!(50013), dec!(3.0)),
                PriceLevel::new(dec!(50014), dec!(4.0)),
                PriceLevel::new(dec!(50015), dec!(5.0)),
            ],
            timestamp_ns: now,
            sequence: 1,
        };

        orderbook_mgr.update(binance);
        orderbook_mgr.update(bybit);

        // Scan for opportunities
        let opportunities = detector.scan_opportunities(&symbol);

        // Should find the opportunity (assuming it passes all validations)
        assert!(!opportunities.is_empty());
    }
}
