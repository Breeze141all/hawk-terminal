use exchange::Kline;
use exchange::util::Price;
use hawk_data::chart::kline::indicators::adapter::kline_to_indicator_frame;
use hawk_data::chart::kline::indicators::core::IndicatorHost;
use hawk_data::chart::kline::indicators::instance::IndicatorInstanceStore;
use hawk_data::chart::kline::indicators::loader::initialize_indicator_catalog;
use hawk_data::chart::kline::indicators::registry::{catalog, IndicatorId};
use hawk_data::chart::kline::indicators::scripting::ScriptIndicatorRuntime;
use hawk_data::chart::kline::indicators::{IndicatorOutputRef, OutputValue};
use indicator_engine::IndicatorFrame;
use std::sync::Arc;

fn ensure_catalog() {
    let _ = initialize_indicator_catalog();
}

fn create_kline(index: usize, open: f64, high: f64, low: f64, close: f64, vol: (f32, f32)) -> Kline {
    Kline {
        time: 1_700_000_000_000 + (index as u64 * 60_000),
        open: Price::from_f64(open),
        high: Price::from_f64(high),
        low: Price::from_f64(low),
        close: Price::from_f64(close),
        volume: vol,
    }
}

fn generate_frames(count: usize) -> Vec<IndicatorFrame> {
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let phase = i as f64 * 0.15;
        let base = 100.0 + phase.sin() * 15.0 + (i as f64 * 0.05);
        let open = base;
        let high = base + 2.0 + (i % 5) as f64 * 0.5;
        let low = base - 2.0 - (i % 3) as f64 * 0.4;
        let close = base + phase.cos() * 1.5;
        let vol = (50.0 + (i % 10) as f32 * 5.0, 40.0 + (i % 7) as f32 * 4.0);
        let kline = create_kline(i, open, high, low, close, vol);
        frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }
    frames
}

// ============================================================================
// 1. Multi-Indicator Topological DAG Scheduling
// ============================================================================

#[test]
fn test_dag_deep_linear_dependency_chain() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_bb = catalog().get(&IndicatorId::new("core.bollinger-bands").unwrap()).unwrap();

    // Pipeline: Kline -> MA1 -> MA2 -> RSI -> BB
    let id_ma1 = instances.create(reg_ma.id.clone());
    let id_ma2 = instances.create(reg_ma.id.clone());
    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_bb = instances.create(reg_bb.id.clone());

    // Configure periods
    let mut cfg_ma1 = reg_ma.default_config();
    cfg_ma1.set("period", 5i64);
    instances.update_config(id_ma1, cfg_ma1);

    let mut cfg_ma2 = reg_ma.default_config();
    cfg_ma2.set("period", 5i64);
    instances.update_config(id_ma2, cfg_ma2);

    // Bindings
    assert!(instances.update_source_binding(
        id_ma2,
        "source",
        Some(IndicatorOutputRef {
            instance_id: id_ma1,
            channel_id: "value".to_owned(),
        }),
    ));

    assert!(instances.update_source_binding(
        id_rsi,
        "source",
        Some(IndicatorOutputRef {
            instance_id: id_ma2,
            channel_id: "value".to_owned(),
        }),
    ));

    assert!(instances.update_source_binding(
        id_bb,
        "source",
        Some(IndicatorOutputRef {
            instance_id: id_rsi,
            channel_id: "value".to_owned(),
        }),
    ));

    // Deliberately shuffle active order to verify topological sort invariance
    let active = vec![id_bb, id_ma2, id_ma1, id_rsi];
    let frames = generate_frames(150);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    // Verify dependencies were tracked
    let deps_bb = host.calculation_ids(&[id_bb]);
    assert_eq!(deps_bb.len(), 4, "BB calculation must require all 4 nodes");

    // Output checks: At bar 149, all indicators in the chain must produce valid values
    let val_ma1 = host.output_value_for(id_ma1, "value", 149).and_then(|v| v.as_number());
    let val_ma2 = host.output_value_for(id_ma2, "value", 149).and_then(|v| v.as_number());
    let val_rsi = host.output_value_for(id_rsi, "value", 149).and_then(|v| v.as_number());
    let val_bb = host.output_value_for(id_bb, "value", 149).and_then(|v| v.as_number());
    let val_upper = host.output_value_for(id_bb, "bollinger.upper", 149).and_then(|v| v.as_number());
    let val_lower = host.output_value_for(id_bb, "bollinger.lower", 149).and_then(|v| v.as_number());

    assert!(val_ma1.is_some(), "MA1 must have value");
    assert!(val_ma2.is_some(), "MA2 must have value");
    assert!(val_rsi.is_some(), "RSI must have value");
    assert!(val_bb.is_some(), "BB basis must have value");
    assert!(val_upper.is_some(), "BB upper must have value");
    assert!(val_lower.is_some(), "BB lower must have value");

    let upper = val_upper.unwrap();
    let basis = val_bb.unwrap();
    let lower = val_lower.unwrap();
    assert!(upper >= basis && basis >= lower, "BB geometric invariants must hold");

    // Active tick preview on active bar 150
    let preview_kline = create_kline(150, 110.0, 112.0, 109.0, 111.5, (10.0, 10.0));
    let preview_frame = kline_to_indicator_frame(150, &preview_kline, None, None);
    host.preview_open(&preview_frame);

    assert!(host.output_value_for(id_bb, "value", 150).is_some());
}

