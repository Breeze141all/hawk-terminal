use indicator_engine::{
    ChartFrame, CompileLimits, IndicatorConfig, IndicatorFrame, InstrumentContext, RuntimeError,
    RuntimeLimits, TimeOrTick, Value, compile, compile_with_limits,
};

fn make_frame(index: usize, time: i64, close: Option<f64>) -> IndicatorFrame {
    IndicatorFrame {
        index,
        time,
        basis: TimeOrTick::Time,
        instrument: InstrumentContext {
            min_tick: Some(0.01),
        },
        chart: ChartFrame {
            open: close,
            high: close.map(|c| c + 1.0),
            low: close.map(|c| c - 1.0),
            close,
            volume: Some(100.0),
            directional_volume_available: false,
            buy_qty: 50.0,
            sell_qty: 50.0,
            trade: None,
        },
        open_interest: None,
        series_inputs: Vec::new().into(),
    }
}

#[test]
fn test_adv_division_by_zero_and_overflow() {
    let script = r#"
language(1)
indicator("DivZero", id = "test.divzero")
format(precision = "base_ticker_min_tick")

int_div_zero = 10 / 0
int_mod_zero = 10 % 0
float_div_zero = 10.0 / 0.0
zero_div_zero = 0.0 / 0.0
int_overflow = 9223372036854775807 + 1
int_underflow = -9223372036854775808 - 1
int_mul_overflow = 9223372036854775807 * 2

plot(int_div_zero, id = "p1")
plot(int_mod_zero, id = "p2")
plot(float_div_zero, id = "p3")
plot(zero_div_zero, id = "p4")
plot(int_overflow, id = "p5")
plot(int_underflow, id = "p6")
plot(int_mul_overflow, id = "p7")
"#;

    let compiled = compile(script, "divzero.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiation should succeed");

    let out = instance
        .evaluate_closed(make_frame(0, 1000, Some(100.0)))
        .expect("runtime should not panic or fail on div/mod by zero or overflow");

    for plot in out.plots.iter() {
        assert_eq!(
            plot.value,
            Value::Na,
            "Plot {} should be Na, got {:?}",
            plot.id,
            plot.value
        );
    }
}

#[test]
fn test_adv_infinite_loop_budget_enforcement() {
    let script = r#"
language(1)
indicator("InfLoop", id = "test.infloop")
format(precision = "base_ticker_min_tick")

i = 0
while true {
    i := i + 1
}

plot(i, id = "res")
"#;

    let compiled = compile(script, "infloop.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate_with_limits(IndicatorConfig::default(), RuntimeLimits::untrusted())
        .expect("instantiation should succeed");

    let result = instance.evaluate_closed(make_frame(0, 1000, Some(100.0)));
    assert!(result.is_err(), "infinite loop must fail execution");
    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ExecutionLimitExceeded | RuntimeError::ResourceLimitExceeded(_)
        ),
        "error should be limit exceeded, got: {:?}",
        err
    );
}

#[test]
fn test_adv_range_loop_huge_bounds() {
    let script = r#"
language(1)
indicator("HugeLoop", id = "test.hugeloop")
format(precision = "base_ticker_min_tick")

total = 0
for i = 0 to 1000000000 {
    total := total + 1
}

plot(total, id = "res")
"#;

    let compiled = compile(script, "hugeloop.ind").expect("compilation should succeed");
    let mut instance = compiled
        .instantiate_with_limits(IndicatorConfig::default(), RuntimeLimits::untrusted())
        .expect("instantiation should succeed");

    let result = instance.evaluate_closed(make_frame(0, 1000, Some(100.0)));
    assert!(
        result.is_err(),
        "huge range loop must be terminated by work budget"
    );
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::ExecutionLimitExceeded | RuntimeError::ResourceLimitExceeded(_)
    ));
}

