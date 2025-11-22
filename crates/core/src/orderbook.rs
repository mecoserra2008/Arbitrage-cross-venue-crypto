use crate::types::{Exchange, OrderBookSnapshot, PriceLevel, Symbol, LiquidityMetrics};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::sync::Arc;

/// Thread-safe order book manager for multiple exchanges and symbols
pub struct OrderBookManager {
    books: Arc<DashMap<(Exchange, Symbol), OrderBook>>,
}

impl OrderBookManager {
    pub fn new() -> Self {
        Self {
            books: Arc::new(DashMap::new()),
        }
    }

    pub fn update(&self, snapshot: OrderBookSnapshot) {
        let key = (snapshot.exchange, snapshot.symbol.clone());

        self.books
            .entry(key)
            .and_modify(|book| book.update(snapshot.clone()))
            .or_insert_with(|| {
                let mut book = OrderBook::new(snapshot.exchange, snapshot.symbol.clone());
                book.update(snapshot);
                book
            });
    }

    pub fn get_snapshot(&self, exchange: Exchange, symbol: &Symbol) -> Option<OrderBookSnapshot> {
        self.books
            .get(&(exchange, symbol.clone()))
            .map(|book| book.snapshot())
    }

    #[inline(always)]
    pub fn get_best_bid(&self, exchange: Exchange, symbol: &Symbol) -> Option<PriceLevel> {
        self.books
            .get(&(exchange, symbol.clone()))
            .and_then(|book| book.best_bid().copied())
    }

    #[inline(always)]
    pub fn get_best_ask(&self, exchange: Exchange, symbol: &Symbol) -> Option<PriceLevel> {
        self.books
            .get(&(exchange, symbol.clone()))
            .and_then(|book| book.best_ask().copied())
    }

    #[inline(always)]
    pub fn get_spread(&self, exchange: Exchange, symbol: &Symbol) -> Option<Decimal> {
        self.books
            .get(&(exchange, symbol.clone()))
            .and_then(|book| book.spread())
    }

    pub fn get_mid_price(&self, exchange: Exchange, symbol: &Symbol) -> Option<Decimal> {
        self.books
            .get(&(exchange, symbol.clone()))
            .and_then(|book| book.mid_price())
    }

