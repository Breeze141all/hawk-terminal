use crate::adapter::AdapterError;
use crate::trades::cache;
use crate::{Ticker, TickerInfo, Trade};
use chrono::NaiveDate;
use rustc_hash::FxHashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Freshness cooldown duration for active candle tip queries in milliseconds.
/// When a REST query reaches the current tip of market trades (< 1000 trades returned),
/// subsequent queries for that ticker at or beyond the tip within this cooldown window
/// return an empty batch immediately to prevent rate limit weight exhaustion.
pub const ACTIVE_TIP_COOLDOWN_MS: u64 = 2000;

type TradeLockMap = FxHashMap<(Ticker, NaiveDate), Arc<tokio::sync::Mutex<()>>>;

/// Shared coordinator for trades data broker.
/// Coordinates concurrent trade fetches across multiple panels and handles:
/// 1. Per-instrument mutual exclusion on intraday fetches to eliminate duplicate REST queries.
/// 2. Active candle tip freshness cache with cooldown.
/// 3. In-flight single-flight request coalescing for identical query parameters.
pub struct TradeCoordinator {
    pub(crate) locks: Mutex<TradeLockMap>,
    tip_cache: Mutex<FxHashMap<Ticker, TipFreshness>>,
}

#[derive(Clone, Copy, Debug)]
struct TipFreshness {
    last_poll: Instant,
    last_trade_t: u64,
}

impl Default for TradeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeCoordinator {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(FxHashMap::default()),
            tip_cache: Mutex::new(FxHashMap::default()),
        }
    }

    /// Prune lock entries older than 2 days from current_date.
    pub fn prune_old_locks(&self, current_date: NaiveDate) {
        let mut map = self.locks.lock().unwrap();
        map.retain(|&(_, date), _| current_date.signed_duration_since(date).num_days().abs() <= 2);
    }

    /// Retrieve or create the mutex for a given ticker and date.
    pub fn get_lock(&self, ticker: Ticker, date: NaiveDate) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.locks.lock().unwrap();
        map.entry((ticker, date))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Check if active candle tip was queried within cooldown for this ticker.
    pub fn is_tip_fresh(&self, ticker: Ticker, from_time: u64) -> bool {
        let now_ms = crate::clock::server_time_ms(Some(ticker.exchange));
        if from_time < now_ms.saturating_sub(120_000) {
            return false;
        }
        let map = self.tip_cache.lock().unwrap();
        map.get(&ticker).is_some_and(|freshness| {
            from_time >= freshness.last_trade_t
                && freshness.last_poll.elapsed() < Duration::from_millis(ACTIVE_TIP_COOLDOWN_MS)
        })
    }

    /// Record that the active candle tip was queried.
    pub fn record_tip_poll(&self, ticker: Ticker, from_time: u64, last_trade_t: Option<u64>) {
        let mut map = self.tip_cache.lock().unwrap();
        map.insert(
            ticker,
            TipFreshness {
                last_poll: Instant::now(),
                last_trade_t: last_trade_t.unwrap_or(from_time),
            },
        );
    }

    /// Clear tip freshness cache (for testing or manual cache invalidate).
    pub fn clear_tip_cache(&self) {
        let mut map = self.tip_cache.lock().unwrap();
        map.clear();
    }

    /// High-level coordinated intraday fetch.
    /// Serializes concurrent fetches for the same instrument on the same day,
    /// checks tip freshness cooldown, re-checks shared cache before invoking adapter,
    /// and records tip freshness upon reaching the active edge.
    pub async fn fetch_intraday<F, Fut>(
        &self,
        ticker_info: TickerInfo,
        from_time: u64,
        data_path: PathBuf,
        today_midnight: u64,
        dispatch_fn: F,
    ) -> Result<(Vec<Trade>, u64), AdapterError>
    where
        F: FnOnce(TickerInfo, u64, PathBuf) -> Fut,
        Fut: Future<Output = Result<(Vec<Trade>, u64), AdapterError>>,
    {
        let ticker = ticker_info.ticker;
        let today_date = chrono::DateTime::from_timestamp_millis(today_midnight as i64)
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());

        self.prune_old_locks(today_date);
        let lock = self.get_lock(ticker, today_date);
        let _guard = lock.lock().await;

        // 1. Cache fast-path check: another panel may have populated the cache while we waited
        let cached_trades =
            cache::load_intraday_trades_from_cache(&data_path, &ticker_info, today_date);
        let max_cached = cached_trades
            .as_ref()
            .and_then(|c| c.last().map(|t| t.time));
        if let Some(cached) = cached_trades
            && !cached.is_empty()
        {
            let t_last = cached.last().unwrap().time;
            if from_time <= t_last {
                let start_idx = cached.partition_point(|t| t.time < from_time);
                let in_gap = start_idx < cached.len()
                    && cached[start_idx].time.saturating_sub(from_time)
                        > cache::INTRADAY_MAX_GAP_MS;
                if !in_gap {
                    return Ok(cache::slice_cached_trades(
                        &cached,
                        start_idx,
                        cache::INTRADAY_MAX_GAP_MS,
                        1000,
                    ));
                }
            }
        }

        // 2. Tip cooldown check: if active candle tip was queried recently, return empty batch
        if self.is_tip_fresh(ticker, from_time) {
            let fallback_next = from_time.saturating_add(cache::INTRADAY_MAX_GAP_MS);
            return Ok((Vec::new(), fallback_next));
        }

        // 3. Dispatch to exchange adapter under the per-ticker lock
        let (trades, next_from) = dispatch_fn(ticker_info, from_time, data_path).await?;

        // 4. Record tip freshness if returned batch indicates end of trades (< 1000 trades)
        if trades.len() < 1000 {
            let is_gap = max_cached.is_some_and(|max_t| next_from <= max_t);
            let now_ms = crate::clock::server_time_ms(Some(ticker.exchange));
            let t = trades.last().map(|t| t.time).unwrap_or(from_time);
            let is_near_tip = t >= now_ms.saturating_sub(120_000);
            if !is_gap && is_near_tip {
                let last_trade_t = trades.last().map(|t| t.time);
                self.record_tip_poll(ticker, from_time, last_trade_t);
            }
        }

        Ok((trades, next_from))
    }
}

