use data::aggr::time::TimeSeries;
use data::chart::kline::indicators::{
    IndicatorHost, IndicatorId, IndicatorInstanceStore, IndicatorOutputRef,
    datapoint_to_indicator_frame, initialize_indicator_catalog, kline_to_indicator_frame,
};
use data::chart::kline::{KlineDataPoint, KlineTrades};
use exchange::adapter::Exchange;
use exchange::util::{Price, PriceStep};
use exchange::{Kline, Timeframe, Trade};
use indicator_engine::TimeOrTick;

fn make_candle(time: u64, open: f64, high: f64, low: f64, close: f64, vol: (f32, f32)) -> Kline {
    Kline {
        time,
        open: Price::from_f64(open),
        high: Price::from_f64(high),
        low: Price::from_f64(low),
        close: Price::from_f64(close),
        volume: (f64::from(vol.0), f64::from(vol.1)),
    }
}

// ============================================================================
// 1. Footprint Immutability: Deep 200+ Price Levels & POC Field Comparison
// ============================================================================

#[test]
fn test_adversarial_footprint_cluster_deep_immutability_250_levels() {
    let _ = initialize_indicator_catalog();

    let step = PriceStep::from_f32(0.5);
    let mut footprint = KlineTrades::new();
    let start_time = 1_700_000_000_000u64;

    // Populate 250 distinct price levels with non-trivial buy and sell quantities
    let total_levels = 250usize;
    for i in 0..total_levels {
        let price_f64 = 100.0 + (i as f64) * 0.5;
        let buy_trade = Trade {
            time: start_time + (i as u64 * 10),
            price: Price::from_f64(price_f64),
            qty: 10.0 + (i as f32) * 0.25,
            is_sell: false,
        };
        let sell_trade = Trade {
            time: start_time + (i as u64 * 10) + 5,
            price: Price::from_f64(price_f64),
            qty: 5.0 + (i as f32) * 0.15,
            is_sell: true,
        };
        footprint.add_trade_to_nearest_bin(&buy_trade, step);
        footprint.add_trade_to_nearest_bin(&sell_trade, step);
    }

    // Designate level 125 as massive volume spike to form an unambiguous Point of Control (POC)
    let spike_trade = Trade {
        time: start_time + 5_000,
        price: Price::from_f64(100.0 + 125.0 * 0.5),
        qty: 50_000.0,
        is_sell: false,
    };
    footprint.add_trade_to_nearest_bin(&spike_trade, step);
    footprint.calculate_poc();

    // Verify initial setup
    assert_eq!(footprint.trades.len(), total_levels);
    let initial_poc = footprint.poc.expect("POC must be present");
    assert_eq!(initial_poc.price, Price::from_f64(100.0 + 125.0 * 0.5));
    let initial_first_time = footprint.first_trade_t();
    let initial_last_time = footprint.last_trade_t();
    let initial_summary = footprint.trade_summary().expect("Summary must be present");

    // Deep snapshot of all 250 price bins
    #[derive(Clone, Debug, PartialEq)]
    struct BinSnapshot {
        buy_qty: f64,
        sell_qty: f64,
        buy_count: usize,
        sell_count: usize,
        first_time: u64,
        last_time: u64,
    }

    let mut snapshot: std::collections::BTreeMap<Price, BinSnapshot> =
        std::collections::BTreeMap::new();
    for (price, bin) in &footprint.trades {
        snapshot.insert(
            *price,
            BinSnapshot {
                buy_qty: bin.buy_qty,
                sell_qty: bin.sell_qty,
                buy_count: bin.buy_count,
                sell_count: bin.sell_count,
                first_time: bin.first_time,
                last_time: bin.last_time,
            },
        );
    }
    assert_eq!(snapshot.len(), total_levels);

    // Package into KlineDataPoint
    let dp = KlineDataPoint {
        kline: make_candle(
            start_time,
            100.0,
            100.0 + (total_levels as f64) * 0.5,
            99.0,
            100.0 + 125.0 * 0.5,
            (
                initial_summary.buy_qty as f32,
                initial_summary.sell_qty as f32,
            ),
        ),
        footprint,
        trades_fetched: true,
    };

    // Instantiate multiple indicators in IndicatorInstanceStore
    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let bb_id = IndicatorId::parse("core.bollinger-bands").unwrap();
    let ma_id = IndicatorId::parse("core.moving-averages").unwrap();

    let inst_rsi = store.create(rsi_id);
    let inst_bb = store.create(bb_id);
    let inst_ma = store.create(ma_id);

    let active_indicators = vec![inst_rsi, inst_bb, inst_ma];
    let mut host = IndicatorHost::default();

    // Generate frame
    let frame = datapoint_to_indicator_frame(
        0,
        &dp,
        None,
        TimeOrTick::Time,
        Some(step.to_f32_lossy() as f64),
        true,
    );

    // Execute full recalculation, closed evaluation, and 200 real-time preview ticks
    host.recalculate(&active_indicators, &store, std::slice::from_ref(&frame));
    host.evaluate_closed(&frame);

    for tick in 0..200 {
        let mut volatile_dp = dp.clone();
        volatile_dp.kline.close = Price::from_f64(100.0 + (tick as f64 * 0.3) % 50.0);
        let preview_frame = datapoint_to_indicator_frame(
            0,
            &volatile_dp,
            None,
            TimeOrTick::Time,
            Some(step.to_f32_lossy() as f64),
            true,
        );
        host.preview_open(&preview_frame);
    }

    // ========================================================================
    // DEEP FIELD-BY-FIELD IMMUTABILITY ASSERTIONS ON ORIGINAL FOOTPRINT
    // ========================================================================
    assert_eq!(
        dp.footprint.trades.len(),
        total_levels,
        "Total price levels in KlineTrades must remain exactly 250"
    );

    // 1. POC price, volume, and status immutability
    let post_poc = dp.footprint.poc.expect("Post POC must exist");
    assert_eq!(
        post_poc.price, initial_poc.price,
        "POC price mutated during indicator evaluation!"
    );
    assert_eq!(
        post_poc.volume.to_bits(),
        initial_poc.volume.to_bits(),
        "POC volume bits mutated during indicator evaluation!"
    );
    assert_eq!(
        post_poc.status, initial_poc.status,
        "POC status mutated during indicator evaluation!"
    );

    // 2. Cached timestamps immutability
    assert_eq!(
        dp.footprint.first_trade_t(),
        initial_first_time,
        "first_trade_t mutated!"
    );
    assert_eq!(
        dp.footprint.last_trade_t(),
        initial_last_time,
        "last_trade_t mutated!"
    );

    // 3. Trade aggregate summary bit-for-bit identity
    let post_summary = dp
        .footprint
        .trade_summary()
        .expect("Post summary must exist");
    assert_eq!(post_summary.buy_count, initial_summary.buy_count);
    assert_eq!(post_summary.sell_count, initial_summary.sell_count);
    assert_eq!(
        post_summary.buy_qty.to_bits(),
        initial_summary.buy_qty.to_bits()
    );
    assert_eq!(
        post_summary.sell_qty.to_bits(),
        initial_summary.sell_qty.to_bits()
    );

    // 4. Deep verify all 250 price bins
    for (price, expected_bin) in snapshot {
        let actual_bin = dp
            .footprint
            .trades
            .get(&price)
            .unwrap_or_else(|| panic!("Price level {:?} disappeared from KlineTrades!", price));

        assert_eq!(
            actual_bin.buy_qty.to_bits(),
            expected_bin.buy_qty.to_bits(),
            "Price {:?}: buy_qty mutated! expected={}, got={}",
            price,
            expected_bin.buy_qty,
            actual_bin.buy_qty
        );
        assert_eq!(
            actual_bin.sell_qty.to_bits(),
            expected_bin.sell_qty.to_bits(),
            "Price {:?}: sell_qty mutated! expected={}, got={}",
            price,
            expected_bin.sell_qty,
            actual_bin.sell_qty
        );
        assert_eq!(
            actual_bin.buy_count, expected_bin.buy_count,
            "Price {:?}: buy_count mutated!",
            price
        );
        assert_eq!(
            actual_bin.sell_count, expected_bin.sell_count,
            "Price {:?}: sell_count mutated!",
            price
        );
        assert_eq!(
            actual_bin.first_time, expected_bin.first_time,
            "Price {:?}: first_time mutated!",
            price
        );
        assert_eq!(
            actual_bin.last_time, expected_bin.last_time,
            "Price {:?}: last_time mutated!",
            price
        );
    }
}

