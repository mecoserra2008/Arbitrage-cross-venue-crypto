use arbitrage_core::{Exchange, MetricsCollector, Symbol, TradeExecution};
use arbitrage_exchanges::ExchangeAPI;
use crate::opportunity::{ArbitrageOpportunity, ExecutionPlan};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Exchange API error: {0}")]
    ExchangeError(String),

    #[error("Insufficient balance: needed {needed}, available {available}")]
    InsufficientBalance { needed: Decimal, available: Decimal },

    #[error("Order placement failed: {0}")]
    OrderFailed(String),

    #[error("Partial fill: expected {expected}, filled {filled}")]
    PartialFill { expected: Decimal, filled: Decimal },
}

/// Executes arbitrage trades across exchanges
pub struct ArbitrageExecutor {
    exchanges: HashMap<Exchange, Arc<dyn ExchangeAPI>>,
}

impl ArbitrageExecutor {
    pub fn new() -> Self {
        Self {
            exchanges: HashMap::new(),
        }
    }

    pub fn add_exchange(&mut self, exchange: Exchange, api: Arc<dyn ExchangeAPI>) {
        self.exchanges.insert(exchange, api);
    }

    /// Execute an arbitrage opportunity
    pub async fn execute(&self, opportunity: &ArbitrageOpportunity) -> Result<ExecutionResult, ExecutionError> {
        let plan = opportunity.execution_plan();

        info!(
            "Executing arbitrage: Buy {} {} @ {} on {}, Sell @ {} on {}",
            plan.buy_leg.quantity,
            opportunity.symbol,
            plan.buy_leg.price,
            plan.buy_leg.exchange,
            plan.sell_leg.price,
            plan.sell_leg.exchange
        );

        // Validate balances
        self.validate_balances(&plan).await?;

        // Execute both legs simultaneously
        let (buy_result, sell_result) = tokio::join!(
            self.execute_leg(&plan.buy_leg),
            self.execute_leg(&plan.sell_leg)
        );

        let buy_order_id = buy_result?;
        let sell_order_id = sell_result?;

        info!(
            "Arbitrage executed: buy_order={}, sell_order={}",
            buy_order_id, sell_order_id
        );

        // Record metrics
        MetricsCollector::record_trade_execution(
            &plan.buy_leg.exchange.to_string(),
            &plan.buy_leg.side.to_string(),
            true,
        );
        MetricsCollector::record_trade_execution(
            &plan.sell_leg.exchange.to_string(),
            &plan.sell_leg.side.to_string(),
            true,
        );

        Ok(ExecutionResult {
            buy_order_id,
            sell_order_id,
            expected_profit: plan.expected_profit,
            actual_profit: Decimal::ZERO, // TODO: Calculate after fills
        })
    }

    async fn validate_balances(&self, plan: &ExecutionPlan) -> Result<(), ExecutionError> {
        let buy_api = self.exchanges.get(&plan.buy_leg.exchange)
            .ok_or_else(|| ExecutionError::ExchangeError(
                format!("No API for exchange {}", plan.buy_leg.exchange)
            ))?;

        let balance = buy_api.get_balance().await
            .map_err(|e| ExecutionError::ExchangeError(e.to_string()))?;

        let needed = plan.buy_leg.price * plan.buy_leg.quantity;

        if balance < needed {
            return Err(ExecutionError::InsufficientBalance {
                needed,
                available: balance,
            });
        }

        Ok(())
    }

    async fn execute_leg(&self, leg: &crate::opportunity::TradeLeg) -> Result<String, ExecutionError> {
        let api = self.exchanges.get(&leg.exchange)
            .ok_or_else(|| ExecutionError::ExchangeError(
                format!("No API for exchange {}", leg.exchange)
            ))?;

        // Use market orders for speed (can be configured to use limit orders)
        let order_id = api
            .place_market_order(&leg.symbol, leg.side, leg.quantity)
            .await
            .map_err(|e| ExecutionError::OrderFailed(e.to_string()))?;

        Ok(order_id)
    }
}

impl Default for ArbitrageExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub buy_order_id: String,
    pub sell_order_id: String,
    pub expected_profit: Decimal,
    pub actual_profit: Decimal,
}
