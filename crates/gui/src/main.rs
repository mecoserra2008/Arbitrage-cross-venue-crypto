#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use arbitrage_core::{Exchange, OrderBookManager, OrderBookSnapshot, Symbol, Timestamp};
use arbitrage_engine::{ArbitrageConfig, ArbitrageDetector, ArbitrageOpportunity, EnhancedArbitrageDetector};
use arbitrage_exchanges::{WebSocketHandler, BinanceAPI};
use arbitrage_risk::{RiskLimits, RiskManager, PnLStats, InventoryManager};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, Level};
use tracing_subscriber;

/// Global application state
struct AppState {
    orderbook_manager: Arc<OrderBookManager>,
    arbitrage_detector: Arc<EnhancedArbitrageDetector>,
    risk_manager: Arc<RiskManager>,
    inventory_manager: Arc<InventoryManager>,
    opportunities: Arc<RwLock<Vec<ArbitrageOpportunity>>>,
    trading_enabled: Arc<RwLock<bool>>,
    price_history: Arc<RwLock<PriceHistory>>,
    spread_history: Arc<RwLock<Vec<SpreadPoint>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PriceHistory {
    timestamps: Vec<String>,
    exchanges: HashMap<String, Vec<f64>>,
}

impl PriceHistory {
    fn new() -> Self {
        Self {
            timestamps: Vec::new(),
            exchanges: HashMap::new(),
        }
    }

