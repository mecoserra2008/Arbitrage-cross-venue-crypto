use arbitrage_core::{Exchange, Side, Symbol, Timestamp};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Represents a detected arbitrage opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub symbol: Symbol,
    pub buy_exchange: Exchange,
    pub sell_exchange: Exchange,
    pub buy_price: Decimal,
    pub sell_price: Decimal,
    pub quantity: Decimal,
    pub gross_profit: Decimal,
    pub net_profit: Decimal,
    pub profit_bps: Decimal, // Basis points (1 bps = 0.01%)
    pub buy_fee: Decimal,
    pub sell_fee: Decimal,
    pub buy_slippage: Decimal,
    pub sell_slippage: Decimal,
    pub timestamp_ns: u64,
    pub latency_ns: u64,
}

impl ArbitrageOpportunity {
    pub fn new(
        symbol: Symbol,
        buy_exchange: Exchange,
        sell_exchange: Exchange,
        buy_price: Decimal,
        sell_price: Decimal,
        quantity: Decimal,
        buy_fee: Decimal,
        sell_fee: Decimal,
        buy_slippage: Decimal,
        sell_slippage: Decimal,
    ) -> Self {
        let adjusted_buy_price = buy_price * (Decimal::ONE + buy_slippage);
        let adjusted_sell_price = sell_price * (Decimal::ONE - sell_slippage);

        let buy_cost = adjusted_buy_price * quantity;
        let sell_proceeds = adjusted_sell_price * quantity;

        let total_fees = buy_fee + sell_fee;
        let gross_profit = sell_proceeds - buy_cost;
        let net_profit = gross_profit - total_fees;

        let profit_bps = if buy_cost > Decimal::ZERO {
            (net_profit / buy_cost) * Decimal::from(10000)
        } else {
            Decimal::ZERO
        };

        Self {
            symbol,
            buy_exchange,
            sell_exchange,
            buy_price,
            sell_price,
            quantity,
            gross_profit,
            net_profit,
            profit_bps,
            buy_fee,
            sell_fee,
            buy_slippage,
            sell_slippage,
            timestamp_ns: Timestamp::now().as_nanos(),
            latency_ns: 0,
        }
    }

    pub fn with_latency(mut self, latency_ns: u64) -> Self {
        self.latency_ns = latency_ns;
        self
    }

    pub fn is_profitable(&self, min_profit_bps: Decimal) -> bool {
        self.net_profit > Decimal::ZERO && self.profit_bps >= min_profit_bps
    }

    pub fn profit_percentage(&self) -> Decimal {
        self.profit_bps / Decimal::from(100)
    }

    pub fn execution_plan(&self) -> ExecutionPlan {
        ExecutionPlan {
            buy_leg: TradeLeg {
                exchange: self.buy_exchange,
                symbol: self.symbol.clone(),
                side: Side::Buy,
                price: self.buy_price,
                quantity: self.quantity,
            },
            sell_leg: TradeLeg {
                exchange: self.sell_exchange,
                symbol: self.symbol.clone(),
                side: Side::Sell,
                price: self.sell_price,
                quantity: self.quantity,
            },
            expected_profit: self.net_profit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLeg {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub buy_leg: TradeLeg,
    pub sell_leg: TradeLeg,
    pub expected_profit: Decimal,
}

/// Configuration for arbitrage detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageConfig {
    pub min_profit_bps: Decimal,
    pub max_position_size: Decimal,
    pub max_slippage_bps: Decimal,
    pub min_liquidity: Decimal,
    pub enabled_exchanges: Vec<Exchange>,
}

impl Default for ArbitrageConfig {
    fn default() -> Self {
        Self {
            min_profit_bps: Decimal::from(5), // 5 bps = 0.05%
            max_position_size: Decimal::from(10000), // $10k USDT
            max_slippage_bps: Decimal::from(10), // 10 bps = 0.10%
            min_liquidity: Decimal::from(50000), // $50k minimum liquidity
            enabled_exchanges: vec![Exchange::Binance, Exchange::Bybit, Exchange::OKX],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrage_core::InstrumentType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_profitable_opportunity() {
        let opp = ArbitrageOpportunity::new(
            Symbol::perpetual("BTC", "USDT"),
            Exchange::Binance,
            Exchange::Bybit,
            dec!(50000.0), // buy at 50000
            dec!(50100.0), // sell at 50100
            dec!(1.0),
            dec!(10.0), // $10 fee buy
            dec!(10.0), // $10 fee sell
            dec!(0.0001), // 1 bps slippage buy
            dec!(0.0001), // 1 bps slippage sell
        );

        assert!(opp.is_profitable(dec!(1.0)));
        assert!(opp.net_profit > Decimal::ZERO);
    }

    #[test]
    fn test_unprofitable_opportunity() {
        let opp = ArbitrageOpportunity::new(
            Symbol::perpetual("BTC", "USDT"),
            Exchange::Binance,
            Exchange::Bybit,
            dec!(50100.0), // buy at 50100
            dec!(50000.0), // sell at 50000 (loss!)
            dec!(1.0),
            dec!(10.0),
            dec!(10.0),
            dec!(0.0001),
            dec!(0.0001),
        );

        assert!(!opp.is_profitable(dec!(1.0)));
        assert!(opp.net_profit < Decimal::ZERO);
    }
}
