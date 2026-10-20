use indicator_engine::{
    ChartFrame, IndicatorConfig, IndicatorFrame, InstrumentContext, TimeOrTick, Value, compile,
};
use std::sync::Arc;

fn test_frame(
    index: usize,
    time: i64,
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    volume: Option<f64>,
) -> IndicatorFrame {
    IndicatorFrame {
        index,
        time,
        basis: TimeOrTick::Time,
        instrument: InstrumentContext {
            min_tick: Some(0.01),
        },
        chart: ChartFrame {
            open,
            high,
            low,
            close,
            volume,
            directional_volume_available: false,
            buy_qty: 0.0,
            sell_qty: 0.0,
            trade: None,
        },
        open_interest: None,
        series_inputs: Arc::new([]),
    }
}

fn evaluate_script(source: &str, frames: Vec<IndicatorFrame>, plot_id: &str) -> Vec<Value> {
    let compiled = compile(source, "adversarial_test.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiation should succeed");
    frames
        .into_iter()
        .map(|f| {
            let output = instance
                .evaluate_closed(f)
                .expect("evaluation should succeed");
            output
                .plots
                .iter()
                .find(|p| p.id == plot_id)
                .map(|p| p.value.clone())
                .unwrap_or(Value::Na)
        })
        .collect()
}

#[test]
fn test_ta_rsi_numerical_precision_and_edge_conditions() {
    let source = r#"
        language(1)
        indicator("Adversarial RSI", id = "test.adv-rsi")
        format(precision = "base_ticker_min_tick")
        period = input.int(3, "Period", min = 1, max = 100)
        plot(ta.rsi(chart.close, period), id = "rsi")
    "#;

    // 1. Alternating prices: [10.0, 20.0, 10.0, 20.0, 10.0]
    let alternating_prices = [10.0, 20.0, 10.0, 20.0, 10.0];
    let frames = alternating_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(&rsi_vals[..3], &[Value::Na, Value::Na, Value::Na]);
    match &rsi_vals[3] {
        Value::Float(v) => assert!(
            (v - 66.6666666667).abs() < 1e-9,
            "Bar 3 RSI expected ~66.6666666667, got {v}"
        ),
        other => panic!("Expected Float at bar 3, got {:?}", other),
    }
    match &rsi_vals[4] {
        Value::Float(v) => assert!(
            (v - 44.4444444444).abs() < 1e-9,
            "Bar 4 RSI expected ~44.4444444444, got {v}"
        ),
        other => panic!("Expected Float at bar 4, got {:?}", other),
    }

    // 2. Flat prices: all 100.0 (0 price changes -> RSI neutral 50.0)
    let flat_prices = [100.0, 100.0, 100.0, 100.0, 100.0];
    let frames = flat_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(50.0),
        "Flat prices must yield exactly 50.0 at bar 3"
    );
    assert_eq!(
        rsi_vals[4],
        Value::Float(50.0),
        "Flat prices must yield exactly 50.0 at bar 4"
    );

    // 3. Zero-loss prices: monotonic increasing prices -> RSI 100.0
    let gain_prices = [100.0, 105.0, 110.0, 115.0, 120.0];
    let frames = gain_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(100.0),
        "Zero-loss prices must yield exactly 100.0 at bar 3"
    );
    assert_eq!(
        rsi_vals[4],
        Value::Float(100.0),
        "Zero-loss prices must yield exactly 100.0 at bar 4"
    );

    // 4. Zero-gain prices: monotonic decreasing prices -> RSI 0.0
    let drop_prices = [100.0, 95.0, 90.0, 85.0, 80.0];
    let frames = drop_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(0.0),
        "Zero-gain prices must yield exactly 0.0 at bar 3"
    );
    assert_eq!(
        rsi_vals[4],
        Value::Float(0.0),
        "Zero-gain prices must yield exactly 0.0 at bar 4"
    );

    // 5. Tiny delta resilience (no underflow / precision collapse)
    let tiny_gain_prices = [10.0, 10.0 + 1e-12, 10.0 + 2e-12, 10.0 + 3e-12];
    let frames = tiny_gain_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(100.0),
        "Tiny gains must yield exactly 100.0"
    );

    let tiny_loss_prices = [10.0, 10.0 - 1e-12, 10.0 - 2e-12, 10.0 - 3e-12];
    let frames = tiny_loss_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(0.0),
        "Tiny losses must yield exactly 0.0"
    );
}

