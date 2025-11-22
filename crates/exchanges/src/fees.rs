use arbitrage_core::{Exchange, FeeStructure};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Fee manager for all exchanges
pub struct FeeManager;

impl FeeManager {
    /// Get fee structure for an exchange (VIP 0 / retail tier)
    pub fn get_fees(exchange: Exchange) -> FeeStructure {
        match exchange {
            Exchange::Binance => FeeStructure::new(
                exchange,
                dec!(0.0002), // 0.02% maker
                dec!(0.0004), // 0.04% taker
            ),
            Exchange::Bybit => FeeStructure::new(
                exchange,
                dec!(0.0001), // 0.01% maker
                dec!(0.0006), // 0.06% taker
            ),
            Exchange::OKX => FeeStructure::new(
                exchange,
                dec!(0.0002), // 0.02% maker
                dec!(0.0005), // 0.05% taker
            ),
            Exchange::Deribit => FeeStructure::new(
                exchange,
                dec!(0.0000), // 0.00% maker (rebate)
                dec!(0.0005), // 0.05% taker
            ),
            Exchange::Kraken => FeeStructure::new(
                exchange,
                dec!(0.0002), // 0.02% maker
                dec!(0.0005), // 0.05% taker
            ),
            Exchange::Coinbase => FeeStructure::new(
                exchange,
                dec!(0.0040), // 0.40% maker
                dec!(0.0060), // 0.60% taker
            ),
        }
    }

    /// Calculate total fee for a trade
    pub fn calculate_fee(
        exchange: Exchange,
        is_maker: bool,
        quantity: Decimal,
        price: Decimal,
    ) -> Decimal {
        let fees = Self::get_fees(exchange);
        let fee_rate = if is_maker { fees.maker_fee } else { fees.taker_fee };
        quantity * price * fee_rate
    }

    /// Calculate net proceeds after fees
    pub fn net_proceeds(
        exchange: Exchange,
        is_maker: bool,
        quantity: Decimal,
        price: Decimal,
    ) -> Decimal {
        let gross = quantity * price;
        let fee = Self::calculate_fee(exchange, is_maker, quantity, price);
        gross - fee
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_fees() {
        let fees = FeeManager::get_fees(Exchange::Binance);
        assert_eq!(fees.maker_fee, dec!(0.0002));
        assert_eq!(fees.taker_fee, dec!(0.0004));
    }

    #[test]
    fn test_fee_calculation() {
        let fee = FeeManager::calculate_fee(
            Exchange::Binance,
            false, // taker
            dec!(1.0),
            dec!(50000.0),
        );
        // 1.0 * 50000.0 * 0.0004 = 20.0
        assert_eq!(fee, dec!(20.0));
    }
}
