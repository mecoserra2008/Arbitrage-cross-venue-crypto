use arbitrage_core::{Exchange, OrderBookSnapshot, Position, Side, Symbol, TradeExecution};
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::error::Error;

/// Common exchange API trait
#[async_trait]
pub trait ExchangeAPI: Send + Sync {
    /// Get exchange identifier
    fn exchange(&self) -> Exchange;

    /// Subscribe to order book updates via WebSocket
    async fn subscribe_orderbook(&self, symbol: &Symbol) -> Result<(), Box<dyn Error>>;

    /// Place a limit order
    async fn place_limit_order(
        &self,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        quantity: Decimal,
    ) -> Result<String, Box<dyn Error>>;

    /// Place a market order
    async fn place_market_order(
        &self,
        symbol: &Symbol,
        side: Side,
        quantity: Decimal,
    ) -> Result<String, Box<dyn Error>>;

    /// Cancel an order
    async fn cancel_order(&self, symbol: &Symbol, order_id: &str) -> Result<(), Box<dyn Error>>;

    /// Get current positions
    async fn get_positions(&self) -> Result<Vec<Position>, Box<dyn Error>>;

    /// Get account balance
    async fn get_balance(&self) -> Result<Decimal, Box<dyn Error>>;

    /// Get current funding rate for perpetual futures
    async fn get_funding_rate(&self, symbol: &Symbol) -> Result<Decimal, Box<dyn Error>>;
}

/// WebSocket message handler
pub trait WebSocketHandler: Send + Sync {
    fn handle_orderbook_update(&self, snapshot: OrderBookSnapshot);
    fn handle_trade(&self, execution: TradeExecution);
    fn handle_error(&self, error: String);
}