#[test]
fn test_ta_sma_precision_and_missing_values() {
    let source = r#"
        language(1)
        indicator("Adversarial SMA", id = "test.adv-sma")
        format(precision = "base_ticker_min_tick")
        plot(ta.sma(chart.close, 4), id = "sma")
    "#;

    let prices = [Some(10.0), Some(20.0), Some(30.0), Some(40.0), Some(50.0)];
    let frames = prices
        .iter()
        .enumerate()
        .map(|(i, &p)| test_frame(i, i as i64 * 60_000, p, p, p, p, Some(1.0)))
        .collect();
    let sma_vals = evaluate_script(source, frames, "sma");
    assert_eq!(sma_vals[0], Value::Na);
    assert_eq!(sma_vals[1], Value::Na);
    assert_eq!(sma_vals[2], Value::Na);
    assert_eq!(sma_vals[3], Value::Float(25.0)); // (10+20+30+40)/4 = 25.0
    assert_eq!(sma_vals[4], Value::Float(35.0)); // (20+30+40+50)/4 = 35.0

    // Missing value inside window invalidates SMA until it exits
    let gap_prices = [
        Some(10.0),
        Some(20.0),
        None,
        Some(40.0),
        Some(50.0),
        Some(60.0),
        Some(70.0),
    ];
    let frames = gap_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| test_frame(i, i as i64 * 60_000, p, p, p, p, Some(1.0)))
        .collect();
    let sma_vals = evaluate_script(source, frames, "sma");
    assert_eq!(sma_vals[2], Value::Na); // missing sample
    assert_eq!(sma_vals[3], Value::Na); // window contains missing sample
    assert_eq!(sma_vals[4], Value::Na); // window contains missing sample
    assert_eq!(sma_vals[5], Value::Na); // window contains missing sample: [None, 40, 50, 60]
    assert_eq!(sma_vals[6], Value::Float(55.0)); // [40, 50, 60, 70] -> (40+50+60+70)/4 = 55.0
}

#[test]
fn test_ta_ema_and_rma_precision_and_recovery() {
    let source = r#"
        language(1)
        indicator("Adversarial EMA and RMA", id = "test.adv-ema-rma")
        format(precision = "base_ticker_min_tick")
        plot(ta.ema(chart.close, 3), id = "ema")
        plot(ta.rma(chart.close, 4), id = "rma")
    "#;

    // EMA length 3: alpha = 2/(3+1) = 0.5
    // RMA length 4: alpha = 1/4 = 0.25
    let prices = [Some(10.0), Some(20.0), Some(30.0), None, Some(50.0)];
    let frames: Vec<_> = prices
        .iter()
        .enumerate()
        .map(|(i, &p)| test_frame(i, i as i64 * 60_000, p, p, p, p, Some(1.0)))
        .collect();

    let compiled = compile(source, "adv_ema_rma.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiation should succeed");

    let outputs: Vec<_> = frames
        .into_iter()
        .map(|f| instance.evaluate_closed(f).expect("eval"))
        .collect();

    let get_plot = |out: &indicator_engine::FrameOutput, id: &str| {
        out.plots.iter().find(|p| p.id == id).unwrap().value.clone()
    };

    // EMA:
    // bar 0: 10.0 (seed)
    // bar 1: 0.5*20 + 0.5*10 = 15.0
    // bar 2: 0.5*30 + 0.5*15 = 22.5
    // bar 3: None -> Na, retains 22.5
    // bar 4: 0.5*50 + 0.5*22.5 = 36.25
    assert_eq!(get_plot(&outputs[0], "ema"), Value::Float(10.0));
    assert_eq!(get_plot(&outputs[1], "ema"), Value::Float(15.0));
    assert_eq!(get_plot(&outputs[2], "ema"), Value::Float(22.5));
    assert_eq!(get_plot(&outputs[3], "ema"), Value::Na);
    assert_eq!(get_plot(&outputs[4], "ema"), Value::Float(36.25));

    // RMA:
    // bar 0: 10.0 (seed)
    // bar 1: (10.0 * 3 + 20.0) / 4 = 12.5
    // bar 2: (12.5 * 3 + 30.0) / 4 = 16.875
    // bar 3: None -> Na, retains 16.875
    // bar 4: (16.875 * 3 + 50.0) / 4 = (50.625 + 50.0) / 4 = 25.15625
    assert_eq!(get_plot(&outputs[0], "rma"), Value::Float(10.0));
    assert_eq!(get_plot(&outputs[1], "rma"), Value::Float(12.5));
    assert_eq!(get_plot(&outputs[2], "rma"), Value::Float(16.875));
    assert_eq!(get_plot(&outputs[3], "rma"), Value::Na);
    assert_eq!(get_plot(&outputs[4], "rma"), Value::Float(25.15625));
}

