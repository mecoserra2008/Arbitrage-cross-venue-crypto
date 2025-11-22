use arbitrage_core::{
    Exchange, OrderBookSnapshot, Position, PriceLevel, Side, Symbol, Timestamp, TradeExecution,
};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::traits::{ExchangeAPI, WebSocketHandler};

type HmacSha256 = Hmac<Sha256>;

const BINANCE_WS_URL: &str = "wss://fstream.binance.com/ws";
const BINANCE_API_URL: &str = "https://fapi.binance.com";

/// Binance Futures API client
pub struct BinanceAPI {
    api_key: String,
    api_secret: String,
    client: Client,
    handler: Arc<dyn WebSocketHandler>,
}

impl BinanceAPI {
    pub fn new(
        api_key: String,
        api_secret: String,
        handler: Arc<dyn WebSocketHandler>,
    ) -> Self {
        Self {
            api_key,
            api_secret,
            client: Client::new(),
            handler,
        }
    }

    /// Sign a request with HMAC SHA256
    fn sign(&self, params: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(params.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Convert symbol to Binance format (e.g., "BTC/USDT-PERP" -> "BTCUSDT")
    fn format_symbol(symbol: &Symbol) -> String {
        format!("{}{}", symbol.base, symbol.quote)
    }

    /// Start WebSocket connection for a symbol
    pub async fn start_websocket(&self, symbol: &Symbol) -> Result<(), Box<dyn Error>> {
        let symbol_formatted = Self::format_symbol(symbol).to_lowercase();
        let url = format!("{}{}@depth20@100ms", BINANCE_WS_URL, symbol_formatted);

        info!("Connecting to Binance WebSocket: {}", url);

        let (ws_stream, _) = connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();

        let handler = self.handler.clone();
        let symbol_clone = symbol.clone();

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Err(e) = Self::handle_depth_message(&text, &symbol_clone, &handler) {
                            error!("Error handling depth message: {}", e);
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            error!("Error sending pong: {}", e);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        warn!("WebSocket closed by server");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        handler.handle_error(format!("WebSocket error: {}", e));
                        break;
                    }
                    _ => {}
                }
            }
            info!("WebSocket connection closed");
        });

        Ok(())
    }

    fn handle_depth_message(
        text: &str,
        symbol: &Symbol,
        handler: &Arc<dyn WebSocketHandler>,
    ) -> Result<(), Box<dyn Error>> {
        #[derive(Deserialize)]
        struct DepthUpdate {
            #[serde(rename = "E")]
            event_time: u64,
            #[serde(rename = "U")]
            first_update_id: u64,
            #[serde(rename = "u")]
            final_update_id: u64,
            #[serde(rename = "b")]
            bids: Vec<[String; 2]>,
            #[serde(rename = "a")]
            asks: Vec<[String; 2]>,
        }

        let update: DepthUpdate = serde_json::from_str(text)?;

        let bids: Vec<PriceLevel> = update
            .bids
            .iter()
            .filter_map(|[price, qty]| {
                let price = price.parse::<Decimal>().ok()?;
                let quantity = qty.parse::<Decimal>().ok()?;
                Some(PriceLevel::new(price, quantity))
            })
            .collect();

        let asks: Vec<PriceLevel> = update
            .asks
            .iter()
            .filter_map(|[price, qty]| {
                let price = price.parse::<Decimal>().ok()?;
                let quantity = qty.parse::<Decimal>().ok()?;
                Some(PriceLevel::new(price, quantity))
            })
            .collect();

        let snapshot = OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: symbol.clone(),
            bids,
            asks,
            timestamp_ns: Timestamp::now().as_nanos(),
            sequence: update.final_update_id,
        };

        handler.handle_orderbook_update(snapshot);
        Ok(())
    }
}

#[async_trait]
impl ExchangeAPI for BinanceAPI {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    async fn subscribe_orderbook(&self, symbol: &Symbol) -> Result<(), Box<dyn Error>> {
        self.start_websocket(symbol).await
    }