    pub fn calculate_liquidity_metrics(&self, exchange: Exchange, symbol: &Symbol) -> Option<LiquidityMetrics> {
        self.books
            .get(&(exchange, symbol.clone()))
            .map(|book| book.liquidity_metrics())
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Single order book for one exchange-symbol pair
pub struct OrderBook {
    exchange: Exchange,
    symbol: Symbol,
    bids: Vec<PriceLevel>,
    asks: Vec<PriceLevel>,
    timestamp_ns: u64,
    sequence: u64,
}

impl OrderBook {
    pub fn new(exchange: Exchange, symbol: Symbol) -> Self {
        Self {
            exchange,
            symbol,
            bids: Vec::new(),
            asks: Vec::new(),
            timestamp_ns: 0,
            sequence: 0,
        }
    }

    pub fn update(&mut self, snapshot: OrderBookSnapshot) {
        // Only update if newer sequence number
        if snapshot.sequence > self.sequence {
            self.bids = snapshot.bids;
            self.asks = snapshot.asks;
            self.timestamp_ns = snapshot.timestamp_ns;
            self.sequence = snapshot.sequence;

            // Ensure sorted: bids descending, asks ascending
            self.bids.sort_by(|a, b| b.price.cmp(&a.price));
            self.asks.sort_by(|a, b| a.price.cmp(&b.price));
        }
    }

    pub fn snapshot(&self) -> OrderBookSnapshot {
        OrderBookSnapshot {
            exchange: self.exchange,
            symbol: self.symbol.clone(),
            bids: self.bids.clone(),
            asks: self.asks.clone(),
            timestamp_ns: self.timestamp_ns,
            sequence: self.sequence,
        }
    }

    #[inline(always)]
    pub fn best_bid(&self) -> Option<&PriceLevel> {
        self.bids.first()
    }

    #[inline(always)]
    pub fn best_ask(&self) -> Option<&PriceLevel> {
        self.asks.first()
    }

    #[inline(always)]
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::TWO),
            _ => None,
        }
    }

    /// Calculate total liquidity up to N levels
    pub fn liquidity_at_depth(&self, depth: usize) -> (Decimal, Decimal) {
        let bid_liquidity: Decimal = self.bids
            .iter()
            .take(depth)
            .map(|level| level.value())
            .sum();

        let ask_liquidity: Decimal = self.asks
            .iter()
            .take(depth)
            .map(|level| level.value())
            .sum();

        (bid_liquidity, ask_liquidity)
    }

    /// Calculate comprehensive liquidity metrics
    pub fn liquidity_metrics(&self) -> LiquidityMetrics {
        let (total_bid, total_ask) = self.liquidity_at_depth(usize::MAX);
        let (bid_5, ask_5) = self.liquidity_at_depth(5);
        let (bid_10, ask_10) = self.liquidity_at_depth(10);

        let imbalance = if total_bid + total_ask > Decimal::ZERO {
            (total_bid - total_ask) / (total_bid + total_ask)
        } else {
            Decimal::ZERO
        };

        LiquidityMetrics {
            total_bid_liquidity: total_bid,
            total_ask_liquidity: total_ask,
            depth_bid_5: bid_5,
            depth_ask_5: ask_5,
            depth_bid_10: bid_10,
            depth_ask_10: ask_10,
            imbalance,
        }
    }

    /// Calculate Volume-Weighted Average Price (VWAP) for a given quantity
    pub fn calculate_vwap(&self, side: crate::types::Side, quantity: Decimal) -> Option<Decimal> {
        let levels = match side {
            crate::types::Side::Buy => &self.asks,
            crate::types::Side::Sell => &self.bids,
        };

        let mut remaining = quantity;
        let mut total_cost = Decimal::ZERO;
        let mut total_qty = Decimal::ZERO;

        for level in levels {
            if remaining <= Decimal::ZERO {
                break;
            }

            let qty = remaining.min(level.quantity);
            total_cost += qty * level.price;
            total_qty += qty;
            remaining -= qty;
        }

        if total_qty > Decimal::ZERO {
            Some(total_cost / total_qty)
        } else {
            None
        }
    }

    /// Estimate slippage for executing a given quantity
    pub fn estimate_slippage(&self, side: crate::types::Side, quantity: Decimal) -> Option<Decimal> {
        let best_price = match side {
            crate::types::Side::Buy => self.best_ask()?.price,
            crate::types::Side::Sell => self.best_bid()?.price,
        };

        let vwap = self.calculate_vwap(side, quantity)?;

        Some((vwap - best_price).abs() / best_price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InstrumentType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_orderbook_update() {
        let mut book = OrderBook::new(
            Exchange::Binance,
            Symbol::perpetual("BTC", "USDT"),
        );

        let snapshot = OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: Symbol::perpetual("BTC", "USDT"),
            bids: vec![
                PriceLevel::new(dec!(50000), dec!(1.0)),
                PriceLevel::new(dec!(49999), dec!(2.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50001), dec!(1.5)),
                PriceLevel::new(dec!(50002), dec!(2.5)),
            ],
            timestamp_ns: 1000,
            sequence: 1,
        };

        book.update(snapshot);

        assert_eq!(book.best_bid().unwrap().price, dec!(50000));
        assert_eq!(book.best_ask().unwrap().price, dec!(50001));
        assert_eq!(book.spread().unwrap(), dec!(1));
        assert_eq!(book.mid_price().unwrap(), dec!(50000.5));
    }

    #[test]
    fn test_vwap_calculation() {
        let mut book = OrderBook::new(
            Exchange::Binance,
            Symbol::perpetual("BTC", "USDT"),
        );

        let snapshot = OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: Symbol::perpetual("BTC", "USDT"),
            bids: vec![
                PriceLevel::new(dec!(50000), dec!(1.0)),
                PriceLevel::new(dec!(49999), dec!(2.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50001), dec!(1.0)),
                PriceLevel::new(dec!(50002), dec!(2.0)),
            ],
            timestamp_ns: 1000,
            sequence: 1,
        };

        book.update(snapshot);

        let vwap = book.calculate_vwap(crate::types::Side::Buy, dec!(2.0)).unwrap();
        // VWAP = (50001 * 1.0 + 50002 * 1.0) / 2.0 = 50001.5
        assert_eq!(vwap, dec!(50001.5));
    }
}