    fn add_price(&mut self, exchange: &str, price: f64, timestamp: String) {
        if self.timestamps.last() != Some(&timestamp) {
            self.timestamps.push(timestamp.clone());

            // Keep only last 60 points
            if self.timestamps.len() > 60 {
                self.timestamps.remove(0);
                for prices in self.exchanges.values_mut() {
                    if !prices.is_empty() {
                        prices.remove(0);
                    }
                }
            }
        }

        self.exchanges
            .entry(exchange.to_string())
            .or_insert_with(Vec::new)
            .push(price);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpreadPoint {
    timestamp: String,
    spread_bps: f64,
}

/// WebSocket handler implementation
struct AppWebSocketHandler {
    orderbook_manager: Arc<OrderBookManager>,
}

impl WebSocketHandler for AppWebSocketHandler {
    fn handle_orderbook_update(&self, snapshot: OrderBookSnapshot) {
        self.orderbook_manager.update(snapshot);
    }

    fn handle_trade(&self, _execution: arbitrage_core::TradeExecution) {
        // Handle trade execution updates
    }

    fn handle_error(&self, error: String) {
        tracing::error!("WebSocket error: {}", error);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DashboardData {
    opportunities: Vec<ArbitrageOpportunity>,
    pnl_stats: PnLStats,
    risk_limits: RiskLimits,
    trading_enabled: bool,
    orderbooks: Vec<OrderBookInfo>,
    price_history: PriceHistory,
    spread_history: Vec<SpreadPoint>,
    venue_data: Vec<VenueData>,
    liquidity_metrics: LiquidityMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
struct OrderBookInfo {
    exchange: String,
    symbol: String,
    best_bid: Option<String>,
    best_ask: Option<String>,
    spread: Option<String>,
    mid_price: Option<String>,
    bid_depth: Vec<PriceLevel>,
    ask_depth: Vec<PriceLevel>,
    timestamp_ns: u64,
    latency_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PriceLevel {
    price: String,
    quantity: String,
    total: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VenueData {
    name: String,
    bid: f64,
    ask: f64,
    spread_bps: f64,
    liquidity: f64,
    is_best_bid: bool,
    is_best_ask: bool,
    latency_us: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct LiquidityMetrics {
    min_required: String,
    avg_depth_5: String,
    max_slippage_bps: String,
    est_slippage_bps: String,
    total_bid_liquidity: String,
    total_ask_liquidity: String,
}

#[tauri::command]
async fn get_dashboard_data(state: State<'_, AppState>) -> Result<DashboardData, String> {
    let opportunities = state.opportunities.read().await.clone();
    let pnl_stats = state.risk_manager.get_pnl_stats();
    let risk_limits = state.risk_manager.get_limits();
    let trading_enabled = *state.trading_enabled.read().await;
    let price_history = state.price_history.read().await.clone();
    let spread_history = state.spread_history.read().await.clone();

    // Get orderbook data for all exchanges and symbols
    let mut orderbooks = Vec::new();
    let mut venue_data = Vec::new();

    let symbols = vec![
        Symbol::perpetual("BTC", "USDT"),
        Symbol::perpetual("ETH", "USDT"),
        Symbol::perpetual("SOL", "USDT"),
    ];
    let exchanges = vec![Exchange::Binance, Exchange::Bybit, Exchange::OKX];

    let mut best_bid = 0.0f64;
    let mut best_ask = f64::MAX;
    let mut venue_prices = Vec::new();

    for symbol in &symbols {
        for &exchange in &exchanges {
            if let Some(snapshot) = state.orderbook_manager.get_snapshot(exchange, symbol) {
                let now = Timestamp::now().as_nanos();
                let latency_us = (now - snapshot.timestamp_ns) / 1000;

                // Calculate bid/ask depths
                let bid_depth: Vec<PriceLevel> = snapshot
                    .bids
                    .iter()
                    .take(10)
                    .map(|level| {
                        let total = level.price * level.quantity;
                        PriceLevel {
                            price: level.price.to_string(),
                            quantity: level.quantity.to_string(),
                            total: total.to_string(),
                        }
                    })
                    .collect();

                let ask_depth: Vec<PriceLevel> = snapshot
                    .asks
                    .iter()
                    .take(10)
                    .map(|level| {
                        let total = level.price * level.quantity;
                        PriceLevel {
                            price: level.price.to_string(),
                            quantity: level.quantity.to_string(),
                            total: total.to_string(),
                        }
                    })
                    .collect();

                orderbooks.push(OrderBookInfo {
                    exchange: exchange.to_string(),
                    symbol: symbol.to_string(),
                    best_bid: snapshot.best_bid().map(|b| b.price.to_string()),
                    best_ask: snapshot.best_ask().map(|a| a.price.to_string()),
                    spread: snapshot.spread().map(|s| s.to_string()),
                    mid_price: snapshot.mid_price().map(|m| m.to_string()),
                    bid_depth,
                    ask_depth,
                    timestamp_ns: snapshot.timestamp_ns,
                    latency_us,
                });

                // Collect venue data
                if let (Some(bid), Some(ask)) = (snapshot.best_bid(), snapshot.best_ask()) {
                    let bid_f64 = bid.price.to_string().parse::<f64>().unwrap_or(0.0);
                    let ask_f64 = ask.price.to_string().parse::<f64>().unwrap_or(0.0);

                    if bid_f64 > best_bid {
                        best_bid = bid_f64;
                    }
                    if ask_f64 < best_ask {
                        best_ask = ask_f64;
                    }

                    venue_prices.push((exchange.to_string(), bid_f64, ask_f64, latency_us));

                    // Calculate total liquidity (top 5 levels)
                    let bid_liquidity: Decimal = snapshot
                        .bids
                        .iter()
                        .take(5)
                        .map(|l| l.price * l.quantity)
                        .sum();

                    let ask_liquidity: Decimal = snapshot
                        .asks
                        .iter()
                        .take(5)
                        .map(|l| l.price * l.quantity)
                        .sum();

                    let total_liquidity = (bid_liquidity + ask_liquidity).to_string().parse::<f64>().unwrap_or(0.0);

                    let spread = ask.price - bid.price;
                    let spread_bps = (spread / bid.price) * Decimal::from(10000);
                    let spread_bps_f64 = spread_bps.to_string().parse::<f64>().unwrap_or(0.0);

                    venue_data.push(VenueData {
                        name: exchange.to_string(),
                        bid: bid_f64,
                        ask: ask_f64,
                        spread_bps: spread_bps_f64,
                        liquidity: total_liquidity,
                        is_best_bid: false, // Will update below
                        is_best_ask: false, // Will update below
                        latency_us,
                    });
                }
            }
        }
    }

    // Mark best bid/ask
    for venue in &mut venue_data {
        venue.is_best_bid = (venue.bid - best_bid).abs() < 0.01;
        venue.is_best_ask = (venue.ask - best_ask).abs() < 0.01;
    }

    // Liquidity metrics
    let liquidity_metrics = LiquidityMetrics {
        min_required: "100000".to_string(),
        avg_depth_5: "250000".to_string(), // Calculate from actual data
        max_slippage_bps: "20".to_string(),
        est_slippage_bps: "5".to_string(), // Calculate from actual slippage
        total_bid_liquidity: "500000".to_string(),
        total_ask_liquidity: "480000".to_string(),
    };

    Ok(DashboardData {
        opportunities,
        pnl_stats,
        risk_limits,
        trading_enabled,
        orderbooks,
        price_history,
        spread_history,
        venue_data,
        liquidity_metrics,
    })
}

#[tauri::command]
async fn get_price_history(state: State<'_, AppState>) -> Result<PriceHistory, String> {
    Ok(state.price_history.read().await.clone())
}

#[tauri::command]
async fn get_spread_history(state: State<'_, AppState>) -> Result<Vec<SpreadPoint>, String> {
    Ok(state.spread_history.read().await.clone())
}

#[tauri::command]
async fn toggle_trading(state: State<'_, AppState>) -> Result<bool, String> {
    let mut enabled = state.trading_enabled.write().await;
    *enabled = !*enabled;
    info!("Trading {}", if *enabled { "enabled" } else { "disabled" });
    Ok(*enabled)
}

#[tauri::command]
async fn update_risk_limits(
    state: State<'_, AppState>,
    limits: RiskLimits,
) -> Result<(), String> {
    state.risk_manager.update_limits(limits);
    info!("Risk limits updated");
    Ok(())
}

#[tauri::command]
async fn reset_circuit_breaker(state: State<'_, AppState>) -> Result<(), String> {
    state.risk_manager.reset_circuit_breaker();
    info!("Circuit breaker reset");
    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting Arbitrage Bot GUI");

    // Initialize components
    let orderbook_manager = Arc::new(OrderBookManager::new());
    let arbitrage_config = ArbitrageConfig::default();
    let inventory_manager = Arc::new(InventoryManager::institutional());

    let arbitrage_detector = Arc::new(
        EnhancedArbitrageDetector::new(arbitrage_config, orderbook_manager.clone())
            .with_inventory_manager(inventory_manager.clone()),
    );

    let risk_limits = RiskLimits::default();
    let risk_manager = Arc::new(RiskManager::new(risk_limits, dec!(100000)));

    let app_state = AppState {
        orderbook_manager: orderbook_manager.clone(),
        arbitrage_detector: arbitrage_detector.clone(),
        risk_manager: risk_manager.clone(),
        inventory_manager: inventory_manager.clone(),
        opportunities: Arc::new(RwLock::new(Vec::new())),
        trading_enabled: Arc::new(RwLock::new(false)),
        price_history: Arc::new(RwLock::new(PriceHistory::new())),
        spread_history: Arc::new(RwLock::new(Vec::new())),
    };

    // Start background arbitrage scanning and data collection
    let detector = arbitrage_detector.clone();
    let orderbook_mgr = orderbook_manager.clone();
    let opportunities = app_state.opportunities.clone();
    let price_history = app_state.price_history.clone();
    let spread_history = app_state.spread_history.clone();

    tokio::spawn(async move {
        let symbols = vec![
            Symbol::perpetual("BTC", "USDT"),
            Symbol::perpetual("ETH", "USDT"),
            Symbol::perpetual("SOL", "USDT"),
        ];

        let mut update_interval = interval(Duration::from_millis(100));

        loop {
            update_interval.tick().await;

            for symbol in &symbols {
                // Scan for opportunities
                let opps = detector.scan_opportunities(symbol);
                if !opps.is_empty() {
                    info!("Found {} opportunities for {}", opps.len(), symbol);
                    *opportunities.write().await = opps;
                }

                // Collect price history
                let exchanges = vec![Exchange::Binance, Exchange::Bybit, Exchange::OKX];
                let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

                let mut prices = Vec::new();
                for exchange in &exchanges {
                    if let Some(mid) = orderbook_mgr.get_mid_price(*exchange, symbol) {
                        let price = mid.to_string().parse::<f64>().unwrap_or(0.0);
                        prices.push(price);

                        price_history.write().await.add_price(
                            &exchange.to_string(),
                            price,
                            timestamp.clone(),
                        );
                    }
                }

                // Calculate spread
                if prices.len() >= 2 {
                    let max_price = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let min_price = prices.iter().cloned().fold(f64::INFINITY, f64::min);
                    let spread_bps = ((max_price - min_price) / min_price) * 10000.0;

                    let mut spread_hist = spread_history.write().await;
                    spread_hist.push(SpreadPoint {
                        timestamp: timestamp.clone(),
                        spread_bps,
                    });

                    // Keep only last 60 points
                    if spread_hist.len() > 60 {
                        spread_hist.remove(0);
                    }
                }
            }
        }
    });

    // Build and run Tauri app
    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_dashboard_data,
            get_price_history,
            get_spread_history,
            toggle_trading,
            update_risk_limits,
            reset_circuit_breaker,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