// ============================================================================
// 2. Replay Isolation: Step Backward & Future Lookahead Leakage Rejection
// ============================================================================

#[test]
fn test_adversarial_replay_isolation_step_backward_zero_future_leakage() {
    let _ = initialize_indicator_catalog();

    // Construct 80 historical candles:
    // Bars 0..=40: regular market action (base = 100.0)
    // Bars 41..=60: extreme anomaly / black swan price explosion (1,000,000.0)
    // Bars 61..=79: extreme crash (0.01)
    let total_bars = 80usize;
    let cutoff_bar = 40usize;
    let advance_bar = 65usize;

    let mut candles = Vec::with_capacity(total_bars);
    let interval_ms = 60_000u64;
    let base_t = 1_700_000_000_000u64;

    for i in 0..total_bars {
        let t = base_t + (i as u64 * interval_ms);
        let (open, high, low, close) = if i <= 40 {
            let p = 100.0 + (i as f64 * 0.5) + (i as f64 * 0.2).sin() * 5.0;
            (p, p + 2.0, p - 2.0, p + 0.5)
        } else if i <= 60 {
            // Extreme anomaly shock: 1,000,000.0
            (1_000_000.0, 1_000_050.0, 999_950.0, 1_000_010.0)
        } else {
            // Extreme crash
            (0.01, 0.02, 0.005, 0.015)
        };
        candles.push(make_candle(t, open, high, low, close, (50.0, 50.0)));
    }

    let all_frames: Vec<_> = candles
        .iter()
        .enumerate()
        .map(|(idx, c)| kline_to_indicator_frame(idx, c, None, None))
        .collect();

    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let ma_id = IndicatorId::parse("core.moving-averages").unwrap();

    let inst_rsi = store.create(rsi_id);
    let inst_ma = store.create(ma_id.clone());

    let reg_ma = data::chart::kline::indicators::catalog()
        .get(&ma_id)
        .unwrap();
    let mut cfg_ma = reg_ma.default_config();
    cfg_ma.set("fast_period", 5i64);
    store.update_config(inst_ma, cfg_ma);

    // Chained DAG: MA attached to RSI output "value"
    assert!(store.update_source_binding(
        inst_ma,
        "source",
        Some(IndicatorOutputRef {
            instance_id: inst_rsi,
            channel_id: "value".to_owned(),
        }),
    ));

    let active = vec![inst_rsi, inst_ma];

    // ========================================================================
    // BASELINE: Pristine evaluation with frames strictly up to cutoff_bar (40)
    // Future bars 41..80 do NOT exist in this run.
    // ========================================================================
    let t_cutoff = candles[cutoff_bar].time;
    let mut baseline_frames = all_frames.clone();
    baseline_frames.retain(|f| (f.time as u64) <= t_cutoff);
    assert_eq!(baseline_frames.len(), cutoff_bar + 1);

    let mut host_baseline = IndicatorHost::default();
    host_baseline.recalculate(&active, &store, &baseline_frames);

    let mut baseline_rsi_outputs = Vec::new();
    let mut baseline_ma_outputs = Vec::new();
    for i in 0..=cutoff_bar {
        baseline_rsi_outputs.push(host_baseline.output_value_for(inst_rsi, "value", i));
        baseline_ma_outputs.push(host_baseline.output_value_for(inst_ma, "value", i));
    }

    // Warmup check: At bar 40 (after 40 bars of data), RSI and chained MA must produce outputs
    assert!(
        baseline_rsi_outputs[cutoff_bar].is_some(),
        "RSI should have output at bar {cutoff_bar}"
    );
    assert!(
        baseline_ma_outputs[cutoff_bar].is_some(),
        "Chained MA should have output at bar {cutoff_bar}"
    );

    // ========================================================================
    // REPLAY RUN:
    // Step 1: Advance replay to bar 65 (deep into anomaly shock).
    // Step 2: Step backward to cutoff bar 40.
    // ========================================================================
    let t_advance = candles[advance_bar].time;
    let mut replay_frames = all_frames.clone();
    replay_frames.retain(|f| (f.time as u64) <= t_advance);
    assert_eq!(replay_frames.len(), advance_bar + 1);

    let mut host_replay = IndicatorHost::default();
    host_replay.recalculate(&active, &store, &replay_frames);

    // Verify advanced replay evaluated the shock at bar 65
    let rsi_at_65 = host_replay.output_value_for(inst_rsi, "value", advance_bar);
    assert!(
        rsi_at_65.is_some(),
        "Advanced replay must have evaluated bar 65"
    );

    // Step backward: Cutoff reset back to t_cutoff (bar 40)
    let mut stepped_back_frames = all_frames.clone();
    stepped_back_frames.retain(|f| (f.time as u64) <= t_cutoff);
    host_replay.recalculate(&active, &store, &stepped_back_frames);

    // ========================================================================
    // ASSERT ZERO LOOKAHEAD LEAKAGE AND ABSOLUTE DETERMINISM
    // ========================================================================
    for i in 0..=cutoff_bar {
        let replay_rsi = host_replay.output_value_for(inst_rsi, "value", i);
        let base_rsi = &baseline_rsi_outputs[i];
        assert_eq!(
            &replay_rsi, base_rsi,
            "Bar {i}: RSI output after stepping backward leaked future state or diverged! replay={:?}, base={:?}",
            replay_rsi, base_rsi
        );

        let replay_ma = host_replay.output_value_for(inst_ma, "value", i);
        let base_ma = &baseline_ma_outputs[i];
        assert_eq!(
            &replay_ma, base_ma,
            "Bar {i}: Chained MA output after stepping backward leaked future state or diverged! replay={:?}, base={:?}",
            replay_ma, base_ma
        );
    }

    // Verify all bars past cutoff (41..total_bars) return None
    for i in (cutoff_bar + 1)..total_bars {
        assert_eq!(
            host_replay.output_value_for(inst_rsi, "value", i),
            None,
            "Future bar {i} must NOT exist in host after stepping back to cutoff {cutoff_bar}"
        );
        assert_eq!(
            host_replay.output_value_for(inst_ma, "value", i),
            None,
            "Future bar {i} must NOT exist in host after stepping back to cutoff {cutoff_bar}"
        );
    }
}

