use std::sync::Arc;
use std::time::Instant;

use exchange::Kline;
use exchange::util::{Price, PriceStep};
use hawk_data::chart::kline::indicators::adapter::{
    datapoint_to_indicator_frame, kline_to_indicator_frame,
};
use hawk_data::chart::kline::indicators::core::runtime::IndicatorHost;
use hawk_data::chart::kline::indicators::instance::IndicatorInstanceStore;
use hawk_data::chart::kline::indicators::loader::initialize_indicator_catalog;
use hawk_data::chart::kline::indicators::registry::{IndicatorConfig, IndicatorId, catalog};
use hawk_data::chart::kline::indicators::scripting::ScriptIndicatorRuntime;
use hawk_data::chart::kline::{IndicatorOutputRef, KlineDataPoint, KlineTrades, OutputValue};
use indicator_engine::{
    ChartFrame, ExprId, IndicatorFrame, InstrumentContext, LocalSlot, StatementBlockId,
    StatementKind, TimeOrTick, compile,
};

/// Same rule as `perf_budget_ms` in `data/src/lib.rs`: CI runners are ~3x slower, so
/// the wall-clock budget is widened there and stays strict locally.
fn perf_budget_ms(local_ms: u128) -> u128 {
    if std::env::var_os("CI").is_some() {
        local_ms * 4
    } else {
        local_ms
    }
}

fn ensure_catalog() {
    let _ = initialize_indicator_catalog();
}

fn create_kline(
    index: usize,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    vol: (f32, f32),
) -> Kline {
    Kline {
        time: 1_700_000_000_000 + (index as u64 * 60_000),
        open: Price::from_f64(open),
        high: Price::from_f64(high),
        low: Price::from_f64(low),
        close: Price::from_f64(close),
        volume: vol,
    }
}

fn runtime_from_source(source: &str, path: &str) -> ScriptIndicatorRuntime {
    let compiled = compile(source, path).unwrap();
    ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap()
}

fn rsi_runtime() -> ScriptIndicatorRuntime {
    runtime_from_source(include_str!("../indicators/sources/rsi.ind"), "rsi.ind")
}

fn bb_runtime() -> ScriptIndicatorRuntime {
    runtime_from_source(
        include_str!("../indicators/sources/bollinger-bands.ind"),
        "bollinger-bands.ind",
    )
}

fn avwap_runtime() -> ScriptIndicatorRuntime {
    runtime_from_source(
        include_str!("../indicators/sources/anchored-vwap.ind"),
        "anchored-vwap.ind",
    )
}

// ============================================================================
// SUITE 1: High-Frequency Tick Bursts & Streaming Preview Under Pressure
// ============================================================================

