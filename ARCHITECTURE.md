# Architecture Documentation

## System Overview

This arbitrage trading bot is designed as a high-performance, institutional-grade system with the following key design principles:

### Design Principles

1. **Ultra-Low Latency**: Nanosecond-precision timing and optimized data structures
2. **Type Safety**: Rust's ownership system prevents data races and memory issues
3. **Fault Tolerance**: Graceful degradation and automatic recovery
4. **Observability**: Comprehensive metrics and logging
5. **Modularity**: Clean separation of concerns across crates

## Component Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        GUI (Tauri)                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │  Dashboard   │  │  Risk Mgmt   │  │   Settings   │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└────────────────────────┬────────────────────────────────────┘
                         │ IPC (Tauri Commands)
                         │
┌────────────────────────┴────────────────────────────────────┐
│                    Application Layer                         │
│  ┌──────────────────────────────────────────────────────┐   │
│  │           Arbitrage Detection Engine                  │   │
│  │  • Opportunity Scanner                                │   │
│  │  • Profitability Calculator                           │   │
│  │  • Execution Planner                                  │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │             Risk Management System                    │   │
│  │  • Position Tracker                                   │   │
│  │  • PnL Calculator                                     │   │
│  │  • Circuit Breaker                                    │   │
│  └──────────────────────────────────────────────────────┘   │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────────┐
│                      Data Layer                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │            OrderBook Manager (DashMap)                │   │
│  │  • Thread-safe LOB aggregation                        │   │
│  │  • Bid/Ask spread calculation                         │   │
│  │  • Liquidity depth analysis                           │   │
│  │  • VWAP computation                                   │   │
│  └──────────────────────────────────────────────────────┘   │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────────┐
│                   Exchange Layer                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │   Binance   │  │    Bybit    │  │     OKX     │         │
│  │  WebSocket  │  │  WebSocket  │  │  WebSocket  │         │
│  │  REST API   │  │  REST API   │  │  REST API   │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

### 1. Market Data Ingestion

```
Exchange WS → Message Parser → OrderBook Update → DashMap Store
                     │
                     └→ Latency Metrics → Prometheus
```

**Optimization**: Zero-copy parsing, lock-free updates

### 2. Opportunity Detection

```
Orderbook Manager → Cross-Venue Scanner → Fee Calculation
                           │
                           ├→ Slippage Estimation
                           ├→ Liquidity Check
                           └→ Profitability Filter
                                    │
                                    └→ Valid Opportunities
```

**Frequency**: 10 scans per second per symbol (100ms interval)

### 3. Trade Execution

```
Opportunity → Risk Check → Execution Plan → Parallel Order Placement
                 │              │                    │
                 │              │                    ├→ Buy Exchange
                 │              │                    └→ Sell Exchange
                 │              │
                 ├→ Position Limits         Order Fill Confirmation
                 ├→ Exposure Limits                  │
                 └→ PnL Limits                       └→ PnL Update
```

### 4. Risk Management Flow

```
Pre-Trade → Position Size Check
         → Exposure Check
         → Daily Loss Check
         → Drawdown Check
              │
              ├→ PASS → Execute
              └→ FAIL → Reject + Log
```

## Threading Model

### Main Threads

1. **WebSocket Receivers** (per exchange)
   - Dedicated thread per exchange connection
   - Handles message deserialization
   - Updates shared orderbook state

2. **Arbitrage Scanner**
   - Periodic scan across all symbols
   - Lock-free reads from orderbook
   - Publishes opportunities to GUI

3. **Execution Engine**
   - Parallel order placement
   - Async HTTP requests
   - Result aggregation

4. **GUI Event Loop**
   - Tauri main thread
   - Handles user interactions
   - Renders UI updates

### Concurrency Primitives

- **DashMap**: Lock-free concurrent hashmap for orderbooks
- **RwLock**: Read-write locks for config/limits
- **Arc**: Shared ownership for managers
- **Channels**: Message passing between threads

## Memory Layout

### Hot Path Optimization

```rust
// OrderBook stored inline, no heap allocations on update
struct OrderBook {
    bids: Vec<PriceLevel>,  // Pre-allocated capacity
    asks: Vec<PriceLevel>,  // Pre-allocated capacity
    timestamp_ns: u64,      // Stack-allocated
    sequence: u64,          // Stack-allocated
}
```

### Cache Efficiency