#[test]
fn test_ta_stdev_precision_and_constant_zero_variance() {
    let source = r#"
        language(1)
        indicator("Adversarial STDEV", id = "test.adv-stdev")
        format(precision = "base_ticker_min_tick")
        plot(ta.stdev(chart.close, 4), id = "stdev")
    "#;

    // Population stdev: [2.0, 4.0, 4.0, 6.0]
    // mean = 4.0, variance = (4 + 0 + 0 + 4) / 4 = 2.0 -> stdev = sqrt(2.0)
    let prices = [2.0, 4.0, 4.0, 6.0];
    let frames = prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let stdev_vals = evaluate_script(source, frames, "stdev");
    match &stdev_vals[3] {
        Value::Float(v) => assert!(
            (v - std::f64::consts::SQRT_2).abs() < 1e-12,
            "Expected sqrt(2) ~1.41421356237, got {v}"
        ),
        other => panic!("Expected Float, got {:?}", other),
    }

    // Identical constant values must give exact 0.0 (no negative zero or NaN from sqrt)
    let const_prices = [42.123, 42.123, 42.123, 42.123];
    let frames = const_prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let stdev_vals = evaluate_script(source, frames, "stdev");
    assert_eq!(
        stdev_vals[3],
        Value::Float(0.0),
        "Constant values must give exact 0.0 standard deviation"
    );
}

#[test]
fn test_ta_vwma_precision_and_zero_volume_division_safety() {
    let source = r#"
        language(1)
        indicator("Adversarial VWMA", id = "test.adv-vwma")
        format(precision = "base_ticker_min_tick")
        plot(ta.vwma(chart.close, chart.volume, 3), id = "vwma")
    "#;

    // Normal calculation:
    // P = [100.0, 110.0, 120.0], V = [1.0, 2.0, 3.0]
    // Weighted = 100*1 + 110*2 + 120*3 = 680. Total vol = 6.0.
    // VWMA = 680 / 6 = 113.33333333333333
    let p_and_v = [(100.0, 1.0), (110.0, 2.0), (120.0, 3.0)];
    let frames = p_and_v
        .iter()
        .enumerate()
        .map(|(i, &(p, v))| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(v),
            )
        })
        .collect();
    let vwma_vals = evaluate_script(source, frames, "vwma");
    match &vwma_vals[2] {
        Value::Float(v) => assert!(
            (v - (680.0 / 6.0)).abs() < 1e-12,
            "Expected 113.3333333333, got {v}"
        ),
        other => panic!("Expected Float, got {:?}", other),
    }

    // Zero volume edge case: total volume = 0.0
    // MUST return Value::Na without panic, NaN, or zero division error
    let zero_vol = [(100.0, 0.0), (110.0, 0.0), (120.0, 0.0)];
    let frames = zero_vol
        .iter()
        .enumerate()
        .map(|(i, &(p, v))| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(v),
            )
        })
        .collect();
    let vwma_vals = evaluate_script(source, frames, "vwma");
    assert_eq!(
        vwma_vals[2],
        Value::Na,
        "VWMA with 0.0 total volume must return Value::Na"
    );
}