#[test]
fn test_dag_diamond_dependency_resolution() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_bb = catalog().get(&IndicatorId::new("core.bollinger-bands").unwrap()).unwrap();

    // Diamond:
    //      Root: MA_root
    //      /         \
    //   MA_left     RSI_right
    //      \
    //     BB_sink (drawn from MA_left)
    let id_root = instances.create(reg_ma.id.clone());
    let id_left = instances.create(reg_ma.id.clone());
    let id_right = instances.create(reg_rsi.id.clone());
    let id_sink = instances.create(reg_bb.id.clone());

    let mut cfg_root = reg_ma.default_config();
    cfg_root.set("fast_period", 5i64);
    instances.update_config(id_root, cfg_root);

    let mut cfg_left = reg_ma.default_config();
    cfg_left.set("fast_period", 5i64);
    instances.update_config(id_left, cfg_left);

    let mut cfg_sink = reg_bb.default_config();
    cfg_sink.set("period", 5i64);
    instances.update_config(id_sink, cfg_sink);

    instances.update_source_binding(id_left, "source", Some(IndicatorOutputRef {
        instance_id: id_root,
        channel_id: "value".to_owned(),
    }));
    instances.update_source_binding(id_right, "source", Some(IndicatorOutputRef {
        instance_id: id_root,
        channel_id: "value".to_owned(),
    }));
    instances.update_source_binding(id_sink, "source", Some(IndicatorOutputRef {
        instance_id: id_left,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_sink, id_right, id_left, id_root];
    let frames = generate_frames(100);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    let dependent_on_root = host.dependent_ids(&[id_root]);
    assert!(dependent_on_root.contains(&id_left));
    assert!(dependent_on_root.contains(&id_right));
    assert!(dependent_on_root.contains(&id_sink));


    assert!(host.output_value_for(id_root, "value", 99).is_some());
    assert!(host.output_value_for(id_left, "value", 99).is_some());
    assert!(host.output_value_for(id_right, "value", 99).is_some());
    assert!(host.output_value_for(id_sink, "value", 99).is_some());
}

// ============================================================================
// 2. Dependency Cycles & Resilient Isolation
// ============================================================================

#[test]
fn test_dag_cycle_rejection_and_healthy_branch_isolation() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();

    // Cycle pair: MA_A <-> MA_B
    let id_cycle_a = instances.create(reg_ma.id.clone());
    let id_cycle_b = instances.create(reg_ma.id.clone());

    instances.update_source_binding(id_cycle_a, "source", Some(IndicatorOutputRef {
        instance_id: id_cycle_b,
        channel_id: "value".to_owned(),
    }));
    instances.update_source_binding(id_cycle_b, "source", Some(IndicatorOutputRef {
        instance_id: id_cycle_a,
        channel_id: "value".to_owned(),
    }));

    // Healthy independent pair: RSI_C -> MA_D
    let id_healthy_c = instances.create(reg_rsi.id.clone());
    let id_healthy_d = instances.create(reg_ma.id.clone());

    instances.update_source_binding(id_healthy_d, "source", Some(IndicatorOutputRef {
        instance_id: id_healthy_c,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_cycle_a, id_healthy_c, id_cycle_b, id_healthy_d];

    // 1. validate_bindings must catch cycle
    let host = IndicatorHost::default();
    let val_result = host.validate_bindings(&active, &instances);
    assert!(val_result.is_err(), "validate_bindings must detect cycle");
    assert!(
        val_result.unwrap_err().contains("cycle"),
        "error must mention dependency cycle"
    );

    // 2. recalculate must NOT crash or hang, must isolate cycle and execute healthy branch
    let mut host = IndicatorHost::default();
    let frames = generate_frames(80);
    host.recalculate(&active, &instances, &frames);

    // Cycle nodes must have diagnostics and return None
    assert!(host.diagnostic_for(id_cycle_a).is_some());
    assert!(host.diagnostic_for(id_cycle_b).is_some());
    assert!(host.output_value_for(id_cycle_a, "value", 79).is_none());
    assert!(host.output_value_for(id_cycle_b, "value", 79).is_none());

    // Healthy branch must evaluate completely normally!
    assert!(host.diagnostic_for(id_healthy_c).is_none());
    assert!(host.diagnostic_for(id_healthy_d).is_none());
    assert!(host.output_value_for(id_healthy_c, "value", 79).is_some());
    assert!(host.output_value_for(id_healthy_d, "value", 79).is_some());

    // Active preview on active bar 80
    let preview_kline = create_kline(80, 105.0, 106.0, 104.0, 105.5, (10.0, 10.0));
    let preview_frame = kline_to_indicator_frame(80, &preview_kline, None, None);
    host.preview_open(&preview_frame);

    assert!(host.output_value_for(id_healthy_c, "value", 80).is_some());
    assert!(host.output_value_for(id_healthy_d, "value", 80).is_some());
    assert!(host.output_value_for(id_cycle_a, "value", 80).is_none());
}

#[test]
fn test_dag_self_reference_rejection() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();

    let id_self = instances.create(reg_ma.id.clone());
    let id_healthy = instances.create(reg_rsi.id.clone());

    instances.update_source_binding(id_self, "source", Some(IndicatorOutputRef {
        instance_id: id_self,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_self, id_healthy];
    let host = IndicatorHost::default();
    let val_result = host.validate_bindings(&active, &instances);
    assert!(val_result.is_err(), "Self-reference binding must fail validation");

    let mut host = IndicatorHost::default();
    let frames = generate_frames(60);
    host.recalculate(&active, &instances, &frames);

    assert!(host.diagnostic_for(id_self).is_some());
    assert!(host.output_value_for(id_self, "value", 59).is_none());
    assert!(host.output_value_for(id_healthy, "value", 59).is_some());
}

// ============================================================================
// 3. Fault Injection & Quarantine Isolation
// ============================================================================

#[test]
fn test_fault_injection_runtime_timeout_quarantine_isolation() {
    // Compile a synthetic script with an infinite loop
    let timeout_script = r#"language(1)
indicator("Timeout Fault", id = "test.fault-timeout")
format(precision = "fixed_step:1")
plot(1.0)
while true {
}
"#;
    let compiled_timeout = indicator_engine::compile(timeout_script, std::path::Path::new("timeout.ind")).unwrap();
    let mut failing_runtime = ScriptIndicatorRuntime::new(
        Arc::new(compiled_timeout),
        hawk_data::chart::kline::indicators::registry::IndicatorConfig::default(),
    ).unwrap();

    // Compile healthy RSI and Bollinger Bands from bundled sources
    let rsi_script = include_str!("../indicators/sources/rsi.ind");
    let compiled_rsi = indicator_engine::compile(rsi_script, std::path::Path::new("rsi.ind")).unwrap();
    let mut healthy_rsi = ScriptIndicatorRuntime::new(
        Arc::new(compiled_rsi),
        hawk_data::chart::kline::indicators::registry::IndicatorConfig::default(),
    ).unwrap();

    let bb_script = include_str!("../indicators/sources/bollinger-bands.ind");
    let compiled_bb = indicator_engine::compile(bb_script, std::path::Path::new("bb.ind")).unwrap();
    let mut healthy_bb = ScriptIndicatorRuntime::new(
        Arc::new(compiled_bb),
        hawk_data::chart::kline::indicators::registry::IndicatorConfig::default(),
    ).unwrap();

    let frames = generate_frames(50);

    for frame in &frames {
        // Step failing runtime
        failing_runtime.evaluate_closed(frame);
        // Step healthy runtimes
        healthy_rsi.evaluate_closed(frame);
        healthy_bb.evaluate_closed(frame);
    }

    // Failing runtime must be quarantined
    assert!(failing_runtime.is_quarantined(), "Faulty indicator must be quarantined");
    assert!(failing_runtime.diagnostic().is_some());
    let diag = failing_runtime.diagnostic().unwrap();
    assert!(
        diag.contains("execution limit exceeded"),
        "Diagnostic must state execution limit exceeded: {diag}"
    );
    assert!(failing_runtime.output_value("plot_0", 49).is_none());

    // Both healthy indicators must have evaluated all 50 bars with ZERO degradation!
    assert!(!healthy_rsi.is_quarantined());
    assert!(!healthy_bb.is_quarantined());
    assert!(healthy_rsi.output_value("value", 49).is_some());
    assert!(healthy_bb.output_value("value", 49).is_some());
    assert!(healthy_bb.output_value("bollinger.upper", 49).is_some());

    // Preview open on active bar 50
    let preview_kline = create_kline(50, 115.0, 116.0, 114.0, 115.5, (5.0, 5.0));
    let preview_frame = kline_to_indicator_frame(50, &preview_kline, None, None);

    failing_runtime.preview_open(&preview_frame);
    healthy_rsi.preview_open(&preview_frame);
    healthy_bb.preview_open(&preview_frame);

    assert!(failing_runtime.is_quarantined());
    assert!(failing_runtime.output_value("plot_0", 50).is_none());
    assert!(healthy_rsi.output_value("value", 50).is_some());
    assert!(healthy_bb.output_value("value", 50).is_some());
}

#[test]
fn test_fault_injection_reconfiguration_error_resilience() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());

    let active = vec![id_rsi, id_ma];
    let frames = generate_frames(50);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    assert!(host.output_value_for(id_rsi, "value", 49).is_some());
    assert!(host.output_value_for(id_ma, "value", 49).is_some());

    // Inject invalid configuration into RSI directly in store (simulate corrupted config)
    let mut bad_config = reg_rsi.default_config();
    bad_config.set("period", -500i64); // negative period violates unsigned contract

    // Reconfigure through host
    let mut bad_instance = instances.get(id_rsi).unwrap().clone();
    bad_instance.config = bad_config;

    host.reconfigure(&bad_instance, &instances);

    // Host must record diagnostic and drop/quarantine the bad instance without corrupting MA
    assert!(host.diagnostic_for(id_rsi).is_some(), "RSI must have reconfiguration diagnostic");
    assert!(host.output_value_for(id_rsi, "value", 49).is_none(), "RSI outputs purged on failure");

    // MA must remain intact and evaluate further bars
    let kline_50 = create_kline(50, 120.0, 121.0, 119.0, 120.5, (10.0, 10.0));
    let frame_50 = kline_to_indicator_frame(50, &kline_50, None, None);
    host.evaluate_closed(&frame_50);

    assert!(host.output_value_for(id_ma, "value", 50).is_some());
}

// ============================================================================
// 4. Dynamic Add / Remove of Indicators at Runtime
// ============================================================================

#[test]
fn test_dynamic_add_and_remove_indicator_runtime_isolation() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_bb = catalog().get(&IndicatorId::new("core.bollinger-bands").unwrap()).unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());

    let mut active = vec![id_rsi, id_ma];
    let frames_100 = generate_frames(100);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames_100);

    // Capture outputs of RSI and MA before adding BB
    let rsi_vals_before: Vec<Option<f64>> = (0..100)
        .map(|i| host.output_value_for(id_rsi, "value", i).and_then(|v| v.as_number()))
        .collect();
    let ma_vals_before: Vec<Option<f64>> = (0..100)
        .map(|i| host.output_value_for(id_ma, "value", i).and_then(|v| v.as_number()))
        .collect();

    // Dynamically register BB at bar 100
    let id_bb = instances.create(reg_bb.id.clone());
    active.push(id_bb);

    let changed = host.set_active(&active, &instances);
    assert_eq!(changed, vec![id_bb], "Only newly added BB should be marked changed");

    // Recalculate only the newly added BB over historical frames
    host.recalculate_selected(&changed, &instances, &frames_100);

    // Verify BB now has values
    assert!(host.output_value_for(id_bb, "value", 99).is_some());

    // CRITICAL INVARIANT: RSI and MA outputs must remain 100% BIT-FOR-BIT IDENTICAL!
    for i in 0..100 {
        let rsi_after = host.output_value_for(id_rsi, "value", i).and_then(|v| v.as_number());
        let ma_after = host.output_value_for(id_ma, "value", i).and_then(|v| v.as_number());

        assert_eq!(rsi_vals_before[i], rsi_after, "Bar {i}: RSI output altered by adding BB!");
        assert_eq!(ma_vals_before[i], ma_after, "Bar {i}: MA output altered by adding BB!");
    }

    // Now dynamically REMOVE MA
    let active_without_ma = vec![id_rsi, id_bb];
    host.set_active(&active_without_ma, &instances);

    assert!(!host.has_entries(&[id_ma]), "MA must be removed from host");
    assert!(host.has_entries(&[id_rsi, id_bb]), "RSI and BB must remain");

    // Feed bar 100
    let kline_100 = create_kline(100, 110.0, 111.0, 109.0, 110.5, (10.0, 10.0));
    let frame_100 = kline_to_indicator_frame(100, &kline_100, None, None);
    host.evaluate_closed(&frame_100);

    assert!(host.output_value_for(id_rsi, "value", 100).is_some());
    assert!(host.output_value_for(id_bb, "value", 100).is_some());
    assert!(host.output_value_for(id_ma, "value", 100).is_none());
}