// ============================================================================
// 3. High-Frequency Real-Time Streaming: 10,000 Trades Burst & Rollover
// ============================================================================

#[test]
fn test_adversarial_high_frequency_burst_streaming_and_rapid_rollover() {
    let _ = initialize_indicator_catalog();

    let timeframe = Timeframe::M1;
    let interval_ms = timeframe.to_milliseconds();
    let tick_size = 0.5f32;
    let step = PriceStep::from_f32(tick_size);
    let start_t = (1_700_000_000_000u64 / interval_ms) * interval_ms;

    let hist_count = 20usize;
    let mut initial_klines = Vec::with_capacity(hist_count);
    for i in 0..hist_count {
        let t = start_t.saturating_sub((hist_count - i) as u64 * interval_ms);
        let p = 100.0 + (i as f64 * 0.5);
        initial_klines.push(make_candle(t, p, p + 1.0, p - 1.0, p + 0.2, (20.0, 20.0)));
    }

    let mut ts = TimeSeries::<KlineDataPoint>::new(timeframe, step, &initial_klines)
        .with_exchange(Exchange::BinanceSpot);

    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let ma_id = IndicatorId::parse("core.moving-averages").unwrap();
    let inst_rsi = store.create(rsi_id);
    let inst_ma = store.create(ma_id.clone());

    let reg_ma = data::chart::kline::indicators::catalog()
        .get(&ma_id)
        .unwrap();
    let mut cfg_ma = reg_ma.default_config();
    cfg_ma.set("fast_period", 5i64);
    store.update_config(inst_ma, cfg_ma);

    let active = vec![inst_rsi, inst_ma];

    // Seed historical frames to warm up indicators
    let hist_frames: Vec<_> = ts
        .datapoints
        .values()
        .enumerate()
        .map(|(idx, dp)| {
            datapoint_to_indicator_frame(
                idx,
                dp,
                None,
                TimeOrTick::Time,
                Some(tick_size as f64),
                true,
            )
        })
        .collect();

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &store, &hist_frames);
    assert_eq!(ts.datapoints.len(), hist_count);

    // ------------------------------------------------------------------------
    // Part A: Burst of 10,000 trades on ACTIVE FORMING BAR
    // ------------------------------------------------------------------------
    let total_active_trades = 10_000usize;
    let mut total_active_volume = 0.0f32;
    let mut active_trades = Vec::with_capacity(total_active_trades);

    for i in 0..total_active_trades {
        let t = start_t + ((i as u64 * 5) % 55_000); // all within the first 1-minute candle
        let p_val = 100.0 + ((i % 50) as f64 * 0.5);
        let qty = 1.0 + (i % 7) as f32;
        let is_sell = i % 2 == 0;
        total_active_volume += qty;
        active_trades.push(Trade {
            time: t,
            price: Price::from_f64(p_val),
            qty,
            is_sell,
        });
    }

    // Stream in 100 bursts of 100 trades
    for chunk in active_trades.chunks(100) {
        let old_dp_len = ts.datapoints.len();
        ts.insert_realtime_trades(chunk);
        let new_dp_len = ts.datapoints.len();

        // Rollover hook logic matching KlineChart with committed_index fix
        if new_dp_len > old_dp_len {
            let start_idx = match host.committed_index() {
                Some(last) => last.saturating_add(1),
                None => 0,
            };
            let target_close = new_dp_len.saturating_sub(1);
            if start_idx < target_close {
                for (offset, dp) in ts
                    .datapoints
                    .values()
                    .skip(start_idx)
                    .take(target_close.saturating_sub(start_idx))
                    .enumerate()
                {
                    let idx = start_idx + offset;
                    let frame = datapoint_to_indicator_frame(
                        idx,
                        dp,
                        None,
                        TimeOrTick::Time,
                        Some(tick_size as f64),
                        true,
                    );
                    host.evaluate_closed(&frame);
                }
            }
        }

        if let Some(latest_dp) = ts.datapoints.values().last() {
            let preview_idx = new_dp_len.saturating_sub(1);
            let preview_frame = datapoint_to_indicator_frame(
                preview_idx,
                latest_dp,
                None,
                TimeOrTick::Time,
                Some(tick_size as f64),
                true,
            );
            host.preview_open(&preview_frame);
        }
    }

    // Assert active bar integrity
    assert_eq!(
        ts.datapoints.len(),
        hist_count + 1,
        "Historical candles + active candle must equal hist_count + 1"
    );

    let active_dp = ts.datapoints.values().last().unwrap();
    let fp_vol: f32 = active_dp
        .footprint
        .trades
        .values()
        .map(|b| b.total_qty())
        .sum();
    assert!(
        (fp_vol - total_active_volume).abs() < 1e-2,
        "Footprint volume {fp_vol} must match total trade volume {total_active_volume}"
    );
    assert!(
        active_dp.footprint.poc.is_some(),
        "Active bar must have valid POC"
    );

    // Indicator memory check: Host must remain healthy without quarantine
    let diagnostic = host.diagnostic_for(inst_rsi);
    assert!(
        diagnostic.is_none(),
        "Indicator must NOT be quarantined with committed_index rollover logic"
    );
}