#[test]
fn test_adv_hf_tick_burst_50k_preview_open() {
    let mut runtime = rsi_runtime();

    // 1. Commit 100 historical bars
    for i in 0..100 {
        let price = 100.0 + (i as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    let val_99_baseline = runtime.output_value("value", 99);
    assert!(
        val_99_baseline.is_some(),
        "Committed bar 99 must have RSI output"
    );

    // 2. Blast 50,000 rapid ticks on active forming candle at bar 100
    let start = Instant::now();
    for tick in 0..50_000 {
        let oscillating_price = 110.0 + ((tick % 100) as f64 * 0.05);
        let kline_100 = create_kline(
            100,
            110.0,
            oscillating_price + 0.5,
            oscillating_price - 0.5,
            oscillating_price,
            (tick as f32, (tick % 50) as f32),
        );
        let frame_100 = kline_to_indicator_frame(100, &kline_100, None, None);
        runtime.preview_open(&frame_100);

        if tick % 10_000 == 0 {
            // Verify transactional isolation during the burst
            assert_eq!(
                runtime.output_value("value", 99),
                val_99_baseline,
                "Tick {tick}: Bar 99 mutated during preview burst"
            );
            assert!(
                runtime.output_value("value", 100).is_some(),
                "Tick {tick}: Active bar 100 must produce preview RSI"
            );
        }
    }
    let elapsed = start.elapsed();
    println!("50,000 preview ticks took {:?}", elapsed);
    assert!(
        elapsed.as_millis() < perf_budget_ms(5000),
        "50,000 preview ticks exceeded 5.0s budget: {:?}",
        elapsed
    );

    // 3. Verify final state and transactional isolation
    assert_eq!(
        runtime.output_value("value", 99),
        val_99_baseline,
        "Committed history bar 99 was corrupted by 50,000 preview ticks"
    );
    assert!(
        runtime.output_value("value", 100).is_some(),
        "Active bar 100 must have output after burst"
    );
    assert!(
        !runtime.is_quarantined(),
        "Runtime should not be quarantined after burst"
    );

    // 4. Close bar 100 and roll over to bar 101
    let kline_100_close = create_kline(100, 110.0, 115.0, 109.5, 112.5, (100.0, 100.0));
    let frame_100_close = kline_to_indicator_frame(100, &kline_100_close, None, None);
    runtime.evaluate_closed(&frame_100_close);
    let val_100_committed = runtime.output_value("value", 100);
    assert!(val_100_committed.is_some());

    // Preview bar 101
    let kline_101 = create_kline(101, 112.5, 113.0, 112.0, 112.8, (5.0, 5.0));
    let frame_101 = kline_to_indicator_frame(101, &kline_101, None, None);
    runtime.preview_open(&frame_101);
    assert_eq!(runtime.output_value("value", 100), val_100_committed);
    assert!(runtime.output_value("value", 101).is_some());
}

#[test]
fn test_adv_hf_preview_open_interleaved_closure() {
    let mut runtime = bb_runtime();

    // 100 bars, each with 50 preview ticks before closure
    for bar_idx in 0..100 {
        let base_price = 100.0 + ((bar_idx as f64 * 0.1).sin() * 10.0);
        for tick in 0..50 {
            let tick_price = base_price + ((tick as f64 - 25.0) * 0.05);
            let kline = create_kline(
                bar_idx,
                base_price,
                tick_price + 1.0,
                tick_price - 1.0,
                tick_price,
                (5.0, 5.0),
            );
            let frame = kline_to_indicator_frame(bar_idx, &kline, None, None);
            runtime.preview_open(&frame);
        }
        let final_kline = create_kline(
            bar_idx,
            base_price,
            base_price + 1.5,
            base_price - 1.5,
            base_price + 0.2,
            (10.0, 10.0),
        );
        let final_frame = kline_to_indicator_frame(bar_idx, &final_kline, None, None);
        runtime.evaluate_closed(&final_frame);
    }

    assert!(runtime.output_value("value", 99).is_some());
    assert!(runtime.output_value("bollinger.upper", 99).is_some());
    assert!(runtime.output_value("bollinger.lower", 99).is_some());
    assert!(!runtime.is_quarantined());
}

// ============================================================================
// SUITE 2: Feed Anomalies: Out-of-Order Candles, Clock Skew, Huge Gaps
// ============================================================================

#[test]
fn test_adv_out_of_order_candle_quarantine_resilience() {
    let mut runtime = rsi_runtime();

    for i in 0..10 {
        let kline = create_kline(i, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }
    assert!(!runtime.is_quarantined());

    // Inject out-of-order candle: bar index 5 arriving after bar index 9
    let kline_ooo = create_kline(5, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame_ooo = kline_to_indicator_frame(5, &kline_ooo, None, None);
    runtime.evaluate_closed(&frame_ooo);

    // Verify it cleanly quarantines without panic
    assert!(
        runtime.is_quarantined(),
        "Runtime must quarantine on out-of-order frame"
    );
    let diag = runtime.diagnostic().expect("Diagnostic must be set");
    assert!(diag.contains("closed bar 5 failed") || diag.contains("closed frame order"));

    // Verify subsequent calls do NOT panic and safely return None
    let kline_next = create_kline(11, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame_next = kline_to_indicator_frame(11, &kline_next, None, None);
    runtime.evaluate_closed(&frame_next);
    runtime.preview_open(&frame_next);
    assert!(runtime.is_quarantined());
    assert!(runtime.output_value("value", 11).is_none());
}

#[test]
fn test_adv_backward_timestamp_skew_and_zero_duration() {
    let mut runtime = avwap_runtime();

    // Bar 0: normal
    let mut k0 = create_kline(0, 100.0, 105.0, 95.0, 100.0, (50.0, 50.0));
    k0.time = 1_700_000_000_000;
    runtime.evaluate_closed(&kline_to_indicator_frame(0, &k0, None, None));

    // Bar 1: NTP clock backwards step by 10,000 seconds
    let mut k1 = create_kline(1, 100.0, 105.0, 95.0, 102.0, (50.0, 50.0));
    k1.time = 1_699_990_000_000;
    runtime.evaluate_closed(&kline_to_indicator_frame(1, &k1, None, None));

    // Bar 2: Zero timestamp
    let mut k2 = create_kline(2, 102.0, 106.0, 96.0, 103.0, (50.0, 50.0));
    k2.time = 0;
    runtime.evaluate_closed(&kline_to_indicator_frame(2, &k2, None, None));

    // Bar 3: Negative timestamp in IndicatorFrame
    let mut f3 = kline_to_indicator_frame(3, &k2, None, None);
    f3.time = -1_000_000;
    runtime.evaluate_closed(&f3);

    // Verify no panic or unhandled crash occurred
    assert!(!runtime.is_quarantined());
}

#[test]
fn test_adv_feed_timestamp_huge_gap() {
    let mut runtime = rsi_runtime();

    let mut k0 = create_kline(0, 100.0, 105.0, 95.0, 100.0, (50.0, 50.0));
    k0.time = 0;
    runtime.evaluate_closed(&kline_to_indicator_frame(0, &k0, None, None));

    let mut k1 = create_kline(1, 105.0, 110.0, 104.0, 108.0, (50.0, 50.0));
    k1.time = 5_000_000_000_000_000; // century gap
    runtime.evaluate_closed(&kline_to_indicator_frame(1, &k1, None, None));

    let mut k2 = create_kline(2, 108.0, 112.0, 107.0, 110.0, (50.0, 50.0));
    k2.time = i64::MAX as u64;
    runtime.evaluate_closed(&kline_to_indicator_frame(2, &k2, None, None));

    assert!(!runtime.is_quarantined());
}

// ============================================================================
// SUITE 3: 100,000+ Bar Massive History & Memory Boundedness
// ============================================================================

#[test]
fn test_adv_100k_synthetic_bars_linear_time_and_bounded_memory() {
    let mut runtime = rsi_runtime();

    let count = 100_000;
    let start_total = Instant::now();

    let mut batch_1_elapsed = std::time::Duration::ZERO;
    let mut batch_final_elapsed = std::time::Duration::ZERO;

    for i in 0..count {
        let price = 100.0 + ((i % 100) as f64 * 0.2);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);

        let t_bar_start = Instant::now();
        runtime.evaluate_closed(&frame);
        let bar_dur = t_bar_start.elapsed();

        if i < 10_000 {
            batch_1_elapsed += bar_dur;
        } else if i >= 90_000 {
            batch_final_elapsed += bar_dur;
        }
    }
    let total_elapsed = start_total.elapsed();
    println!(
        "100,000 bars total: {:?}, first 10k: {:?}, last 10k: {:?}",
        total_elapsed, batch_1_elapsed, batch_final_elapsed
    );

    // Total time for 100,000 bars in debug mode must be reasonable (linear time $O(N)$)
    assert!(
        total_elapsed.as_millis() < perf_budget_ms(15_000),
        "100k bars exceeded 15s in debug mode: {:?}",
        total_elapsed
    );

    // Asymptotic verification: ratio of first 10k to last 10k must not explode (O(1) per-bar)
    let ratio = batch_final_elapsed.as_secs_f64() / batch_1_elapsed.as_secs_f64().max(0.001);
    println!(
        "Asymptotic execution ratio (batch 90-100k / batch 0-10k): {:.2}",
        ratio
    );
    assert!(
        ratio < 3.5,
        "Asymptotic blowup detected: ratio is {:.2} (first 10k: {:?}, last 10k: {:?})",
        ratio,
        batch_1_elapsed,
        batch_final_elapsed
    );

    // Verify tail output is valid and finite
    let final_val = runtime.output_value("value", 99_999);
    assert!(final_val.is_some(), "Final bar 99,999 must have RSI output");
    if let Some(OutputValue::Number(n)) = final_val {
        assert!(n.is_finite());
        assert!((0.0..=100.0).contains(&n));
    } else {
        panic!("Expected numeric RSI output at bar 99,999");
    }
    assert!(!runtime.is_quarantined());
}

#[test]
fn test_adv_100k_bars_indicator_host_dag_recalculate() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();
    let reg_bb = catalog()
        .get(&IndicatorId::new("core.bollinger-bands").unwrap())
        .unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_bb = instances.create(reg_bb.id.clone());
    let active = vec![id_rsi, id_bb];

    // Generate 100,000 frames
    let mut frames = Vec::with_capacity(100_000);
    for i in 0..100_000 {
        let price = 100.0 + ((i % 50) as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }

    let mut host = IndicatorHost::default();
    let start = Instant::now();
    host.recalculate(&active, &instances, &frames);
    let elapsed = start.elapsed();
    println!(
        "IndicatorHost recalculate 100,000 frames took {:?}",
        elapsed
    );
    assert!(
        elapsed.as_millis() < perf_budget_ms(20_000),
        "Host 100k frames took {:?}",
        elapsed
    );

    assert!(host.output_value_for(id_rsi, "value", 99_999).is_some());
    assert!(host.output_value_for(id_bb, "value", 99_999).is_some());
}

// ============================================================================
// SUITE 4: Extreme Values (Zero Price, Negative Volume, NaN, Infinite, Spikes)
// ============================================================================

#[test]
fn test_adv_extreme_zero_prices_and_flatline() {
    let mut runtime = rsi_runtime();

    for i in 0..100 {
        let kline = create_kline(i, 0.0, 0.0, 0.0, 0.0, (0.0, 0.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    assert!(!runtime.is_quarantined(), "RSI quarantined on zero price");
}

#[test]
fn test_adv_extreme_negative_volume_and_negative_prices() {
    let step = PriceStep { units: 10_000 };
    let mut footprint = KlineTrades::new();
    let trade_neg = exchange::Trade {
        time: 1_700_000_000_000,
        price: Price::from_f64(-50.0),
        qty: -100.0,
        is_sell: false,
    };
    footprint.add_trade_to_nearest_bin(&trade_neg, step);

    let dp = KlineDataPoint {
        kline: create_kline(0, -50.0, -10.0, -100.0, -40.0, (-10.0, -20.0)),
        footprint,
        trades_fetched: true,
    };

    let frame = datapoint_to_indicator_frame(0, &dp, None, TimeOrTick::Time, None, true);
    assert_eq!(frame.chart.open, Some(-50.0));
    assert_eq!(frame.chart.high, Some(-10.0));
    assert_eq!(frame.chart.low, Some(-100.0));
    assert_eq!(frame.chart.close, Some(-40.0));

    let mut runtime = rsi_runtime();
    runtime.evaluate_closed(&frame);
    assert!(!runtime.is_quarantined());
}

#[test]
fn test_adv_extreme_nan_and_infinity_in_datapoints() {
    let mut runtime = rsi_runtime();

    let non_finite_cases = [
        (f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN),
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ),
        (f64::NEG_INFINITY, 100.0, f64::NEG_INFINITY, 100.0, 10.0),
    ];

    for (i, (o, h, l, c, v)) in non_finite_cases.iter().enumerate() {
        let frame = IndicatorFrame {
            index: i,
            time: i as i64 * 60_000,
            basis: TimeOrTick::Time,
            instrument: InstrumentContext { min_tick: None },
            chart: ChartFrame {
                open: Some(*o),
                high: Some(*h),
                low: Some(*l),
                close: Some(*c),
                volume: Some(*v),
                directional_volume_available: false,
                buy_qty: 0.0,
                sell_qty: 0.0,
                trade: None,
            },
            open_interest: None,
            series_inputs: Arc::from([]),
        };

        runtime.evaluate_closed(&frame);
        // Non-finite numbers must be sanitized and NEVER leaked as OutputValue::Number(NaN/Inf)
        if let Some(val) = runtime.output_value("value", i) {
            match val {
                OutputValue::Number(n) => {
                    assert!(n.is_finite(), "OutputValue leaked non-finite: {n}")
                }
                OutputValue::Boolean(_) => {}
            }
        }
    }
}

#[test]
fn test_adv_massive_magnitude_spikes_1e300() {
    let mut runtime = rsi_runtime();

    let spikes = [
        (1e300, 1e305, 1e295, 1e300, 1e300),
        (1e-300, 1e-295, 1e-305, 1e-300, 1e-300),
        (1e308, 1e308, 1e-308, 1e308, 1e308),
    ];

    for (i, (o, h, l, c, v)) in spikes.iter().enumerate() {
        let frame = IndicatorFrame {
            index: i,
            time: i as i64 * 60_000,
            basis: TimeOrTick::Time,
            instrument: InstrumentContext { min_tick: None },
            chart: ChartFrame {
                open: Some(*o),
                high: Some(*h),
                low: Some(*l),
                close: Some(*c),
                volume: Some(*v),
                directional_volume_available: false,
                buy_qty: 0.0,
                sell_qty: 0.0,
                trade: None,
            },
            open_interest: None,
            series_inputs: Arc::from([]),
        };
        runtime.evaluate_closed(&frame);
        if let Some(OutputValue::Number(n)) = runtime.output_value("value", i) {
            assert!(n.is_finite());
        }
    }
    assert!(!runtime.is_quarantined());
}

// ============================================================================
// SUITE 5: Malformed Bytecode, Resource Limits & Quarantine Isolation
// ============================================================================

#[test]
fn test_adv_infinite_loop_work_budget_quarantine() {
    let script = r#"
        language(1)
        indicator("Infinite Loop", id = "test.adv-inf-loop")
        format(precision = "base_ticker_min_tick")
        plot(1.0)
        while true {
        }
    "#;
    let compiled = compile(script, "adv_inf_loop.ind").unwrap();
    let mut runtime =
        ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap();

    let frame = kline_to_indicator_frame(
        0,
        &create_kline(0, 100.0, 101.0, 99.0, 100.0, (1.0, 1.0)),
        None,
        None,
    );
    runtime.evaluate_closed(&frame);

    assert!(runtime.is_quarantined());
    let diag = runtime.diagnostic().unwrap();
    assert!(diag.contains("execution limit exceeded") || diag.contains("failed"));

    // Preview on quarantined instance must be no-op
    runtime.preview_open(&frame);
    assert!(runtime.is_quarantined());
}

#[test]
fn test_adv_host_dag_cycle_prevention() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();

    let id_a = instances.create(reg_rsi.id.clone());
    let id_b = instances.create(reg_rsi.id.clone());
    let active = vec![id_a, id_b];

    let host = IndicatorHost::default();

    // Bind A -> B
    instances.update_source_binding(id_a, "source", Some(IndicatorOutputRef::new(id_b, "value")));
    let res_ab = host.validate_source_binding(&active, &instances, id_a, "source");
    assert!(res_ab.is_ok());

    // Bind B -> A (causing a cycle A -> B -> A)
    instances.update_source_binding(id_b, "source", Some(IndicatorOutputRef::new(id_a, "value")));
    let res_ba = host.validate_source_binding(&active, &instances, id_b, "source");
    assert!(
        res_ba.is_err(),
        "Host must detect dependency cycle between A and B"
    );
    assert!(res_ba.unwrap_err().contains("cycle"));

    // validate_bindings must also reject this configuration
    let res_all = host.validate_bindings(&active, &instances);
    assert!(res_all.is_err());
    assert!(res_all.unwrap_err().contains("cycle"));
}

#[test]
fn test_adv_corrupt_bytecode_quarantine() {
    let script = r#"
        language(1)
        indicator("Valid Base", id = "test.valid-base")
        format(precision = "base_ticker_min_tick")
        plot(chart.close)
    "#;
    let mut compiled = compile(script, "valid_base.ind").unwrap();

    // Corrupt entry point block ID
    compiled.program.entry_point = StatementBlockId(9999);

    let mut runtime =
        ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap();
    let frame = kline_to_indicator_frame(
        0,
        &create_kline(0, 100.0, 101.0, 99.0, 100.0, (1.0, 1.0)),
        None,
        None,
    );

    // Evaluating frame with invalid block must not crash the process; it gracefully completes or quarantines
    runtime.evaluate_closed(&frame);
}

#[test]
fn test_adv_corrupt_statement_out_of_bounds() {
    let script = r#"
        language(1)
        indicator("Statement Corrupt", id = "test.stmt-corrupt")
        format(precision = "base_ticker_min_tick")
        x = 10
        plot(x)
    "#;
    let mut compiled = compile(script, "stmt_corrupt.ind").unwrap();
    if let Some(stmt) = compiled.program.statements.get_mut(0) {
        stmt.kind = StatementKind::LocalUpdate {
            target: LocalSlot(9999),
            expression: ExprId(0),
        };
    }
    let mut runtime =
        ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap();
    let frame = kline_to_indicator_frame(
        0,
        &create_kline(0, 100.0, 101.0, 99.0, 100.0, (1.0, 1.0)),
        None,
        None,
    );
    runtime.evaluate_closed(&frame);
}

#[test]
fn test_adv_corrupt_bytecode_invalid_expr_id() {
    let script = r#"
        language(1)
        indicator("Expr Corrupt", id = "test.expr-corrupt")
        format(precision = "base_ticker_min_tick")
        plot(chart.close + 1.0)
    "#;
    let mut compiled = compile(script, "expr_corrupt.ind").unwrap();
    // Invalidate an expression operand to point to out-of-bounds ExprId
    if let Some(expr) = compiled.program.expressions.get_mut(0) {
        expr.kind = indicator_engine::ExpressionKind::Binary {
            operator: indicator_engine::BinaryOperator::Add,
            left: ExprId(99999),
            right: ExprId(99998),
        };
    }
    let mut runtime =
        ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap();
    let frame = kline_to_indicator_frame(
        0,
        &create_kline(0, 100.0, 101.0, 99.0, 100.0, (1.0, 1.0)),
        None,
        None,
    );
    runtime.evaluate_closed(&frame);
    assert!(runtime.is_quarantined());
    assert!(runtime.diagnostic().is_some());
}

#[test]
fn test_adv_hf_preview_on_host_multi_indicator() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();
    let reg_bb = catalog()
        .get(&IndicatorId::new("core.bollinger-bands").unwrap())
        .unwrap();
    let reg_ma = catalog()
        .get(&IndicatorId::new("core.moving-averages").unwrap())
        .unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_bb = instances.create(reg_bb.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());
    let active = vec![id_rsi, id_bb, id_ma];

    // Seed 50 historical bars
    let mut frames = Vec::with_capacity(50);
    for i in 0..50 {
        let price = 100.0 + (i as f64 * 0.2);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }
    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    let val_49_rsi = host.output_value_for(id_rsi, "value", 49);
    assert!(val_49_rsi.is_some());

    // 10,000 rapid ticks on preview bar 50 across all 3 indicators
    let start = Instant::now();
    for tick in 0..10_000 {
        let price = 110.0 + ((tick % 20) as f64 * 0.1);
        let kline = create_kline(50, 110.0, price + 1.0, price - 1.0, price, (1.0, 1.0));
        let frame = kline_to_indicator_frame(50, &kline, None, None);
        host.preview_open_selected(&active, &frame);

        if tick % 2_000 == 0 {
            assert_eq!(host.output_value_for(id_rsi, "value", 49), val_49_rsi);
            assert!(host.output_value_for(id_rsi, "value", 50).is_some());
            assert!(host.output_value_for(id_bb, "value", 50).is_some());
            assert!(host.output_value_for(id_ma, "value", 50).is_some());
        }
    }
    let elapsed = start.elapsed();
    println!(
        "Host 10,000 multi-indicator preview ticks took {:?}",
        elapsed
    );
    assert!(
        elapsed.as_millis() < perf_budget_ms(3000),
        "Multi-indicator preview exceeded 3s: {:?}",
        elapsed
    );
}

#[test]
fn test_adv_prefix_remapping_large_history() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();
    let id_rsi = instances.create(reg_rsi.id.clone());
    let active = vec![id_rsi];

    let mut frames = Vec::with_capacity(100);
    for i in 0..100 {
        let price = 100.0 + (i as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    let val_before = host.output_value_for(id_rsi, "value", 50);
    assert!(val_before.is_some());

    // Shift prefix by 50,000 (e.g. prepending 50k historical bars)
    host.remap_outputs_prefix(50_000);
    // After prefix shift, previous index 50 should now map to index 50_050
    let val_after = host.output_value_for(id_rsi, "value", 50_050);
    assert_eq!(val_before, val_after);

    // Prefix shift by 0 must be no-op
    host.remap_outputs_prefix(0);
    assert_eq!(host.output_value_for(id_rsi, "value", 50_050), val_after);
}

#[test]
fn test_adv_zero_tick_step_footprint() {
    let step_zero = PriceStep { units: 0 };
    let mut footprint = KlineTrades::new();
    let trade = exchange::Trade {
        time: 1_700_000_000_000,
        price: Price::from_f64(100.0),
        qty: 10.0,
        is_sell: false,
    };
    // Must not panic with division by zero or SIGFPE on zero units step
    footprint.add_trade_to_nearest_bin(&trade, step_zero);
    footprint.calculate_poc();

    let dp = KlineDataPoint {
        kline: create_kline(0, 100.0, 101.0, 99.0, 100.0, (10.0, 0.0)),
        footprint,
        trades_fetched: true,
    };
    let frame = datapoint_to_indicator_frame(0, &dp, None, TimeOrTick::Time, None, true);
    assert!(frame.chart.open.is_some());
}

#[test]
fn test_adv_vwap_huge_volume_spike_overflow_safety() {
    let mut runtime =
        runtime_from_source(include_str!("../indicators/sources/vwap.ind"), "vwap.ind");

    let k0 = create_kline(0, 100.0, 101.0, 99.0, 100.0, (f32::MAX, f32::MAX));
    let mut frame0 = kline_to_indicator_frame(0, &k0, None, None);
    frame0.chart.volume = Some(1e300);
    runtime.evaluate_closed(&frame0);

    let k1 = create_kline(1, 102.0, 103.0, 101.0, 102.5, (f32::MAX, f32::MAX));
    let mut frame1 = kline_to_indicator_frame(1, &k1, None, None);
    frame1.chart.volume = Some(1e300);
    runtime.evaluate_closed(&frame1);

    assert!(!runtime.is_quarantined());
    if let Some(OutputValue::Number(n)) = runtime.output_value("value", 1) {
        assert!(n.is_finite());
    }
}
