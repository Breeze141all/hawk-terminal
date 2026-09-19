use data::config::bundle::*;
use data::layout::dashboard::Dashboard;
use data::layout::pane::{Axis, Pane};
use data::layout::{Layout, WindowSpec};
use data::{Theme, UserTimezone};

#[test]
fn test_single_layout_bundle_export_import() {
    let pane = Pane::Split {
        axis: Axis::Horizontal,
        ratio: 0.6,
        a: Box::new(Pane::default()),
        b: Box::new(Pane::default()),
    };

    let popouts = vec![(
        Pane::default(),
        WindowSpec {
            width: 800.0,
            height: 600.0,
            pos_x: 100.0,
            pos_y: 100.0,
        },
    )];

    let layout = Layout {
        name: "Scalping Pro".to_string(),
        dashboard: Dashboard {
            pane,
            popout: popouts,
        },
    };

    let bundle = ConfigBundle::new_layout(
        layout,
        Some(BundleMetadata {
            name: "Scalping Pro".to_string(),
            description: Some("Custom orderflow setup".to_string()),
            author: Some("TraderX".to_string()),
        }),
    );

    let json = bundle.to_json_pretty().expect("Must serialize");
    let imported = ConfigBundle::from_json(&json).expect("Must deserialize and validate");

    assert_eq!(imported.schema_version, 1);
    assert_eq!(imported.export_type, ExportType::Layout);
    assert_eq!(imported.metadata.as_ref().unwrap().name, "Scalping Pro");

    if let BundlePayload::Layout(l) = imported.payload {
        assert_eq!(l.name, "Scalping Pro");
        assert_eq!(l.dashboard.popout.len(), 1);
        assert_eq!(l.dashboard.popout[0].1.width, 800.0);
    } else {
        panic!("Expected Layout payload");
    }
}

#[test]
fn test_full_workspace_bundle_export_import() {
    let layouts = vec![
        Layout {
            name: "Main Desk".to_string(),
            dashboard: Dashboard::default(),
        },
        Layout {
            name: "Footprint 5m".to_string(),
            dashboard: Dashboard::default(),
        },
    ];

    let ws = WorkspaceBundle {
        layouts,
        active_layout: Some("Footprint 5m".to_string()),
        custom_theme: Some(Theme::default()),
        timezone: Some(UserTimezone::default()),
        tickers_table: None,
        audio_cfg: None,
        size_in_quote_ccy: Some(exchange::SizeUnit::Base),
        default_kline_config: None,
    };

    let bundle = ConfigBundle::new_workspace(
        ws,
        Some(BundleMetadata {
            name: "Complete Workspace".to_string(),
            description: None,
            author: Some("Trader".to_string()),
        }),
    );

    let json = bundle.to_json_pretty().expect("Must serialize");
    let imported = ConfigBundle::from_json(&json).expect("Must deserialize");

    assert_eq!(imported.export_type, ExportType::Workspace);
    if let BundlePayload::Workspace(w) = imported.payload {
        assert_eq!(w.layouts.len(), 2);
        assert_eq!(w.active_layout.as_deref(), Some("Footprint 5m"));
    } else {
        panic!("Expected Workspace payload");
    }
}

#[test]
fn test_dos_protection_payload_too_large() {
    // 5 MB + 1 byte
    let huge_str = " ".repeat(5 * 1024 * 1024 + 1);
    let result = ConfigBundle::from_json(&huge_str);
    assert!(matches!(
        result,
        Err(BundleValidationError::PayloadTooLarge(_))
    ));
}

#[test]
fn test_dos_protection_pane_tree_too_deep() {
    let mut pane = Pane::default();
    for _ in 0..18 {
        pane = Pane::Split {
            axis: Axis::Vertical,
            ratio: 0.5,
            a: Box::new(pane),
            b: Box::new(Pane::default()),
        };
    }

    let mut layout = Layout {
        name: "Too Deep".to_string(),
        dashboard: Dashboard {
            pane,
            popout: vec![],
        },
    };

    let result = sanitize_layout(&mut layout);
    assert_eq!(
        result,
        Err(BundleValidationError::PaneTreeTooDeep(MAX_PANE_DEPTH))
    );
}

