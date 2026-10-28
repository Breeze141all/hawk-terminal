use super::OutputValue;
use crate::chart::kline::indicators::adapter::{
    datapoint_to_indicator_frame, kline_to_indicator_frame,
};
use crate::chart::kline::indicators::core::IndicatorHost;
use crate::chart::kline::indicators::instance::IndicatorInstanceStore;
use crate::chart::kline::indicators::loader::initialize_indicator_catalog;
use crate::chart::kline::indicators::registry::{IndicatorConfig, IndicatorId, catalog};
use crate::chart::kline::indicators::scripting::ScriptIndicatorRuntime;
use crate::chart::kline::{KlineDataPoint, KlineTrades};
use exchange::Kline;
use exchange::util::{Price, PriceStep};
use indicator_engine::{BaseSeriesSource, RuntimeError, TimeOrTick};
use std::sync::Arc;

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
        volume: (f64::from(vol.0), f64::from(vol.1)),
    }
}

// ----------------------------------------------------------------------------
// Suite 1: Synthetic Data Generators & Conversion
// ----------------------------------------------------------------------------

#[test]
fn test_frame_conversion_full_datapoint() {
    let step = PriceStep { units: 10_000 };
    let mut footprint = KlineTrades::new();
    let trade_buy = exchange::Trade {
        time: 1_700_000_000_000,
        price: Price::from_f64(100.0),
        qty: 150.0,
        is_sell: false,
    };
    let trade_sell = exchange::Trade {
        time: 1_700_000_000_100,
        price: Price::from_f64(102.0),
        qty: 50.0,
        is_sell: true,
    };
    footprint.add_trade_to_nearest_bin(&trade_buy, step);
    footprint.add_trade_to_nearest_bin(&trade_sell, step);
    footprint.calculate_poc();

    let dp = KlineDataPoint {
        kline: create_kline(0, 100.0, 105.0, 98.0, 102.0, (150.0, 50.0)),
        footprint,
        trades_fetched: true,
    };

    let frame = datapoint_to_indicator_frame(0, &dp, None, TimeOrTick::Time, None, true);

    assert_eq!(frame.chart.open, Some(100.0));
    assert_eq!(frame.chart.high, Some(105.0));
    assert_eq!(frame.chart.low, Some(98.0));
    assert_eq!(frame.chart.close, Some(102.0));
    assert_eq!(frame.chart.volume, Some(200.0));
    assert!(frame.chart.directional_volume_available);
    assert_eq!(frame.chart.buy_qty, 150.0);
    assert_eq!(frame.chart.sell_qty, 50.0);

    assert_eq!(BaseSeriesSource::Hl2.value(&frame), Some(101.5));
    let hlc3 = BaseSeriesSource::Hlc3.value(&frame).unwrap();
    assert!((hlc3 - (105.0 + 98.0 + 102.0) / 3.0).abs() < 1e-6);
    assert_eq!(BaseSeriesSource::Ohlc4.value(&frame), Some(101.25));
}

#[test]
fn test_frame_conversion_minimal_candle_without_trades() {
    let dp = KlineDataPoint {
        kline: create_kline(0, 100.0, 105.0, 98.0, 102.0, (-1.0, 50.0)),
        footprint: KlineTrades::new(),
        trades_fetched: false,
    };

    let frame = datapoint_to_indicator_frame(0, &dp, None, TimeOrTick::Time, None, true);

    assert_eq!(frame.chart.open, Some(100.0));
    assert_eq!(frame.chart.high, Some(105.0));
    assert_eq!(frame.chart.low, Some(98.0));
    assert_eq!(frame.chart.close, Some(102.0));
    assert_eq!(frame.chart.volume, Some(50.0));
    assert!(!frame.chart.directional_volume_available);
    assert!(frame.chart.trade.is_none());
}

// ----------------------------------------------------------------------------
// Suite 2: Two-Phase Execution Lifecycle
// ----------------------------------------------------------------------------