// ============================================================================
// 5. Warmup Lookback Correctness
// ============================================================================

#[test]
fn test_warmup_lookback_dependency_accumulation() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());

    // Configure MA period to 200 bars
    let mut ma_cfg = reg_ma.default_config();
    ma_cfg.set("period", 200i64);
    instances.update_config(id_ma, ma_cfg);

    // Bind MA to RSI
    instances.update_source_binding(id_ma, "source", Some(IndicatorOutputRef {
        instance_id: id_rsi,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_rsi, id_ma];

    // Standalone RSI warmup is ~14-20 bars
    let rsi_warmup = reg_rsi.warmup_bars(&reg_rsi.default_config()).unwrap_or(0);
    assert!(rsi_warmup >= 14, "RSI warmup must be >= 14");

    // Dependency warmup must be own warmup (200) + dependency warmup (rsi_warmup)
    let total_warmup = instances.dependency_warmup_bars(&active);
    assert!(
        total_warmup >= 200 + rsi_warmup,
        "Total warmup {total_warmup} must accumulate both nodes (>= {})",
        200 + rsi_warmup
    );
}

#[test]
fn test_warmup_slice_lookback_equivalence() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    let id_ma = instances.create(reg_ma.id.clone());
    let mut ma_cfg = reg_ma.default_config();
    ma_cfg.set("period", 50i64);
    instances.update_config(id_ma, ma_cfg);

    let active = vec![id_ma];
    let all_frames = generate_frames(300);

    // Host 1: evaluates all 300 frames
    let mut host_full = IndicatorHost::default();
    host_full.recalculate(&active, &instances, &all_frames);
    let full_val_299 = host_full.output_value_for(id_ma, "value", 299).and_then(|v| v.as_number()).unwrap();

    // Host 2: evaluates only the required warmup lookback slice (bars 240..300, 60 bars > 50-bar period)
    let slice_frames = &all_frames[240..300];
    let mut host_slice = IndicatorHost::default();
    host_slice.recalculate(&active, &instances, slice_frames);
    let slice_val_299 = host_slice.output_value_for(id_ma, "value", 299).and_then(|v| v.as_number()).unwrap();

    // MA on exact same data over warmup lookback must match within machine precision!
    assert!(
        (full_val_299 - slice_val_299).abs() < 1e-10,
        "Warmup slice value {slice_val_299} must equal full run value {full_val_299}"
    );
}

