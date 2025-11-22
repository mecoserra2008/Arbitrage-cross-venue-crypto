use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Risk limits for trading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum position size per symbol (in quote currency)
    pub max_position_size: Decimal,

    /// Maximum total exposure across all positions
    pub max_total_exposure: Decimal,

    /// Maximum loss per trade
    pub max_loss_per_trade: Decimal,

    /// Maximum daily loss
    pub max_daily_loss: Decimal,

    /// Maximum drawdown percentage
    pub max_drawdown_pct: Decimal,

    /// Maximum number of open positions
    pub max_open_positions: usize,

    /// Maximum leverage allowed
    pub max_leverage: Decimal,

    /// Minimum profit threshold (in basis points)
    pub min_profit_bps: Decimal,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position_size: Decimal::from(100000), // $100k
            max_total_exposure: Decimal::from(500000), // $500k
            max_loss_per_trade: Decimal::from(1000), // $1k
            max_daily_loss: Decimal::from(5000), // $5k
            max_drawdown_pct: Decimal::from(10), // 10%
            max_open_positions: 10,
            max_leverage: Decimal::from(5), // 5x
            min_profit_bps: Decimal::from(5), // 5 bps = 0.05%
        }
    }
}

impl RiskLimits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn conservative() -> Self {
        Self {
            max_position_size: Decimal::from(10000),
            max_total_exposure: Decimal::from(50000),
            max_loss_per_trade: Decimal::from(100),
            max_daily_loss: Decimal::from(500),
            max_drawdown_pct: Decimal::from(5),
            max_open_positions: 3,
            max_leverage: Decimal::from(2),
            min_profit_bps: Decimal::from(10),
        }
    }

    pub fn aggressive() -> Self {
        Self {
            max_position_size: Decimal::from(500000),
            max_total_exposure: Decimal::from(2000000),
            max_loss_per_trade: Decimal::from(5000),
            max_daily_loss: Decimal::from(20000),
            max_drawdown_pct: Decimal::from(20),
            max_open_positions: 50,
            max_leverage: Decimal::from(10),
            min_profit_bps: Decimal::from(2),
        }
    }
}