#[test]
fn test_ta_avwap_precision_boundary_reset_and_zero_volume() {
    let source = r#"
        language(1)
        indicator("Adversarial AVWAP", id = "test.adv-avwap")
        format(precision = "base_ticker_min_tick")
        plot(ta.avwap(chart.close, chart.volume, "Day"), id = "avwap")
    "#;

    // 2024-01-01 10:00:00 UTC = 1704103200000 ms
    // 2024-01-01 15:00:00 UTC = 1704121200000 ms
    // 2024-01-02 01:00:00 UTC = 1704157200000 ms (New Day!)
    let frames = vec![
        test_frame(
            0,
            1704103200000,
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(10.0),
        ),
        test_frame(
            1,
            1704121200000,
            Some(120.0),
            Some(120.0),
            Some(120.0),
            Some(120.0),
            Some(10.0),
        ),
        test_frame(
            2,
            1704157200000,
            Some(200.0),
            Some(200.0),
            Some(200.0),
            Some(200.0),
            Some(5.0),
        ),
    ];
    let avwap_vals = evaluate_script(source, frames, "avwap");

    // Bar 0: Day 1, price 100, vol 10 -> AVWAP = 100.0
    assert_eq!(avwap_vals[0], Value::Float(100.0));
    // Bar 1: Day 1, (100*10 + 120*10) / 20 = 110.0
    assert_eq!(avwap_vals[1], Value::Float(110.0));
    // Bar 2: Day 2, boundary resets, new day total: 200*5 / 5 = 200.0
    assert_eq!(avwap_vals[2], Value::Float(200.0));

    // Zero volume returns Na safely without panic
    let zero_vol_frames = vec![test_frame(
        0,
        1704103200000,
        Some(100.0),
        Some(100.0),
        Some(100.0),
        Some(100.0),
        Some(0.0),
    )];
    let avwap_vals = evaluate_script(source, zero_vol_frames, "avwap");
    assert_eq!(
        avwap_vals[0],
        Value::Na,
        "AVWAP with 0 volume must return Value::Na"
    );
}

#[test]
fn test_nan_inf_normalization_to_na() {
    let source = r#"
        language(1)
        indicator("Adversarial Non-finite", id = "test.adv-nonfinite")
        format(precision = "base_ticker_min_tick")
        plot(1.0 / 0.0, id = "div_zero")
        plot(0.0 / 0.0, id = "zero_div_zero")
        plot(math.abs(na), id = "abs_na")
    "#;

    let frames = vec![test_frame(
        0,
        0,
        Some(1.0),
        Some(1.0),
        Some(1.0),
        Some(1.0),
        Some(1.0),
    )];

    let compiled = compile(source, "adv_nonfinite.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiation should succeed");
    let output = instance
        .evaluate_closed(frames.into_iter().next().unwrap())
        .expect("eval");

    let get_plot = |id: &str| {
        output
            .plots
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .value
            .clone()
    };

    assert_eq!(
        get_plot("div_zero"),
        Value::Na,
        "1.0 / 0.0 must normalize to Value::Na"
    );
    assert_eq!(
        get_plot("zero_div_zero"),
        Value::Na,
        "0.0 / 0.0 must normalize to Value::Na"
    );
    assert_eq!(
        get_plot("abs_na"),
        Value::Na,
        "math.abs(na) must be Value::Na"
    );
}