// ============================================================================
// 6. Strict Numerical Determinism & Preview Isolation
// ============================================================================

#[test]
fn test_numerical_determinism_across_multiple_host_instances() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();
    let reg_bb = catalog().get(&IndicatorId::new("core.bollinger-bands").unwrap()).unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());
    let id_bb = instances.create(reg_bb.id.clone());

    // Chain MA onto RSI
    instances.update_source_binding(id_ma, "source", Some(IndicatorOutputRef {
        instance_id: id_rsi,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_rsi, id_ma, id_bb];
    let frames = generate_frames(250);

    // Run 1
    let mut host_1 = IndicatorHost::default();
    host_1.recalculate(&active, &instances, &frames);

    // Run 2 (completely independent host instance)
    let mut host_2 = IndicatorHost::default();
    host_2.recalculate(&active, &instances, &frames);

    // Verify bit-for-bit identity across every single bar and channel
    for i in 0..250 {
        let v1_rsi = host_1.output_value_for(id_rsi, "value", i);
        let v2_rsi = host_2.output_value_for(id_rsi, "value", i);
        assert_eq!(v1_rsi, v2_rsi, "Bar {i}: RSI determinism mismatch");

        let v1_ma = host_1.output_value_for(id_ma, "value", i);
        let v2_ma = host_2.output_value_for(id_ma, "value", i);
        assert_eq!(v1_ma, v2_ma, "Bar {i}: MA determinism mismatch");

        let v1_upper = host_1.output_value_for(id_bb, "bollinger.upper", i);
        let v2_upper = host_2.output_value_for(id_bb, "bollinger.upper", i);
        assert_eq!(v1_upper, v2_upper, "Bar {i}: BB upper determinism mismatch");

        if let (Some(OutputValue::Number(n1)), Some(OutputValue::Number(n2))) = (v1_rsi, v2_rsi) {
            assert_eq!(n1.to_bits(), n2.to_bits(), "Bar {i}: RSI bits mismatch");
        }
    }
}

#[test]
fn test_transactional_preview_open_zero_leakage_to_closed_commitment() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    let id_rsi = instances.create(reg_rsi.id.clone());
    let id_ma = instances.create(reg_ma.id.clone());
    let active = vec![id_rsi, id_ma];

    let frames_100 = generate_frames(100);

    // Host Clean: Evaluates frames 0..100 closed, then frame 101 closed
    let mut host_clean = IndicatorHost::default();
    host_clean.recalculate(&active, &instances, &frames_100);

    let kline_101_final = create_kline(100, 115.0, 118.0, 114.0, 117.5, (25.0, 30.0));
    let frame_101_final = kline_to_indicator_frame(100, &kline_101_final, None, None);
    host_clean.evaluate_closed(&frame_101_final);

    // Host Storm: Evaluates frames 0..100 closed, then 500 volatile preview ticks on bar 101, then frame 101 closed
    let mut host_storm = IndicatorHost::default();
    host_storm.recalculate(&active, &instances, &frames_100);

    for tick in 0..500 {
        let price = 110.0 + ((tick % 25) as f64 * 0.4) - ((tick % 13) as f64 * 0.3);
        let kline_preview = create_kline(100, 115.0, price + 1.0, price - 1.0, price, (1.0, 1.0));
        let frame_preview = kline_to_indicator_frame(100, &kline_preview, None, None);
        host_storm.preview_open(&frame_preview);
    }

    // Now commit the exact same final candle
    host_storm.evaluate_closed(&frame_101_final);

    // Committed outputs must be 100% bit-for-bit identical between Host Clean and Host Storm
    let rsi_clean = host_clean.output_value_for(id_rsi, "value", 100).and_then(|v| v.as_number()).unwrap();
    let rsi_storm = host_storm.output_value_for(id_rsi, "value", 100).and_then(|v| v.as_number()).unwrap();
    assert_eq!(
        rsi_clean.to_bits(),
        rsi_storm.to_bits(),
        "RSI: Preview storm leaked into closed commitment! clean={rsi_clean}, storm={rsi_storm}"
    );

    let ma_clean = host_clean.output_value_for(id_ma, "value", 100).and_then(|v| v.as_number()).unwrap();
    let ma_storm = host_storm.output_value_for(id_ma, "value", 100).and_then(|v| v.as_number()).unwrap();
    assert_eq!(
        ma_clean.to_bits(),
        ma_storm.to_bits(),
        "MA: Preview storm leaked into closed commitment! clean={ma_clean}, storm={ma_storm}"
    );
}

