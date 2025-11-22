use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

/// WebSocket connection manager with automatic reconnection
pub struct WebSocketManager {
    url: String,
    reconnect_attempts: Arc<RwLock<u32>>,
    max_reconnect_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
    is_connected: Arc<RwLock<bool>>,
}

impl WebSocketManager {
    pub fn new(url: String) -> Self {
        Self {
            url,
            reconnect_attempts: Arc::new(RwLock::new(0)),
            max_reconnect_attempts: 10,
            base_backoff_ms: 1000,  // Start with 1 second
            max_backoff_ms: 60000,  // Max 60 seconds
            is_connected: Arc::new(RwLock::new(false)),
        }
    }

    pub fn institutional(url: String) -> Self {
        Self {
            url,
            reconnect_attempts: Arc::new(RwLock::new(0)),
            max_reconnect_attempts: 5,
            base_backoff_ms: 500,
            max_backoff_ms: 30000,
            is_connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Calculate exponential backoff duration
    fn calculate_backoff(&self, attempt: u32) -> Duration {
        let backoff_ms = self.base_backoff_ms * 2u64.pow(attempt);
        let capped_backoff = backoff_ms.min(self.max_backoff_ms);
        Duration::from_millis(capped_backoff)
    }

    /// Connect with automatic reconnection on failure
    pub async fn connect_with_retry<F, Fut>(
        &self,
        message_handler: F,
    ) -> Result<(), WebSocketError>
    where
        F: Fn(String) -> Fut + Send + 'static + Clone,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>> + Send,
    {
        loop {
            let attempts = *self.reconnect_attempts.read().await;

            if attempts >= self.max_reconnect_attempts {
                error!(
                    "Max reconnection attempts ({}) reached for {}",
                    self.max_reconnect_attempts, self.url
                );
                return Err(WebSocketError::MaxReconnectAttemptsReached);
            }

            if attempts > 0 {
                let backoff = self.calculate_backoff(attempts);
                warn!(
                    "Reconnection attempt {} for {} after {:?}",
                    attempts + 1,
                    self.url,
                    backoff
                );
                sleep(backoff).await;
            }

            match self.connect_once(message_handler.clone()).await {
                Ok(_) => {
                    info!("Successfully connected to {}", self.url);
                    *self.reconnect_attempts.write().await = 0;
                    *self.is_connected.write().await = true;
                }
                Err(e) => {
                    error!("Connection failed for {}: {}", self.url, e);
                    *self.is_connected.write().await = false;
                    *self.reconnect_attempts.write().await += 1;
                    continue;
                }
            }
        }
    }

    async fn connect_once<F, Fut>(&self, message_handler: F) -> Result<(), WebSocketError>
    where
        F: Fn(String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>> + Send,
    {
        info!("Connecting to WebSocket: {}", self.url);

        let (ws_stream, _) = connect_async(&self.url)
            .await
            .map_err(|e| WebSocketError::ConnectionFailed(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        // Heartbeat to keep connection alive
        let url_clone = self.url.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(30)).await;
                if let Err(e) = write.send(Message::Ping(vec![1, 2, 3, 4])).await {
                    warn!("Failed to send ping to {}: {}", url_clone, e);
                    break;
                }
            }
        });

        // Message processing loop
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = message_handler(text).await {
                        error!("Error handling message: {}", e);
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data) {
                        if let Err(e) = message_handler(text).await {
                            error!("Error handling binary message: {}", e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    // Respond to ping
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        error!("Failed to send pong: {}", e);
                        return Err(WebSocketError::SendFailed(e.to_string()));
                    }
                }
                Ok(Message::Pong(_)) => {
                    // Pong received, connection is alive
                }
                Ok(Message::Close(frame)) => {
                    warn!("WebSocket closed: {:?}", frame);
                    return Err(WebSocketError::ConnectionClosed);
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    return Err(WebSocketError::ReceiveFailed(e.to_string()));
                }
                _ => {}
            }
        }

        warn!("WebSocket stream ended for {}", self.url);
        Err(WebSocketError::ConnectionClosed)
    }

    pub async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    pub async fn reset_reconnect_counter(&self) {
        *self.reconnect_attempts.write().await = 0;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Failed to send message: {0}")]
    SendFailed(String),

    #[error("Failed to receive message: {0}")]
    ReceiveFailed(String),

    #[error("Max reconnection attempts reached")]
    MaxReconnectAttemptsReached,

    #[error("Message parsing error: {0}")]
    ParseError(String),
}
