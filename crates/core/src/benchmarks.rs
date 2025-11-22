/// Performance benchmarks for critical operations
#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;

    const ITERATIONS: usize = 10_000;

    #[test]
    fn bench_orderbook_update() {
        let mgr = crate::OrderBookManager::new();
        let snapshot = create_test_snapshot();

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            mgr.update(snapshot.clone());
        }
        let elapsed = start.elapsed();

        let avg_ns = elapsed.as_nanos() / ITERATIONS as u128;
        println!("Orderbook update: {} ns/op", avg_ns);

        // Target: < 500ns per update
        assert!(avg_ns < 500, "Orderbook update too slow: {}ns", avg_ns);
    }

    #[test]
    fn bench_best_bid_ask() {
        let mgr = crate::OrderBookManager::new();
        let snapshot = create_test_snapshot();
        mgr.update(snapshot.clone());

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = mgr.get_best_bid(snapshot.exchange, &snapshot.symbol);
            let _ = mgr.get_best_ask(snapshot.exchange, &snapshot.symbol);
        }
        let elapsed = start.elapsed();

        let avg_ns = elapsed.as_nanos() / (ITERATIONS * 2) as u128;
        println!("Best bid/ask: {} ns/op", avg_ns);

        // Target: < 50ns per lookup
        assert!(avg_ns < 50, "Best bid/ask too slow: {}ns", avg_ns);
    }

    #[test]
    fn bench_validation_quick() {
        use crate::optimized::FastValidator;

        let now = crate::Timestamp::now().as_nanos();

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = FastValidator::is_fresh(now, now, 1_000_000_000);
        }
        let elapsed = start.elapsed();

        let avg_ns = elapsed.as_nanos() / ITERATIONS as u128;
        println!("Fast validation: {} ns/op", avg_ns);

        // Target: < 10ns per check
        assert!(avg_ns < 10, "Validation too slow: {}ns", avg_ns);
    }

    fn create_test_snapshot() -> crate::OrderBookSnapshot {
        use crate::types::*;
        use rust_decimal_macros::dec;

        OrderBookSnapshot {
            exchange: Exchange::Binance,
            symbol: Symbol::perpetual("BTC", "USDT"),
            bids: vec![
                PriceLevel::new(dec!(50000), dec!(1.0)),
                PriceLevel::new(dec!(49999), dec!(2.0)),
                PriceLevel::new(dec!(49998), dec!(3.0)),
            ],
            asks: vec![
                PriceLevel::new(dec!(50001), dec!(1.0)),
                PriceLevel::new(dec!(50002), dec!(2.0)),
                PriceLevel::new(dec!(50003), dec!(3.0)),
            ],
            timestamp_ns: crate::Timestamp::now().as_nanos(),
            sequence: 1,
        }
    }
}