#[test]
fn test_dag_deep_linear_10_node_stress() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    const DEPTH: usize = 10;
    let mut ids = Vec::with_capacity(DEPTH);

    for i in 0..DEPTH {
        let id = instances.create(reg_ma.id.clone());
        let mut cfg = reg_ma.default_config();
        cfg.set("fast_period", 3i64);
        instances.update_config(id, cfg);

        if i > 0 {
            instances.update_source_binding(id, "source", Some(IndicatorOutputRef {
                instance_id: ids[i - 1],
                channel_id: "value".to_owned(),
            }));
        }
        ids.push(id);
    }

    // Deliberately reverse active array to stress topological sorter
    let mut active = ids.clone();
    active.reverse();

    // 10 nodes with period 3 require ~30 bars to warm up
    let frames = generate_frames(80);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    // Verify all 10 nodes are evaluated
    for (i, id) in ids.iter().enumerate() {
        assert!(
            host.output_value_for(*id, "value", 79).is_some(),
            "Node {i} in 10-node chain must have output at bar 79"
        );
    }
}

#[test]
fn test_fault_cascading_isolation_downstream_resilience() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    // Node A: RSI (will be broken)
    let id_a = instances.create(reg_rsi.id.clone());
    // Node B: MA depending on A
    let id_b = instances.create(reg_ma.id.clone());
    // Node C: MA depending on B
    let id_c = instances.create(reg_ma.id.clone());
    // Node D: Independent healthy RSI
    let id_d = instances.create(reg_rsi.id.clone());

    let mut cfg_b = reg_ma.default_config();
    cfg_b.set("fast_period", 5i64);
    instances.update_config(id_b, cfg_b);

    let mut cfg_c = reg_ma.default_config();
    cfg_c.set("fast_period", 5i64);
    instances.update_config(id_c, cfg_c);

    instances.update_source_binding(id_b, "source", Some(IndicatorOutputRef {
        instance_id: id_a,
        channel_id: "value".to_owned(),
    }));
    instances.update_source_binding(id_c, "source", Some(IndicatorOutputRef {
        instance_id: id_b,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_a, id_b, id_c, id_d];
    let frames = generate_frames(60);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    // Verify initial healthy state
    assert!(host.output_value_for(id_a, "value", 59).is_some());
    assert!(host.output_value_for(id_b, "value", 59).is_some());
    assert!(host.output_value_for(id_c, "value", 59).is_some());
    assert!(host.output_value_for(id_d, "value", 59).is_some());

    // Break Node A by invalidating its configuration
    let mut bad_config = reg_rsi.default_config();
    bad_config.set("period", -999i64);
    let mut bad_instance = instances.get(id_a).unwrap().clone();
    bad_instance.config = bad_config;

    host.reconfigure(&bad_instance, &instances);

    // Node A is dropped/quarantined
    assert!(host.diagnostic_for(id_a).is_some());

    // Feed new bar 60: Downstream nodes B and C receive None for inputs, must NOT panic
    let kline_60 = create_kline(60, 110.0, 112.0, 109.0, 111.0, (10.0, 10.0));
    let frame_60 = kline_to_indicator_frame(60, &kline_60, None, None);
    host.evaluate_closed(&frame_60);

    // Node D (independent) must evaluate with zero degradation!
    assert!(host.output_value_for(id_d, "value", 60).is_some());

    // Active preview must also not panic
    host.preview_open(&frame_60);
    assert!(host.output_value_for(id_d, "value", 60).is_some());
}

#[test]
fn test_dynamic_cycle_injection_and_unblocking_recovery() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_ma = catalog().get(&IndicatorId::new("core.moving-averages").unwrap()).unwrap();

    let id_a = instances.create(reg_ma.id.clone());
    let id_b = instances.create(reg_ma.id.clone());
    let id_c = instances.create(reg_ma.id.clone());

    let mut cfg = reg_ma.default_config();
    cfg.set("fast_period", 5i64);
    instances.update_config(id_a, cfg.clone());
    instances.update_config(id_b, cfg.clone());
    instances.update_config(id_c, cfg);

    // Linear chain: A -> B -> C
    instances.update_source_binding(id_b, "source", Some(IndicatorOutputRef {
        instance_id: id_a,
        channel_id: "value".to_owned(),
    }));
    instances.update_source_binding(id_c, "source", Some(IndicatorOutputRef {
        instance_id: id_b,
        channel_id: "value".to_owned(),
    }));

    let active = vec![id_a, id_b, id_c];
    let frames = generate_frames(50);

    let mut host = IndicatorHost::default();
    host.recalculate(&active, &instances, &frames);

    assert!(host.output_value_for(id_c, "value", 49).is_some());
    assert!(host.diagnostic_for(id_a).is_none());

    // INJECT CYCLE: Set A source to C (A -> B -> C -> A)
    instances.update_source_binding(id_a, "source", Some(IndicatorOutputRef {
        instance_id: id_c,
        channel_id: "value".to_owned(),
    }));

    host.set_active(&active, &instances);

    // All nodes in cycle must now be detected and blocked
    assert!(host.diagnostic_for(id_a).is_some());
    assert!(host.diagnostic_for(id_b).is_some());
    assert!(host.diagnostic_for(id_c).is_some());

    // Feeding bar 50 must not hang or panic
    let kline_50 = create_kline(50, 115.0, 116.0, 114.0, 115.0, (10.0, 10.0));
    let frame_50 = kline_to_indicator_frame(50, &kline_50, None, None);
    host.evaluate_closed(&frame_50);

    // BREAK CYCLE: Remove A's binding to C
    instances.update_source_binding(id_a, "source", None);
    host.set_active(&active, &instances);

    // Diagnostics must be cleared and nodes unblocked!
    assert!(host.diagnostic_for(id_a).is_none(), "A must be unblocked");
    assert!(host.diagnostic_for(id_b).is_none(), "B must be unblocked");
    assert!(host.diagnostic_for(id_c).is_none(), "C must be unblocked");
}