#[test]
fn test_lifecycle_closed_history_sequential_commitment() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).expect("core.rsi registered");
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..100 {
        let price = 100.0 + (i as f64 * 0.5);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    assert!(runtime.output_value("value", 99).is_some());

    // Test that re-feeding an earlier bar to an IndicatorInstance fails monotonic ordering
    let reg = catalog().get(&rsi_id).unwrap();
    let mut instance = reg
        .compiled
        .instantiate(indicator_engine::IndicatorConfig::default())
        .unwrap();
    for i in 0..10 {
        let kline = create_kline(i, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        instance.evaluate_closed(frame).unwrap();
    }
    let kline_past = create_kline(8, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame_past = kline_to_indicator_frame(8, &kline_past, None, None);
    let err = instance.evaluate_closed(frame_past);
    assert!(matches!(
        err,
        Err(RuntimeError::InvalidValue {
            operation: "closed frame order"
        })
    ));
}

#[test]
fn test_lifecycle_active_bar_preview_transactional_isolation() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).expect("core.rsi registered");
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..100 {
        let price = 100.0 + ((i as f64 * 0.3).sin() * 5.0);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    let val_99_before = runtime.output_value("value", 99);
    assert!(val_99_before.is_some());

    // Preview tick 1 on active bar 100 (close = 105.0)
    let kline_100_t1 = create_kline(100, 100.0, 106.0, 99.0, 105.0, (5.0, 5.0));
    let frame_100_t1 = kline_to_indicator_frame(100, &kline_100_t1, None, None);
    runtime.preview_open(&frame_100_t1);

    let val_100_t1 = runtime.output_value("value", 100);
    assert!(val_100_t1.is_some());
    assert_eq!(runtime.output_value("value", 99), val_99_before);

    // Preview tick 2 on active bar 100 (close = 115.0)
    let kline_100_t2 = create_kline(100, 100.0, 116.0, 99.0, 115.0, (10.0, 5.0));
    let frame_100_t2 = kline_to_indicator_frame(100, &kline_100_t2, None, None);
    runtime.preview_open(&frame_100_t2);

    let val_100_t2 = runtime.output_value("value", 100);
    assert!(val_100_t2.is_some());
    assert_ne!(val_100_t1, val_100_t2);
    assert_eq!(runtime.output_value("value", 99), val_99_before);
}

#[test]
fn test_lifecycle_high_frequency_preview_zero_leak_benchmark() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).expect("core.rsi registered");
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..100 {
        let price = 100.0 + (i as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    let start = std::time::Instant::now();
    for tick in 0..1_000 {
        let close = 110.0 + ((tick % 20) as f64 * 0.2);
        let kline = create_kline(100, 110.0, close + 1.0, 109.0, close, (1.0, 1.0));
        let frame = kline_to_indicator_frame(100, &kline, None, None);
        runtime.preview_open(&frame);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < crate::perf_budget_ms(500),
        "1,000 preview ticks took {:?}",
        elapsed
    );
    assert!(runtime.output_value("value", 100).is_some());
}

#[test]
fn test_lifecycle_candle_close_and_rollover() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).expect("core.rsi registered");
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..=100 {
        let price = 100.0 + (i as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    let val_100 = runtime.output_value("value", 100);
    assert!(val_100.is_some());

    let kline_101 = create_kline(101, 110.1, 111.0, 110.0, 110.5, (5.0, 5.0));
    let frame_101 = kline_to_indicator_frame(101, &kline_101, None, None);
    runtime.preview_open(&frame_101);

    assert!(runtime.output_value("value", 101).is_some());
    assert_eq!(runtime.output_value("value", 100), val_100);
}

// ----------------------------------------------------------------------------
// Suite 3: Static Warmup Lookback Analysis
// ----------------------------------------------------------------------------

#[test]
fn test_warmup_analysis_core_indicators() {
    ensure_catalog();

    let rsi_reg = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();
    let rsi_warmup = rsi_reg
        .compiled
        .warmup_bars(&indicator_engine::IndicatorConfig::default());
    assert_eq!(rsi_warmup, Some(26));

    let bb_reg = catalog()
        .get(&IndicatorId::new("core.bollinger-bands").unwrap())
        .unwrap();
    let bb_warmup = bb_reg
        .compiled
        .warmup_bars(&indicator_engine::IndicatorConfig::default());
    assert_eq!(bb_warmup, Some(20));

    let ma_reg = catalog()
        .get(&IndicatorId::new("core.moving-averages").unwrap())
        .unwrap();
    let ma_warmup = ma_reg
        .compiled
        .warmup_bars(&indicator_engine::IndicatorConfig::default());
    assert_eq!(ma_warmup, Some(250));

    let avwap_reg = catalog()
        .get(&IndicatorId::new("core.anchored-vwap").unwrap())
        .unwrap();
    assert!(avwap_reg.compiled.requires_anchored_history());
}

#[test]
fn test_warmup_dynamic_config_recalculation() {
    ensure_catalog();
    let rsi_reg = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();

    let mut values = std::collections::BTreeMap::new();
    values.insert("period".to_owned(), indicator_engine::InputValue::Int(50));
    let cfg50 = indicator_engine::IndicatorConfig { values };
    assert_eq!(rsi_reg.compiled.warmup_bars(&cfg50), Some(62));

    let mut values2 = std::collections::BTreeMap::new();
    values2.insert("period".to_owned(), indicator_engine::InputValue::Int(50));
    values2.insert(
        "ema_period".to_owned(),
        indicator_engine::InputValue::Int(21),
    );
    let cfg50_21 = indicator_engine::IndicatorConfig { values: values2 };
    assert_eq!(rsi_reg.compiled.warmup_bars(&cfg50_21), Some(72));
}

#[test]
fn test_warmup_window_clamping_to_runtime_limits() {
    ensure_catalog();
    let rsi_reg = catalog()
        .get(&IndicatorId::new("core.rsi").unwrap())
        .unwrap();

    let mut values = std::collections::BTreeMap::new();
    values.insert(
        "period".to_owned(),
        indicator_engine::InputValue::Int(100_000),
    );
    let huge_cfg = indicator_engine::IndicatorConfig { values };
    let warmup = rsi_reg.compiled.warmup_bars(&huge_cfg).unwrap();
    assert!((5_000..=5_050).contains(&warmup));
}

// ----------------------------------------------------------------------------
// Suite 4: Cache Management & History Eviction
// ----------------------------------------------------------------------------

#[test]
fn test_cache_history_eviction_ring_buffer() {
    let script = r#"language(1)
indicator("Ring Buffer", id = "test.ring-buffer", max_bars_back = 50)
format(precision = "fixed_step:1")
plot(ta.sma(chart.close, 50))
"#;
    let compiled = indicator_engine::compile(script, std::path::Path::new("test.ind")).unwrap();
    let mut instance = compiled
        .instantiate(indicator_engine::IndicatorConfig::default())
        .unwrap();

    for i in 0..1_000 {
        let kline = create_kline(i, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        instance.evaluate_closed(frame).unwrap();
    }
    assert_eq!(instance.committed_index(), Some(999));
}

#[test]
fn test_cache_config_update_and_replay() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).unwrap();
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..100 {
        let price = 100.0 + ((i as f64 * 0.2).sin() * 10.0);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }
    let val_rsi14 = runtime.output_value("value", 99);
    assert!(val_rsi14.is_some());

    let mut new_config = reg.default_config();
    new_config.set("period", 28i64);
    let changed = runtime.set_config(&new_config).unwrap();
    assert!(changed);
    assert!(runtime.output_value("value", 99).is_none());

    for i in 0..100 {
        let price = 100.0 + ((i as f64 * 0.2).sin() * 10.0);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }
    let val_rsi28 = runtime.output_value("value", 99);
    assert!(val_rsi28.is_some());
    assert_ne!(val_rsi14, val_rsi28);
}

