use arbitrage_core::{Exchange, Position, Symbol};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use tracing::{error, warn};

/// Tracks order fills and reconciles with exchange state
pub struct PositionReconciler {
    local_positions: Arc<DashMap<(Exchange, Symbol), TrackedPosition>>,
    reconciliation_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct TrackedPosition {
    pub position: Position,
    pub pending_orders: Vec<PendingOrder>,
    pub last_reconciled: Instant,
    pub reconciliation_errors: u32,
}

#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub order_id: String,
    pub side: arbitrage_core::Side,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub status: OrderStatus,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderStatus {
    Pending,
    PartiallyFilled,
    Filled,
    Cancelled,
    Failed,
}

impl PositionReconciler {
    pub fn new(reconciliation_interval_secs: u64) -> Self {
        Self {
            local_positions: Arc::new(DashMap::new()),
            reconciliation_interval: Duration::from_secs(reconciliation_interval_secs),
        }
    }

    pub fn institutional() -> Self {
        Self::new(30) // Reconcile every 30 seconds
    }

    /// Track a new order
    pub fn track_order(
        &self,
        exchange: Exchange,
        symbol: Symbol,
        order_id: String,
        side: arbitrage_core::Side,
        quantity: Decimal,
    ) {
        let key = (exchange, symbol.clone());

        self.local_positions
            .entry(key)
            .and_modify(|tracked| {
                tracked.pending_orders.push(PendingOrder {
                    order_id: order_id.clone(),
                    side,
                    quantity,
                    filled_quantity: Decimal::ZERO,
                    status: OrderStatus::Pending,
                    timestamp: Instant::now(),
                });
            })
            .or_insert_with(|| TrackedPosition {
                position: Position {
                    exchange,
                    symbol: symbol.clone(),
                    quantity: Decimal::ZERO,
                    entry_price: Decimal::ZERO,
                    current_price: Decimal::ZERO,
                    unrealized_pnl: Decimal::ZERO,
                    realized_pnl: Decimal::ZERO,
                    leverage: Decimal::ONE,
                },
                pending_orders: vec![PendingOrder {
                    order_id,
                    side,
                    quantity,
                    filled_quantity: Decimal::ZERO,
                    status: OrderStatus::Pending,
                    timestamp: Instant::now(),
                }],
                last_reconciled: Instant::now(),
                reconciliation_errors: 0,
            });
    }

    /// Update order fill status
    pub fn update_order_fill(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
        order_id: &str,
        filled_quantity: Decimal,
        status: OrderStatus,
    ) {
        let key = (exchange, symbol.clone());

        if let Some(mut tracked) = self.local_positions.get_mut(&key) {
            if let Some(order) = tracked
                .pending_orders
                .iter_mut()
                .find(|o| o.order_id == order_id)
            {
                order.filled_quantity = filled_quantity;
                order.status = status;

                // Update position if filled
                if order.status == OrderStatus::Filled {
                    let qty_change = match order.side {
                        arbitrage_core::Side::Buy => order.quantity,
                        arbitrage_core::Side::Sell => -order.quantity,
                    };
                    tracked.position.quantity += qty_change;
                }
            }
        }
    }

    /// Reconcile local positions with exchange state
    pub async fn reconcile_all<F, Fut>(
        &self,
        position_fetcher: F,
    ) -> Result<ReconciliationReport, ReconciliationError>
    where
        F: Fn(Exchange) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Position>, Box<dyn std::error::Error>>>,
    {
        let mut report = ReconciliationReport::default();

        for mut entry in self.local_positions.iter_mut() {
            let (exchange, _symbol) = entry.key();
            let tracked = entry.value_mut();

            // Fetch actual positions from exchange
            match position_fetcher(*exchange).await {
                Ok(exchange_positions) => {
                    tracked.last_reconciled = Instant::now();

                    // Find matching position
                    if let Some(exchange_pos) = exchange_positions
                        .iter()
                        .find(|p| p.symbol == tracked.position.symbol)
                    {
                        let local_qty = tracked.position.quantity;
                        let exchange_qty = exchange_pos.quantity;

                        if (local_qty - exchange_qty).abs() > Decimal::new(1, 6) {
                            // Discrepancy detected (tolerance: 0.000001)
                            warn!(
                                "Position discrepancy detected: {} {} - Local: {}, Exchange: {}",
                                exchange, tracked.position.symbol, local_qty, exchange_qty
                            );

                            report.discrepancies.push(PositionDiscrepancy {
                                exchange: *exchange,
                                symbol: tracked.position.symbol.clone(),
                                local_quantity: local_qty,
                                exchange_quantity: exchange_qty,
                                difference: local_qty - exchange_qty,
                            });

                            // Sync to exchange state
                            tracked.position.quantity = exchange_qty;
                            tracked.position.entry_price = exchange_pos.entry_price;
                            tracked.position.unrealized_pnl = exchange_pos.unrealized_pnl;
                            report.positions_synced += 1;
                        } else {
                            report.positions_matched += 1;
                        }
                    }

                    tracked.reconciliation_errors = 0;
                }
                Err(e) => {
                    error!("Failed to fetch positions from {}: {}", exchange, e);
                    tracked.reconciliation_errors += 1;
                    report.errors += 1;

                    if tracked.reconciliation_errors >= 3 {
                        error!(
                            "Multiple reconciliation failures for {} - position may be stale",
                            exchange
                        );
                    }
                }
            }
        }

        Ok(report)
    }

    /// Start automatic reconciliation loop
    pub fn start_auto_reconciliation<F, Fut>(self: Arc<Self>, position_fetcher: F)
    where
        F: Fn(Exchange) -> Fut + Send + Sync + 'static + Clone,
        Fut: std::future::Future<Output = Result<Vec<Position>, Box<dyn std::error::Error>>>
            + Send
            + 'static,
    {
        tokio::spawn(async move {
            let mut interval_timer = interval(self.reconciliation_interval);

            loop {
                interval_timer.tick().await;

                match self.reconcile_all(position_fetcher.clone()).await {
                    Ok(report) => {
                        if report.discrepancies.is_empty() {
                            tracing::debug!(
                                "Reconciliation complete: {} matched, {} synced",
                                report.positions_matched,
                                report.positions_synced
                            );
                        } else {
                            warn!(
                                "Reconciliation found {} discrepancies",
                                report.discrepancies.len()
                            );
                        }
                    }
                    Err(e) => {
                        error!("Reconciliation failed: {}", e);
                    }
                }
            }
        });
    }

    /// Get current tracked position
    pub fn get_position(&self, exchange: Exchange, symbol: &Symbol) -> Option<Position> {
        self.local_positions
            .get(&(exchange, symbol.clone()))
            .map(|tracked| tracked.position.clone())
    }

    /// Get pending orders for a position
    pub fn get_pending_orders(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
    ) -> Vec<PendingOrder> {
        self.local_positions
            .get(&(exchange, symbol.clone()))
            .map(|tracked| tracked.pending_orders.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
pub struct ReconciliationReport {
    pub positions_matched: u32,
    pub positions_synced: u32,
    pub discrepancies: Vec<PositionDiscrepancy>,
    pub errors: u32,
}

#[derive(Debug, Clone)]
pub struct PositionDiscrepancy {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub local_quantity: Decimal,
    pub exchange_quantity: Decimal,
    pub difference: Decimal,
}

#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    #[error("Failed to fetch positions: {0}")]
    FetchFailed(String),

    #[error("Position not found")]
    PositionNotFound,
}
