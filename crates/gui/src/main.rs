#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use arbitrage_core::{Exchange, OrderBookManager, OrderBookSnapshot, Symbol, Timestamp};
use arbitrage_engine::{ArbitrageConfig, ArbitrageDetector, ArbitrageOpportunity};
use arbitrage_exchanges::{WebSocketHandler, BinanceAPI};
use arbitrage_risk::{RiskLimits, RiskManager, PnLStats};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::RwLock;
use tracing::{info, Level};
use tracing_subscriber;

/// Global application state
struct AppState {
    orderbook_manager: Arc<OrderBookManager>,
    arbitrage_detector: Arc<ArbitrageDetector>,
    risk_manager: Arc<RiskManager>,
    opportunities: Arc<RwLock<Vec<ArbitrageOpportunity>>>,
    trading_enabled: Arc<RwLock<bool>>,
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
}

#[derive(Debug, Serialize, Deserialize)]
struct OrderBookInfo {
    exchange: String,
    symbol: String,
    best_bid: Option<String>,
    best_ask: Option<String>,
    spread: Option<String>,
    mid_price: Option<String>,
}

#[tauri::command]
async fn get_dashboard_data(state: State<'_, AppState>) -> Result<DashboardData, String> {
    let opportunities = state.opportunities.read().await.clone();
    let pnl_stats = state.risk_manager.get_pnl_stats();
    let risk_limits = state.risk_manager.get_limits();
    let trading_enabled = *state.trading_enabled.read().await;

    // Get sample orderbook data
    let mut orderbooks = Vec::new();
    let symbols = vec![Symbol::perpetual("BTC", "USDT"), Symbol::perpetual("ETH", "USDT")];
    let exchanges = vec![Exchange::Binance, Exchange::Bybit, Exchange::OKX];

    for symbol in &symbols {
        for &exchange in &exchanges {
            if let Some(snapshot) = state.orderbook_manager.get_snapshot(exchange, symbol) {
                orderbooks.push(OrderBookInfo {
                    exchange: exchange.to_string(),
                    symbol: symbol.to_string(),
                    best_bid: snapshot.best_bid().map(|b| b.price.to_string()),
                    best_ask: snapshot.best_ask().map(|a| a.price.to_string()),
                    spread: snapshot.spread().map(|s| s.to_string()),
                    mid_price: snapshot.mid_price().map(|m| m.to_string()),
                });
            }
        }
    }

    Ok(DashboardData {
        opportunities,
        pnl_stats,
        risk_limits,
        trading_enabled,
        orderbooks,
    })
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
    let arbitrage_detector = Arc::new(ArbitrageDetector::new(
        arbitrage_config,
        orderbook_manager.clone(),
    ));

    let risk_limits = RiskLimits::default();
    let risk_manager = Arc::new(RiskManager::new(risk_limits, dec!(100000)));

    let app_state = AppState {
        orderbook_manager: orderbook_manager.clone(),
        arbitrage_detector: arbitrage_detector.clone(),
        risk_manager: risk_manager.clone(),
        opportunities: Arc::new(RwLock::new(Vec::new())),
        trading_enabled: Arc::new(RwLock::new(false)),
    };

    // Start background arbitrage scanning
    let detector = arbitrage_detector.clone();
    let orderbook_mgr = orderbook_manager.clone();
    let opportunities = app_state.opportunities.clone();

    tokio::spawn(async move {
        let symbols = vec![
            Symbol::perpetual("BTC", "USDT"),
            Symbol::perpetual("ETH", "USDT"),
        ];

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            for symbol in &symbols {
                let opps = detector.scan_opportunities(symbol);
                if !opps.is_empty() {
                    info!("Found {} opportunities for {}", opps.len(), symbol);
                    *opportunities.write().await = opps;
                }
            }
        }
    });

    // Build and run Tauri app
    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_dashboard_data,
            toggle_trading,
            update_risk_limits,
            reset_circuit_breaker,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