#[test]
fn test_boundary_conditions_zero_and_single_frame() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_rsi = catalog().get(&IndicatorId::new("core.rsi").unwrap()).unwrap();
    let id_rsi = instances.create(reg_rsi.id.clone());
    let active = vec![id_rsi];

    let mut host = IndicatorHost::default();

    // 1. Zero frames: Must not panic
    host.recalculate(&active, &instances, &[]);
    assert!(host.output_value_for(id_rsi, "value", 0).is_none());

    // 2. Single frame: Must not panic
    let kline_0 = create_kline(0, 100.0, 101.0, 99.0, 100.0, (10.0, 10.0));
    let frame_0 = kline_to_indicator_frame(0, &kline_0, None, None);
    host.recalculate(&active, &instances, std::slice::from_ref(&frame_0));

    // 3. Repeated previews on single frame
    for _ in 0..10 {
        host.preview_open(&frame_0);
    }

    // 4. Recalculate selected with empty frames
    host.recalculate_selected(&[id_rsi], &instances, &[]);
    assert!(host.has_entries(&[id_rsi]));
}

#[test]
fn test_numerical_determinism_under_synthetic_gap_and_spikes() {
    ensure_catalog();
    let mut instances = IndicatorInstanceStore::default();
    let reg_bb = catalog().get(&IndicatorId::new("core.bollinger-bands").unwrap()).unwrap();
    let id_bb = instances.create(reg_bb.id.clone());
    let active = vec![id_bb];

    // Generate extreme frames: massive spikes, near-zero prices, zero volume
    let mut extreme_frames = Vec::new();
    for i in 0..100 {
        let price = match i % 10 {
            0 => 1e-4,
            5 => 1e7,
            _ => 100.0 + (i as f64 * 2.5),
        };
        let vol = if i % 3 == 0 { (0.0, 0.0) } else { (100.0, 50.0) };
        let kline = create_kline(i, price, price * 1.05, price * 0.95, price, vol);
        extreme_frames.push(kline_to_indicator_frame(i, &kline, None, None));
    }

    let mut host1 = IndicatorHost::default();
    host1.recalculate(&active, &instances, &extreme_frames);

    let mut host2 = IndicatorHost::default();
    host2.recalculate(&active, &instances, &extreme_frames);

    for i in 0..100 {
        let v1 = host1.output_value_for(id_bb, "value", i);
        let v2 = host2.output_value_for(id_bb, "value", i);
        assert_eq!(v1, v2, "Extreme bar {i}: value determinism mismatch");
    }
}