- Order book levels sorted for fast best bid/ask access
- Recent opportunities cached for GUI rendering
- Metrics buffered before flush

## Performance Characteristics

### Latency Breakdown (Target)

| Operation | Latency | Notes |
|-----------|---------|-------|
| WS message arrival | 0ns | Network-dependent |
| Message parsing | 1-10μs | JSON deserialization |
| Orderbook update | 0.1-1μs | DashMap insert |
| Opportunity scan | 100-500μs | Cross-venue comparison |
| Risk check | 10-50μs | Limit validation |
| Order placement | 5-20ms | Network + exchange |

### Memory Usage

- Base system: ~100 MB
- Per orderbook: ~10 KB (100 levels)
- 10 symbols × 3 exchanges = ~300 KB orderbook data
- GUI renderer: ~50 MB

### CPU Usage

- Idle: <1% (waiting on I/O)
- Active trading: 5-15% (1 core)
- Peak: 30-40% (rapid order placement)

## Error Handling Strategy

### Exchange Errors

```rust
WebSocket Error → Reconnect with exponential backoff
                   │
                   ├→ Max 5 retries
                   └→ Alert if persistent
```

### Execution Errors

```rust
Order Failed → Cancel opposite leg
            → Update PnL
            → Log for review
```

### Risk Violations

```rust
Limit Breached → Halt trading (circuit breaker)
              → Send alert
              → Wait for manual intervention
```

## Configuration Management

### Hierarchical Config

1. **Compile-time**: Type definitions, constants
2. **File-based**: `config/default.toml`
3. **Environment**: `.env` for secrets
4. **Runtime**: GUI adjustments

### Hot Reload

- Risk limits: Yes (via GUI)
- Exchange credentials: No (requires restart)
- Trading pairs: No (requires restart)

## Monitoring & Observability

### Metrics Collection

```rust
Orderbook Update → Record latency
Opportunity Found → Increment counter
Trade Executed → Update histogram
```

**Export**: Prometheus format on `:9090/metrics`

### Logging Levels

- **ERROR**: System failures, critical issues
- **WARN**: Risk violations, reconnections
- **INFO**: Trade executions, opportunities
- **DEBUG**: Detailed flow, orderbook updates
- **TRACE**: Per-message logging (dev only)

## Security Architecture

### API Key Management

- Keys stored in environment variables
- Never logged or displayed in GUI
- Separate read/trade permissions

### Network Security

- TLS for all exchange connections
- API signature verification
- Rate limit compliance

### Operational Security

- No remote access by default
- Local GUI only
- Optional webhook notifications (HTTPS only)

## Scalability Considerations

### Horizontal Scaling

**Not Applicable** - Single-instance design for:
- Low-latency requirement (no network hops)
- State consistency (shared orderbook)
- Exchange rate limits (per API key)

### Vertical Scaling

- Add more symbols: Linear memory growth
- Add more exchanges: Linear CPU growth
- More orderbook depth: Linear memory growth

**Limits**:
- ~50 symbols per instance (CPU-bound)
- ~10 exchanges per instance (WebSocket limit)

## Future Enhancements

### Planned Optimizations

1. **NUMA-aware allocation**: Pin memory to CPU socket
2. **Kernel bypass networking**: DPDK for sub-microsecond latency
3. **Hardware timestamping**: NIC-level packet timestamps
4. **JIT compilation**: Runtime code generation for hot paths

### Architectural Extensions

1. **Multi-account**: Run multiple strategies in parallel
2. **Distributed orderbook**: Shared state across instances
3. **ML integration**: Predictive opportunity filtering
4. **FIX protocol**: Direct exchange connectivity

---

## Development Guidelines

### Adding a New Exchange

1. Create `crates/exchanges/src/{exchange}.rs`
2. Implement `ExchangeAPI` trait
3. Add WebSocket message handlers
4. Update `FeeManager` with fee structure
5. Add tests for API calls and parsing

### Modifying Risk Logic

1. Update `RiskLimits` struct if adding new limits
2. Implement check in `RiskManager::can_trade()`
3. Add corresponding GUI controls
4. Update documentation

### Performance Testing

```bash
# Benchmark critical paths
cargo bench

# Profile with perf
cargo build --release
perf record -g ./target/release/arbitrage-bot
perf report
```

---

**Architecture Last Updated**: 2025-11-22