    async fn place_limit_order(
        &self,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        quantity: Decimal,
    ) -> Result<String, Box<dyn Error>> {
        let symbol_formatted = Self::format_symbol(symbol);
        let timestamp = chrono::Utc::now().timestamp_millis();

        let side_str = match side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let params = format!(
            "symbol={}&side={}&type=LIMIT&timeInForce=GTC&quantity={}&price={}&timestamp={}",
            symbol_formatted, side_str, quantity, price, timestamp
        );

        let signature = self.sign(&params);
        let url = format!("{}/fapi/v1/order?{}&signature={}", BINANCE_API_URL, params, signature);

        let response = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        #[derive(Deserialize)]
        struct OrderResponse {
            #[serde(rename = "orderId")]
            order_id: u64,
        }

        let order_response: OrderResponse = response.json().await?;
        Ok(order_response.order_id.to_string())
    }

    async fn place_market_order(
        &self,
        symbol: &Symbol,
        side: Side,
        quantity: Decimal,
    ) -> Result<String, Box<dyn Error>> {
        let symbol_formatted = Self::format_symbol(symbol);
        let timestamp = chrono::Utc::now().timestamp_millis();

        let side_str = match side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let params = format!(
            "symbol={}&side={}&type=MARKET&quantity={}&timestamp={}",
            symbol_formatted, side_str, quantity, timestamp
        );

        let signature = self.sign(&params);
        let url = format!("{}/fapi/v1/order?{}&signature={}", BINANCE_API_URL, params, signature);

        let response = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        #[derive(Deserialize)]
        struct OrderResponse {
            #[serde(rename = "orderId")]
            order_id: u64,
        }

        let order_response: OrderResponse = response.json().await?;
        Ok(order_response.order_id.to_string())
    }

    async fn cancel_order(&self, symbol: &Symbol, order_id: &str) -> Result<(), Box<dyn Error>> {
        let symbol_formatted = Self::format_symbol(symbol);
        let timestamp = chrono::Utc::now().timestamp_millis();

        let params = format!(
            "symbol={}&orderId={}&timestamp={}",
            symbol_formatted, order_id, timestamp
        );

        let signature = self.sign(&params);
        let url = format!("{}/fapi/v1/order?{}&signature={}", BINANCE_API_URL, params, signature);

        self.client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        Ok(())
    }

    async fn get_positions(&self) -> Result<Vec<Position>, Box<dyn Error>> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let params = format!("timestamp={}", timestamp);
        let signature = self.sign(&params);
        let url = format!(
            "{}/fapi/v2/positionRisk?{}&signature={}",
            BINANCE_API_URL, params, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PositionRisk {
            symbol: String,
            position_amt: String,
            entry_price: String,
            mark_price: String,
            un_realized_profit: String,
            leverage: String,
        }

        let positions: Vec<PositionRisk> = response.json().await?;

        Ok(positions
            .into_iter()
            .filter_map(|p| {
                let quantity = p.position_amt.parse::<Decimal>().ok()?;
                if quantity == Decimal::ZERO {
                    return None;
                }

                Some(Position {
                    exchange: Exchange::Binance,
                    symbol: Symbol::perpetual(&p.symbol[0..3], &p.symbol[3..]),
                    quantity,
                    entry_price: p.entry_price.parse().ok()?,
                    current_price: p.mark_price.parse().ok()?,
                    unrealized_pnl: p.un_realized_profit.parse().ok()?,
                    realized_pnl: Decimal::ZERO,
                    leverage: p.leverage.parse().ok()?,
                })
            })
            .collect())
    }

    async fn get_balance(&self) -> Result<Decimal, Box<dyn Error>> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let params = format!("timestamp={}", timestamp);
        let signature = self.sign(&params);
        let url = format!(
            "{}/fapi/v2/balance?{}&signature={}",
            BINANCE_API_URL, params, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Balance {
            asset: String,
            balance: String,
        }

        let balances: Vec<Balance> = response.json().await?;
        let usdt_balance = balances
            .iter()
            .find(|b| b.asset == "USDT")
            .and_then(|b| b.balance.parse::<Decimal>().ok())
            .unwrap_or(Decimal::ZERO);

        Ok(usdt_balance)
    }

    async fn get_funding_rate(&self, symbol: &Symbol) -> Result<Decimal, Box<dyn Error>> {
        let symbol_formatted = Self::format_symbol(symbol);
        let url = format!("{}/fapi/v1/premiumIndex?symbol={}", BINANCE_API_URL, symbol_formatted);

        let response = self.client.get(&url).send().await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PremiumIndex {
            last_funding_rate: String,
        }

        let premium: PremiumIndex = response.json().await?;
        Ok(premium.last_funding_rate.parse()?)
    }
}
