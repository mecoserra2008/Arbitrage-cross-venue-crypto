use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// PnL (Profit and Loss) tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnLTracker {
    realized_pnl: Decimal,
    unrealized_pnl: Decimal,
    total_fees_paid: Decimal,
    total_volume: Decimal,
    winning_trades: usize,
    losing_trades: usize,
    trade_history: VecDeque<TradeRecord>,
    daily_pnl: Decimal,
    peak_equity: Decimal,
    current_drawdown: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp_ns: u64,
    pub profit: Decimal,
    pub fees: Decimal,
    pub volume: Decimal,
}

impl PnLTracker {
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            realized_pnl: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            total_fees_paid: Decimal::ZERO,
            total_volume: Decimal::ZERO,
            winning_trades: 0,
            losing_trades: 0,
            trade_history: VecDeque::new(),
            daily_pnl: Decimal::ZERO,
            peak_equity: initial_equity,
            current_drawdown: Decimal::ZERO,
        }
    }

    pub fn record_trade(&mut self, profit: Decimal, fees: Decimal, volume: Decimal, timestamp_ns: u64) {
        self.realized_pnl += profit;
        self.total_fees_paid += fees;
        self.total_volume += volume;
        self.daily_pnl += profit;

        if profit > Decimal::ZERO {
            self.winning_trades += 1;
        } else if profit < Decimal::ZERO {
            self.losing_trades += 1;
        }

        self.trade_history.push_back(TradeRecord {
            timestamp_ns,
            profit,
            fees,
            volume,
        });

        // Keep only last 10000 trades
        if self.trade_history.len() > 10000 {
            self.trade_history.pop_front();
        }

        // Update drawdown
        self.update_drawdown();
    }

    pub fn update_unrealized_pnl(&mut self, unrealized: Decimal) {
        self.unrealized_pnl = unrealized;
        self.update_drawdown();
    }

    fn update_drawdown(&mut self) {
        let current_equity = self.realized_pnl + self.unrealized_pnl;
        if current_equity > self.peak_equity {
            self.peak_equity = current_equity;
            self.current_drawdown = Decimal::ZERO;
        } else {
            self.current_drawdown = ((self.peak_equity - current_equity) / self.peak_equity) * Decimal::from(100);
        }
    }

    pub fn reset_daily_pnl(&mut self) {
        self.daily_pnl = Decimal::ZERO;
    }

    pub fn total_pnl(&self) -> Decimal {
        self.realized_pnl + self.unrealized_pnl
    }

    pub fn net_pnl(&self) -> Decimal {
        self.total_pnl() - self.total_fees_paid
    }

    pub fn win_rate(&self) -> f64 {
        let total_trades = self.winning_trades + self.losing_trades;
        if total_trades == 0 {
            0.0
        } else {
            (self.winning_trades as f64) / (total_trades as f64) * 100.0
        }
    }

    pub fn average_profit_per_trade(&self) -> Decimal {
        let total_trades = self.winning_trades + self.losing_trades;
        if total_trades == 0 {
            Decimal::ZERO
        } else {
            self.realized_pnl / Decimal::from(total_trades)
        }
    }

    pub fn realized_pnl(&self) -> Decimal {
        self.realized_pnl
    }

    pub fn unrealized_pnl(&self) -> Decimal {
        self.unrealized_pnl
    }

    pub fn daily_pnl(&self) -> Decimal {
        self.daily_pnl
    }

    pub fn current_drawdown(&self) -> Decimal {
        self.current_drawdown
    }

    pub fn total_fees(&self) -> Decimal {
        self.total_fees_paid
    }

    pub fn total_trades(&self) -> usize {
        self.winning_trades + self.losing_trades
    }

    pub fn get_stats(&self) -> PnLStats {
        PnLStats {
            realized_pnl: self.realized_pnl,
            unrealized_pnl: self.unrealized_pnl,
            total_pnl: self.total_pnl(),
            net_pnl: self.net_pnl(),
            total_fees: self.total_fees_paid,
            total_volume: self.total_volume,
            winning_trades: self.winning_trades,
            losing_trades: self.losing_trades,
            total_trades: self.total_trades(),
            win_rate: self.win_rate(),
            avg_profit_per_trade: self.average_profit_per_trade(),
            daily_pnl: self.daily_pnl,
            current_drawdown: self.current_drawdown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnLStats {
    pub realized_pnl: Decimal,
    pub unrealized_pnl: Decimal,
    pub total_pnl: Decimal,
    pub net_pnl: Decimal,
    pub total_fees: Decimal,
    pub total_volume: Decimal,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub total_trades: usize,
    pub win_rate: f64,
    pub avg_profit_per_trade: Decimal,
    pub daily_pnl: Decimal,
    pub current_drawdown: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_pnl_tracking() {
        let mut tracker = PnLTracker::new(dec!(100000));

        tracker.record_trade(dec!(100), dec!(10), dec!(1000), 1000);
        tracker.record_trade(dec!(-50), dec!(10), dec!(1000), 2000);
        tracker.record_trade(dec!(200), dec!(10), dec!(1000), 3000);

        assert_eq!(tracker.realized_pnl(), dec!(250));
        assert_eq!(tracker.total_fees(), dec!(30));
        assert_eq!(tracker.total_trades(), 3);
        assert_eq!(tracker.winning_trades, 2);
        assert_eq!(tracker.losing_trades, 1);
    }
}
