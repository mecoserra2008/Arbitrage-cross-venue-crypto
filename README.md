# Multi-Venue Arbitrage Trading Bot

An institutional-grade arbitrage trading system for cryptocurrency perpetual futures with nanosecond precision, built in Rust.

## Features

### Core Capabilities
- **Multi-Venue Trading**: Simultaneous connectivity to multiple exchanges (Binance, Bybit, OKX, and more)
- **Nanosecond Precision**: High-performance timing and latency tracking for ultra-low latency execution
- **Perpetual Futures**: Focus on perpetual swap contracts across major assets
- **Real-time Order Book**: Live LOB aggregation and analysis from multiple exchanges
- **Advanced Analytics**: Bid-ask spread tracking, liquidity depth analysis, VWAP calculation

### Risk Management
- **Position Limits**: Configurable per-position and total exposure limits
- **PnL Tracking**: Real-time profit/loss monitoring with detailed trade history
- **Circuit Breakers**: Automatic trading halt on breach of risk thresholds
- **Drawdown Protection**: Automatic shutdown at maximum drawdown levels
- **Fee Optimization**: Exchange-specific fee structures and calculations

### Arbitrage Engine
- **Opportunity Detection**: Real-time scanning for cross-venue price discrepancies
- **Slippage Estimation**: Advanced order book analysis for execution cost prediction
- **Profitability Analysis**: Fee-adjusted profit calculation in basis points
- **Smart Execution**: Simultaneous order placement across multiple venues

### GUI Dashboard
- **Real-time Monitoring**: Live view of opportunities, positions, and PnL
- **Risk Controls**: Interactive risk limit adjustment
- **Order Book Visualization**: Multi-exchange price level comparison
- **Performance Metrics**: Win rate, average profit, volume statistics

## Architecture

```
arbitrage-bot/
├── crates/
│   ├── core/           # Core types, orderbook, timing, metrics
│   ├── exchanges/      # Exchange API connectors (Binance, Bybit, OKX)
│   ├── arbitrage/      # Opportunity detection and execution
│   ├── risk/           # Risk management and PnL tracking
│   └── gui/            # Tauri-based desktop GUI
├── config/             # Configuration files
└── src/                # Main CLI application
```

### Technology Stack
- **Language**: Rust (for performance and safety)
- **Async Runtime**: Tokio (for concurrent WebSocket and API operations)
- **GUI Framework**: Tauri (native desktop app with web UI)
- **WebSockets**: tokio-tungstenite (real-time market data)
- **HTTP Client**: reqwest (REST API calls)
- **Concurrency**: DashMap, parking_lot (lock-free data structures)
- **Metrics**: metrics + Prometheus exporter

## Installation