pub static COORDINATOR: LazyLock<TradeCoordinator> = LazyLock::new(TradeCoordinator::new);

/// Generic single-flight coalescing for in-flight async requests.
pub struct SingleFlight<K, V> {
    in_flight: Mutex<FxHashMap<K, Arc<tokio::sync::watch::Sender<Option<V>>>>>,
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> Default for SingleFlight<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> SingleFlight<K, V> {
    pub fn new() -> Self {
        Self {
            in_flight: Mutex::new(FxHashMap::default()),
        }
    }

    pub async fn execute<F, Fut>(&self, key: K, make_fut: F) -> V
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = V>,
    {
        let mut make_fut = Some(make_fut);
        loop {
            let (rx, is_leader) = {
                let mut map = self.in_flight.lock().unwrap();
                if let Some(tx) = map.get(&key) {
                    (tx.subscribe(), false)
                } else {
                    let (tx, rx) = tokio::sync::watch::channel(None);
                    map.insert(key.clone(), Arc::new(tx));
                    (rx, true)
                }
            };

            if !is_leader {
                let mut rx = rx;
                if rx.borrow().is_none() {
                    let _ = rx.wait_for(|v| v.is_some()).await;
                }
                if let Some(val) = rx.borrow().as_ref() {
                    return val.clone();
                }
                // Leader dropped or channel closed without a result; retry to become leader or follow next leader.
                continue;
            }

            // RAII guard ensuring in-flight entry cleanup on task abort or panic
            struct Guard<'a, K: Eq + std::hash::Hash + Clone, V> {
                flight: &'a SingleFlight<K, V>,
                key: K,
                completed: bool,
            }

            impl<'a, K: Eq + std::hash::Hash + Clone, V> Drop for Guard<'a, K, V> {
                fn drop(&mut self) {
                    if !self.completed {
                        let mut map = self.flight.in_flight.lock().unwrap();
                        map.remove(&self.key);
                    }
                }
            }

            let mut guard = Guard {
                flight: self,
                key: key.clone(),
                completed: false,
            };

            let fut = make_fut
                .take()
                .expect("make_fut must be available when becoming leader")();
            let result = fut.await;
            guard.completed = true;

            let mut map = self.in_flight.lock().unwrap();
            if let Some(tx) = map.remove(&key) {
                let _ = tx.send(Some(result.clone()));
            }

            return result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Exchange;
    use crate::util::Price;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_single_flight_coalescing() {
        let sf = Arc::new(SingleFlight::<String, u32>::new());
        let call_count = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..5 {
            let sf_clone = sf.clone();
            let count_clone = call_count.clone();
            handles.push(tokio::spawn(async move {
                sf_clone
                    .execute("BTCUSDT".to_string(), move || async move {
                        count_clone.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        42
                    })
                    .await
            }));
        }

        for h in handles {
            let val = h.await.unwrap();
            assert_eq!(val, 42);
        }

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_tip_freshness_cooldown() {
        let coordinator = TradeCoordinator::new();
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let now_ms = crate::clock::server_time_ms(Some(ticker.exchange));
        let tip_time = now_ms.saturating_sub(10_000);

        assert!(!coordinator.is_tip_fresh(ticker, tip_time));

        coordinator.record_tip_poll(ticker, tip_time, Some(tip_time + 500));
        assert!(coordinator.is_tip_fresh(ticker, tip_time + 500));
        assert!(coordinator.is_tip_fresh(ticker, tip_time + 600));
        assert!(!coordinator.is_tip_fresh(ticker, tip_time + 400)); // older range not suppressed
    }

    #[tokio::test]
    async fn test_intraday_coordination_serves_from_cache_sequentially() {
        let coordinator = TradeCoordinator::new();
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let ticker_info = TickerInfo::new(ticker, 0.1, 0.001, None);

        let temp_dir =
            std::env::temp_dir().join(format!("hawk_coord_test_{}", uuid::Uuid::new_v4()));
        let base_t = 1_000_000u64;

        let dispatch_counter = Arc::new(AtomicUsize::new(0));

        // First call: fetches and populates cache
        let counter1 = dispatch_counter.clone();
        let (batch1, next1) = coordinator
            .fetch_intraday(
                ticker_info,
                base_t,
                temp_dir.clone(),
                base_t,
                move |ti, from, path| {
                    let c = counter1.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        let trades = vec![Trade {
                            time: from,
                            price: Price::from_f32(50000.0),
                            qty: 1.0,
                            is_sell: false,
                        }];
                        let today_date = chrono::DateTime::from_timestamp_millis(from as i64)
                            .unwrap()
                            .date_naive();
                        let _ = cache::save_intraday_trades_to_cache_buffered(
                            &path, &ti, today_date, &trades,
                        );
                        let next = from + 1000;
                        Ok((trades, next))
                    }
                },
            )
            .await
            .unwrap();

        assert_eq!(batch1.len(), 1);
        assert_eq!(next1, base_t + 1000);
        assert_eq!(dispatch_counter.load(Ordering::SeqCst), 1);

        // Second call from a concurrent panel for the same instrument / time:
        // Must be served from cache without invoking dispatch_fn!
        let counter2 = dispatch_counter.clone();
        let (batch2, _) = coordinator
            .fetch_intraday(
                ticker_info,
                base_t,
                temp_dir.clone(),
                base_t,
                move |_, _, _| {
                    let c = counter2.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok((Vec::new(), base_t))
                    }
                },
            )
            .await
            .unwrap();

        assert_eq!(batch2.len(), 1);
        // Dispatch counter must still be 1 (second call was completely intercepted by cache)
        assert_eq!(dispatch_counter.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_multi_panel_1h_and_15m_concurrent_active_candle_deduplication() {
        let coordinator = Arc::new(TradeCoordinator::new());
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let ticker_info = TickerInfo::new(ticker, 0.1, 0.001, None);

        let temp_dir =
            std::env::temp_dir().join(format!("hawk_multi_panel_{}", uuid::Uuid::new_v4()));
        let now_ms = crate::clock::server_time_ms(Some(ticker_info.exchange()));
        let tip_time = now_ms.saturating_sub(5_000); // 5s before now (near tip)
        let t_15m = tip_time.saturating_sub(30_000);
        let t_1h = t_15m.saturating_sub(45 * 60 * 1000);
        let today_midnight = t_1h.saturating_sub(60_000);

        // Mock trades for 14:00..14:45 (part 1) and 14:45..14:45:30 (part 2 - active 15m candle)
        let trades_1h_head = vec![
            Trade {
                time: t_1h + 10_000,
                price: Price::from_f32(65000.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: t_1h + 30 * 1000,
                price: Price::from_f32(65100.0),
                qty: 2.0,
                is_sell: true,
            },
        ];
        let trades_15m_active = vec![
            Trade {
                time: t_15m + 10_000,
                price: Price::from_f32(65200.0),
                qty: 1.5,
                is_sell: false,
            },
            Trade {
                time: tip_time,
                price: Price::from_f32(65250.0),
                qty: 3.0,
                is_sell: true,
            },
        ];

        let rest_calls = Arc::new(AtomicUsize::new(0));

        let coord1 = coordinator.clone();
        let dir1 = temp_dir.clone();
        let rest1 = rest_calls.clone();
        let trades_15m_clone = trades_15m_active.clone();
        let h_15m = tokio::spawn(async move {
            coord1
                .fetch_intraday(
                    ticker_info,
                    t_15m,
                    dir1.clone(),
                    today_midnight,
                    move |ti, from, path| {
                        let calls = rest1.clone();
                        let trades = trades_15m_clone.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            let date = chrono::DateTime::from_timestamp_millis(from as i64)
                                .unwrap()
                                .date_naive();
                            let _ = cache::save_intraday_trades_to_cache_buffered(
                                &path, &ti, date, &trades,
                            );
                            Ok((trades, tip_time + 1))
                        }
                    },
                )
                .await
        });

        let coord2 = coordinator.clone();
        let dir2 = temp_dir.clone();
        let rest2 = rest_calls.clone();
        let trades_1h_clone = trades_1h_head.clone();
        let h_1h = tokio::spawn(async move {
            coord2
                .fetch_intraday(
                    ticker_info,
                    t_1h,
                    dir2.clone(),
                    today_midnight,
                    move |ti, from, path| {
                        let calls = rest2.clone();
                        let trades = trades_1h_clone.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            let date = chrono::DateTime::from_timestamp_millis(from as i64)
                                .unwrap()
                                .date_naive();
                            let _ = cache::save_intraday_trades_to_cache_buffered(
                                &path, &ti, date, &trades,
                            );
                            Ok((trades, t_15m))
                        }
                    },
                )
                .await
        });

        let (res_15m, res_1h) = tokio::join!(h_15m, h_1h);
        let batch_15m = res_15m.unwrap().unwrap().0;
        let batch_1h = res_1h.unwrap().unwrap().0;

        assert!(!batch_15m.is_empty(), "15m panel must receive trades");
        assert!(!batch_1h.is_empty(), "1h panel must receive trades");

        // Now panel 1h advances to t_15m (the active 15m candle)
        // Since 15m already fetched it into shared cache, 1h must be served from cache without any new REST call!
        let rest3 = rest_calls.clone();
        let (batch_1h_second, _) = coordinator
            .fetch_intraday(
                ticker_info,
                t_15m,
                temp_dir.clone(),
                today_midnight,
                move |_, _, _| {
                    let calls = rest3.clone();
                    async move {
                        calls.fetch_add(100, Ordering::SeqCst); // Must not be called!
                        Ok((Vec::new(), 0))
                    }
                },
            )
            .await
            .unwrap();

        assert_eq!(
            batch_1h_second.len(),
            2,
            "1h panel should get the 2 active 15m trades from shared cache"
        );
        assert_eq!(
            rest_calls.load(Ordering::SeqCst),
            2,
            "Total REST calls must not exceed 2 (one for 1h head, one for 15m active)"
        );

        // Now both panels query at the tip (tip_time + 1)
        // Both should be intercepted by tip cooldown with 0 REST calls!
        let (batch_tip1, _) = coordinator
            .fetch_intraday(
                ticker_info,
                tip_time + 1,
                temp_dir.clone(),
                today_midnight,
                |_, _, _| async {
                    panic!("Must not query REST at tip within cooldown");
                },
            )
            .await
            .unwrap();
        assert!(batch_tip1.is_empty());

        let (batch_tip2, _) = coordinator
            .fetch_intraday(
                ticker_info,
                tip_time + 1,
                temp_dir.clone(),
                today_midnight,
                |_, _, _| async {
                    panic!("Must not query REST at tip within cooldown");
                },
            )
            .await
            .unwrap();
        assert!(batch_tip2.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_intraday_gap_fetch_under_1000_trades_does_not_trigger_tip_cooldown() {
        let coordinator = TradeCoordinator::new();
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let ticker_info = TickerInfo::new(ticker, 0.1, 0.001, None);

        let temp_dir =
            std::env::temp_dir().join(format!("hawk_coord_gap_{}", uuid::Uuid::new_v4()));
        let today_midnight = 1_000_000u64;
        let today_date = chrono::DateTime::from_timestamp_millis(today_midnight as i64)
            .unwrap()
            .date_naive();

        // 1. Populate cache with a trade far in the day (e.g. 5_000_000)
        let end_trade = vec![Trade {
            time: 5_000_000,
            price: Price::from_f32(50000.0),
            qty: 1.0,
            is_sell: false,
        }];
        cache::save_intraday_trades_to_cache_buffered(
            &temp_dir,
            &ticker_info,
            today_date,
            &end_trade,
        )
        .unwrap();

        // 2. Fetch a historical gap at 2_000_000 with only 1 trade (< 1000) and next_from = 2_060_000 <= max_cached (5_000_000)
        let (gap_trades, next_from) = coordinator
            .fetch_intraday(
                ticker_info,
                2_000_000,
                temp_dir.clone(),
                today_midnight,
                |_, from, _| async move {
                    let trades = vec![Trade {
                        time: from,
                        price: Price::from_f32(49900.0),
                        qty: 0.5,
                        is_sell: true,
                    }];
                    Ok((trades, from + 60_000))
                },
            )
            .await
            .unwrap();

        assert_eq!(gap_trades.len(), 1);
        assert_eq!(next_from, 2_060_000);

        // 3. Since next_from <= max_cached (2_060_000 <= 5_000_000), tip freshness MUST NOT be recorded!
        assert!(
            !coordinator.is_tip_fresh(ticker, 2_060_000),
            "Gap fetch before max_cached must not register tip freshness"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_empty_cache_historical_trades_under_1000_does_not_trigger_tip_cooldown() {
        let coordinator = TradeCoordinator::new();
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let ticker_info = TickerInfo::new(ticker, 0.1, 0.001, None);

        let temp_dir =
            std::env::temp_dir().join(format!("hawk_coord_empty_cache_{}", uuid::Uuid::new_v4()));
        let now_ms = crate::clock::server_time_ms(Some(ticker.exchange));
        // Historical query 5 hours ago (midnight or early morning)
        let historical_t = now_ms.saturating_sub(5 * 3600 * 1000);
        let today_midnight = historical_t.saturating_sub(3600 * 1000);

        // Fetch historical trades when cache is completely empty (max_cached is None)
        let (batch, next_from) = coordinator
            .fetch_intraday(
                ticker_info,
                historical_t,
                temp_dir.clone(),
                today_midnight,
                |_, from, _| async move {
                    // Small batch of 5 trades earlier in the day
                    let trades = vec![Trade {
                        time: from,
                        price: Price::from_f32(50000.0),
                        qty: 1.0,
                        is_sell: false,
                    }];
                    Ok((trades, from + 60_000))
                },
            )
            .await
            .unwrap();

        assert_eq!(batch.len(), 1);
        assert_eq!(next_from, historical_t + 60_000);

        // Must NOT register tip freshness because historical_t is far from current market tip
        assert!(
            !coordinator.is_tip_fresh(ticker, historical_t + 60_000),
            "Historical fetch on empty cache must not trigger tip freshness cooldown"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_single_flight_leader_drop_recovery() {
        let flight = Arc::new(SingleFlight::<String, u32>::new());
        let flight_clone1 = flight.clone();
        let flight_clone2 = flight.clone();

        let started = Arc::new(tokio::sync::Notify::new());
        let started_clone = started.clone();

        let leader_handle = tokio::spawn(async move {
            flight_clone1
                .execute("test_key".to_string(), || async move {
                    started_clone.notify_waiters();
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    100u32
                })
                .await
        });

        started.notified().await;

        let follower_handle = tokio::spawn(async move {
            flight_clone2
                .execute("test_key".to_string(), || async move { 200u32 })
                .await
        });

        // Abort the leader task
        leader_handle.abort();

        let result = follower_handle.await.expect("Follower task should finish");
        assert_eq!(
            result, 200,
            "Follower must recover and execute when leader drops"
        );
    }

    #[test]
    fn test_ex09_lock_pruning() {
        let coordinator = TradeCoordinator::new();
        let ticker = Ticker::new("BTCUSDT", crate::Exchange::BinanceLinear);
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let day_minus_5 = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let day_minus_1 = chrono::NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();

        let _lock_old = coordinator.get_lock(ticker, day_minus_5);
        let _lock_recent = coordinator.get_lock(ticker, day_minus_1);
        let _lock_today = coordinator.get_lock(ticker, today);

        assert_eq!(coordinator.locks.lock().unwrap().len(), 3);

        // Prune entries older than 2 days
        coordinator.prune_old_locks(today);

        let map = coordinator.locks.lock().unwrap();
        assert!(map.contains_key(&(ticker, day_minus_1)));
        assert!(map.contains_key(&(ticker, today)));
        assert!(!map.contains_key(&(ticker, day_minus_5)));
    }
}
