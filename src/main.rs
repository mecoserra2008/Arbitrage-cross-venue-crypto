use arbitrage_core::{Exchange, OrderBookManager, OrderBookSnapshot, PriceLevel, Symbol, Timestamp};
use arbitrage_engine::{ArbitrageConfig, ArbitrageDetector};
use arbitrage_risk::{RiskLimits, RiskManager};
use rust_decimal_macros::dec;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Arbitrage Bot - Multi-Venue Trading System");
    info!("Initializing components...");

    // Initialize orderbook manager
    let orderbook_manager = Arc::new(OrderBookManager::new());

    // Initialize arbitrage detector
    let config = ArbitrageConfig::default();
    let detector = ArbitrageDetector::new(config, orderbook_manager.clone());

    // Initialize risk manager
    let risk_limits = RiskLimits::default();
    let risk_manager = RiskManager::new(risk_limits, dec!(100000));

    info!("System initialized successfully");
    info!("Risk Limits:");
    info!("  Max Position Size: ${}", risk_manager.get_limits().max_position_size);
    info!("  Max Total Exposure: ${}", risk_manager.get_limits().max_total_exposure);
    info!("  Min Profit (bps): {}", risk_manager.get_limits().min_profit_bps);

    // Demo: Simulate some orderbook data
    let symbol = Symbol::perpetual("BTC", "USDT");

    let binance_snapshot = OrderBookSnapshot {
        exchange: Exchange::Binance,
        symbol: symbol.clone(),
        bids: vec![
            PriceLevel::new(dec!(50000), dec!(1.5)),
            PriceLevel::new(dec!(49999), dec!(2.0)),
        ],
        asks: vec![
            PriceLevel::new(dec!(50001), dec!(1.2)),
            PriceLevel::new(dec!(50002), dec!(2.5)),
        ],
        timestamp_ns: Timestamp::now().as_nanos(),
        sequence: 1,
    };

    let bybit_snapshot = OrderBookSnapshot {
        exchange: Exchange::Bybit,
        symbol: symbol.clone(),
        bids: vec![
            PriceLevel::new(dec!(50010), dec!(1.8)), // Higher bid - arbitrage opportunity!
            PriceLevel::new(dec!(50009), dec!(2.2)),
        ],
        asks: vec![
            PriceLevel::new(dec!(50011), dec!(1.5)),
            PriceLevel::new(dec!(50012), dec!(2.0)),
        ],
        timestamp_ns: Timestamp::now().as_nanos(),
        sequence: 1,
    };

    orderbook_manager.update(binance_snapshot);
    orderbook_manager.update(bybit_snapshot);

    info!("Updated orderbooks for {}", symbol);

    // Scan for opportunities
    let opportunities = detector.scan_opportunities(&symbol);

    if opportunities.is_empty() {
        info!("No arbitrage opportunities found");
    } else {
        info!("Found {} arbitrage opportunities:", opportunities.len());
        for opp in &opportunities {
            info!(
                "  {} -> {}: Buy @ {} on {}, Sell @ {} on {} | Profit: {} ({} bps)",
                symbol,
                symbol,
                opp.buy_price,
                opp.buy_exchange,
                opp.sell_price,
                opp.sell_exchange,
                opp.net_profit,
                opp.profit_bps
            );
        }
    }

    info!("Demo completed. Use the GUI for full functionality.");
}