#[test]
fn test_cache_quarantine_resilience() {
    let script = r#"language(1)
indicator("Loop", id = "test.loop")
format(precision = "fixed_step:1")
plot(1.0)
while true {
}
"#;
    let compiled = indicator_engine::compile(script, std::path::Path::new("loop.ind")).unwrap();
    let mut runtime =
        ScriptIndicatorRuntime::new(Arc::new(compiled), IndicatorConfig::default()).unwrap();

    let kline = create_kline(0, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame = kline_to_indicator_frame(0, &kline, None, None);
    runtime.evaluate_closed(&frame);

    assert!(runtime.is_quarantined());
    assert!(runtime.diagnostic().is_some());

    let kline2 = create_kline(1, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame2 = kline_to_indicator_frame(1, &kline2, None, None);
    runtime.preview_open(&frame2);
    assert!(runtime.is_quarantined());
    assert!(runtime.output_value("plot_0", 1).is_none());
}

// ----------------------------------------------------------------------------
// Suite 5: Numerical Precision of Bundled Indicators
// ----------------------------------------------------------------------------

#[test]
fn test_rsi_numerical_accuracy_against_analytical_model() {
    ensure_catalog();
    let rsi_id = IndicatorId::new("core.rsi").unwrap();
    let reg = catalog().get(&rsi_id).unwrap();
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    let mut closes = Vec::with_capacity(100);
    for i in 0..100 {
        let price = 100.0 + (i as f64 * 0.4) + ((i % 7) as f64 * 0.8) - ((i % 3) as f64 * 0.5);
        closes.push(price);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    let period = 14;
    let mut gains = vec![0.0; 100];
    let mut losses = vec![0.0; 100];
    for i in 1..100 {
        let change = closes[i] - closes[i - 1];
        if change > 0.0 {
            gains[i] = change;
        } else {
            losses[i] = -change;
        }
    }

    let mut avg_gain = gains[1..=period].iter().sum::<f64>() / period as f64;
    let mut avg_loss = losses[1..=period].iter().sum::<f64>() / period as f64;

    let mut analytical_rsi = vec![None; 100];
    let rs = if avg_loss == 0.0 {
        100.0
    } else {
        avg_gain / avg_loss
    };
    analytical_rsi[period] = Some(100.0 - (100.0 / (1.0 + rs)));

    for i in (period + 1)..100 {
        avg_gain = (avg_gain * (period as f64 - 1.0) + gains[i]) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + losses[i]) / period as f64;
        let rs = if avg_loss == 0.0 {
            100.0
        } else {
            avg_gain / avg_loss
        };
        analytical_rsi[i] = Some(100.0 - (100.0 / (1.0 + rs)));
    }

    for (i, expected_rsi) in analytical_rsi.iter().enumerate().take(100).skip(23) {
        let expected = expected_rsi.unwrap();
        let actual = match runtime.output_value("value", i).unwrap() {
            OutputValue::Number(n) => n,
            _ => panic!("Expected numeric RSI output"),
        };
        assert!(
            (actual - expected).abs() < 1e-4,
            "Bar {i}: actual {actual} vs analytical {expected}"
        );
    }
}

#[test]
fn test_bollinger_bands_geometry_and_invariants() {
    ensure_catalog();
    let bb_id = IndicatorId::new("core.bollinger-bands").unwrap();
    let reg = catalog().get(&bb_id).unwrap();
    let mut runtime = reg.create_runtime(reg.default_config()).unwrap();

    for i in 0..100 {
        let price = 100.0 + ((i as f64 * 0.2).sin() * 10.0);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }

    for i in 20..100 {
        let upper = match runtime.output_value("bollinger.upper", i).unwrap() {
            OutputValue::Number(n) => n,
            _ => panic!("Expected numeric upper"),
        };
        let basis = match runtime.output_value("value", i).unwrap() {
            OutputValue::Number(n) => n,
            _ => panic!("Expected numeric basis"),
        };
        let lower = match runtime.output_value("bollinger.lower", i).unwrap() {
            OutputValue::Number(n) => n,
            _ => panic!("Expected numeric lower"),
        };

        assert!(upper >= basis, "Bar {i}: upper {upper} < basis {basis}");
        assert!(basis >= lower, "Bar {i}: basis {basis} < lower {lower}");
        let diff_upper = upper - basis;
        let diff_lower = basis - lower;
        assert!(
            (diff_upper - diff_lower).abs() < 1e-5,
            "Bar {i}: asymmetry: {diff_upper} vs {diff_lower}"
        );
    }

    let mut zero_config = reg.default_config();
    zero_config.set("std_dev_multiplier", 0.0f64);
    runtime.set_config(&zero_config).unwrap();
    for i in 0..100 {
        let price = 100.0 + ((i as f64 * 0.2).sin() * 10.0);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        let frame = kline_to_indicator_frame(i, &kline, None, None);
        runtime.evaluate_closed(&frame);
    }
    for i in 20..100 {
        let upper = runtime
            .output_value("bollinger.upper", i)
            .unwrap()
            .as_number()
            .unwrap();
        let basis = runtime
            .output_value("value", i)
            .unwrap()
            .as_number()
            .unwrap();
        let lower = runtime
            .output_value("bollinger.lower", i)
            .unwrap()
            .as_number()
            .unwrap();
        assert!((upper - basis).abs() < 1e-6);
        assert!((lower - basis).abs() < 1e-6);
    }
}

// ----------------------------------------------------------------------------
// Suite 6: Multi-Indicator Concurrency & Coexistence in IndicatorHost
// ----------------------------------------------------------------------------

#[test]
fn test_host_multi_indicator_coexistence() {
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
    let mut frames = Vec::with_capacity(300);
    for i in 0..300 {
        let price = 100.0 + (i as f64 * 0.1);
        let kline = create_kline(i, price, price + 1.0, price - 1.0, price, (10.0, 10.0));
        frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    assert!(host.output_value_for(id_rsi, "value", 299).is_some());
    assert!(host.output_value_for(id_bb, "value", 299).is_some());
    assert!(host.output_value_for(id_ma, "value", 299).is_some());

    // Preview tick
    let preview_kline = create_kline(300, 130.0, 131.0, 129.0, 130.5, (5.0, 5.0));
    let preview_frame = kline_to_indicator_frame(300, &preview_kline, None, None);
    host.preview_open_selected(&active, &preview_frame);

    assert!(host.output_value_for(id_rsi, "value", 300).is_some());
    assert!(host.output_value_for(id_bb, "value", 300).is_some());
    assert!(host.output_value_for(id_ma, "value", 300).is_some());
}