#[test]
fn test_checked_integer_arithmetic_overflow_safety() {
    let source = r#"
        language(1)
        indicator("Adversarial Integer Checked", id = "test.adv-int-overflow")
        format(precision = "base_ticker_min_tick")
        plot(9223372036854775807 + 1, id = "add_overflow")
        plot(-9223372036854775807 - 2, id = "sub_underflow")
        plot(9223372036854775807 * 2, id = "mul_overflow")
        plot(100 / 0, id = "int_div_zero")
        plot(100 % 0, id = "int_rem_zero")
        plot((-9223372036854775807 - 1) % -1, id = "int_min_rem_neg_one")
    "#;

    let frames = vec![test_frame(
        0,
        0,
        Some(1.0),
        Some(1.0),
        Some(1.0),
        Some(1.0),
        Some(1.0),
    )];

    let compiled = compile(source, "adv_int_overflow.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiation should succeed");
    let output = instance
        .evaluate_closed(frames.into_iter().next().unwrap())
        .expect("eval");

    let get_plot = |id: &str| {
        output
            .plots
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .value
            .clone()
    };

    assert_eq!(
        get_plot("add_overflow"),
        Value::Na,
        "i64::MAX + 1 must return Value::Na without panic"
    );
    assert_eq!(
        get_plot("sub_underflow"),
        Value::Na,
        "i64::MIN - 1 must return Value::Na without panic"
    );
    assert_eq!(
        get_plot("mul_overflow"),
        Value::Na,
        "i64::MAX * 2 must return Value::Na without panic"
    );
    assert_eq!(
        get_plot("int_div_zero"),
        Value::Na,
        "Integer / 0 must return Value::Na without panic"
    );
    assert_eq!(
        get_plot("int_rem_zero"),
        Value::Na,
        "Integer % 0 must return Value::Na without panic"
    );
    assert_eq!(
        get_plot("int_min_rem_neg_one"),
        Value::Na,
        "i64::MIN % -1 must return Value::Na without panic/SIGFPE"
    );
}

#[test]
fn test_long_horizon_10k_bars_drift_and_asymptotic_stability() {
    let source = r#"
        language(1)
        indicator("Adversarial 10k Horizon", id = "test.adv-10k")
        format(precision = "base_ticker_min_tick")
        plot(ta.rsi(chart.close, 14), id = "rsi")
        plot(ta.sma(chart.close, 50), id = "sma")
        plot(ta.ema(chart.close, 50), id = "ema")
        plot(ta.rma(chart.close, 50), id = "rma")
        plot(ta.stdev(chart.close, 50), id = "stdev")
        plot(ta.vwma(chart.close, chart.volume, 50), id = "vwma")
    "#;

    let compiled = compile(source, "adv_10k.ind").expect("compile");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate");

    let count = 10_000;
    let mut last_output = None;
    for i in 0..count {
        let frame = test_frame(
            i,
            i as i64 * 60_000,
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(10.0),
        );
        let out = instance.evaluate_closed(frame).expect("eval");
        if i == count - 1 {
            last_output = Some(out);
        }
    }

    let out = last_output.unwrap();
    let get_plot = |id: &str| out.plots.iter().find(|p| p.id == id).unwrap().value.clone();

    assert_eq!(
        get_plot("rsi"),
        Value::Float(50.0),
        "10k bars constant close must yield exactly 50.0 RSI"
    );
    assert_eq!(
        get_plot("sma"),
        Value::Float(100.0),
        "10k bars constant close must yield exactly 100.0 SMA"
    );
    match get_plot("ema") {
        Value::Float(v) => assert!(
            (v - 100.0).abs() < 1e-12,
            "10k bars EMA must converge to 100.0, got {v}"
        ),
        other => panic!("Expected Float for EMA, got {:?}", other),
    }
    match get_plot("rma") {
        Value::Float(v) => assert!(
            (v - 100.0).abs() < 1e-12,
            "10k bars RMA must converge to 100.0, got {v}"
        ),
        other => panic!("Expected Float for RMA, got {:?}", other),
    }
    assert_eq!(
        get_plot("stdev"),
        Value::Float(0.0),
        "10k bars constant close must yield exactly 0.0 STDEV"
    );
    match get_plot("vwma") {
        Value::Float(v) => assert!(
            (v - 100.0).abs() < 1e-12,
            "10k bars VWMA must yield 100.0, got {v}"
        ),
        other => panic!("Expected Float for VWMA, got {:?}", other),
    }
}