#[test]
fn test_adv_deep_expression_nesting_parser() {
    let depth = 2000;
    let mut expr = String::new();
    for _ in 0..depth {
        expr.push('(');
    }
    expr.push_str("chart.close");
    for _ in 0..depth {
        expr.push(')');
    }

    let script = format!(
        "language(1)\nindicator(\"DeepParen\", id = \"test.deepparen\")\nformat(precision = \"base_ticker_min_tick\")\nplot({expr}, id = \"p\")"
    );

    // This should either parse or gracefully return a Diagnostic error without stack overflow
    let result = compile(&script, "deepparen.ind");
    let _ = result;
}

#[test]
fn test_adv_recursion_rejection() {
    let recursive_script = r#"
language(1)
indicator("Rec", id = "test.rec")
format(precision = "base_ticker_min_tick")

fn f(x) {
    return f(x + 1)
}

plot(f(1), id = "res")
"#;

    let result = compile(recursive_script, "rec.ind");
    assert!(
        result.is_err(),
        "recursive functions must be rejected at compile time or fail diagnostics"
    );
}

#[test]
fn test_adv_memory_collection_limits() {
    let script = r#"
language(1)
indicator("ArrayExplode", id = "test.arrayexplode")
format(precision = "base_ticker_min_tick")

arr = array.new()
while true {
    arr := array.push(arr, chart.close)
}

plot(array.size(arr), id = "res")
"#;

    let compiled = compile(script, "arrayexplode.ind").expect("compile ok");
    let mut instance = compiled
        .instantiate_with_limits(IndicatorConfig::default(), RuntimeLimits::untrusted())
        .expect("instantiate ok");

    let result = instance.evaluate_closed(make_frame(0, 1000, Some(100.0)));
    assert!(result.is_err(), "unbounded array growth must hit limits");
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::ExecutionLimitExceeded | RuntimeError::ResourceLimitExceeded(_)
    ));
}

#[test]
fn test_adv_two_phase_transactional_isolation() {
    let script = r#"
language(1)
indicator("TwoPhase", id = "test.twophase")
format(precision = "base_ticker_min_tick")

state count = 0
count := count + 1

plot(count, id = "count")
"#;

    let compiled = compile(script, "twophase.ind").expect("compile ok");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate ok");

    // Commit bar 0
    let out0 = instance
        .evaluate_closed(make_frame(0, 1000, Some(100.0)))
        .expect("bar 0 ok");
    let p0 = out0.plots.iter().find(|p| p.id == "count").unwrap();
    assert_eq!(p0.value, Value::Int(1));

    // Preview bar 1 multiple times with different values
    for _ in 0..100 {
        let out_prev = instance
            .preview_open(make_frame(1, 1060, Some(105.0)))
            .expect("preview ok");
        let p_prev = out_prev.plots.iter().find(|p| p.id == "count").unwrap();
        // In preview, count should be 2, but NOT mutate the base state for subsequent previews
        assert_eq!(p_prev.value, Value::Int(2));
    }

    // Now commit bar 1
    let out1 = instance
        .evaluate_closed(make_frame(1, 1060, Some(105.0)))
        .expect("bar 1 ok");
    let p1 = out1.plots.iter().find(|p| p.id == "count").unwrap();
    assert_eq!(p1.value, Value::Int(2));

    // And bar 2
    let out2 = instance
        .evaluate_closed(make_frame(2, 1120, Some(110.0)))
        .expect("bar 2 ok");
    let p2 = out2.plots.iter().find(|p| p.id == "count").unwrap();
    assert_eq!(p2.value, Value::Int(3));
}