### Prerequisites
- Rust 1.70+ ([rustup.rs](https://rustup.rs))
- Node.js 18+ (for GUI frontend)
- PostgreSQL (optional, for trade history)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/arbitrage-bot.git
cd arbitrage-bot

# Build all components
cargo build --release

# Build GUI application
cd crates/gui
cargo build --release
```

## Configuration

### 1. Environment Variables

Copy `.env.example` to `.env` and fill in your API credentials:

```bash
cp .env.example .env
```

**Important**: Never commit your `.env` file with real API keys!

### 2. Configuration File

Edit `config/default.toml` to customize trading parameters:

- **Trading pairs**: Add/remove symbols to monitor
- **Exchanges**: Enable/disable specific exchanges
- **Risk limits**: Set position sizes, max loss, drawdown limits
- **Arbitrage settings**: Minimum profit threshold, slippage tolerance

## Usage

### CLI Mode

Run the basic arbitrage scanner:

```bash
cargo run --release
```

### GUI Mode

Launch the desktop application:

```bash
cd crates/gui
cargo run --release
```

The GUI provides:
- Real-time dashboard with live opportunities
- Position management
- PnL tracking and analytics
- Risk limit controls
- Circuit breaker management

### API Endpoints

The system exposes Tauri commands for the frontend:
- `get_dashboard_data`: Fetch current state
- `toggle_trading`: Enable/disable automated trading
- `update_risk_limits`: Modify risk parameters
- `reset_circuit_breaker`: Resume trading after halt

## Performance Characteristics

### Latency Targets
- **WebSocket processing**: < 100 microseconds
- **Opportunity detection**: < 1 millisecond
- **Order execution**: < 10 milliseconds (network-dependent)

### Throughput
- **Order book updates**: 10,000+ per second
- **Concurrent WebSockets**: 50+ streams
- **Arbitrage scans**: 10 per second per symbol

## Risk Management

### Default Limits
- Max position size: $10,000 per trade
- Max total exposure: $500,000
- Max daily loss: $5,000
- Max drawdown: 10%
- Min profit: 5 basis points (0.05%)

### Circuit Breakers
Trading automatically halts when:
- Daily loss exceeds limit
- Drawdown exceeds threshold
- API errors exceed retry limit
- Manual intervention via GUI

## Fee Structures

| Exchange | Maker Fee | Taker Fee |
|----------|-----------|-----------|
| Binance  | 0.02%     | 0.04%     |
| Bybit    | 0.01%     | 0.06%     |
| OKX      | 0.02%     | 0.05%     |
| Deribit  | -0.00%    | 0.05%     |

*Note: Fees are for VIP 0 tier. Update in `crates/exchanges/src/fees.rs` for your tier.*

## Monitoring

### Prometheus Metrics

Exposed on `:9090/metrics`:
- `orderbook_update_latency_ns`: Orderbook processing time
- `websocket_message_latency_ns`: End-to-end message latency
- `arbitrage_opportunities_total`: Count of detected opportunities
- `arbitrage_profit_bps`: Profit distribution
- `trade_executions_total`: Execution success/failure counts
- `position_quantity`: Current position sizes
- `position_unrealized_pnl`: Unrealized profit/loss

### Logging

Structured logging with tracing:
```bash
RUST_LOG=debug cargo run  # Verbose logging
RUST_LOG=info cargo run   # Standard logging
```

## Development

### Running Tests

```bash
# Run all tests
cargo test --all

# Run specific crate tests
cargo test -p arbitrage-core
cargo test -p arbitrage-engine
```

### Adding New Exchanges

1. Implement `ExchangeAPI` trait in `crates/exchanges/src/`
2. Add WebSocket message parsing
3. Update fee structure in `fees.rs`
4. Add to `Exchange` enum in `core/types.rs`

### Code Structure

- **arbitrage-core**: Platform-agnostic types and utilities
- **arbitrage-exchanges**: Exchange-specific API implementations
- **arbitrage-engine**: Arbitrage logic and execution
- **arbitrage-risk**: Risk management and PnL tracking
- **arbitrage-gui**: Desktop application

## Security Considerations

⚠️ **Important Security Notes**:

1. **API Keys**: Never commit API keys. Use environment variables.
2. **Permissions**: Use read-only keys for monitoring, trade permissions only when needed.
3. **IP Whitelisting**: Enable on exchanges for additional security.
4. **Position Limits**: Start with small limits to test the system.
5. **Testnet**: Use exchange testnets before live trading.

## Disclaimer

**This software is for educational purposes only.**

- Cryptocurrency trading carries substantial risk of loss.
- Past performance does not guarantee future results.
- The developers assume no liability for financial losses.
- Test thoroughly before deploying with real capital.
- Ensure compliance with local regulations.

## Performance Tuning

### For Ultra-Low Latency

1. **CPU Affinity**: Pin critical threads to dedicated cores
2. **NUMA Awareness**: Allocate memory on same node as processing
3. **Network**: Use kernel bypass (DPDK) for sub-microsecond latency
4. **Compiler Flags**: Enable LTO and CPU-specific optimizations

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### Production Deployment

1. Run on collocated servers near exchange data centers
2. Use dedicated network connections
3. Monitor system metrics (CPU, memory, network)
4. Set up alerting for anomalies
5. Implement graceful shutdown handling

## Roadmap

- [ ] Complete Bybit and OKX API implementations
- [ ] Add support for spot markets
- [ ] Implement triangular arbitrage
- [ ] Add backtesting framework
- [ ] Machine learning for profit prediction
- [ ] Mobile app for monitoring
- [ ] Multi-account support
- [ ] Telegram/Discord alerts

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure `cargo fmt` and `cargo clippy` pass
5. Submit a pull request

## License

MIT License - see LICENSE file for details

## Support

For questions or issues:
- GitHub Issues: Report bugs and feature requests
- Documentation: See `/docs` for detailed guides

## Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/)
- [Tokio](https://tokio.rs/)
- [Tauri](https://tauri.app/)
- [rust_decimal](https://github.com/paupino/rust-decimal)

---

**Happy Trading! 🚀📈**