// ============================================================================
// 4. High-Frequency Real-Time Streaming & Rollover (Correct Lifecycle Contract)
// ============================================================================

#[test]
fn test_adversarial_high_frequency_burst_streaming_and_rapid_rollover_clean_lifecycle() {
    let _ = initialize_indicator_catalog();

    let timeframe = Timeframe::M1;
    let interval_ms = timeframe.to_milliseconds();
    let tick_size = 0.5f32;
    let step = PriceStep::from_f32(tick_size);
    let start_t = (1_700_000_000_000u64 / interval_ms) * interval_ms;

    let hist_count = 20usize;
    let mut initial_klines = Vec::with_capacity(hist_count);
    for i in 0..hist_count {
        let t = start_t.saturating_sub((hist_count - i) as u64 * interval_ms);
        let p = 100.0 + (i as f64 * 0.5);
        initial_klines.push(make_candle(t, p, p + 1.0, p - 1.0, p + 0.2, (20.0, 20.0)));
    }

    let mut ts = TimeSeries::<KlineDataPoint>::new(timeframe, step, &initial_klines)
        .with_exchange(Exchange::BinanceSpot);

    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let ma_id = IndicatorId::parse("core.moving-averages").unwrap();
    let inst_rsi = store.create(rsi_id);
    let inst_ma = store.create(ma_id.clone());

    let reg_ma = data::chart::kline::indicators::catalog()
        .get(&ma_id)
        .unwrap();
    let mut cfg_ma = reg_ma.default_config();
    cfg_ma.set("fast_period", 5i64);
    store.update_config(inst_ma, cfg_ma);

    let active = vec![inst_rsi, inst_ma];

    // Seed historical frames to warm up indicators (strictly closed frames 0..hist_count)
    let hist_frames: Vec<_> = ts
        .datapoints
        .values()
        .enumerate()
        .map(|(idx, dp)| {
            datapoint_to_indicator_frame(
                idx,
                dp,
                None,
                TimeOrTick::Time,
                Some(tick_size as f64),
                true,
            )
        })
        .collect();

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &store, &hist_frames);
    assert_eq!(ts.datapoints.len(), hist_count);

    // Track last closed index to guarantee monotonic closed frame execution
    let mut last_committed_idx = hist_count - 1;

    // ------------------------------------------------------------------------
    // Part A: Burst of 10,000 trades on ACTIVE FORMING BAR
    // ------------------------------------------------------------------------
    let total_active_trades = 10_000usize;
    let mut total_active_volume = 0.0f32;
    let mut active_trades = Vec::with_capacity(total_active_trades);

    for i in 0..total_active_trades {
        let t = start_t + ((i as u64 * 5) % 55_000); // all within the active 1-minute candle
        let p_val = 100.0 + ((i % 50) as f64 * 0.5);
        let qty = 1.0 + (i % 7) as f32;
        let is_sell = i % 2 == 0;
        total_active_volume += qty;
        active_trades.push(Trade {
            time: t,
            price: Price::from_f64(p_val),
            qty,
            is_sell,
        });
    }

    // Stream in 100 bursts of 100 trades
    for chunk in active_trades.chunks(100) {
        let old_dp_len = ts.datapoints.len();
        ts.insert_realtime_trades(chunk);
        let new_dp_len = ts.datapoints.len();

        if new_dp_len > old_dp_len {
            let values: Vec<_> = ts.datapoints.values().collect();
            // Close newly completed bars (only if not already committed)
            let target_close = new_dp_len.saturating_sub(1);
            for idx in (last_committed_idx + 1)..target_close {
                if let Some(dp) = values.get(idx) {
                    let frame = datapoint_to_indicator_frame(
                        idx,
                        dp,
                        None,
                        TimeOrTick::Time,
                        Some(tick_size as f64),
                        true,
                    );
                    host.evaluate_closed(&frame);
                    last_committed_idx = idx;
                }
            }
        }

        if let Some(latest_dp) = ts.datapoints.values().last() {
            let preview_idx = new_dp_len.saturating_sub(1);
            let preview_frame = datapoint_to_indicator_frame(
                preview_idx,
                latest_dp,
                None,
                TimeOrTick::Time,
                Some(tick_size as f64),
                true,
            );
            host.preview_open(&preview_frame);
        }
    }

    assert_eq!(ts.datapoints.len(), hist_count + 1);
    let active_dp = ts.datapoints.values().last().unwrap();
    let fp_vol: f32 = active_dp
        .footprint
        .trades
        .values()
        .map(|b| b.total_qty())
        .sum();
    assert!(
        (fp_vol - total_active_volume).abs() < 1e-2,
        "Footprint volume {fp_vol} must match total trade volume {total_active_volume}"
    );
    assert!(
        active_dp.footprint.poc.is_some(),
        "Active bar must have valid POC"
    );

    assert!(
        host.diagnostic_for(inst_rsi).is_none(),
        "RSI must not be quarantined"
    );
    assert!(
        host.output_value_for(inst_rsi, "value", hist_count)
            .is_some()
    );
    assert!(
        host.output_value_for(inst_ma, "value", hist_count)
            .is_some()
    );

    // ------------------------------------------------------------------------
    // Part B: Rapid Rollover Storm — 100 Consecutive Candle Rollovers (10,000 trades)
    // ------------------------------------------------------------------------
    let rollover_bars = 100usize;
    let trades_per_bar = 100usize;
    let mut rollover_trades = Vec::with_capacity(rollover_bars * trades_per_bar);

    for bar_idx in 1..=rollover_bars {
        let bar_start = start_t + (bar_idx as u64 * interval_ms);
        for trade_idx in 0..trades_per_bar {
            let t = bar_start + (trade_idx as u64 * 500); // within the minute
            let p_val = 100.0 + (bar_idx as f64 * 0.3) + ((trade_idx % 20) as f64 * 0.5);
            let qty = 2.0;
            let is_sell = trade_idx % 3 == 0;
            rollover_trades.push(Trade {
                time: t,
                price: Price::from_f64(p_val),
                qty,
                is_sell,
            });
        }
    }

    // Stream across boundaries in chunks of 50 trades
    for chunk in rollover_trades.chunks(50) {
        let old_dp_len = ts.datapoints.len();
        ts.insert_realtime_trades(chunk);
        let new_dp_len = ts.datapoints.len();

        if new_dp_len > old_dp_len {
            let values: Vec<_> = ts.datapoints.values().collect();
            let target_close = new_dp_len.saturating_sub(1);
            for idx in (last_committed_idx + 1)..target_close {
                if let Some(dp) = values.get(idx) {
                    let frame = datapoint_to_indicator_frame(
                        idx,
                        dp,
                        None,
                        TimeOrTick::Time,
                        Some(tick_size as f64),
                        true,
                    );
                    host.evaluate_closed(&frame);
                    last_committed_idx = idx;
                }
            }
        }

        if let Some(latest_dp) = ts.datapoints.values().last() {
            let preview_idx = new_dp_len.saturating_sub(1);
            let preview_frame = datapoint_to_indicator_frame(
                preview_idx,
                latest_dp,
                None,
                TimeOrTick::Time,
                Some(tick_size as f64),
                true,
            );
            host.preview_open(&preview_frame);
        }
    }

    let final_len = ts.datapoints.len();
    assert_eq!(
        final_len,
        hist_count + 1 + rollover_bars,
        "Total candles must be hist_count + 1 active + 100 rolled over"
    );

    // Finalize the last candle
    let values: Vec<_> = ts.datapoints.values().collect();
    let last_closed_idx = final_len - 1;
    let final_frame = datapoint_to_indicator_frame(
        last_closed_idx,
        values[last_closed_idx],
        None,
        TimeOrTick::Time,
        Some(tick_size as f64),
        true,
    );
    host.evaluate_closed(&final_frame);

    // ------------------------------------------------------------------------
    // Subsystem Coexistence & Memory Assertions
    // ------------------------------------------------------------------------
    // 1. Every candle has consistent OHLC and POC
    for (time, dp) in &ts.datapoints {
        assert!(
            dp.kline.high >= dp.kline.low,
            "Bar at {time}: high < low ({:?} < {:?})",
            dp.kline.high,
            dp.kline.low
        );
        assert!(dp.kline.high >= dp.kline.open, "Bar at {time}: high < open");
        assert!(
            dp.kline.high >= dp.kline.close,
            "Bar at {time}: high < close"
        );
        assert!(dp.kline.low <= dp.kline.open, "Bar at {time}: low > open");
        assert!(dp.kline.low <= dp.kline.close, "Bar at {time}: low > close");
        if !dp.footprint.trades.is_empty() {
            assert!(
                dp.footprint.poc.is_some(),
                "Bar at {time}: POC must be calculated after trades insertion"
            );
        }
    }

    // Verify all 101 bars with streaming trades have calculated POC
    for dp in ts.datapoints.values().skip(hist_count) {
        assert!(
            dp.footprint.poc.is_some(),
            "Streaming bar must have calculated POC"
        );
    }

    // 2. Indicators evaluated properly across all 121 bars without quarantine
    assert!(host.diagnostic_for(inst_rsi).is_none());
    assert!(host.diagnostic_for(inst_ma).is_none());

    for idx in 0..final_len {
        let rsi_val = host.output_value_for(inst_rsi, "value", idx);
        let ma_val = host.output_value_for(inst_ma, "value", idx);

        if idx >= 14 {
            assert!(
                rsi_val.is_some(),
                "Bar {idx}: RSI must have output after warmup"
            );
            assert!(
                ma_val.is_some(),
                "Bar {idx}: MA must have output after warmup"
            );
        }
    }

    // 3. Zero memory leak: no ghost bars exist beyond final_len
    assert_eq!(
        host.output_value_for(inst_rsi, "value", final_len + 10),
        None,
        "No ghost outputs beyond final length"
    );
}