#[test]
fn test_adv_preview_on_fresh_instance_before_closed_bars() {
    let script = r#"
language(1)
indicator("FreshPreview", id = "test.freshpreview")
format(precision = "base_ticker_min_tick")

state count = 0
count := count + 1

plot(count, id = "count")
plot(barstate.isfirst, id = "isfirst")
plot(barstate.isrealtime, id = "isrealtime")
plot(barstate.islast, id = "islast")
plot(barstate.isconfirmed, id = "isconfirmed")
"#;

    let compiled = compile(script, "freshpreview.ind").expect("compile ok");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate ok");

    // Preview open bar 0 before any evaluate_closed!
    let out = instance
        .preview_open(make_frame(0, 1000, Some(100.0)))
        .expect("preview on fresh instance ok");

    let is_first = out.plots.iter().find(|p| p.id == "isfirst").unwrap();
    let is_realtime = out.plots.iter().find(|p| p.id == "isrealtime").unwrap();
    let is_last = out.plots.iter().find(|p| p.id == "islast").unwrap();
    let is_confirmed = out.plots.iter().find(|p| p.id == "isconfirmed").unwrap();

    assert_eq!(is_first.value, Value::Bool(true));
    assert_eq!(is_realtime.value, Value::Bool(true));
    assert_eq!(is_last.value, Value::Bool(true));
    assert_eq!(is_confirmed.value, Value::Bool(false));
}

#[test]
fn test_adv_all_none_and_nan_market_frames() {
    let script = r#"
language(1)
indicator("NullFeeds", id = "test.nullfeeds")
format(precision = "base_ticker_min_tick")

sma = ta.sma(chart.close, 14)
rsi = ta.rsi(chart.close, 14)
cum = ta.cum(chart.volume)

plot(sma, id = "sma")
plot(rsi, id = "rsi")
plot(cum, id = "cum")
"#;

    let compiled = compile(script, "nullfeeds.ind").expect("compile ok");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate ok");

    // Feed 50 completely empty frames
    for i in 0..50 {
        let frame = IndicatorFrame {
            index: i,
            time: (i * 60) as i64,
            basis: TimeOrTick::Time,
            instrument: InstrumentContext { min_tick: None },
            chart: ChartFrame {
                open: None,
                high: None,
                low: None,
                close: None,
                volume: None,
                directional_volume_available: false,
                buy_qty: 0.0,
                sell_qty: 0.0,
                trade: None,
            },
            open_interest: None,
            series_inputs: Vec::new().into(),
        };

        let out = instance
            .evaluate_closed(frame)
            .expect("should handle None frames without panic");
        for p in out.plots.iter() {
            assert_eq!(p.value, Value::Na, "output for None frames must be Na");
        }
    }
}

#[test]
fn test_adv_source_limits_exceeded() {
    let huge_source = "a".repeat(2 * 1024 * 1024); // 2MB
    let limits = CompileLimits {
        max_source_bytes: 1024 * 1024,
        ..Default::default()
    };
    let result = compile_with_limits(&huge_source, "huge.ind", limits);
    assert!(
        result.is_err(),
        "must reject source exceeding max_source_bytes"
    );
}

#[test]
fn test_adv_ta_window_bounds_validation() {
    for bad_len in ["-5", "0", "1000000"] {
        let script = format!(
            "language(1)\nindicator(\"BadWindow\", id = \"test.badwindow\")\nformat(precision = \"base_ticker_min_tick\")\nplot(ta.sma(chart.close, {bad_len}), id = \"sma\")"
        );
        let res = compile(&script, "badwindow.ind");
        if let Ok(compiled) = res {
            let inst_res = compiled
                .instantiate_with_limits(IndicatorConfig::default(), RuntimeLimits::untrusted());
            assert!(
                inst_res.is_err()
                    || inst_res
                        .unwrap()
                        .evaluate_closed(make_frame(0, 1000, Some(100.0)))
                        .is_err(),
                "Window length {} should have failed",
                bad_len
            );
        }
    }
}

