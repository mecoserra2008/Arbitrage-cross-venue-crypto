use metrics::{counter, gauge, histogram};
use std::time::Duration;

/// Register and emit metrics for the arbitrage system
pub struct MetricsCollector;

impl MetricsCollector {
    /// Record order book update latency
    pub fn record_orderbook_latency(exchange: &str, latency_ns: u64) {
        histogram!(
            "orderbook_update_latency_ns",
            "exchange" => exchange.to_string()
        ).record(latency_ns as f64);
    }

    /// Record WebSocket message latency (from exchange timestamp to processing)
    pub fn record_websocket_latency(exchange: &str, latency_ns: u64) {
        histogram!(
            "websocket_message_latency_ns",
            "exchange" => exchange.to_string()
        ).record(latency_ns as f64);
    }

    /// Record arbitrage opportunity detection
    pub fn record_arbitrage_opportunity(
        buy_exchange: &str,
        sell_exchange: &str,
        profit_bps: f64,
    ) {
        counter!(
            "arbitrage_opportunities_total",
            "buy_exchange" => buy_exchange.to_string(),
            "sell_exchange" => sell_exchange.to_string()
        ).increment(1);

        histogram!(
            "arbitrage_profit_bps",
            "buy_exchange" => buy_exchange.to_string(),
            "sell_exchange" => sell_exchange.to_string()
        ).record(profit_bps);
    }

    /// Record trade execution
    pub fn record_trade_execution(exchange: &str, side: &str, success: bool) {
        counter!(
            "trade_executions_total",
            "exchange" => exchange.to_string(),
            "side" => side.to_string(),
            "success" => success.to_string()
        ).increment(1);
    }

    /// Record execution latency (from signal to order placement)
    pub fn record_execution_latency(latency_ns: u64) {
        histogram!("execution_latency_ns").record(latency_ns as f64);
    }

    /// Update current spread for a trading pair
    pub fn update_spread(exchange: &str, symbol: &str, spread_bps: f64) {
        gauge!(
            "current_spread_bps",
            "exchange" => exchange.to_string(),
            "symbol" => symbol.to_string()
        ).set(spread_bps);
    }

    /// Update orderbook depth
    pub fn update_orderbook_depth(exchange: &str, symbol: &str, bids: usize, asks: usize) {
        gauge!(
            "orderbook_bid_depth",
            "exchange" => exchange.to_string(),
            "symbol" => symbol.to_string()
        ).set(bids as f64);

        gauge!(
            "orderbook_ask_depth",
            "exchange" => exchange.to_string(),
            "symbol" => symbol.to_string()
        ).set(asks as f64);
    }

    /// Record position update
    pub fn update_position(exchange: &str, symbol: &str, quantity: f64, unrealized_pnl: f64) {
        gauge!(
            "position_quantity",
            "exchange" => exchange.to_string(),
            "symbol" => symbol.to_string()
        ).set(quantity);

        gauge!(
            "position_unrealized_pnl",
            "exchange" => exchange.to_string(),
            "symbol" => symbol.to_string()
        ).set(unrealized_pnl);
    }

    /// Record API rate limit usage
    pub fn record_rate_limit_usage(exchange: &str, used: u64, limit: u64) {
        gauge!(
            "api_rate_limit_used",
            "exchange" => exchange.to_string()
        ).set(used as f64);

        gauge!(
            "api_rate_limit_total",
            "exchange" => exchange.to_string()
        ).set(limit as f64);
    }

    /// Record WebSocket reconnection
    pub fn record_websocket_reconnect(exchange: &str) {
        counter!(
            "websocket_reconnects_total",
            "exchange" => exchange.to_string()
        ).increment(1);
    }

    /// Record error
    pub fn record_error(component: &str, error_type: &str) {
        counter!(
            "errors_total",
            "component" => component.to_string(),
            "error_type" => error_type.to_string()
        ).increment(1);
    }
}