#[test]
fn test_dos_protection_too_many_panes() {
    fn build_balanced_tree(depth: usize) -> Pane {
        if depth == 0 {
            Pane::default()
        } else {
            Pane::Split {
                axis: Axis::Horizontal,
                ratio: 0.5,
                a: Box::new(build_balanced_tree(depth - 1)),
                b: Box::new(build_balanced_tree(depth - 1)),
            }
        }
    }

    // 2^6 leaves + branches = 127 nodes (> 64)
    let pane = build_balanced_tree(6);
    let mut layout = Layout {
        name: "Too Many Panes".to_string(),
        dashboard: Dashboard {
            pane,
            popout: vec![],
        },
    };

    let result = sanitize_layout(&mut layout);
    assert_eq!(
        result,
        Err(BundleValidationError::TooManyPanes(MAX_PANES_PER_LAYOUT))
    );
}

#[test]
fn test_float_and_window_sanitization() {
    let mut spec = WindowSpec {
        width: f32::NAN,
        height: -100.0,
        pos_x: f32::INFINITY,
        pos_y: 9999999.0,
    };

    sanitize_window_spec(&mut spec);
    assert_eq!(spec.width, 1024.0);
    assert_eq!(spec.height, 768.0);
    assert_eq!(spec.pos_x, 0.0);
    assert_eq!(spec.pos_y, 0.0);

    let mut pane = Pane::Split {
        axis: Axis::Horizontal,
        ratio: 0.0001, // outside [0.05, 0.95]
        a: Box::new(Pane::default()),
        b: Box::new(Pane::default()),
    };
    let mut count = 0;
    let _ = validate_and_sanitize_pane(&mut pane, 1, &mut count);
    if let Pane::Split { ratio, .. } = pane {
        assert_eq!(ratio, 0.5);
    } else {
        panic!("Expected Split");
    }
}

#[test]
fn test_backward_compatible_legacy_formats() {
    // 1. Raw Layout JSON
    let raw_layout = Layout {
        name: "Old Style Layout".to_string(),
        dashboard: Dashboard::default(),
    };
    let raw_layout_json = serde_json::to_string(&raw_layout).unwrap();
    let parsed_layout = ConfigBundle::from_json(&raw_layout_json).expect("Must parse raw Layout");
    assert_eq!(parsed_layout.export_type, ExportType::Layout);

    // 2. Raw State JSON
    let raw_state = data::State {
        layout_manager: data::Layouts {
            layouts: vec![raw_layout],
            active_layout: Some("Old Style Layout".to_string()),
        },
        ..Default::default()
    };
    let raw_state_json = serde_json::to_string(&raw_state).unwrap();
    let parsed_state = ConfigBundle::from_json(&raw_state_json).expect("Must parse raw State");
    assert_eq!(parsed_state.export_type, ExportType::Workspace);
}