#[test]
fn test_adv_malformed_syntax_diagnostics() {
    let bad_scripts = [
        (
            "unterminated_string",
            "language(1)\nindicator(\"Unterm\nformat(precision = \"base_ticker_min_tick\")\nplot(chart.close)",
        ),
        (
            "invalid_tokens",
            "language(1)\nindicator(\"Bad\"))\nformat(precision = \"base_ticker_min_tick\")\nplot(chart.close)",
        ),
        (
            "missing_format",
            "language(1)\nindicator(\"NoFormat\", id = \"test.noformat\")\nplot(chart.close)",
        ),
        (
            "duplicate_indicator",
            "language(1)\nindicator(\"A\", id = \"test.a\")\nindicator(\"B\", id = \"test.b\")\nplot(chart.close)",
        ),
        (
            "type_error_add_string_num",
            "language(1)\nindicator(\"TypeError\", id = \"test.te\")\nformat(precision = \"base_ticker_min_tick\")\nx = \"hello\" + 10\nplot(x)",
        ),
    ];

    for (name, script) in bad_scripts {
        let result = compile(script, format!("{name}.ind"));
        assert!(result.is_err(), "Script {} should fail compilation", name);
        let diags = result.unwrap_err();
        assert!(!diags.is_empty(), "Script {} should have diagnostics", name);
    }
}

#[test]
fn test_adv_input_validation_boundaries() {
    let script = r#"
language(1)
indicator("InputBound", id = "test.inputbound")
format(precision = "base_ticker_min_tick")

period = input.int(14, "Period", min = 2, max = 50)
plot(ta.sma(chart.close, period), id = "sma")
"#;

    let compiled = compile(script, "inputbound.ind").expect("compile ok");

    // Test 1: Int above max (51 > 50)
    let bad_config_high =
        IndicatorConfig::default().with("period", indicator_engine::InputValue::Int(51));
    let inst_res = compiled.instantiate(bad_config_high);
    assert!(inst_res.is_err(), "Int above max must be rejected");

    // Test 2: Int below min (1 < 2)
    let bad_config_low =
        IndicatorConfig::default().with("period", indicator_engine::InputValue::Int(1));
    let inst_res = compiled.instantiate(bad_config_low);
    assert!(inst_res.is_err(), "Int below min must be rejected");

    // Test 3: String instead of Int
    let bad_config_type = IndicatorConfig::default()
        .with("period", indicator_engine::InputValue::String("14".into()));
    let inst_res = compiled.instantiate(bad_config_type);
    assert!(inst_res.is_err(), "Type mismatch must be rejected");

    // Test 4: Valid within bounds (20)
    let good_config =
        IndicatorConfig::default().with("period", indicator_engine::InputValue::Int(20));
    let inst_res = compiled.instantiate(good_config);
    assert!(inst_res.is_ok(), "Valid config must be accepted");
}

#[test]
fn test_adv_heavy_multi_ta_20k_bars_stress() {
    let script = r#"
language(1)
indicator("MultiTA", id = "test.multita")
format(precision = "base_ticker_min_tick")

s = ta.sma(chart.close, 20)
e = ta.ema(chart.close, 20)
r = ta.rsi(chart.close, 14)
sd = ta.stdev(chart.close, 20)
c = ta.cum(chart.volume)

plot(s, id = "sma")
plot(e, id = "ema")
plot(r, id = "rsi")
plot(sd, id = "stdev")
plot(c, id = "cum")
"#;

    let compiled = compile(script, "multita.ind").expect("compile ok");
    let mut instance = compiled
        .instantiate(IndicatorConfig::default())
        .expect("instantiate ok");

    let total_bars = 20_000;
    for i in 0..total_bars {
        let price = 50_000.0 + (i as f64 * 0.1).sin() * 5_000.0;
        let frame = make_frame(i, 1_700_000_000 + (i as i64 * 60), Some(price));

        let closed_out = instance
            .evaluate_closed(frame.clone())
            .expect("20k closed bars should evaluate without error");
        assert_eq!(closed_out.plots.len(), 5);

        // Every 50 bars, do a preview tick
        if i % 50 == 0 {
            let preview_frame = make_frame(
                i + 1,
                1_700_000_000 + ((i as i64 + 1) * 60),
                Some(price + 10.0),
            );
            let prev_out = instance
                .preview_open(preview_frame)
                .expect("preview should evaluate");
            assert_eq!(prev_out.plots.len(), 5);
        }
    }
}