#[test]
fn test_negative_prices_and_extreme_magnitudes() {
    let source = r#"
        language(1)
        indicator("Adversarial Extremes", id = "test.adv-extremes")
        format(precision = "base_ticker_min_tick")
        plot(ta.rsi(chart.close, 3), id = "rsi")
    "#;

    // Negative prices: [-100.0, -90.0, -80.0, -70.0] -> strictly increasing deltas (+10.0)
    let neg_increasing = [-100.0, -90.0, -80.0, -70.0];
    let frames = neg_increasing
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(100.0),
        "Negative prices with gains must yield 100.0 RSI"
    );

    // Negative prices: [-70.0, -80.0, -90.0, -100.0] -> strictly decreasing deltas (-10.0)
    let neg_decreasing = [-70.0, -80.0, -90.0, -100.0];
    let frames = neg_decreasing
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            test_frame(
                i,
                i as i64 * 60_000,
                Some(p),
                Some(p),
                Some(p),
                Some(p),
                Some(1.0),
            )
        })
        .collect();
    let rsi_vals = evaluate_script(source, frames, "rsi");
    assert_eq!(
        rsi_vals[3],
        Value::Float(0.0),
        "Negative prices with drops must yield 0.0 RSI"
    );
}

#[test]
fn test_avwap_week_and_month_boundary_resets() {
    let source = r#"
        language(1)
        indicator("Adversarial AVWAP Boundaries", id = "test.adv-avwap-bounds")
        format(precision = "base_ticker_min_tick")
        plot(ta.avwap(chart.close, chart.volume, "Week"), id = "week_vwap")
        plot(ta.avwap(chart.close, chart.volume, "Month"), id = "month_vwap")
        plot(ta.avwap(chart.close, chart.volume, "unknown_anchor"), id = "invalid_anchor")
    "#;

    // Week test:
    // Sunday 2024-01-07 23:59:00 UTC = 1704671940000 ms
    // Monday 2024-01-08 00:01:00 UTC = 1704672060000 ms (New week!)
    // Month test:
    // Jan 31 2024 23:59:00 UTC = 1706745540000 ms
    // Feb 01 2024 00:01:00 UTC = 1706745660000 ms (New month!)
    let frames = vec![
        test_frame(
            0,
            1704671940000,
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(100.0),
            Some(10.0),
        ),
        test_frame(
            1,
            1704672060000,
            Some(200.0),
            Some(200.0),
            Some(200.0),
            Some(200.0),
            Some(5.0),
        ),
        test_frame(
            2,
            1706745540000,
            Some(300.0),
            Some(300.0),
            Some(300.0),
            Some(300.0),
            Some(10.0),
        ),
        test_frame(
            3,
            1706745660000,
            Some(400.0),
            Some(400.0),
            Some(400.0),
            Some(400.0),
            Some(10.0),
        ),
    ];

    let compiled = compile(source, "adv_avwap_bounds.ind").expect("compile");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate");

    let outputs: Vec<_> = frames
        .into_iter()
        .map(|f| instance.evaluate_closed(f).expect("eval"))
        .collect();

    let get_plot = |out: &indicator_engine::FrameOutput, id: &str| {
        out.plots.iter().find(|p| p.id == id).unwrap().value.clone()
    };

    // Sunday (Week 1): 100.0
    assert_eq!(get_plot(&outputs[0], "week_vwap"), Value::Float(100.0));
    // Monday (Week 2): week boundary resets -> 200.0
    assert_eq!(get_plot(&outputs[1], "week_vwap"), Value::Float(200.0));

    // Jan 31 (Month 1):
    // Total Jan: price_vol = 100*10 + 200*5 + 300*10 = 1000 + 1000 + 3000 = 5000. vol = 10 + 5 + 10 = 25. 5000 / 25 = 200.0.
    assert_eq!(get_plot(&outputs[2], "month_vwap"), Value::Float(200.0));

    // Feb 01 (Month 2): month boundary resets! Price 400.0, Vol 10.0 -> 400.0
    assert_eq!(get_plot(&outputs[3], "month_vwap"), Value::Float(400.0));

    // Invalid anchor returns Value::Na without panic
    assert_eq!(get_plot(&outputs[0], "invalid_anchor"), Value::Na);
}
