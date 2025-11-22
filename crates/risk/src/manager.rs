use arbitrage_core::{Exchange, Position, Symbol};
use crate::limits::RiskLimits;
use crate::pnl::PnLTracker;
use dashmap::DashMap;
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("Position size exceeds limit: {0} > {1}")]
    PositionSizeExceeded(Decimal, Decimal),

    #[error("Total exposure exceeds limit: {0} > {1}")]
    TotalExposureExceeded(Decimal, Decimal),

    #[error("Daily loss limit exceeded: {0} > {1}")]
    DailyLossExceeded(Decimal, Decimal),

    #[error("Drawdown limit exceeded: {0}% > {1}%")]
    DrawdownExceeded(Decimal, Decimal),

    #[error("Max open positions exceeded: {0} > {1}")]
    MaxPositionsExceeded(usize, usize),

    #[error("Profit below minimum threshold: {0} bps < {1} bps")]
    ProfitBelowThreshold(Decimal, Decimal),

    #[error("Circuit breaker triggered: {0}")]
    CircuitBreaker(String),
}

/// Risk manager for the arbitrage system
pub struct RiskManager {
    limits: Arc<RwLock<RiskLimits>>,
    pnl_tracker: Arc<RwLock<PnLTracker>>,
    positions: Arc<DashMap<(Exchange, Symbol), Position>>,
    circuit_breaker_active: Arc<RwLock<bool>>,
}

impl RiskManager {
    pub fn new(limits: RiskLimits, initial_equity: Decimal) -> Self {
        Self {
            limits: Arc::new(RwLock::new(limits)),
            pnl_tracker: Arc::new(RwLock::new(PnLTracker::new(initial_equity))),
            positions: Arc::new(DashMap::new()),
            circuit_breaker_active: Arc::new(RwLock::new(false)),
        }
    }

    /// Check if a trade is allowed based on risk limits
    pub fn can_trade(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
        position_size: Decimal,
        expected_profit: Decimal,
    ) -> Result<(), RiskError> {
        // Check circuit breaker
        if *self.circuit_breaker_active.read() {
            return Err(RiskError::CircuitBreaker("System halted".to_string()));
        }

        let limits = self.limits.read();

        // Check position size
        if position_size > limits.max_position_size {
            warn!("Position size {} exceeds limit {}", position_size, limits.max_position_size);
            return Err(RiskError::PositionSizeExceeded(
                position_size,
                limits.max_position_size,
            ));
        }

        // Check total exposure
        let current_exposure = self.calculate_total_exposure();
        let new_exposure = current_exposure + position_size;
        if new_exposure > limits.max_total_exposure {
            warn!("Total exposure {} exceeds limit {}", new_exposure, limits.max_total_exposure);
            return Err(RiskError::TotalExposureExceeded(
                new_exposure,
                limits.max_total_exposure,
            ));
        }

        // Check number of open positions
        let open_positions = self.positions.len();
        if open_positions >= limits.max_open_positions {
            warn!("Open positions {} exceeds limit {}", open_positions, limits.max_open_positions);
            return Err(RiskError::MaxPositionsExceeded(
                open_positions,
                limits.max_open_positions,
            ));
        }

        // Check daily loss
        let daily_pnl = self.pnl_tracker.read().daily_pnl();
        if daily_pnl < -limits.max_daily_loss {
            error!("Daily loss {} exceeds limit {}", daily_pnl.abs(), limits.max_daily_loss);
            self.trigger_circuit_breaker("Daily loss limit exceeded");
            return Err(RiskError::DailyLossExceeded(
                daily_pnl.abs(),
                limits.max_daily_loss,
            ));
        }

        // Check drawdown
        let drawdown = self.pnl_tracker.read().current_drawdown();
        if drawdown > limits.max_drawdown_pct {
            error!("Drawdown {}% exceeds limit {}%", drawdown, limits.max_drawdown_pct);
            self.trigger_circuit_breaker("Drawdown limit exceeded");
            return Err(RiskError::DrawdownExceeded(
                drawdown,
                limits.max_drawdown_pct,
            ));
        }

        Ok(())
    }

    /// Record a completed trade
    pub fn record_trade(
        &self,
        exchange: Exchange,
        symbol: Symbol,
        profit: Decimal,
        fees: Decimal,
        volume: Decimal,
        timestamp_ns: u64,
    ) {
        self.pnl_tracker.write().record_trade(profit, fees, volume, timestamp_ns);
    }

    /// Update position
    pub fn update_position(&self, position: Position) {
        let key = (position.exchange, position.symbol.clone());
        self.positions.insert(key, position);
    }

    /// Remove position
    pub fn remove_position(&self, exchange: Exchange, symbol: &Symbol) {
        self.positions.remove(&(exchange, symbol.clone()));
    }

    /// Get all positions
    pub fn get_positions(&self) -> Vec<Position> {
        self.positions.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Calculate total exposure across all positions
    pub fn calculate_total_exposure(&self) -> Decimal {
        self.positions
            .iter()
            .map(|entry| {
                let pos = entry.value();
                pos.quantity.abs() * pos.current_price
            })
            .sum()
    }

    /// Trigger circuit breaker
    pub fn trigger_circuit_breaker(&self, reason: &str) {
        error!("CIRCUIT BREAKER TRIGGERED: {}", reason);
        *self.circuit_breaker_active.write() = true;
    }

    /// Reset circuit breaker
    pub fn reset_circuit_breaker(&self) {
        *self.circuit_breaker_active.write() = false;
    }

    /// Check if circuit breaker is active
    pub fn is_circuit_breaker_active(&self) -> bool {
        *self.circuit_breaker_active.read()
    }

    /// Reset daily PnL (call at start of each day)
    pub fn reset_daily_pnl(&self) {
        self.pnl_tracker.write().reset_daily_pnl();
    }

    /// Get PnL stats
    pub fn get_pnl_stats(&self) -> crate::pnl::PnLStats {
        self.pnl_tracker.read().get_stats()
    }

    /// Update risk limits
    pub fn update_limits(&self, limits: RiskLimits) {
        *self.limits.write() = limits;
    }

    /// Get current risk limits
    pub fn get_limits(&self) -> RiskLimits {
        self.limits.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrage_core::InstrumentType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_position_size_limit() {
        let limits = RiskLimits {
            max_position_size: dec!(1000),
            ..Default::default()
        };

        let manager = RiskManager::new(limits, dec!(100000));

        let result = manager.can_trade(
            Exchange::Binance,
            &Symbol::perpetual("BTC", "USDT"),
            dec!(2000),
            dec!(10),
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RiskError::PositionSizeExceeded(_, _)));
    }

    #[test]
    fn test_allowed_trade() {
        let limits = RiskLimits::default();
        let manager = RiskManager::new(limits, dec!(100000));

        let result = manager.can_trade(
            Exchange::Binance,
            &Symbol::perpetual("BTC", "USDT"),
            dec!(1000),
            dec!(10),
        );

        assert!(result.is_ok());
    }
}