#[test]
fn test_workspace_bundle_kline_config_persistence() {
    use data::chart::kline::{Config, PositionFlowColors, TpoElementColor};

    let custom_colors = PositionFlowColors {
        new_longs: TpoElementColor::Cyan,
        new_shorts: TpoElementColor::Orange,
        long_forced_close: TpoElementColor::Purple,
        short_forced_close: TpoElementColor::White,
    };

    let mut custom_cfg = Config::factory_default();
    custom_cfg.position_flow_colors = custom_colors;
    custom_cfg.rolling_vwap_window_hours = 48;

    let ws = WorkspaceBundle {
        layouts: vec![Layout {
            name: "Test Layout".to_string(),
            dashboard: Dashboard::default(),
        }],
        active_layout: Some("Test Layout".to_string()),
        custom_theme: None,
        timezone: None,
        tickers_table: None,
        audio_cfg: None,
        size_in_quote_ccy: None,
        default_kline_config: Some(custom_cfg),
    };

    let bundle = ConfigBundle::new_workspace(ws, None);
    let json = bundle.to_json_pretty().expect("Must serialize");
    assert!(json.contains("position_flow_colors"));
    assert!(json.contains("rolling_vwap_window_hours"));

    let imported = ConfigBundle::from_json(&json).expect("Must deserialize");
    if let BundlePayload::Workspace(imported_ws) = imported.payload {
        assert_eq!(imported_ws.default_kline_config, Some(custom_cfg));
    } else {
        panic!("Expected Workspace payload");
    }

    // Verify backward compatibility: legacy State JSON without default_kline_config
    let raw_state = data::State {
        layout_manager: data::Layouts {
            layouts: vec![Layout {
                name: "Legacy Layout".to_string(),
                dashboard: Dashboard::default(),
            }],
            active_layout: Some("Legacy Layout".to_string()),
        },
        ..Default::default()
    };
    let mut state_json_val: serde_json::Value = serde_json::to_value(&raw_state).unwrap();
    state_json_val
        .as_object_mut()
        .unwrap()
        .remove("default_kline_config");
    let state_json_str = serde_json::to_string(&state_json_val).unwrap();
    let parsed_legacy =
        ConfigBundle::from_json(&state_json_str).expect("Must parse legacy State JSON");
    if let BundlePayload::Workspace(ws) = parsed_legacy.payload {
        assert_eq!(ws.default_kline_config, None);
    } else {
        panic!("Expected Workspace payload");
    }
}

#[test]
fn test_rolling_vwap_config_serde_backward_compat() {
    use data::chart::kline::{Config, RollingVwapColors, TpoElementColor};

    // 1. Deserializing legacy JSON without rolling_vwap_colors and rolling_vwap_show_panel
    let legacy_json = r#"{
        "position_flow_colors": {
            "new_longs": "Auto",
            "new_shorts": "Auto",
            "long_forced_close": "Auto",
            "short_forced_close": "Auto"
        },
        "rolling_vwap_window_hours": 24,
        "rolling_vwap_show_7d": true,
        "rolling_vwap_show_30d": true,
        "rolling_vwap_show_90d": true,
        "rolling_vwap_show_365d": true,
        "liq_show_bands": true,
        "liq_show_histogram": true,
        "magnet_mode": true
    }"#;

    let cfg: Config = serde_json::from_str(legacy_json).expect("Must deserialize legacy Config");
    assert_eq!(cfg.rolling_vwap_colors, RollingVwapColors::default());
    assert!(!cfg.rolling_vwap_show_panel);

    // 2. Custom rolling VWAP colors and panel serialization roundtrip
    let mut custom = Config::factory_default();
    custom.rolling_vwap_show_panel = true;
    custom.rolling_vwap_colors = RollingVwapColors {
        d7: TpoElementColor::Custom([10, 20, 30]),
        d30: TpoElementColor::Purple,
        d90: TpoElementColor::White,
        d365: TpoElementColor::Green,
    };

    let serialized = serde_json::to_string(&custom).expect("Must serialize");
    let deserialized: Config = serde_json::from_str(&serialized).expect("Must deserialize");
    assert_eq!(custom, deserialized);
    assert!(deserialized.rolling_vwap_show_panel);
    assert_eq!(deserialized.rolling_vwap_colors.d7_rgb(), [10, 20, 30]);
}

#[test]
fn test_from_json_markdown_code_block_stripping() {
    let layout = Layout {
        name: "Markdown Test".to_string(),
        dashboard: Dashboard::default(),
    };
    let bundle = ConfigBundle::new_layout(layout, None);
    let json = bundle.to_json_pretty().unwrap();

    // 1. Enclosed in ```json ... ```
    let wrapped_json = format!("```json\n{json}\n```");
    let parsed1 = ConfigBundle::from_json(&wrapped_json).expect("Must parse ```json fence");
    assert_eq!(parsed1.export_type, ExportType::Layout);

    // 2. Enclosed in ``` ... ```
    let wrapped_plain = format!("```\n{json}\n```");
    let parsed2 = ConfigBundle::from_json(&wrapped_plain).expect("Must parse ``` fence");
    assert_eq!(parsed2.export_type, ExportType::Layout);
}
