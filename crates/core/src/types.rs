use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported exchanges
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    Bybit,
    OKX,
    Deribit,
    Kraken,
    Coinbase,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Binance => write!(f, "Binance"),
            Exchange::Bybit => write!(f, "Bybit"),
            Exchange::OKX => write!(f, "OKX"),
            Exchange::Deribit => write!(f, "Deribit"),
            Exchange::Kraken => write!(f, "Kraken"),
            Exchange::Coinbase => write!(f, "Coinbase"),
        }
    }
}

/// Trading pair
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
    pub instrument_type: InstrumentType,
}

impl Symbol {
    pub fn new(base: impl Into<String>, quote: impl Into<String>, instrument_type: InstrumentType) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
            instrument_type,
        }
    }

    pub fn perpetual(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self::new(base, quote, InstrumentType::PerpetualFuture)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}-{}", self.base, self.quote, self.instrument_type)
    }
}

/// Instrument type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentType {
    Spot,
    PerpetualFuture,
    Future { expiry: i64 },
    Option { expiry: i64, strike: Decimal, is_call: bool },
}

impl fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentType::Spot => write!(f, "SPOT"),
            InstrumentType::PerpetualFuture => write!(f, "PERP"),
            InstrumentType::Future { expiry } => write!(f, "FUT-{}", expiry),
            InstrumentType::Option { expiry, strike, is_call } => {
                write!(f, "{}-{}-{}", if *is_call { "CALL" } else { "PUT" }, strike, expiry)
            }
        }
    }
}

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

/// Order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    PostOnly,
    ImmediateOrCancel,
    FillOrKill,
}

/// Price level in order book
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

impl PriceLevel {
    pub fn new(price: Decimal, quantity: Decimal) -> Self {
        Self { price, quantity }
    }

    pub fn value(&self) -> Decimal {
        self.price * self.quantity
    }
}

/// Order book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub timestamp_ns: u64,
    pub sequence: u64,
}

impl OrderBookSnapshot {
    pub fn best_bid(&self) -> Option<&PriceLevel> {
        self.bids.first()
    }

    pub fn best_ask(&self) -> Option<&PriceLevel> {
        self.asks.first()
    }

    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::TWO),
            _ => None,
        }
    }
}

/// Fee structure for an exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeStructure {
    pub exchange: Exchange,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub funding_rate: Option<Decimal>,
}

impl FeeStructure {
    pub fn new(exchange: Exchange, maker_fee: Decimal, taker_fee: Decimal) -> Self {
        Self {
            exchange,
            maker_fee,
            taker_fee,
            funding_rate: None,
        }
    }

    pub fn with_funding_rate(mut self, funding_rate: Decimal) -> Self {
        self.funding_rate = Some(funding_rate);
        self
    }
}

/// Liquidity metrics for a price level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityMetrics {
    pub total_bid_liquidity: Decimal,
    pub total_ask_liquidity: Decimal,
    pub depth_bid_5: Decimal,
    pub depth_ask_5: Decimal,
    pub depth_bid_10: Decimal,
    pub depth_ask_10: Decimal,
    pub imbalance: Decimal, // (bid_liquidity - ask_liquidity) / (bid_liquidity + ask_liquidity)
}

/// Trade execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeExecution {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
    pub timestamp_ns: u64,
    pub order_id: String,
}

/// Position information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub quantity: Decimal, // Positive for long, negative for short
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    pub leverage: Decimal,
}

impl Position {
    pub fn is_long(&self) -> bool {
        self.quantity > Decimal::ZERO
    }

    pub fn is_short(&self) -> bool {
        self.quantity < Decimal::ZERO
    }

    pub fn update_pnl(&mut self, current_price: Decimal) {
        self.current_price = current_price;
        let price_diff = current_price - self.entry_price;
        self.unrealized_pnl = self.quantity * price_diff;
    }
}
