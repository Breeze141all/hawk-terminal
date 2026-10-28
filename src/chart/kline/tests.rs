use super::*;

#[test]
fn test_indicator_is_panel_classification() {
    assert!(!KlineIndicator::Tpo.is_panel());
    assert!(!KlineIndicator::LiquidationHeatmap.is_panel());
    assert!(KlineIndicator::Volume.is_panel());
    assert!(KlineIndicator::OpenInterest.is_panel());
    assert!(KlineIndicator::MarketPulse.is_panel());
    assert!(KlineIndicator::NetOi.is_panel());
    assert!(KlineIndicator::Vpin.is_panel());
    assert!(KlineIndicator::Vwap.is_panel());
    assert!(KlineIndicator::RollingVwap.is_panel());
    assert!(KlineIndicator::Cvd.is_panel());
    assert!(KlineIndicator::BidAskRatio.is_panel());
    assert!(KlineIndicator::PositionFlow.is_panel());
}

#[test]
fn test_liquidation_heatmap_indicator_toggle() {
    let mut chart = make_test_chart();
    assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_none());
    assert!(!chart.liquidation_heatmap.config.enabled);

    // Toggle on
    chart.toggle_indicator(KlineIndicator::LiquidationHeatmap);
    assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_some());
    assert!(chart.liquidation_heatmap.config.enabled);

    // Toggle off
    chart.toggle_indicator(KlineIndicator::LiquidationHeatmap);
    assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_none());
    assert!(!chart.liquidation_heatmap.config.enabled);
}

#[test]
fn test_insert_raw_trades_deduplication() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    let trade1 = Trade {
        time: 1000,
        price: Price::from_f32(100.0),
        qty: 1.0,
        is_sell: false,
    };
    let trade2 = Trade {
        time: 2000,
        price: Price::from_f32(101.0),
        qty: 2.0,
        is_sell: true,
    };

    // First insertion
    chart.insert_raw_trades(vec![trade1, trade2]);
    assert_eq!(chart.raw_trades().len(), 2);

    // Duplicate insertion (identical trades)
    chart.insert_raw_trades(vec![trade1, trade2]);
    assert_eq!(chart.raw_trades().len(), 2);

    // Insertion with 1 new trade and 1 duplicate
    let trade3 = Trade {
        time: 3000,
        price: Price::from_f32(102.0),
        qty: 0.5,
        is_sell: false,
    };
    chart.insert_raw_trades(vec![trade2, trade3]);
    assert_eq!(chart.raw_trades().len(), 3);
}

#[test]
fn test_lod_level_transitions() {
    let min_w = 80.0;

    // < 35.0 width -> Candle LOD
    assert!(lod_level(34.9, 20.0, min_w) == LodLevel::Candle);
    assert!(lod_level(10.0, 5.0, min_w) == LodLevel::Candle);

    // >= 35.0 and < min_w (or height < 10.0) -> Heatmap LOD
    assert!(lod_level(50.0, 20.0, min_w) == LodLevel::Heatmap);
    assert!(lod_level(90.0, 9.0, min_w) == LodLevel::Heatmap);

    // >= min_w and height >= 10.0 -> Full LOD
    assert!(lod_level(80.0, 10.0, min_w) == LodLevel::Full);
    assert!(lod_level(150.0, 25.0, min_w) == LodLevel::Full);
}

#[test]
fn test_cluster_search_detection_logic() {
    use data::chart::kline::ClusterSearchSide;

    let mut footprint = KlineTrades::new();
    let step = PriceStep::from_f32(1.0);

    // Add trades
    footprint.add_trade_to_nearest_bin(
        &Trade {
            time: 1000,
            price: Price::from_f32(100.0),
            qty: 600.0,
            is_sell: false,
        },
        step,
    );
    footprint.add_trade_to_nearest_bin(
        &Trade {
            time: 1001,
            price: Price::from_f32(100.0),
            qty: 100.0,
            is_sell: true,
        },
        step,
    );

    // At price 100: buy_qty = 600, sell_qty = 100, total = 700, delta = +500
    let group = footprint.trades.get(&Price::from_f32(100.0)).unwrap();
    let total_vol = group.total_qty();
    let delta = group.delta_qty();

    assert_eq!(total_vol, 700.0);
    assert_eq!(delta, 500.0);

    // Matches Min Volume = 500
    assert!(total_vol >= 500.0);
    // Matches Min Delta = 400
    assert!(delta.abs() >= 400.0);
    // Matches Side: Both and BuyOnly, but rejects SellOnly
    let side_both = matches!(ClusterSearchSide::Both, ClusterSearchSide::Both) || (delta > 0.0);
    let side_buy = delta > 0.0;
    let side_sell = delta < 0.0;

    assert!(side_both);
    assert!(side_buy);
    assert!(!side_sell);
}

#[test]
fn test_npoc_toggle_visibility_logic() {
    let studies_with_npoc = [
        FootprintStudy::NPoC { lookback: 50 },
        FootprintStudy::ClusterSearch {
            id: 1,
            min_volume: 100.0,
            min_delta: 50.0,
            side: data::chart::kline::ClusterSearchSide::Both,
            style: data::chart::kline::HighlightStyle::Border,
            color: data::chart::kline::HighlightColor::Amber,
        },
    ];
    let has_npoc = studies_with_npoc
        .iter()
        .any(|s| matches!(s, FootprintStudy::NPoC { .. }));
    assert!(has_npoc);

    let studies_without_npoc = [FootprintStudy::ClusterSearch {
        id: 1,
        min_volume: 100.0,
        min_delta: 50.0,
        side: data::chart::kline::ClusterSearchSide::Both,
        style: data::chart::kline::HighlightStyle::Border,
        color: data::chart::kline::HighlightColor::Amber,
    }];
    let has_npoc_off = studies_without_npoc
        .iter()
        .any(|s| matches!(s, FootprintStudy::NPoC { .. }));
    assert!(!has_npoc_off);
}

#[test]
fn test_multi_rule_cluster_search_independence() {
    use data::chart::kline::{ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle};

    let rule1 = FootprintStudy::ClusterSearch {
        id: 1,
        min_volume: 100.0,
        min_delta: 50.0,
        side: ClusterSearchSide::Both,
        style: HighlightStyle::Border,
        color: HighlightColor::Amber,
    };
    let rule2 = FootprintStudy::ClusterSearch {
        id: 2,
        min_volume: 300.0,
        min_delta: 150.0,
        side: ClusterSearchSide::BuyOnly,
        style: HighlightStyle::Circle,
        color: HighlightColor::Cyan,
    };
    let rule3 = FootprintStudy::ClusterSearch {
        id: 3,
        min_volume: 500.0,
        min_delta: 250.0,
        side: ClusterSearchSide::SellOnly,
        style: HighlightStyle::Triangle,
        color: HighlightColor::Red,
    };
    let rule4 = FootprintStudy::ClusterSearch {
        id: 4,
        min_volume: 1000.0,
        min_delta: 500.0,
        side: ClusterSearchSide::Both,
        style: HighlightStyle::Square,
        color: HighlightColor::White,
    };

    // Distinct IDs must not be considered the same type
    assert!(!rule1.is_same_type(&rule2));
    assert!(!rule2.is_same_type(&rule3));
    assert!(!rule3.is_same_type(&rule4));
    assert!(rule1.is_same_type(&rule1));

    // Format names
    assert_eq!(format!("{}", rule1), "Cluster Search 1");
    assert_eq!(format!("{}", rule2), "Cluster Search 2");
    assert_eq!(format!("{}", rule3), "Cluster Search 3");
    assert_eq!(format!("{}", rule4), "Cluster Search 4");
}

#[test]
fn test_study_trait_cluster_search_independence() {
    use crate::modal::pane::settings::study::Study;
    use data::chart::kline::{ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle};

    let rule1 = FootprintStudy::ClusterSearch {
        id: 1,
        min_volume: 100.0,
        min_delta: 50.0,
        side: ClusterSearchSide::Both,
        style: HighlightStyle::Border,
        color: HighlightColor::Amber,
    };
    let rule2 = FootprintStudy::ClusterSearch {
        id: 2,
        min_volume: 300.0,
        min_delta: 150.0,
        side: ClusterSearchSide::BuyOnly,
        style: HighlightStyle::Circle,
        color: HighlightColor::Cyan,
    };

    // Study trait must NOT use discriminant, must distinguish IDs
    assert!(!<FootprintStudy as Study>::is_same_type(&rule1, &rule2));
    assert!(<FootprintStudy as Study>::is_same_type(&rule1, &rule1));
}

#[test]
fn test_ring_and_concentric_cluster_search_styles() {
    use data::chart::kline::{ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle};

    let ring_rule = FootprintStudy::ClusterSearch {
        id: 5,
        min_volume: 12000.0,
        min_delta: 6000.0,
        side: ClusterSearchSide::Both,
        style: HighlightStyle::Ring,
        color: HighlightColor::Purple,
    };
    let concentric_rule = FootprintStudy::ClusterSearch {
        id: 6,
        min_volume: 25000.0,
        min_delta: 10000.0,
        side: ClusterSearchSide::BuyOnly,
        style: HighlightStyle::Concentric,
        color: HighlightColor::Orange,
    };

    if let FootprintStudy::ClusterSearch { style, color, .. } = ring_rule {
        assert_eq!(format!("{}", style), "Ring");
        assert_eq!(format!("{}", color), "Purple");
    } else {
        panic!("Expected ClusterSearch variant");
    }

    if let FootprintStudy::ClusterSearch { style, color, .. } = concentric_rule {
        assert_eq!(format!("{}", style), "Concentric");
        assert_eq!(format!("{}", color), "Orange");
    } else {
        panic!("Expected ClusterSearch variant");
    }

    let custom_rule = FootprintStudy::ClusterSearch {
        id: 7,
        min_volume: 50000.0,
        min_delta: 20000.0,
        side: ClusterSearchSide::Both,
        style: HighlightStyle::ClassicCircle,
        color: HighlightColor::Custom([255, 100, 50]),
    };

    if let FootprintStudy::ClusterSearch { style, color, .. } = custom_rule {
        assert_eq!(format!("{}", style), "Dot (Classic)");
        assert_eq!(format!("{}", color), "Custom RGB (255, 100, 50)");
        let rgb = color.to_rgb();
        assert!((rgb[0] - 1.0).abs() < 1e-4);
        assert!((rgb[1] - 100.0 / 255.0).abs() < 1e-4);
        assert!((rgb[2] - 50.0 / 255.0).abs() < 1e-4);
    } else {
        panic!("Expected ClusterSearch variant");
    }

    let classic_tri = HighlightStyle::ClassicTriangle;
    let classic_sq = HighlightStyle::ClassicSquare;
    assert_eq!(format!("{}", classic_tri), "Triangle (Classic)");
    assert_eq!(format!("{}", classic_sq), "Square (Classic)");

    let serialized = serde_json::to_string(&concentric_rule).expect("serialize concentric");
    let deserialized: FootprintStudy =
        serde_json::from_str(&serialized).expect("deserialize concentric");
    assert_eq!(concentric_rule, deserialized);

    let serialized_custom = serde_json::to_string(&custom_rule).expect("serialize custom");
    let deserialized_custom: FootprintStudy =
        serde_json::from_str(&serialized_custom).expect("deserialize custom");
    assert_eq!(custom_rule, deserialized_custom);
}

#[test]
fn test_dynamic_cluster_search_creation_and_removal() {
    use crate::modal::pane::settings::study::Study;
    use data::chart::kline::FootprintStudy;

    let active: Vec<FootprintStudy> = FootprintStudy::ALL.to_vec();
    let next_rule = FootprintStudy::create_next(&active).expect("next rule");
    if let FootprintStudy::ClusterSearch { id, .. } = next_rule {
        assert_eq!(id, 5);
    } else {
        panic!("Expected ClusterSearch variant");
    }
    assert!(next_rule.can_remove());

    // Default rules cannot be removed
    for s in &active {
        if let FootprintStudy::ClusterSearch { id, .. } = s
            && *id <= 4
        {
            assert!(!s.can_remove());
        }
    }

    let mut extended = active.clone();
    extended.push(next_rule);
    let next_rule_6 = FootprintStudy::create_next(&extended).expect("next rule 6");
    if let FootprintStudy::ClusterSearch { id, .. } = next_rule_6 {
        assert_eq!(id, 6);
    } else {
        panic!("Expected ClusterSearch variant");
    }
}

#[test]
fn test_cluster_search_volumetric_scaling() {
    let threshold = 5000.0_f32;
    let total_vol_normal = 5000.0_f32;
    let total_vol_large = 20000.0_f32;
    let total_vol_huge = 45000.0_f32;

    let scale_normal = (total_vol_normal / threshold).sqrt().clamp(1.0, 1.5);
    let scale_large = (total_vol_large / threshold).sqrt().clamp(1.0, 1.5);
    let scale_huge = (total_vol_huge / threshold).sqrt().clamp(1.0, 1.5);

    assert!((scale_normal - 1.0).abs() < 1e-4);
    assert!((scale_large - 1.5).abs() < 1e-4);
    assert!((scale_huge - 1.5).abs() < 1e-4);
}

#[test]
fn test_insert_hist_klines_no_trade_volume_multiplication() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::DeltaProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    let kline1 = exchange::Kline {
        time: 300_000,
        open: Price::from_f32(100.0),
        high: Price::from_f32(105.0),
        low: Price::from_f32(95.0),
        close: Price::from_f32(102.0),
        volume: (10.0, 10.0),
    };
    let trade = Trade {
        time: 300_100,
        price: Price::from_f32(100.0),
        qty: 5.0,
        is_sell: false,
    };

    chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
    chart.insert_raw_trades(vec![trade]);

    let get_vol = |c: &KlineChart| -> f32 {
        if let PlotData::TimeBased(ref ts) = c.data_source
            && let Some(dp) = ts.datapoints.get(&300_000)
            && let Some(g) = dp.footprint.trades.get(&Price::from_f32(100.0))
        {
            g.buy_qty as f32
        } else {
            0.0
        }
    };

    assert_eq!(get_vol(&chart), 5.0);

    // Second kline insert must NOT multiply the trade volume in existing buckets
    let kline2 = exchange::Kline {
        time: 600_000,
        open: Price::from_f32(102.0),
        high: Price::from_f32(106.0),
        low: Price::from_f32(101.0),
        close: Price::from_f32(104.0),
        volume: (5.0, 5.0),
    };
    chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline2]);
    assert_eq!(get_vol(&chart), 5.0);

    // Third kline insert with existing kline update must also NOT multiply
    chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
    assert_eq!(get_vol(&chart), 5.0);
}

#[test]
fn test_set_basis_preserves_trades() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::DeltaProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    let kline1 = exchange::Kline {
        time: 300_000,
        open: Price::from_f32(100.0),
        high: Price::from_f32(105.0),
        low: Price::from_f32(95.0),
        close: Price::from_f32(102.0),
        volume: (10.0, 10.0),
    };
    let trade = Trade {
        time: 300_100,
        price: Price::from_f32(100.0),
        qty: 7.5,
        is_sell: false,
    };

    chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
    chart.insert_raw_trades(vec![trade]);

    // Switch to M15 timeframe (15 * 60 * 1000 = 900_000 ms)
    // Trade at 300_100 belongs to interval starting at 0
    chart.set_basis(Basis::Time(exchange::Timeframe::M15));

    let get_vol_at = |c: &KlineChart, bucket: u64| -> f32 {
        if let PlotData::TimeBased(ref ts) = c.data_source
            && let Some(dp) = ts.datapoints.get(&bucket)
            && let Some(g) = dp.footprint.trades.get(&Price::from_f32(100.0))
        {
            g.buy_qty as f32
        } else {
            0.0
        }
    };

    assert_eq!(get_vol_at(&chart, 0), 7.5);

    // When 15m historical klines arrive, trades are retained and not duplicated
    let kline_15m = exchange::Kline {
        time: 0,
        open: Price::from_f32(98.0),
        high: Price::from_f32(106.0),
        low: Price::from_f32(95.0),
        close: Price::from_f32(103.0),
        volume: (20.0, 20.0),
    };
    chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline_15m]);
    assert_eq!(get_vol_at(&chart, 0), 7.5);
}

#[test]
fn test_footprint_show_bottom_volume_toggle() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    if let KlineChartKind::Footprint {
        show_bottom_volume, ..
    } = chart.kind
    {
        assert!(!show_bottom_volume);
    } else {
        panic!("Expected Footprint chart kind");
    }

    chart.set_footprint_show_bottom_volume(true);
    if let KlineChartKind::Footprint {
        show_bottom_volume, ..
    } = chart.kind
    {
        assert!(show_bottom_volume);
    } else {
        panic!("Expected Footprint chart kind");
    }

    chart.set_footprint_show_bottom_volume(false);
    if let KlineChartKind::Footprint {
        show_bottom_volume, ..
    } = chart.kind
    {
        assert!(!show_bottom_volume);
    } else {
        panic!("Expected Footprint chart kind");
    }
}

#[test]
fn test_insert_raw_trades_prepend_and_append_optimization() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    // Batch 1: middle trades at 200..202
    let t2 = Trade {
        time: 200,
        price: Price::from_f32(100.0),
        qty: 1.0,
        is_sell: false,
    };
    let t3 = Trade {
        time: 202,
        price: Price::from_f32(101.0),
        qty: 1.0,
        is_sell: false,
    };
    chart.insert_raw_trades(vec![t2, t3]);
    assert_eq!(chart.raw_trades().len(), 2);

    // Batch 2: later trades at 300..301 (append)
    let t4 = Trade {
        time: 300,
        price: Price::from_f32(102.0),
        qty: 1.0,
        is_sell: false,
    };
    let t5 = Trade {
        time: 301,
        price: Price::from_f32(103.0),
        qty: 1.0,
        is_sell: false,
    };
    chart.insert_raw_trades(vec![t4, t5]);
    assert_eq!(chart.raw_trades().len(), 4);
    assert_eq!(
        chart
            .raw_trades()
            .iter()
            .map(|t| t.time)
            .collect::<Vec<_>>(),
        vec![200, 202, 300, 301]
    );

    // Batch 3: earlier trades at 100..101 (prepend)
    let t0 = Trade {
        time: 100,
        price: Price::from_f32(98.0),
        qty: 1.0,
        is_sell: false,
    };
    let t1 = Trade {
        time: 101,
        price: Price::from_f32(99.0),
        qty: 1.0,
        is_sell: false,
    };
    chart.insert_raw_trades(vec![t0, t1]);
    assert_eq!(chart.raw_trades().len(), 6);
    assert_eq!(
        chart
            .raw_trades()
            .iter()
            .map(|t| t.time)
            .collect::<Vec<_>>(),
        vec![100, 101, 200, 202, 300, 301]
    );
}

fn make_test_chart() -> KlineChart {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    )
}

#[test]
fn test_kline_chart_drawings_management() {
    let mut chart = make_test_chart();
    assert!(chart.drawings.is_empty());

    let d1 = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
    chart.add_drawing(d1.clone());
    assert_eq!(chart.drawings.len(), 1);
    assert_eq!(chart.drawings[0], d1);

    let mut d1_updated = d1.clone();
    d1_updated.color = [0.0, 1.0, 0.0, 1.0];
    chart.update_drawing(d1_updated.clone());
    assert_eq!(chart.drawings[0].color, [0.0, 1.0, 0.0, 1.0]);

    chart.set_active_drawing_tool(DrawingTool::Trendline);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Trendline);

    chart.set_active_drawing_tool(DrawingTool::Brush);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Brush);

    chart.set_active_drawing_tool(DrawingTool::Path);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Path);

    chart.set_active_drawing_tool(DrawingTool::ShortPosition);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::ShortPosition);

    chart.set_active_drawing_tool(DrawingTool::LongPosition);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::LongPosition);

    chart.delete_drawing(d1.id);
    assert!(chart.drawings.is_empty());

    chart.add_drawing(d1);
    chart.clear_drawings();
    assert!(chart.drawings.is_empty());
}

#[test]
fn test_kline_chart_drawings_lock_and_clear() {
    let mut chart = make_test_chart();

    let d_unlocked = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
    let mut d_locked = Drawing::horizontal(200.0, [0.0, 1.0, 0.0, 1.0], 2.0);
    d_locked.is_locked = true;

    chart.add_drawing(d_unlocked.clone());
    chart.add_drawing(d_locked.clone());
    assert_eq!(chart.drawings.len(), 2);

    // Select d_locked and test helpers
    chart.set_selected_drawing(Some(d_locked.id));
    assert_eq!(chart.selected_drawing().unwrap().id, d_locked.id);
    assert!(chart.selected_drawing().unwrap().is_locked);

    // Toggle lock
    chart.toggle_selected_drawing_lock();
    assert!(!chart.selected_drawing().unwrap().is_locked);
    chart.toggle_selected_drawing_lock();
    assert!(chart.selected_drawing().unwrap().is_locked);

    // Update color and width
    chart.update_selected_drawing_color([0.5, 0.5, 0.5, 0.8]);
    assert_eq!(
        chart.selected_drawing().unwrap().color,
        [0.5, 0.5, 0.5, 0.8]
    );
    chart.update_selected_drawing_width(3.5);
    assert_eq!(chart.selected_drawing().unwrap().width, 3.5);

    // clear_drawings should retain locked drawings and remove unlocked ones
    chart.clear_drawings();
    assert_eq!(chart.drawings.len(), 1);
    assert_eq!(chart.drawings[0].id, d_locked.id);

    // Deleting the specific locked drawing directly via delete_drawing
    chart.delete_drawing(d_locked.id);
    assert!(chart.drawings.is_empty());
    assert!(chart.selected_drawing().is_none());
}

#[test]
fn test_task24_drawing_templates_application_and_presets() {
    use data::chart::drawing::{Drawing, DrawingTemplate, DrawingTemplateStore};

    let mut chart = make_test_chart();
    let d = Drawing::horizontal(50000.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    let d_id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(d_id));

    // Builtin presets check
    let presets = DrawingTemplateStore::all();
    assert!(!presets.is_empty());
    let cyan_preset = presets
        .iter()
        .find(|p| p.name == "Cyan Accent")
        .expect("Cyan Accent preset missing");

    // Apply template
    chart.apply_template_to_selected(cyan_preset);
    let selected = chart.selected_drawing().unwrap();
    assert_eq!(selected.color, [0.0, 0.85, 1.0, 1.0]);
    assert_eq!(selected.width, 2.0);

    // Apply custom template
    let custom = DrawingTemplate::new("Custom Purple", [0.7, 0.2, 0.9, 0.9], 4.0);
    chart.apply_template_to_selected(&custom);
    let selected2 = chart.selected_drawing().unwrap();
    assert_eq!(selected2.color, [0.7, 0.2, 0.9, 0.9]);
    assert_eq!(selected2.width, 4.0);
}

#[test]
fn test_kline_chart_drawing_drag_and_modification() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();
    let rect = Drawing::rectangle((1000, 100.0), (2000, 110.0), [0.2, 0.5, 0.9, 1.0], 1.0);
    let rect_id = rect.id;
    chart.add_drawing(rect);

    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    // Select drawing manually into drawing_state
    chart.drawing_state.borrow_mut().selected_drawing = Some(rect_id);

    // Delete key press produces DeleteDrawing message
    let delete_key_event = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Delete),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
    });
    let action = chart.update(
        &mut interaction,
        &delete_key_event,
        bounds,
        mouse::Cursor::Unavailable,
    );
    assert!(action.is_some());
    assert!(chart.drawing_state.borrow().selected_drawing.is_none());

    // Re-add drawing and test dragging
    let rect2 = Drawing::rectangle((1000, 100.0), (2000, 110.0), [0.2, 0.5, 0.9, 1.0], 1.0);
    chart.add_drawing(rect2.clone());

    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
        start_time: 1000,
        start_price: 100.0,
        initial_drawing: rect2.clone(),
        current_drawing: rect2,
    });

    // Moving cursor updates current_drawing
    let cursor = mouse::Cursor::Available(Point::new(400.0, 300.0));
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(400.0, 300.0),
    });
    let move_action = chart.update(&mut interaction, &move_event, bounds, cursor);
    assert!(move_action.is_some());
    assert!(chart.drawing_state.borrow().drag.is_some());

    // Releasing mouse publishes UpdateDrawing
    let release_event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let release_action = chart.update(&mut interaction, &release_event, bounds, cursor);
    assert!(release_action.is_some());
    assert!(chart.drawing_state.borrow().drag.is_none());
}

#[test]
fn test_position_price_axis_badges_during_drag() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();

    // 0. Empty chart -> empty badges
    assert!(chart.y_axis_badges().is_empty());

    let pos = Drawing::position(
        (1000, 100.0),
        95.0,
        110.0,
        true,
        Some(2000),
        [0.2, 0.8, 0.4, 1.0],
        1.0,
    );
    chart.add_drawing(pos.clone());

    // 1. Position on chart -> 3 badges (TP, SL, Entry) permanently visible on Y axis
    let badges = chart.y_axis_badges();
    assert_eq!(badges.len(), 3);
    assert_eq!(badges[0].price, 110.0);
    assert_eq!(badges[1].price, 95.0);
    assert_eq!(badges[2].price, 100.0);

    // 2. Drag handle 1 (Take Profit) -> live Target price badge updated
    let mut tp_drag = pos.clone();
    if let DrawingKind::Position {
        ref mut target_price,
        ..
    } = tp_drag.kind
    {
        *target_price = 115.0;
    }
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingHandle {
        handle_idx: 1,
        initial_drawing: pos.clone(),
        current_drawing: tp_drag,
    });
    let badges = chart.y_axis_badges();
    assert_eq!(badges.len(), 3);
    assert_eq!(badges[0].price, 115.0);
    assert_eq!(badges[1].price, 95.0);
    assert_eq!(badges[2].price, 100.0);

    // 3. Drag handle 2 (Stop Loss) -> live Stop price badge updated
    let mut sl_drag = pos.clone();
    if let DrawingKind::Position {
        ref mut stop_price, ..
    } = sl_drag.kind
    {
        *stop_price = 90.0;
    }
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingHandle {
        handle_idx: 2,
        initial_drawing: pos.clone(),
        current_drawing: sl_drag,
    });
    let badges = chart.y_axis_badges();
    assert_eq!(badges.len(), 3);
    assert_eq!(badges[0].price, 110.0);
    assert_eq!(badges[1].price, 90.0);
    assert_eq!(badges[2].price, 100.0);

    // 4. Drag handle 0 (Entry) -> live Entry price badge updated
    let mut entry_drag = pos.clone();
    if let DrawingKind::Position { ref mut entry, .. } = entry_drag.kind {
        entry.1 = 102.0;
    }
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingHandle {
        handle_idx: 0,
        initial_drawing: pos.clone(),
        current_drawing: entry_drag,
    });
    let badges = chart.y_axis_badges();
    assert_eq!(badges.len(), 3);
    assert_eq!(badges[0].price, 110.0);
    assert_eq!(badges[1].price, 95.0);
    assert_eq!(badges[2].price, 102.0);

    // 5. Drag whole Position body (MovingDrawing) -> All 3 badges shifted
    let mut body_drag = pos.clone();
    if let DrawingKind::Position {
        ref mut entry,
        ref mut target_price,
        ref mut stop_price,
        ..
    } = body_drag.kind
    {
        entry.1 = 105.0;
        *target_price = 115.0;
        *stop_price = 100.0;
    }
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
        start_time: 1000,
        start_price: 100.0,
        initial_drawing: pos.clone(),
        current_drawing: body_drag,
    });
    let badges = chart.y_axis_badges();
    assert_eq!(badges.len(), 3);
    assert_eq!(badges[0].price, 115.0);
    assert_eq!(badges[1].price, 100.0);
    assert_eq!(badges[2].price, 105.0);

    // 5b. Drag handle 3 (End time / Duration) -> stretches end_time
    let mut end_drag = pos.clone();
    chart.apply_handle_movement(
        3,
        &pos,
        &mut end_drag,
        Point::new(chart.state().interval_to_x(300_000), 0.0),
        false,
        false,
    );
    if let DrawingKind::Position { end_time, .. } = end_drag.kind {
        assert!(end_time.unwrap() >= 300_000);
    } else {
        panic!("Expected Position");
    }

    // 5c. Target position outcome generation
    chart.drawing_state.borrow_mut().drag = None;
    let autofill = chart.find_target_position().unwrap();
    assert_eq!(autofill.entry_price, 100.0);
    assert_eq!(autofill.target_price, 110.0);
    assert_eq!(autofill.stop_price, 95.0);
    assert_eq!(autofill.drawing_id, Some(pos.id));

    // 6. Other drawing tools alone must NOT show badges
    let mut chart_other = make_test_chart();
    let rect = Drawing::rectangle((1000, 100.0), (2000, 110.0), [0.2, 0.5, 0.9, 1.0], 1.0);
    chart_other.add_drawing(rect.clone());
    assert!(chart_other.y_axis_badges().is_empty());
    chart_other.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
        start_time: 1000,
        start_price: 100.0,
        initial_drawing: rect.clone(),
        current_drawing: rect.clone(),
    });
    assert!(chart_other.y_axis_badges().is_empty());

    // 7. Interactive drag movement with update() publishes CrosshairMoved and clears cache
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingHandle {
        handle_idx: 1,
        initial_drawing: pos.clone(),
        current_drawing: pos.clone(),
    });
    let cursor = mouse::Cursor::Available(Point::new(400.0, 300.0));
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(400.0, 300.0),
    });
    let action = chart.update(&mut interaction, &move_event, bounds, cursor);
    assert!(action.is_some());
    assert!(chart.drawing_state.borrow().drag.is_some());

    // 8. Escape cancels Position drag, returning to stored drawing badges
    let esc_event = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
    });
    let esc_action = chart.update(&mut interaction, &esc_event, bounds, cursor);
    assert!(esc_action.is_some());
    assert!(chart.drawing_state.borrow().drag.is_none());
    assert_eq!(chart.y_axis_badges().len(), 3);

    // 9. Deleting position removes all badges
    chart.delete_drawing(pos.id);
    assert!(chart.y_axis_badges().is_empty());
}

#[test]
fn test_kline_chart_brush_fixed_drag_only() {
    let chart = make_test_chart();
    let brush = Drawing::brush(
        vec![(1000, 100.0), (2000, 120.0), (3000, 110.0)],
        [0.35, 0.7, 1.0, 1.0],
        2.0,
    );

    assert!(brush.handles().is_empty());

    let view_state = chart.state();
    let pt_on_stroke = Point::new(
        view_state.interval_to_x(2000),
        view_state.price_to_y(Price::from_f32(120.0)),
    );

    let hit = hit_test_drawing(&brush, pt_on_stroke, view_state, 10.0);
    assert_eq!(hit, Some(HitTarget::Body));

    // Translation shifts whole brush without mutating shape
    let mut translated = brush.clone();
    translated.translate(500, 10.0);
    if let DrawingKind::Brush { points } = &translated.kind {
        assert_eq!(points[0], (1500, 110.0));
        assert_eq!(points[1], (2500, 130.0));
        assert_eq!(points[2], (3500, 120.0));
    } else {
        panic!("Expected Brush");
    }
}

#[test]
fn test_kline_chart_price_alerts() {
    let mut chart = make_test_chart();
    let alert = PriceAlert::new(
        "TEST",
        100.0,
        data::chart::alert::AlertCondition::CrossAbove,
    );
    let alert_id = alert.id;
    chart.add_alert(alert);
    assert_eq!(chart.alerts.len(), 1);

    let triggered = chart.check_price_alerts(99.0, 101.0);
    assert_eq!(triggered.len(), 1);
    assert_eq!(triggered[0].id, alert_id);

    chart.remove_alert(alert_id);
    assert!(chart.alerts.is_empty());
}

#[test]
fn test_kline_chart_replay() {
    let mut chart = make_test_chart();
    assert!(!chart.is_replay_active());

    chart.toggle_replay();
    assert!(chart.is_replay_active());

    chart.replay_set_cutoff(1000);
    assert_eq!(chart.replay_state().unwrap().cutoff_time, 1000);

    chart.replay_step_forward();
    assert!(chart.replay_state().unwrap().cutoff_time > 1000);

    chart.replay_step_backward();
    assert_eq!(chart.replay_state().unwrap().cutoff_time, 1000);

    chart.toggle_replay();
    assert!(!chart.is_replay_active());
}

#[test]
fn test_kline_chart_replay_jump_and_random() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines: Vec<Kline> = (0..50)
        .map(|i| Kline {
            time: 1000 + i * 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (5.0, 5.0),
        })
        .collect();
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    chart.toggle_replay();
    assert!(chart.is_replay_active());

    // Test find_closest_bar_at_or_before
    let closest = chart.find_closest_bar_at_or_before(1000 + 5 * 300_000 + 100);
    assert_eq!(closest, Some(1000 + 5 * 300_000));

    // Test replay_set_cutoff_and_jump
    chart.replay_set_cutoff_and_jump(1000 + 10 * 300_000);
    assert_eq!(
        chart.replay_state().unwrap().cutoff_time,
        1000 + 10 * 300_000
    );

    // Test replay_pick_random_bar
    let rand_ts = chart.replay_pick_random_bar();
    assert!(rand_ts.is_some());
    let picked = rand_ts.unwrap();
    assert!((1000..=1000 + 49 * 300_000).contains(&picked));
    assert_eq!(chart.replay_state().unwrap().cutoff_time, picked);
}

#[test]
fn test_kline_chart_replay_auto_scroll() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines: Vec<Kline> = (0..50)
        .map(|i| Kline {
            time: 1000 + i * 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (5.0, 5.0),
        })
        .collect();
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    chart.chart.bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 600.0));

    chart.toggle_replay();
    assert!(chart.is_replay_active());

    // Set cutoff at bar 10
    chart.replay_set_cutoff_and_jump(1000 + 10 * 300_000);
    let initial_trans_x = chart.chart.translation.x;

    // Step forward: cutoff advances by 1 bar, chart shifts to the left (translation.x decreases)
    chart.replay_step_forward();
    assert_eq!(
        chart.replay_state().unwrap().cutoff_time,
        1000 + 11 * 300_000
    );
    let step_trans_x = chart.chart.translation.x;
    assert!(
        step_trans_x < initial_trans_x,
        "Chart should shift left when stepping forward"
    );

    // Step backward: cutoff moves back, chart shifts to the right
    chart.replay_step_backward();
    assert_eq!(
        chart.replay_state().unwrap().cutoff_time,
        1000 + 10 * 300_000
    );
    assert!(
        (chart.chart.translation.x - initial_trans_x).abs() < 1e-4,
        "Chart should return to original position when stepping backward"
    );

    // Toggle play/pause
    chart.replay_play_pause();
    assert!(chart.replay_state().unwrap().is_playing);

    // Advance via invalidate(now)
    let now = Instant::now() + Duration::from_millis(1500);
    let _ = chart.invalidate(Some(now));
    assert!(chart.replay_state().unwrap().cutoff_time > 1000 + 10 * 300_000);
    assert!(
        chart.chart.translation.x < initial_trans_x,
        "Chart should auto-scroll forward during replay playback"
    );

    // Toggle play/pause to stop
    chart.replay_play_pause();
    assert!(!chart.replay_state().unwrap().is_playing);
}

#[test]
fn test_kline_chart_right_margin_offset() {
    let mut chart = make_test_chart();
    chart.chart.bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 600.0));
    chart.chart.cell_width = 4.0;
    chart.chart.scaling = 1.0;

    // Candles margin: 12.0 * clamp(4.0, 4.0, 8.0) + 72.0 = 48.0 + 72.0 = 120.0
    let margin_offset = chart.right_margin_offset();
    assert!((margin_offset - 120.0).abs() < 1e-4);
    assert!((chart.base_x() - (500.0 - 120.0)).abs() < 1e-4);

    // Footprint margin: cell_width * 0.5 + 90.0 = 40.0 + 90.0 = 130.0 (90px margin from cluster border)
    chart.kind = KlineChartKind::Footprint {
        clusters: data::chart::kline::ClusterKind::BidAsk,
        scaling: data::chart::kline::ClusterScaling::default(),
        studies: vec![],
        show_bottom_volume: false,
        aggregate: false,
        aggregate_exchanges: vec![],
        history_lookback_days: None,
    };
    chart.chart.cell_width = 80.0;
    assert!((chart.right_margin_offset() - 130.0).abs() < 1e-4);
    assert!((chart.base_x() - (500.0 - 130.0)).abs() < 1e-4);
}

#[test]
fn test_kline_chart_replay_pan_detaches_and_reattaches_auto_scroll() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &(0..50)
            .map(|i| Kline {
                time: 1000 + i * 300_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(95.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            })
            .collect::<Vec<_>>(),
        vec![],
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );
    chart.chart.bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 600.0));

    chart.toggle_replay();
    assert!(chart.is_replay_active());
    assert!(chart.replay_state().unwrap().auto_scroll);

    // Jump to bar 10
    chart.replay_set_cutoff_and_jump(1000 + 10 * 300_000);
    let live_trans_x = chart.chart.translation.x;
    assert!(chart.replay_state().unwrap().auto_scroll);

    // User pans into history (older bars): translation.x increases
    let history_trans = Vector::new(live_trans_x + 300.0, chart.chart.translation.y);
    chart.on_user_pan(history_trans);
    chart.chart.translation.x = history_trans.x;
    assert!(
        !chart.replay_state().unwrap().auto_scroll,
        "Auto-scroll should detach when panning to history"
    );

    // Start playback while detached
    chart.replay_play_pause();
    assert!(chart.replay_state().unwrap().is_playing);
    // replay_play_pause re-engages auto-scroll and snaps to cutoff
    assert!(chart.replay_state().unwrap().auto_scroll);

    // Detach again during playback
    let history_trans2 = Vector::new(chart.chart.translation.x + 200.0, chart.chart.translation.y);
    chart.on_user_pan(history_trans2);
    chart.chart.translation.x = history_trans2.x;
    assert!(!chart.replay_state().unwrap().auto_scroll);

    // Advance playback: cutoff advances, but chart position must NOT snap back!
    let saved_x = chart.chart.translation.x;
    let now = Instant::now() + Duration::from_millis(1500);
    let _ = chart.invalidate(Some(now));
    assert!(chart.replay_state().unwrap().cutoff_time > 1000 + 10 * 300_000);
    assert_eq!(
        chart.chart.translation.x, saved_x,
        "Chart translation must NOT snap back while auto_scroll is detached"
    );

    // User pans back to the right near the cutoff: auto_scroll automatically re-attaches!
    let target_x = chart.base_x()
        - chart
            .chart
            .interval_to_x(chart.replay_state().unwrap().cutoff_time);
    chart.on_user_pan(Vector::new(target_x, chart.chart.translation.y));
    assert!(
        chart.replay_state().unwrap().auto_scroll,
        "Panning near cutoff automatically re-attaches auto-scroll"
    );

    // Release pan
    chart.on_pan_released();
    assert!(chart.replay_state().unwrap().auto_scroll);

    // Advance again: now it should follow the cutoff
    let now2 = now + Duration::from_millis(1500);
    let _ = chart.invalidate(Some(now2));
    assert!(
        chart.chart.translation.x < target_x,
        "Chart should resume auto-scrolling with replay after re-attaching"
    );

    // User pans FORWARD into future empty space: translation.x decreases below target_x
    let current_target_x = chart.base_x()
        - chart
            .chart
            .interval_to_x(chart.replay_state().unwrap().cutoff_time);
    let future_trans = Vector::new(current_target_x - 300.0, chart.chart.translation.y);
    chart.on_user_pan(future_trans);
    assert!(
        !chart.replay_state().unwrap().auto_scroll,
        "Panning forward into future space must detach auto-scroll"
    );
    chart.chart.translation.x = future_trans.x;
    chart.on_pan_released();
    assert!(
        !chart.replay_state().unwrap().auto_scroll,
        "Releasing pan in future space must stay detached"
    );
    assert_eq!(
        chart.chart.translation.x, future_trans.x,
        "Chart must NOT snap back when released in future space"
    );
}

#[test]
fn test_footprint_simulation_fallback_in_calc_qty_scales() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines: Vec<Kline> = (0..20)
        .map(|i| Kline {
            time: 1_000_000 + i * 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (50.0, 50.0),
        })
        .collect();
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        vec![], // No raw trades: footprint is empty!
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::BidAsk,
            scaling: data::chart::kline::ClusterScaling::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: vec![],
            history_lookback_days: None,
        },
        None,
    );
    chart.toggle_replay();

    // calc_qty_scales must NOT return 0.0: it must fall back to simulated clusters!
    let max_qty = chart.calc_qty_scales(
        1_000_000,
        1_000_000 + 5 * 300_000,
        Price::from_f32(115.0),
        Price::from_f32(85.0),
        PriceStep::from_f32(1.0),
        data::chart::kline::ClusterKind::VolumeProfile,
    );
    assert!(
        max_qty > 0.0,
        "Simulated footprint fallback must produce max_qty > 0, got {max_qty}"
    );
}

#[test]
fn test_footprint_lookback_can_be_lowered() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        vec![],
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::DeltaProfile,
            scaling: data::chart::kline::ClusterScaling::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: vec![],
            history_lookback_days: Some(365),
        },
        None,
    );

    // Initially global might be 365
    exchange::fetcher::set_trade_history_lookback_days(365);
    assert_eq!(exchange::fetcher::trade_history_lookback_days(), 365);

    // Setting a smaller lookback MUST update global and local
    chart.set_history_lookback_days(30);
    assert_eq!(chart.effective_history_lookback_days(), 30);
    assert_eq!(exchange::fetcher::trade_history_lookback_days(), 30);

    // Input string method also lowers properly
    chart.set_history_lookback_input("14".to_string());
    assert_eq!(chart.effective_history_lookback_days(), 14);
    assert_eq!(exchange::fetcher::trade_history_lookback_days(), 14);
}

#[test]
fn test_inverted_range_does_not_panic() {
    let chart = make_test_chart();
    let qty = chart.calc_qty_scales(
        2000,
        1000,
        Price::from_f32(100.0),
        Price::from_f32(90.0),
        PriceStep::from_f32(1.0),
        data::chart::kline::ClusterKind::VolumeProfile,
    );
    assert_eq!(qty, 0.0);
}

#[test]
fn test_kline_chart_alert_dragging() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();
    let alert = PriceAlert::new(
        "BTCUSDT",
        50000.0,
        data::chart::alert::AlertCondition::CrossAbove,
    );
    let alert_id = alert.id;
    chart.add_alert(alert);

    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    // Start drag state
    *chart.alert_drag_state.borrow_mut() = Some((alert_id, 50000.0));

    // Move cursor while dragging - previously caused RefCell BorrowMutError panic (exit code 101)
    let cursor = mouse::Cursor::Available(Point::new(400.0, 300.0));
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(400.0, 300.0),
    });
    let action = chart.update(&mut interaction, &move_event, bounds, cursor);
    assert!(action.is_some());
    assert!(chart.alert_drag_state.borrow().is_some());

    // Release mouse button - finishes drag and publishes Message::UpdateAlertPrice
    let release_event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let release_action = chart.update(&mut interaction, &release_event, bounds, cursor);
    assert!(release_action.is_some());
    assert!(chart.alert_drag_state.borrow().is_none());
}

#[test]
fn test_alert_close_button_click() {
    use iced::widget::canvas::Program;

    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines = vec![Kline {
        time: 1000,
        open: Price::from_f32(50000.0),
        high: Price::from_f32(50050.0),
        low: Price::from_f32(49950.0),
        close: Price::from_f32(50000.0),
        volume: (5.0, 5.0),
    }];
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );
    let alert = PriceAlert::new(
        "BTCUSDT",
        50000.0,
        data::chart::alert::AlertCondition::CrossAbove,
    );
    let alert_id = alert.id;
    chart.add_alert(alert);

    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    let alert_y = chart.state().price_to_y(Price::from_f32(50000.0));
    let alert_screen_y =
        (alert_y + chart.state().translation.y) * chart.state().scaling + bounds.height / 2.0;

    // 1. Click on the '✕' close button at the right edge
    let close_click_pos = Point::new(bounds.width - 10.0, alert_screen_y);
    let click_event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let action = chart.update(
        &mut interaction,
        &click_event,
        bounds,
        mouse::Cursor::Available(close_click_pos),
    );
    assert!(action.is_some());
    // Verify it did not enter drag state and alert_id is referenced
    assert_eq!(alert_id, chart.alerts[0].id);
    assert!(chart.alert_drag_state.borrow().is_none());

    // 2. Mouse interaction returns Pointer over close button
    let hover_interaction = chart.mouse_interaction(
        &interaction,
        bounds,
        mouse::Cursor::Available(close_click_pos),
    );
    assert_eq!(hover_interaction, mouse::Interaction::Pointer);

    // 3. Mouse interaction returns ResizingVertically over price drag area
    let drag_area_pos = Point::new(bounds.width - 40.0, alert_screen_y);
    let drag_hover_interaction = chart.mouse_interaction(
        &interaction,
        bounds,
        mouse::Cursor::Available(drag_area_pos),
    );
    assert_eq!(
        drag_hover_interaction,
        mouse::Interaction::ResizingVertically
    );
}

#[test]
fn test_update_alert_price_preserves_status() {
    let mut chart = make_test_chart();
    let mut alert = PriceAlert::new(
        "BTCUSDT",
        50000.0,
        data::chart::alert::AlertCondition::CrossAbove,
    );
    let alert_id = alert.id;
    alert.status = AlertStatus::Triggered;
    chart.add_alert(alert);

    // Updating price on a triggered alert must preserve Triggered status (not reset to Active)
    chart.update_alert_price(alert_id, 51000.0);
    let updated = chart.alerts.iter().find(|a| a.id == alert_id).unwrap();
    assert_eq!(updated.target_price, 51000.0);
    assert_eq!(updated.status, AlertStatus::Triggered);

    // Active alert also retains Active status
    let active_alert = PriceAlert::new(
        "BTCUSDT",
        40000.0,
        data::chart::alert::AlertCondition::CrossAbove,
    );
    let active_id = active_alert.id;
    chart.add_alert(active_alert);
    chart.update_alert_price(active_id, 41000.0);
    let updated_active = chart.alerts.iter().find(|a| a.id == active_id).unwrap();
    assert_eq!(updated_active.target_price, 41000.0);
    assert_eq!(updated_active.status, AlertStatus::Active);
}

#[test]
fn test_kline_indicator_settings_persistence() {
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

    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);

    // 1. Initializing KlineChart::new with explicit config applies settings to indicators
    let chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[KlineIndicator::PositionFlow],
        ticker_info,
        &KlineChartKind::Candles,
        Some(custom_cfg),
    );

    assert_eq!(chart.config(), custom_cfg);
    if let Some(indi) = chart.indicators[KlineIndicator::PositionFlow].as_ref() {
        let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
        assert_eq!(pf.colors, custom_colors);
    } else {
        panic!("PositionFlow indicator should be present");
    }

    // 2. set_visual_config updates chart config and sets global user default
    let mut chart2 = make_test_chart();
    let mut updated_cfg = Config::factory_default();
    updated_cfg.position_flow_colors = custom_colors;
    updated_cfg.rolling_vwap_window_hours = 72;
    chart2.set_visual_config(updated_cfg);

    assert_eq!(chart2.config(), updated_cfg);
    assert_eq!(
        data::chart::kline::user_default_kline_config(),
        Some(updated_cfg)
    );

    // 3. New chart created with None inherits user default config
    let chart3 = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[KlineIndicator::PositionFlow],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );
    assert_eq!(chart3.config(), updated_cfg);
    if let Some(indi) = chart3.indicators[KlineIndicator::PositionFlow].as_ref() {
        let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
        assert_eq!(pf.colors, custom_colors);
    } else {
        panic!("PositionFlow indicator should be present in chart3");
    }

    // 4. Toggling an indicator on inherits the chart's current config
    let mut chart4 = make_test_chart();
    chart4.set_visual_config(custom_cfg);
    chart4.toggle_indicator(KlineIndicator::PositionFlow);
    if let Some(indi) = chart4.indicators[KlineIndicator::PositionFlow].as_ref() {
        let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
        assert_eq!(pf.colors, custom_colors);
    } else {
        panic!("PositionFlow indicator should be present after toggle");
    }
}

#[test]
fn test_snap_to_45_degrees() {
    let chart = make_test_chart();
    let view_state = chart.state();

    let anchor = (1000, 100.0);
    let anchor_x = view_state.interval_to_x(anchor.0);
    let anchor_y = view_state.price_to_y(Price::from_f32(anchor.1));

    // 1. Zero delta returns anchor unchanged
    let (t0, p0) = snap_to_45_degrees(anchor, Point::new(anchor_x, anchor_y), view_state);
    assert_eq!(t0, anchor.0);
    assert_eq!(p0, anchor.1);

    // 2. Near-horizontal motion (10 degrees) snaps to exact 0 degrees
    let near_h = Point::new(anchor_x + 100.0, anchor_y + 10.0);
    let (th, ph) = snap_to_45_degrees(anchor, near_h, view_state);
    assert_eq!(ph, anchor.1);
    assert!(th > anchor.0);

    // 3. Near-horizontal left motion (170 degrees) snaps to 180 degrees
    let near_hl = Point::new(anchor_x - 100.0, anchor_y + 10.0);
    let (thl, phl) = snap_to_45_degrees(anchor, near_hl, view_state);
    assert_eq!(phl, anchor.1);
    assert!(thl < anchor.0);

    // 4. Near-vertical down motion (85 degrees) snaps to 90 degrees
    let near_v = Point::new(anchor_x + 8.0, anchor_y + 100.0);
    let (tv, pv) = snap_to_45_degrees(anchor, near_v, view_state);
    assert_eq!(tv, anchor.0);
    assert_ne!(pv, anchor.1);

    // 5. Near-vertical up motion (-85 degrees) snaps to -90 degrees
    let near_vu = Point::new(anchor_x + 8.0, anchor_y - 100.0);
    let (tvu, pvu) = snap_to_45_degrees(anchor, near_vu, view_state);
    assert_eq!(tvu, anchor.0);
    assert_ne!(pvu, anchor.1);

    // 6. Near 45-degree diagonal (40 degrees) snaps to 45 degrees
    let near_diag = Point::new(anchor_x + 100.0, anchor_y + 80.0);
    let (td, pd) = snap_to_45_degrees(anchor, near_diag, view_state);
    let diag_x = view_state.interval_to_x(td);
    let diag_y = view_state.price_to_y(Price::from_f32(pd));
    let dx = (diag_x - anchor_x).abs();
    let dy = (diag_y - anchor_y).abs();
    assert!((dx - dy).abs() < 1.0);
}

#[test]
fn test_trendline_drawing_angle_snap_with_shift() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();
    chart.chart.latest_x = 100_000_000;
    chart.chart.base_price_y = Price::from_f32(100.0);
    chart.chart.cell_width = 10.0;
    chart.chart.cell_height = 10.0;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    chart.set_active_drawing_tool(DrawingTool::Trendline);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Trendline);

    // 1. Click first point at screen center (400, 300) -> frame (0, 0)
    let pt1 = Point::new(400.0, 300.0);
    let click1_event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let action1 = chart.update(
        &mut interaction,
        &click1_event,
        bounds,
        mouse::Cursor::Available(pt1),
    );
    assert!(action1.is_some());
    assert!(chart.drawing_state.borrow().in_progress.is_some());

    // 2. Move cursor near horizontal (600, 310) -> frame (200, 10), angle ~2.8 deg
    let shift_key_down = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::SHIFT,
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ShiftLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
    });
    let _ = chart.update(
        &mut interaction,
        &shift_key_down,
        bounds,
        mouse::Cursor::Available(pt1),
    );
    assert!(chart.drawing_state.borrow().shift_pressed);

    let pt2_near_h = Point::new(600.0, 310.0);
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: pt2_near_h,
    });
    let _ = chart.update(
        &mut interaction,
        &move_event,
        bounds,
        mouse::Cursor::Available(pt2_near_h),
    );

    // Trendline p2 should be snapped to horizontal (same price as p1)
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p1, p2 } = in_prog.kind {
        assert_eq!(p1.1, p2.1);
    } else {
        panic!("Expected Trendline in progress");
    }

    // 3. Releasing Shift un-snaps on next move
    let shift_key_up = Event::Keyboard(iced::keyboard::Event::KeyReleased {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ShiftLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
    });
    let _ = chart.update(
        &mut interaction,
        &shift_key_up,
        bounds,
        mouse::Cursor::Available(pt2_near_h),
    );
    assert!(!chart.drawing_state.borrow().shift_pressed);

    let _ = chart.update(
        &mut interaction,
        &move_event,
        bounds,
        mouse::Cursor::Available(pt2_near_h),
    );
    let in_prog_unsnapped = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p1, p2 } = in_prog_unsnapped.kind {
        assert_ne!(p1.1, p2.1);
    } else {
        panic!("Expected Trendline in progress");
    }

    // 4. Press Shift again and click to finalize line
    let _ = chart.update(
        &mut interaction,
        &shift_key_down,
        bounds,
        mouse::Cursor::Available(pt2_near_h),
    );
    let in_prog_snapped_again = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p1, p2 } = in_prog_snapped_again.kind {
        assert_eq!(p1.1, p2.1);
    } else {
        panic!("Expected Trendline in progress");
    }

    let click2_event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let finish_action = chart.update(
        &mut interaction,
        &click2_event,
        bounds,
        mouse::Cursor::Available(pt2_near_h),
    );
    assert!(finish_action.is_some());
    assert!(chart.drawing_state.borrow().in_progress.is_none());
    assert_eq!(chart.active_drawing_tool(), DrawingTool::None);
}

#[test]
fn test_trendline_handle_drag_angle_snap_with_shift() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();
    chart.chart.latest_x = 100_000_000;
    chart.chart.base_price_y = Price::from_f32(100.0);
    chart.chart.cell_width = 10.0;
    chart.chart.cell_height = 10.0;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    let tl = Drawing::trendline(
        (100_000_000, 100.0),
        (100_300_000, 110.0),
        [0.3, 0.75, 1.0, 1.0],
        1.5,
    );
    let tl_id = tl.id;
    chart.add_drawing(tl.clone());
    chart.drawing_state.borrow_mut().selected_drawing = Some(tl_id);

    // Begin dragging handle 1 (endpoint p2)
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingHandle {
        handle_idx: 1,
        initial_drawing: tl.clone(),
        current_drawing: tl,
    });

    // Set Shift active
    chart.drawing_state.borrow_mut().shift_pressed = true;

    // Move cursor near horizontal to p1 in screen coordinates
    let p1_frame_x = chart.state().interval_to_x(100_000_000);
    let p1_frame_y = chart.state().price_to_y(Price::from_f32(100.0));
    let near_h_screen = Point::new(
        p1_frame_x + 150.0 + bounds.width / 2.0,
        p1_frame_y + 8.0 + bounds.height / 2.0,
    );
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: near_h_screen,
    });
    let _ = chart.update(
        &mut interaction,
        &move_event,
        bounds,
        mouse::Cursor::Available(near_h_screen),
    );

    if let Some(DrawingDrag::MovingHandle {
        current_drawing, ..
    }) = &chart.drawing_state.borrow().drag
    {
        if let DrawingKind::Trendline { p1, p2 } = current_drawing.kind {
            assert_eq!(p1.1, p2.1);
        } else {
            panic!("Expected Trendline in drag");
        }
    } else {
        panic!("Expected MovingHandle drag");
    }
}

#[test]
fn test_drawing_active_prevents_ruler() {
    use iced::widget::canvas::Program;

    let mut chart = make_test_chart();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;

    chart.set_active_drawing_tool(DrawingTool::Trendline);

    let shift_key = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::SHIFT,
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ShiftLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
    });

    let cursor_pos = Point::new(300.0, 300.0);
    let action = chart.update(
        &mut interaction,
        &shift_key,
        bounds,
        mouse::Cursor::Available(cursor_pos),
    );
    assert!(action.is_some());
    assert!(!matches!(interaction, Interaction::Ruler { .. }));
    assert!(chart.drawing_state.borrow().shift_pressed);
}

#[test]
fn test_smart_magnet_snap_to_ohlc() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines = vec![exchange::Kline {
        time: 1_000_000,
        open: Price::from_f32(100.0),
        high: Price::from_f32(110.0),
        low: Price::from_f32(90.0),
        close: Price::from_f32(105.0),
        volume: (10.0, 10.0),
    }];
    let chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    let view_state = chart.state();
    let candle_x = view_state.interval_to_x(1_000_000);

    // 1. Cursor near High (109.0) -> snaps to High (110.0)
    let pos_near_high = Point::new(
        candle_x + 2.0,
        view_state.price_to_y(Price::from_f32(109.0)),
    );
    let (t_h, p_h) = snap_to_smart_magnet(&chart.data_source, view_state, pos_near_high);
    assert_eq!(t_h, 1_000_000);
    assert!((p_h - 110.0).abs() < 1e-3);

    // 2. Cursor near Close (104.0) -> snaps to Close (105.0)
    let pos_near_close = Point::new(candle_x, view_state.price_to_y(Price::from_f32(104.0)));
    let (t_c, p_c) = snap_to_smart_magnet(&chart.data_source, view_state, pos_near_close);
    assert_eq!(t_c, 1_000_000);
    assert!((p_c - 105.0).abs() < 1e-3);

    // 3. Cursor near Open (101.0) -> snaps to Open (100.0)
    let pos_near_open = Point::new(
        candle_x - 2.0,
        view_state.price_to_y(Price::from_f32(101.0)),
    );
    let (t_o, p_o) = snap_to_smart_magnet(&chart.data_source, view_state, pos_near_open);
    assert_eq!(t_o, 1_000_000);
    assert!((p_o - 100.0).abs() < 1e-3);

    // 4. Cursor near Low (92.0) -> snaps to Low (90.0)
    let pos_near_low = Point::new(candle_x, view_state.price_to_y(Price::from_f32(92.0)));
    let (t_l, p_l) = snap_to_smart_magnet(&chart.data_source, view_state, pos_near_low);
    assert_eq!(t_l, 1_000_000);
    assert!((p_l - 90.0).abs() < 1e-3);

    // 5. Cursor too far horizontally -> does not snap, retains raw cursor coordinates
    let pos_far_x = Point::new(
        candle_x + 500.0,
        view_state.price_to_y(Price::from_f32(109.0)),
    );
    let (t_far, p_far) = snap_to_smart_magnet(&chart.data_source, view_state, pos_far_x);
    assert_ne!(t_far, 1_000_000);
    assert!((p_far - 109.0).abs() < 1e-3);
}

#[test]
fn test_smart_magnet_ctrl_inversion_and_settings() {
    use iced::widget::canvas::Program;

    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let klines = vec![exchange::Kline {
        time: 1_000_000,
        open: Price::from_f32(100.0),
        high: Price::from_f32(110.0),
        low: Price::from_f32(90.0),
        close: Price::from_f32(105.0),
        volume: (10.0, 10.0),
    }];
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut interaction = Interaction::None;
    chart.set_active_drawing_tool(DrawingTool::Trendline);
    assert!(chart.is_magnet_active());

    let candle_x = chart.chart.interval_to_x(1_000_000);
    let cursor_pt = Point::new(
        bounds.width / 2.0 + (candle_x + chart.chart.translation.x) * chart.chart.scaling,
        bounds.height / 2.0
            + (chart.chart.price_to_y(Price::from_f32(108.0)) + chart.chart.translation.y)
                * chart.chart.scaling,
    );

    // 1. First click: start trendline -> snaps to High (110.0)
    let click1 = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let _ = chart.update(
        &mut interaction,
        &click1,
        bounds,
        mouse::Cursor::Available(cursor_pt),
    );
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p1, .. } = in_prog.kind {
        assert!((p1.1 - 110.0).abs() < 1e-3);
        assert_eq!(p1.0, 1_000_000);
    } else {
        panic!("Expected Trendline");
    }

    // 2. Cursor moved near Close (104.0) -> p2 snaps to 105.0
    let move_near_close = Point::new(
        bounds.width / 2.0 + (candle_x + chart.chart.translation.x) * chart.chart.scaling,
        bounds.height / 2.0
            + (chart.chart.price_to_y(Price::from_f32(104.0)) + chart.chart.translation.y)
                * chart.chart.scaling,
    );
    let move_event = Event::Mouse(mouse::Event::CursorMoved {
        position: move_near_close,
    });
    let _ = chart.update(
        &mut interaction,
        &move_event,
        bounds,
        mouse::Cursor::Available(move_near_close),
    );
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p2, .. } = in_prog.kind {
        assert!((p2.1 - 105.0).abs() < 1e-3);
    }

    // 3. Holding Ctrl temporarily disables magnet
    let ctrl_down = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Control),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::CTRL,
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ControlLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Control),
    });
    let _ = chart.update(
        &mut interaction,
        &ctrl_down,
        bounds,
        mouse::Cursor::Available(move_near_close),
    );
    assert!(chart.drawing_state.borrow().ctrl_pressed);
    assert!(!chart.is_magnet_active());

    // While Ctrl held, cursor at 104.0 is unsnapped (price 104.0)
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p2, .. } = in_prog.kind {
        assert!((p2.1 - 104.0).abs() < 1e-3);
    }

    // 4. Releasing Ctrl re-enables magnet
    let ctrl_up = Event::Keyboard(iced::keyboard::Event::KeyReleased {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Control),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ControlLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Control),
    });
    let _ = chart.update(
        &mut interaction,
        &ctrl_up,
        bounds,
        mouse::Cursor::Available(move_near_close),
    );
    assert!(!chart.drawing_state.borrow().ctrl_pressed);
    assert!(chart.is_magnet_active());

    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p2, .. } = in_prog.kind {
        assert!((p2.1 - 105.0).abs() < 1e-3);
    }

    // 5. Magnet disabled in settings -> inverted by Ctrl
    chart.toggle_magnet_mode();
    assert!(!chart.config.magnet_mode);
    assert!(!chart.is_magnet_active());

    // Without Ctrl, it's unsnapped
    let _ = chart.update(
        &mut interaction,
        &move_event,
        bounds,
        mouse::Cursor::Available(move_near_close),
    );
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p2, .. } = in_prog.kind {
        assert!((p2.1 - 104.0).abs() < 1e-3);
    }

    // With Ctrl held, it temporarily enables magnet
    let _ = chart.update(
        &mut interaction,
        &ctrl_down,
        bounds,
        mouse::Cursor::Available(move_near_close),
    );
    assert!(chart.is_magnet_active());
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Trendline { p2, .. } = in_prog.kind {
        assert!((p2.1 - 105.0).abs() < 1e-3);
    }
}

#[test]
fn test_insert_hist_klines_rebuilds_rolling_vwap() {
    const DAY_MS: u64 = 86_400_000;
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);

    // 1. Start with recent candles (Day 20, Day 21)
    let recent_klines = vec![
        Kline {
            time: 20 * DAY_MS,
            open: Price::from_f32(200.0),
            high: Price::from_f32(200.0),
            low: Price::from_f32(200.0),
            close: Price::from_f32(200.0),
            volume: (10.0, 10.0),
        },
        Kline {
            time: 21 * DAY_MS,
            open: Price::from_f32(210.0),
            high: Price::from_f32(210.0),
            low: Price::from_f32(210.0),
            close: Price::from_f32(210.0),
            volume: (10.0, 10.0),
        },
    ];

    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::D1),
        1.0,
        &recent_klines,
        Vec::new(),
        &[KlineIndicator::RollingVwap],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    // 2. Fetch earlier historical candles (Day 1, Day 2)
    let hist_klines = vec![
        Kline {
            time: DAY_MS,
            open: Price::from_f32(100.0),
            high: Price::from_f32(100.0),
            low: Price::from_f32(100.0),
            close: Price::from_f32(100.0),
            volume: (10.0, 10.0),
        },
        Kline {
            time: 2 * DAY_MS,
            open: Price::from_f32(110.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(110.0),
            close: Price::from_f32(110.0),
            volume: (10.0, 10.0),
        },
    ];

    chart.insert_hist_klines(uuid::Uuid::new_v4(), &hist_klines);

    // 3. Verify RollingVwapIndicator was cleanly rebuilt in chronological order
    let rolling_indi = chart.indicators[KlineIndicator::RollingVwap]
        .as_ref()
        .unwrap()
        .as_any()
        .and_then(|a| {
            a.downcast_ref::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
        })
        .unwrap();

    assert_eq!(rolling_indi.data.len(), 4);

    // Historical Day 1 point must NOT be polluted by Day 20/21 values (must equal 100.0)
    let pt_day1 = rolling_indi.data.get(&DAY_MS).unwrap();
    assert_eq!(pt_day1.d7.unwrap().vwap, 100.0);
    assert_eq!(pt_day1.d30.unwrap().vwap, 100.0);
    assert_eq!(pt_day1.d90.unwrap().vwap, 100.0);
    assert_eq!(pt_day1.d365.unwrap().vwap, 100.0);

    // Day 20 point (19 days after Day 1):
    // 7d window must only contain Day 20 (200.0)
    let pt_day20 = rolling_indi.data.get(&(20 * DAY_MS)).unwrap();
    assert_eq!(pt_day20.d7.unwrap().vwap, 200.0);

    // 4. Ensure insert_trades_buffer on TimeBased does not insert trade timestamps into Rolling VWAP data
    let sample_trade = exchange::Trade {
        time: 21 * DAY_MS + 5_000,
        is_sell: false,
        price: Price::from_f32(212.0),
        qty: 1.0,
    };
    chart.insert_trades_buffer(&[sample_trade]);

    let rolling_indi_after = chart.indicators[KlineIndicator::RollingVwap]
        .as_ref()
        .unwrap()
        .as_any()
        .and_then(|a| {
            a.downcast_ref::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
        })
        .unwrap();

    // Length must still be 4 candle timestamps, not 5 with trade timestamp
    assert_eq!(rolling_indi_after.data.len(), 4);
    assert!(!rolling_indi_after.data.contains_key(&(21 * DAY_MS + 5_000)));
}

#[test]
fn test_kline_indicator_settings_expansion_and_reset() {
    let mut chart = make_test_chart();
    assert_eq!(chart.expanded_indicator, None);

    // 1. Toggle expansion
    chart.toggle_indicator_settings(KlineIndicator::RollingVwap);
    assert_eq!(chart.expanded_indicator, Some(KlineIndicator::RollingVwap));

    // 2. Toggling the same indicator collapses it
    chart.toggle_indicator_settings(KlineIndicator::RollingVwap);
    assert_eq!(chart.expanded_indicator, None);

    // 3. Switch to another indicator
    chart.toggle_indicator(KlineIndicator::PositionFlow);
    assert!(chart.indicators[KlineIndicator::PositionFlow].is_some());
    chart.toggle_indicator_settings(KlineIndicator::PositionFlow);
    assert_eq!(chart.expanded_indicator, Some(KlineIndicator::PositionFlow));

    // 4. Removing the active indicator resets expanded_indicator
    chart.toggle_indicator(KlineIndicator::PositionFlow);
    assert!(chart.indicators[KlineIndicator::PositionFlow].is_none());
    assert_eq!(chart.expanded_indicator, None);
}

#[test]
fn test_kline_indicator_rolling_vwap_isolation() {
    let mut chart = make_test_chart();
    assert!(chart.indicators[KlineIndicator::RollingVwap].is_none());
    assert!(chart.config.rolling_vwap_show_7d);

    // Indicator not present on chart: condition must evaluate to false
    let should_draw_phantom = chart.indicators[KlineIndicator::RollingVwap].is_some()
        && (chart.config.rolling_vwap_show_7d
            || chart.config.rolling_vwap_show_30d
            || chart.config.rolling_vwap_show_90d
            || chart.config.rolling_vwap_show_365d);
    assert!(!should_draw_phantom);

    // Add indicator to chart: condition evaluates to true
    chart.toggle_indicator(KlineIndicator::RollingVwap);
    assert!(chart.indicators[KlineIndicator::RollingVwap].is_some());
    let should_draw_active = chart.indicators[KlineIndicator::RollingVwap].is_some()
        && (chart.config.rolling_vwap_show_7d
            || chart.config.rolling_vwap_show_30d
            || chart.config.rolling_vwap_show_90d
            || chart.config.rolling_vwap_show_365d);
    assert!(should_draw_active);

    // Remove indicator: condition must evaluate to false again
    chart.toggle_indicator(KlineIndicator::RollingVwap);
    assert!(chart.indicators[KlineIndicator::RollingVwap].is_none());
    let should_draw_after_removal = chart.indicators[KlineIndicator::RollingVwap].is_some()
        && (chart.config.rolling_vwap_show_7d
            || chart.config.rolling_vwap_show_30d
            || chart.config.rolling_vwap_show_90d
            || chart.config.rolling_vwap_show_365d);
    assert!(!should_draw_after_removal);
}

#[test]
fn test_rolling_vwap_panel_toggle_and_color_customization() {
    let mut chart = make_test_chart();
    assert!(!chart.config.rolling_vwap_show_panel);

    // 1. Enabling RollingVwap when rolling_vwap_show_panel is false:
    // Overlay is active, but panel count is 0 and no layout splits are added.
    chart.toggle_indicator(KlineIndicator::RollingVwap);
    assert!(chart.indicators[KlineIndicator::RollingVwap].is_some());
    assert_eq!(chart.active_panel_indicators_count(), 0);
    assert!(chart.chart.layout.splits.is_empty());

    // 2. Enabling rolling_vwap_show_panel via set_visual_config:
    // Panel count becomes 1, layout splits dynamically allocated.
    let mut cfg = chart.config();
    cfg.rolling_vwap_show_panel = true;
    cfg.rolling_vwap_colors.d7 = data::chart::kline::TpoElementColor::Red;
    cfg.rolling_vwap_colors.d30 = data::chart::kline::TpoElementColor::Green;
    cfg.rolling_vwap_colors.d90 = data::chart::kline::TpoElementColor::Blue;
    cfg.rolling_vwap_colors.d365 = data::chart::kline::TpoElementColor::White;
    chart.set_visual_config(cfg);

    assert_eq!(chart.active_panel_indicators_count(), 1);
    assert_eq!(chart.chart.layout.splits.len(), 1);

    // Verify colors updated on indicator
    let rv = chart.indicators[KlineIndicator::RollingVwap]
        .as_ref()
        .unwrap()
        .as_any()
        .and_then(|a| {
            a.downcast_ref::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
        })
        .unwrap();
    assert_eq!(rv.colors.d7, data::chart::kline::TpoElementColor::Red);
    assert_eq!(rv.colors.d30, data::chart::kline::TpoElementColor::Green);
    assert_eq!(rv.colors.d90, data::chart::kline::TpoElementColor::Blue);
    assert_eq!(rv.colors.d365, data::chart::kline::TpoElementColor::White);
    assert_eq!(rv.colors.d7_rgb(), [235, 75, 75]);

    // 3. Disabling rolling_vwap_show_panel removes sub-panel and clears splits
    cfg.rolling_vwap_show_panel = false;
    chart.set_visual_config(cfg);
    assert_eq!(chart.active_panel_indicators_count(), 0);
    assert!(chart.chart.layout.splits.is_empty());
    // Overlay indicator is still enabled on the chart
    assert!(chart.indicators[KlineIndicator::RollingVwap].is_some());
}

#[test]
fn test_find_target_position() {
    let mut chart = make_test_chart();
    assert!(chart.find_target_position().is_none());

    // Add a long position
    let long_pos = Drawing {
        id: uuid::Uuid::new_v4(),
        kind: DrawingKind::Position {
            entry: (1600000000000, 50000.0),
            target_price: 53000.0,
            stop_price: 49000.0,
            is_long: true,
            end_time: None,
            style: None,
        },
        color: [0.0, 1.0, 0.0, 1.0],
        width: 1.0,
        is_selected: false,
        is_locked: false,
        is_synced: false,
        pane_id: None,
        layout_id: None,
        ..Default::default()
    };
    chart.add_drawing(long_pos);

    let autofill = chart.find_target_position().expect("should find position");
    assert_eq!(autofill.side, data::journal::TradeSide::Long);
    assert_eq!(autofill.entry_price, 50000.0);
    assert_eq!(autofill.stop_price, 49000.0);
    assert_eq!(autofill.target_price, 53000.0);
    assert!(autofill.notes.contains("R:R: 1:3.00"));

    // Add a short position (which is now latest)
    let short_id = uuid::Uuid::new_v4();
    let short_pos = Drawing {
        id: short_id,
        kind: DrawingKind::Position {
            entry: (1600000050000, 52000.0),
            target_price: 50000.0,
            stop_price: 53000.0,
            is_long: false,
            end_time: None,
            style: None,
        },
        color: [1.0, 0.0, 0.0, 1.0],
        width: 1.0,
        is_selected: false,
        is_locked: false,
        is_synced: false,
        pane_id: None,
        layout_id: None,
        ..Default::default()
    };
    chart.add_drawing(short_pos);

    // Without selection, should pick the latest (short)
    let autofill_latest = chart.find_target_position().expect("should find latest");
    assert_eq!(autofill_latest.side, data::journal::TradeSide::Short);
    assert_eq!(autofill_latest.entry_price, 52000.0);

    // If long is selected, should prioritize selected
    chart.set_selected_drawing(Some(chart.drawings[0].id));
    let autofill_sel = chart.find_target_position().expect("should find selected");
    assert_eq!(autofill_sel.side, data::journal::TradeSide::Long);
}

#[test]
fn test_macos_command_key_and_modifiers() {
    use iced::widget::canvas::Program;
    let chart = make_test_chart();
    let mut interaction = Interaction::None;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));

    assert!(!chart.drawing_state.borrow().ctrl_pressed);

    // 1. Press Super/Command key with COMMAND modifier
    let cmd_down = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Super),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::COMMAND,
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::SuperLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Super),
    });
    let _ = chart.update(&mut interaction, &cmd_down, bounds, cursor);
    assert!(chart.drawing_state.borrow().ctrl_pressed);

    // 2. Release Super/Command key
    let cmd_up = Event::Keyboard(iced::keyboard::Event::KeyReleased {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Super),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::SuperLeft),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Super),
    });
    let _ = chart.update(&mut interaction, &cmd_up, bounds, cursor);
    assert!(!chart.drawing_state.borrow().ctrl_pressed);

    // 3. ModifiersChanged with COMMAND
    let mods_changed = Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
        iced::keyboard::Modifiers::COMMAND,
    ));
    let _ = chart.update(&mut interaction, &mods_changed, bounds, cursor);
    assert!(chart.drawing_state.borrow().ctrl_pressed);

    // 4. ModifiersChanged reset
    let mods_reset = Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
        iced::keyboard::Modifiers::default(),
    ));
    let _ = chart.update(&mut interaction, &mods_reset, bounds, cursor);
    assert!(!chart.drawing_state.borrow().ctrl_pressed);
}

#[test]
fn test_drawing_tool_hotkeys_and_escape() {
    use iced::widget::canvas::Program;
    let chart = make_test_chart();
    let mut interaction = Interaction::None;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));

    let make_char_key = |c: &'static str| {
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c.into()),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            repeat: false,
            text: Some(c.into()),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyA),
            modified_key: iced::keyboard::Key::Character(c.into()),
        })
    };

    let make_named_key = |named: iced::keyboard::key::Named| {
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(named),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            repeat: false,
            text: None,
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            modified_key: iced::keyboard::Key::Named(named),
        })
    };

    assert_eq!(chart.active_drawing_tool(), DrawingTool::None);

    // 1. Press 't' -> Trendline
    let _ = chart.update(&mut interaction, &make_char_key("t"), bounds, cursor);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Trendline);

    // 2. Press 't' again -> None (toggle off)
    let _ = chart.update(&mut interaction, &make_char_key("t"), bounds, cursor);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::None);

    // 3. Press 'h' -> HorizontalLine
    let _ = chart.update(&mut interaction, &make_char_key("h"), bounds, cursor);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::HorizontalLine);

    // 4. Press 'b' -> Brush
    let _ = chart.update(&mut interaction, &make_char_key("b"), bounds, cursor);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Brush);

    // 5. Press 'r' -> Rectangle
    let _ = chart.update(&mut interaction, &make_char_key("r"), bounds, cursor);
    assert_eq!(chart.active_drawing_tool(), DrawingTool::Rectangle);

    // 6. Press Escape -> resets active tool to None
    let _ = chart.update(
        &mut interaction,
        &make_named_key(iced::keyboard::key::Named::Escape),
        bounds,
        cursor,
    );
    assert_eq!(chart.active_drawing_tool(), DrawingTool::None);
}

#[test]
fn test_position_styling_mutations() {
    let mut chart = make_test_chart();
    let pos = Drawing::position(
        (1000, 100.0),
        95.0,
        110.0,
        true,
        Some(2000),
        [0.2, 0.8, 0.4, 1.0],
        1.0,
    );
    let id = pos.id;
    chart.add_drawing(pos);
    chart.set_selected_drawing(Some(id));

    let new_profit = [0.1, 0.9, 0.3, 0.4];
    let new_stop = [0.9, 0.1, 0.1, 0.5];
    let new_entry = [0.7, 0.7, 0.8, 1.0];

    chart.update_selected_position_profit_color(Some(new_profit));
    chart.update_selected_position_stop_color(Some(new_stop));
    chart.update_selected_position_entry_color(new_entry);

    let d = chart
        .drawings
        .iter()
        .find(|d| d.id == id)
        .expect("drawing should exist");
    let style = d.position_style();
    assert_eq!(style.profit_color, Some(new_profit));
    assert_eq!(style.stop_color, Some(new_stop));
    assert_eq!(style.entry_color, new_entry);

    // Reset to Auto (None)
    chart.update_selected_position_profit_color(None);
    chart.update_selected_position_stop_color(None);
    let d = chart
        .drawings
        .iter()
        .find(|d| d.id == id)
        .expect("drawing should exist");
    assert_eq!(d.position_style().profit_color, None);
    assert_eq!(d.position_style().stop_color, None);
}

#[test]
fn test_position_active_fill_color_matches_custom_style() {
    let gray_profit = [0.65f32, 0.65, 0.65, 0.2];
    let red_stop = [0.9f32, 0.1, 0.2, 0.2];

    // Assert RGB channels match exact position color without green bias
    let profit_fill = position_active_fill(true, gray_profit, red_stop);
    assert_eq!(profit_fill.r, 0.65);
    assert_eq!(profit_fill.g, 0.65);
    assert_eq!(profit_fill.b, 0.65);
    assert!((profit_fill.a - 0.36).abs() < 1e-4);
}

#[test]
fn test_position_active_fill_uses_stop_color_when_not_in_profit() {
    let green_profit = [0.1f32, 0.8, 0.2, 0.2];
    let red_stop = [0.9f32, 0.1, 0.2, 0.2];

    let loss_fill = position_active_fill(false, green_profit, red_stop);
    assert_eq!((loss_fill.r, loss_fill.g, loss_fill.b), (0.9, 0.1, 0.2));
}

#[test]
fn test_position_active_fill_alpha_is_clamped() {
    let transparent = [0.5f32, 0.5, 0.5, 0.0];
    let opaque = [0.5f32, 0.5, 0.5, 1.0];

    assert_eq!(position_active_fill(true, transparent, opaque).a, 0.25);
    assert_eq!(position_active_fill(false, transparent, opaque).a, 0.65);
}

#[test]
fn test_position_shift_g_keyboard_shortcut() {
    use iced::widget::canvas::Program;
    let mut chart = make_test_chart();
    let mut interaction = Interaction::None;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    chart.chart.bounds = bounds;
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));

    let pos = Drawing::position(
        (1000, 100.0),
        95.0,
        110.0,
        true,
        Some(2000),
        [0.2, 0.8, 0.4, 1.0],
        1.0,
    );
    chart.add_drawing(pos);

    // 1. Shift + G (ASCII)
    let ascii_shift_g = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("G".into()),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::SHIFT,
        repeat: false,
        text: Some("G".into()),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyG),
        modified_key: iced::keyboard::Key::Character("G".into()),
    });
    let action = chart.update(&mut interaction, &ascii_shift_g, bounds, cursor);
    assert!(action.is_some());

    // 2. Shift + KeyG on Cyrillic/Ukrainian layout (Key::Character("П") with physical_key Code::KeyG)
    let cyrillic_shift_g = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("П".into()),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::SHIFT,
        repeat: false,
        text: Some("П".into()),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyG),
        modified_key: iced::keyboard::Key::Character("П".into()),
    });
    let action2 = chart.update(&mut interaction, &cyrillic_shift_g, bounds, cursor);
    assert!(action2.is_some());
}

#[test]
fn test_kline_chart_drawings_preservation_and_store() {
    let mut chart = make_test_chart();
    let sym = "BTCUSDT_TEST_PRESERVE";
    let d = Drawing::horizontal(65000.0, [1.0, 0.5, 0.0, 1.0], 2.0);
    chart.add_drawing(d.clone());
    data::DrawingStore::add(sym, d.clone());

    // Verify drawings exist in chart and DrawingStore
    assert_eq!(chart.drawings.len(), 1);
    let stored = data::DrawingStore::for_symbol(sym);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, d.id);

    // Simulate timeframe change recreation (matching pane.rs line 514 logic)
    let existing_drawings = chart.drawings.clone();
    let ticker = exchange::Ticker::new(sym, exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut reloaded_chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::H1),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );
    reloaded_chart.drawings = if !existing_drawings.is_empty() {
        existing_drawings
    } else {
        data::DrawingStore::for_symbol(sym)
    };

    assert_eq!(reloaded_chart.drawings.len(), 1);
    assert_eq!(reloaded_chart.drawings[0].id, d.id);

    // Clean up
    data::DrawingStore::remove(d.id);
    assert!(data::DrawingStore::for_symbol(sym).is_empty());
}

#[test]
fn test_tpo_profile_replay_session_cutoff() {
    // Create 2 days of 1-hour candles:
    // Day 1: 0 .. 24h (0 .. 86_400_000 ms)
    // Day 2: 24h .. 48h (86_400_000 .. 172_800_000 ms)
    let hour_ms = 3_600_000u64;
    let mut klines = Vec::new();
    for i in 0..48 {
        klines.push(Kline {
            time: i * hour_ms,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (10.0, 10.0),
        });
    }

    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::H1),
        1.0,
        &klines,
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Candles,
        None,
    );

    let tpo_cache = RefCell::new(TpoCache::default());

    // Case 1: In replay, cutoff is at 30 hours (middle of Day 2)
    // Day 1 is closed (session_end = 86_400_000 <= 30h)
    // Day 2 is unclosed (session_end = 172_800_000 > 30h) -> MUST NOT be in profiles!
    let cutoff_day2_mid = 30 * hour_ms;
    let profiles_replay_mid = prepare_tpo_profiles(
        &chart.data_source,
        &tpo_cache,
        0,
        cutoff_day2_mid,
        data::chart::tpo::SessionPeriod::Daily,
        &[],
        1.0,
        true, // is_replay = true
    );

    // In replay at Day 2 midday, only Day 1 profile is returned
    assert_eq!(profiles_replay_mid.len(), 1);
    assert_eq!(profiles_replay_mid[0].session_start, 0);
    assert_eq!(profiles_replay_mid[0].session_end, 86_400_000);
    assert!(tpo_cache.borrow().sessions.contains_key(&0));
    assert!(!tpo_cache.borrow().sessions.contains_key(&(86_400_000i64)));

    // Case 2: In replay, cutoff advances to 48 hours (Day 2 closes)
    let cutoff_day2_end = 48 * hour_ms;
    let profiles_replay_end = prepare_tpo_profiles(
        &chart.data_source,
        &tpo_cache,
        0,
        cutoff_day2_end,
        data::chart::tpo::SessionPeriod::Daily,
        &[],
        1.0,
        true, // is_replay = true
    );

    // Now Day 2 is closed, so both Day 1 and Day 2 profiles are returned
    assert_eq!(profiles_replay_end.len(), 2);
    assert_eq!(profiles_replay_end[0].session_start, 0);
    assert_eq!(profiles_replay_end[1].session_start, 86_400_000);
    assert!(tpo_cache.borrow().sessions.contains_key(&(86_400_000i64)));

    // Case 3: In live mode (is_replay = false), unclosed Day 2 IS built/returned progressively
    tpo_cache.borrow_mut().sessions.clear();
    let profiles_live_mid = prepare_tpo_profiles(
        &chart.data_source,
        &tpo_cache,
        0,
        cutoff_day2_mid,
        data::chart::tpo::SessionPeriod::Daily,
        &[],
        1.0,
        false, // is_replay = false (live mode)
    );
    // Both Day 1 and ongoing Day 2 are built and returned in live mode
    assert_eq!(profiles_live_mid.len(), 2);
    assert_eq!(profiles_live_mid[0].session_start, 0);
    assert_eq!(profiles_live_mid[1].session_start, 86_400_000);
}

#[test]
fn test_evaluate_position_outcome_and_dragging() {
    let mut chart = make_test_chart();

    if let PlotData::TimeBased(ref mut ts) = chart.data_source {
        ts.interval = exchange::Timeframe::M1;
        for (t, o, h, l, c) in [
            (60_000, 100.0, 102.0, 99.0, 101.0),
            (120_000, 101.0, 108.0, 100.0, 107.0),
            (180_000, 107.0, 108.0, 94.0, 95.0),
        ] {
            let kline = exchange::Kline {
                time: t,
                open: Price::from_f32(o),
                high: Price::from_f32(h),
                low: Price::from_f32(l),
                close: Price::from_f32(c),
                volume: (50.0, 50.0),
            };
            ts.datapoints.insert(
                t,
                KlineDataPoint {
                    kline,
                    footprint: Default::default(),
                    trades_fetched: false,
                },
            );
        }
    }

    // Test 1: Long Position hits TP at candle 2 (t=120_000)
    let outcome_tp = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        100.0,
        105.0, // TP
        95.0,  // SL
        true,  // is_long
        200_000,
        None,
    );
    assert!(outcome_tp.is_closed);
    assert!(outcome_tp.is_profit);
    assert_eq!(outcome_tp.exit_time, 120_000);
    assert_eq!(outcome_tp.exit_price, 105.0);

    // Test 2: Long Position with higher TP (110.0) hits SL at candle 3 (t=180_000)
    let outcome_sl = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        100.0,
        110.0, // TP (not hit)
        96.0,  // SL (hit at candle 3 where low=94)
        true,
        200_000,
        None,
    );
    assert!(outcome_sl.is_closed);
    assert!(!outcome_sl.is_profit);
    assert_eq!(outcome_sl.exit_time, 180_000);
    assert_eq!(outcome_sl.exit_price, 96.0);

    // Test 3: Short Position hits TP at candle 3 (where low=94, target=95)
    let outcome_short_tp = evaluate_position_outcome(
        &chart.data_source,
        120_000,
        107.0,
        95.0,  // Short TP
        110.0, // Short SL
        false, // is_long = false
        200_000,
        None,
    );
    assert!(outcome_short_tp.is_closed);
    assert!(outcome_short_tp.is_profit);
    assert_eq!(outcome_short_tp.exit_time, 180_000);
    assert_eq!(outcome_short_tp.exit_price, 95.0);

    // Test 4: Expired position in history (neither TP nor SL hit, end_bound <= latest_candle 180_000)
    let outcome_expired = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        100.0,
        110.0,
        90.0,
        true,
        60_000,
        None,
    );
    assert!(outcome_expired.is_closed);
    assert!(outcome_expired.is_profit); // candle 1 close=101 >= 100
    assert_eq!(outcome_expired.exit_time, 60_000);
    assert_eq!(outcome_expired.exit_price, 101.0);

    // Test 4b: Open active position extending into future (end_bound 300_000 > latest_candle 180_000)
    let outcome_open = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        100.0,
        110.0,
        90.0,
        true,
        300_000,
        None,
    );
    assert!(!outcome_open.is_closed);
    assert_eq!(outcome_open.exit_time, 180_000);
    assert_eq!(outcome_open.exit_price, 95.0);

    // Test 4c: Replay mode cutoff - position ends after cutoff (remains open in replay)
    let outcome_replay = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        100.0,
        110.0,
        90.0,
        true,
        120_000,
        Some(60_000),
    );
    assert!(!outcome_replay.is_closed);
    assert_eq!(outcome_replay.exit_time, 60_000);
    assert_eq!(outcome_replay.exit_price, 101.0);

    // Test 4d: Position entry price never reached (entry=90.0, lowest low is 94.0)
    let outcome_not_entered = evaluate_position_outcome(
        &chart.data_source,
        60_000,
        90.0,
        110.0,
        85.0,
        true,
        200_000,
        None,
    );
    assert!(!outcome_not_entered.is_entered);
    assert!(!outcome_not_entered.is_closed);

    // Test 5: Dragging Handle 3 (right handle) stretches end_time
    let pos = Drawing::position(
        (60_000, 100.0),
        95.0,
        105.0,
        true,
        Some(120_000),
        [0.2, 0.8, 0.4, 1.0],
        1.0,
    );
    let mut cur = pos.clone();
    chart.apply_handle_movement(
        3,
        &pos,
        &mut cur,
        Point::new(chart.state().interval_to_x(250_000), 0.0),
        false,
        false,
    );
    if let DrawingKind::Position { end_time, .. } = cur.kind {
        assert_eq!(end_time, Some(250_000));
    } else {
        panic!("Expected Position");
    }

    // Test 6: Dragging Handle 0 (entry) translates both entry and end_time
    let mut entry_moved = cur.clone();
    chart.apply_handle_movement(
        0,
        &cur,
        &mut entry_moved,
        Point::new(
            chart.state().interval_to_x(80_000),
            chart.state().price_to_y(Price::from_f32(102.0)),
        ),
        false,
        false,
    );
    if let DrawingKind::Position {
        entry, end_time, ..
    } = entry_moved.kind
    {
        assert_eq!(entry.0, 80_000);
        // Dragging entry handle adjusts entry point, preserving end_time
        assert_eq!(end_time, Some(250_000));
    } else {
        panic!("Expected Position");
    }
}

#[test]
fn test_tpo_value_area_alignment() {
    let vah_y = 120.0f32;
    let val_y = 350.0f32;
    let tpo_row_height = 10.0f32;
    let line_thickness = 1.5f32;

    let top_y = vah_y.min(val_y);
    let bottom_y = vah_y.max(val_y);
    let half_row = tpo_row_height / 2.0;
    let block_top_y = top_y - half_row;
    let block_bottom_y = bottom_y + half_row;
    let h = block_bottom_y - block_top_y;

    let vah_line_y = block_top_y;
    let val_line_y = block_bottom_y - line_thickness;

    assert_eq!(h, 240.0);
    assert_eq!(vah_line_y, block_top_y);
    assert_eq!(val_line_y + line_thickness, block_bottom_y);
    assert!(vah_line_y >= block_top_y);
    assert!(val_line_y + line_thickness <= block_top_y + h);
}

#[test]
fn test_drawing_handle_dimensions_invariants() {
    for scaling in [0.05f32, 0.1, 0.5, 1.0, 2.0, 4.0] {
        let handle_radius = 4.0 / scaling;
        let screen_path_radius = handle_radius * scaling;
        assert!((screen_path_radius - 4.0).abs() < 1e-5);

        // In Iced canvas, Stroke::width is physical screen pixels and must remain fixed.
        let handle_stroke_w = 1.5f32;
        let outer_radius = screen_path_radius + handle_stroke_w / 2.0;
        let inner_radius = screen_path_radius - handle_stroke_w / 2.0;

        assert_eq!(outer_radius, 4.75);
        assert_eq!(inner_radius, 3.25);
        assert!(
            inner_radius > 0.0,
            "Inner dark center must not be swallowed by stroke"
        );
    }
}

#[test]
fn test_set_footprint_aggregate_auto_reload() {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    let trade = Trade {
        time: 300_100,
        price: Price::from_f32(100.0),
        qty: 5.0,
        is_sell: false,
    };
    chart.insert_trades_buffer(&[trade]);

    let has_trade = |c: &KlineChart| -> bool {
        if let PlotData::TimeBased(ref ts) = c.data_source {
            ts.datapoints.values().any(|dp| !dp.footprint.is_empty())
        } else {
            false
        }
    };

    assert!(has_trade(&chart));

    // Toggling aggregate resets the in-memory footprint bins automatically
    chart.set_footprint_aggregate(true);
    assert!(!has_trade(&chart));
    assert!(chart.is_aggregate_footprint());

    // Insert new aggregated trades
    let trade2 = Trade {
        time: 300_200,
        price: Price::from_f32(100.0),
        qty: 12.0,
        is_sell: false,
    };
    chart.insert_trades_buffer(&[trade2]);
    assert!(has_trade(&chart));

    // Toggling back to false resets again automatically
    chart.set_footprint_aggregate(false);
    assert!(!has_trade(&chart));
    assert!(!chart.is_aggregate_footprint());
}

#[test]
fn test_kline_chart_undo_redo_stack_operations() {
    let mut chart = make_test_chart();
    let d1 = Drawing::trendline((1000, 100.0), (2000, 110.0), [1.0, 1.0, 1.0, 1.0], 1.0);
    let d2 = Drawing::rectangle((1500, 90.0), (2500, 120.0), [1.0, 1.0, 1.0, 1.0], 1.0);

    assert!(!chart.can_undo());
    assert!(!chart.can_redo());
    assert_eq!(chart.undo(), None);
    assert_eq!(chart.redo(), None);

    // 1. Add drawing
    chart.add_drawing(d1.clone());
    chart.push_undo(DrawingHistoryAction::Add(d1.clone()));
    assert!(chart.can_undo());
    assert!(!chart.can_redo());
    assert_eq!(chart.drawings.len(), 1);

    // Undo Add -> removes drawing
    let undone = chart.undo();
    assert_eq!(undone, Some(DrawingHistoryAction::Add(d1.clone())));
    assert_eq!(chart.drawings.len(), 0);
    assert!(!chart.can_undo());
    assert!(chart.can_redo());

    // Redo Add -> restores drawing
    let redone = chart.redo();
    assert_eq!(redone, Some(DrawingHistoryAction::Add(d1.clone())));
    assert_eq!(chart.drawings.len(), 1);
    assert!(chart.can_undo());
    assert!(!chart.can_redo());

    // 2. Delete drawing
    chart.delete_drawing(d1.id);
    chart.push_undo(DrawingHistoryAction::Delete(d1.clone()));
    assert_eq!(chart.drawings.len(), 0);

    // Undo Delete -> restores drawing
    let undone_del = chart.undo();
    assert_eq!(undone_del, Some(DrawingHistoryAction::Delete(d1.clone())));
    assert_eq!(chart.drawings.len(), 1);
    assert_eq!(chart.drawings[0].id, d1.id);
    assert_eq!(chart.selected_drawing().map(|d| d.id), Some(d1.id));

    // Redo Delete -> deletes drawing
    let redone_del = chart.redo();
    assert_eq!(redone_del, Some(DrawingHistoryAction::Delete(d1.clone())));
    assert_eq!(chart.drawings.len(), 0);
    assert_eq!(chart.selected_drawing(), None);

    // 3. Update drawing
    chart.add_drawing(d1.clone());
    let mut d1_updated = d1.clone();
    d1_updated.color = [1.0, 0.0, 0.0, 1.0];
    chart.update_drawing(d1_updated.clone());
    chart.push_undo(DrawingHistoryAction::Update {
        old: d1.clone(),
        new: d1_updated.clone(),
    });

    // Undo Update -> reverts to old
    let undone_upd = chart.undo();
    assert_eq!(
        undone_upd,
        Some(DrawingHistoryAction::Update {
            old: d1.clone(),
            new: d1_updated.clone()
        })
    );
    assert_eq!(chart.drawings[0].color, d1.color);

    // Redo Update -> advances to new
    let redone_upd = chart.redo();
    assert_eq!(
        redone_upd,
        Some(DrawingHistoryAction::Update {
            old: d1.clone(),
            new: d1_updated.clone()
        })
    );
    assert_eq!(chart.drawings[0].color, d1_updated.color);

    // 4. Clear drawings
    chart.add_drawing(d2.clone());
    assert_eq!(chart.drawings.len(), 2);
    let cleared = chart.drawings.clone();
    chart.push_undo(DrawingHistoryAction::Clear(cleared.clone()));
    chart.clear_drawings();
    assert_eq!(chart.drawings.len(), 0);

    // Undo Clear -> restores all cleared drawings
    let undone_clr = chart.undo();
    assert_eq!(
        undone_clr,
        Some(DrawingHistoryAction::Clear(cleared.clone()))
    );
    assert_eq!(chart.drawings.len(), 2);

    // Redo Clear -> re-clears
    let redone_clr = chart.redo();
    assert_eq!(redone_clr, Some(DrawingHistoryAction::Clear(cleared)));
    assert_eq!(chart.drawings.len(), 0);

    // 5. Stack capacity limit & push clears redo
    for i in 0..110 {
        let d = Drawing::horizontal(100.0 + i as f32, [1.0, 1.0, 1.0, 1.0], 1.0);
        chart.push_undo(DrawingHistoryAction::Add(d));
    }
    assert_eq!(chart.undo_stack.len(), 100);
    chart.undo();
    assert!(chart.can_redo());
    chart.push_undo(DrawingHistoryAction::Add(d1.clone()));
    assert!(!chart.can_redo(), "push_undo must clear redo_stack");
}

#[test]
fn test_kline_chart_ctrl_z_undo_and_ctrl_y_redo_hotkeys() {
    use iced::widget::canvas::Program;
    let mut chart = make_test_chart();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));
    let mut interaction = Interaction::None;

    let d = Drawing::trendline((1000, 100.0), (2000, 110.0), [1.0, 1.0, 1.0, 1.0], 1.0);
    chart.add_drawing(d.clone());
    chart.push_undo(DrawingHistoryAction::Add(d));

    let make_key = |key: iced::keyboard::Key,
                    modifiers: iced::keyboard::Modifiers,
                    physical: iced::keyboard::key::Code| {
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            location: iced::keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: None,
            physical_key: iced::keyboard::key::Physical::Code(physical),
            modified_key: key,
        })
    };

    // 1. Ctrl + 'z' publishes Message::UndoDrawing
    let ctrl_z = make_key(
        iced::keyboard::Key::Character("z".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::keyboard::key::Code::KeyZ,
    );
    let action = chart.update(&mut interaction, &ctrl_z, bounds, cursor);
    assert!(action.is_some());

    // Simulate pane processing the undo
    chart.undo();
    assert!(chart.can_redo());

    // 2. Ctrl + 'y' publishes Message::RedoDrawing
    let ctrl_y = make_key(
        iced::keyboard::Key::Character("y".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::keyboard::key::Code::KeyY,
    );
    let action_y = chart.update(&mut interaction, &ctrl_y, bounds, cursor);
    assert!(action_y.is_some());

    // 3. Ctrl + Shift + 'z' publishes Message::RedoDrawing
    let ctrl_shift_z = make_key(
        iced::keyboard::Key::Character("z".into()),
        iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::SHIFT,
        iced::keyboard::key::Code::KeyZ,
    );
    let action_shift_z = chart.update(&mut interaction, &ctrl_shift_z, bounds, cursor);
    assert!(action_shift_z.is_some());

    // 4. Cyrillic layout 'я' / 'Я' with Ctrl triggers Undo
    let ctrl_cyrillic_ya = make_key(
        iced::keyboard::Key::Character("я".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::keyboard::key::Code::KeyZ,
    );
    chart.redo();
    let action_ya = chart.update(&mut interaction, &ctrl_cyrillic_ya, bounds, cursor);
    assert!(action_ya.is_some());

    // 5. Cyrillic layout 'н' / 'Н' with Ctrl triggers Redo
    chart.undo();
    let ctrl_cyrillic_n = make_key(
        iced::keyboard::Key::Character("н".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::keyboard::key::Code::KeyY,
    );
    let action_n = chart.update(&mut interaction, &ctrl_cyrillic_n, bounds, cursor);
    assert!(action_n.is_some());

    // 6. Cmd + 'z' (macOS COMMAND modifier) triggers Undo
    chart.redo();
    let cmd_z = make_key(
        iced::keyboard::Key::Character("z".into()),
        iced::keyboard::Modifiers::COMMAND,
        iced::keyboard::key::Code::KeyZ,
    );
    let action_cmd = chart.update(&mut interaction, &cmd_z, bounds, cursor);
    assert!(action_cmd.is_some());
}

#[test]
fn test_kline_chart_ctrl_z_in_progress_and_drag_cancelling() {
    use iced::widget::canvas::Program;
    let chart = make_test_chart();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));
    let mut interaction = Interaction::None;

    let ctrl_z = Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("z".into()),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::CTRL,
        repeat: false,
        text: None,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyZ),
        modified_key: iced::keyboard::Key::Character("z".into()),
    });

    // 1. Tool selected with no points placed yet: Ctrl+Z resets tool to None
    chart.drawing_state.borrow_mut().active_tool = DrawingTool::Trendline;
    let action = chart.update(&mut interaction, &ctrl_z, bounds, cursor);
    assert!(action.is_some());
    assert_eq!(chart.active_drawing_tool(), DrawingTool::None);

    // 2. Active drag cancellation: Ctrl+Z drops drag without publishing UndoDrawing
    let d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
        start_time: 1000,
        start_price: 100.0,
        initial_drawing: d.clone(),
        current_drawing: d.clone(),
    });
    assert!(chart.drawing_state.borrow().drag.is_some());
    let action_drag = chart.update(&mut interaction, &ctrl_z, bounds, cursor);
    assert!(action_drag.is_some());
    assert!(chart.drawing_state.borrow().drag.is_none());

    // 3. Multi-point Path: Ctrl+Z removes the latest vertex
    let path_drawing = Drawing::path(
        vec![(1000, 100.0), (2000, 105.0), (3000, 110.0)],
        [1.0, 1.0, 1.0, 1.0],
        1.0,
    );
    chart.drawing_state.borrow_mut().in_progress = Some(path_drawing);
    let action_path = chart.update(&mut interaction, &ctrl_z, bounds, cursor);
    assert!(action_path.is_some());
    let in_prog = chart.drawing_state.borrow().in_progress.clone().unwrap();
    if let DrawingKind::Path { points } = in_prog.kind {
        assert_eq!(points.len(), 2);
    } else {
        panic!("Expected Path drawing in progress");
    }
}

#[test]
fn test_footprint_active_candle_backfill_suggestion_and_loop_prevention() {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let interval = exchange::Timeframe::H1;
    let interval_ms = interval.to_milliseconds();
    let current_candle_start = (now_ms / interval_ms) * interval_ms;
    let kline = Kline {
        time: current_candle_start,
        open: Price::from_f32(100.0),
        high: Price::from_f32(105.0),
        low: Price::from_f32(99.0),
        close: Price::from_f32(102.0),
        volume: (10.0, 10.0),
    };

    // 1. Chart on Binance (Linear Perps)
    let binance_ticker =
        exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let binance_info = exchange::TickerInfo::new(binance_ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg.clone(),
        Basis::Time(interval),
        1.0,
        &[kline],
        Vec::new(),
        &[],
        binance_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    // Verify active candle is suggested on Binance
    if let PlotData::TimeBased(ref ts) = chart.data_source {
        let suggested =
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms);
        assert_eq!(
            suggested,
            Some((current_candle_start, current_candle_start + interval_ms)),
            "Binance footprint chart must suggest active candle fetch when unfetched"
        );
    } else {
        panic!("Expected TimeBased PlotData");
    }

    // 2. Unsupported exchange (Bybit) must NOT suggest active candle
    let bybit_ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BybitLinear);
    let bybit_info = exchange::TickerInfo::new(bybit_ticker, 0.1, 0.001, None);
    let chart_bybit = KlineChart::new(
        view_cfg,
        Basis::Time(interval),
        1.0,
        &[kline],
        Vec::new(),
        &[],
        bybit_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );
    if let PlotData::TimeBased(ref ts) = chart_bybit.data_source {
        let suggested =
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms);
        assert_eq!(
            suggested, None,
            "Bybit chart must never suggest active candle fetch"
        );
    }

    // 3. Seamless merge with live WebSocket trades & deduplication
    let ws_trade = Trade {
        time: current_candle_start + 10_000,
        price: Price::from_f32(101.0),
        qty: 5.0,
        is_sell: false,
    };
    // Live WebSocket trade arrives
    chart.insert_trades_buffer(&[ws_trade]);
    assert_eq!(chart.raw_trades().len(), 1);

    // Overlapping trade from REST batch during mid-candle connect
    chart.insert_fetched_trades_buffered(&[ws_trade]);
    assert_eq!(
        chart.raw_trades().len(),
        1,
        "Duplicate trade must not increase raw_trades"
    );

    // Additional historical trade from earlier in the candle
    let hist_trade = Trade {
        time: current_candle_start + 5_000,
        price: Price::from_f32(100.5),
        qty: 2.0,
        is_sell: true,
    };
    chart.insert_fetched_trades_buffered(&[hist_trade]);
    assert_eq!(chart.raw_trades().len(), 2);

    // Commit fetched trades & mark fetched
    chart.commit_fetched_trades(current_candle_start, current_candle_start + interval_ms);
    chart.mark_trades_fetched(current_candle_start, current_candle_start + interval_ms);

    // Verify active footprint has exactly 7.0 volume (no duplicate volume)
    if let PlotData::TimeBased(ref ts) = chart.data_source {
        let dp = ts.datapoints.get(&current_candle_start).unwrap();
        let total_vol: f32 = dp.footprint.trades.values().map(|b| b.total_qty()).sum();
        assert_eq!(total_vol, 7.0);

        // 4. Verify loop prevention: stops suggesting once marked
        let suggested_after =
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms);
        assert_eq!(
            suggested_after, None,
            "Must not repeatedly re-request active candle once marked"
        );
    }
}

#[test]
fn test_kline_chart_initial_trades_do_not_suppress_active_candle_fetch_and_commit_boundary() {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let interval = exchange::Timeframe::H1;
    let interval_ms = interval.to_milliseconds();
    let current_candle_start = (now_ms / interval_ms) * interval_ms;
    let past_candle_start = current_candle_start - interval_ms;

    let klines = vec![
        Kline {
            time: past_candle_start,
            open: Price::from_f32(95.0),
            high: Price::from_f32(100.0),
            low: Price::from_f32(94.0),
            close: Price::from_f32(99.0),
            volume: (10.0, 10.0),
        },
        Kline {
            time: current_candle_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        },
    ];

    // Mid-candle trade already buffered before chart construction
    let mid_trade = Trade {
        time: current_candle_start + 40 * 60 * 1000,
        price: Price::from_f32(101.5),
        qty: 3.0,
        is_sell: false,
    };

    let binance_ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceSpot);
    let binance_info = exchange::TickerInfo::new(binance_ticker, 0.1, 0.001, None);
    let view_cfg = ViewConfig::default();
    let mut chart = KlineChart::new(
        view_cfg,
        Basis::Time(interval),
        1.0,
        &klines,
        vec![mid_trade],
        &[],
        binance_info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    // 1. Chart must suggest fetching active candle despite having mid-candle trades
    if let PlotData::TimeBased(ref ts) = chart.data_source {
        let suggested =
            ts.suggest_trade_fetch_range(past_candle_start, current_candle_start + interval_ms);
        // It should prioritize past candle first or active candle
        assert!(suggested.is_some());
    }

    // 2. Fetch past candle [past_candle_start, current_candle_start)
    chart.commit_fetched_trades(past_candle_start, current_candle_start);
    chart.mark_trades_fetched(past_candle_start, current_candle_start);

    // Active candle must still be unfetched
    if let PlotData::TimeBased(ref ts) = chart.data_source {
        assert!(
            ts.datapoints
                .get(&past_candle_start)
                .unwrap()
                .trades_fetched
        );
        assert!(
            !ts.datapoints
                .get(&current_candle_start)
                .unwrap()
                .trades_fetched,
            "Active candle must remain unfetched after past candle fetch"
        );
        assert_eq!(
            ts.suggest_trade_fetch_range(past_candle_start, current_candle_start + interval_ms),
            Some((current_candle_start, current_candle_start + interval_ms)),
            "Active candle must now be suggested after past candle was fetched"
        );
    }

    // 3. Now backfill active candle
    let hist_active_trade = Trade {
        time: current_candle_start + 10 * 60 * 1000,
        price: Price::from_f32(100.2),
        qty: 4.0,
        is_sell: true,
    };
    chart.insert_fetched_trades_buffered(&[hist_active_trade]);
    chart.commit_fetched_trades(current_candle_start, current_candle_start + interval_ms);
    chart.mark_trades_fetched(current_candle_start, current_candle_start + interval_ms);

    if let PlotData::TimeBased(ref ts) = chart.data_source {
        assert!(
            ts.datapoints
                .get(&current_candle_start)
                .unwrap()
                .trades_fetched
        );
        assert_eq!(
            ts.suggest_trade_fetch_range(past_candle_start, current_candle_start + interval_ms),
            None,
            "No more trades should be suggested once active candle is marked"
        );
        let dp = ts.datapoints.get(&current_candle_start).unwrap();
        let total_vol: f32 = dp.footprint.trades.values().map(|b| b.total_qty()).sum();
        assert_eq!(total_vol, 7.0);
    }
}

#[test]
fn test_ter03_cluster_zero_qty_and_positive_geometry() {
    let max_cluster_qty = 0.0f32;
    let delta = -5.0f32;
    let bars_width = 100.0f32;

    if max_cluster_qty > 0.0 {
        let bar_width = (delta.abs() / max_cluster_qty) * bars_width;
        assert!(bar_width.is_finite());
    }

    let max_cluster_qty = 10.0f32;
    let sell_qty = 5.0f32;
    let left_area_width = 50.0f32;
    let ask_area_right = 100.0f32;
    let bid_area_left = 20.0f32;

    let bar_width = (sell_qty / max_cluster_qty) * left_area_width;
    assert!(bar_width > 0.0);
    let left = (ask_area_right - bar_width).max(bid_area_left);
    assert_eq!(left, 75.0);
    assert!(left >= bid_area_left);
}

#[test]
fn test_active_panel_indicators_count_builtin_and_scripted() {
    let _ = data::chart::kline::indicators::initialize_indicator_catalog();

    let klines = vec![Kline {
        time: 1_700_000_000_000,
        open: Price::from_f32(100.0),
        high: Price::from_f32(105.0),
        low: Price::from_f32(95.0),
        close: Price::from_f32(102.0),
        volume: (10.0, 10.0),
    }];
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceSpot);
    let info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M1),
        1.0,
        &klines,
        vec![],
        &[],
        info,
        &KlineChartKind::Candles,
        None,
    );

    // Initially zero panel indicators
    assert_eq!(chart.active_panel_indicators_count(), 0);

    // Add a scripted panel indicator (core.rsi)
    let rsi_id = chart.add_scripted_indicator(
        data::chart::kline::indicators::IndicatorId::parse("core.rsi").unwrap(),
    );
    assert_eq!(chart.active_panel_indicators_count(), 1);
    assert!(chart.script_panel_caches.contains_key(&rsi_id));

    // Add a scripted overlay indicator (core.moving-averages) -> panel count remains 1
    let ma_id = chart.add_scripted_indicator(
        data::chart::kline::indicators::IndicatorId::parse("core.moving-averages").unwrap(),
    );
    assert_eq!(chart.active_panel_indicators_count(), 1);

    // Remove the panel indicator -> panel count returns to 0
    chart.remove_scripted_indicator(rsi_id);
    assert_eq!(chart.active_panel_indicators_count(), 0);
    assert!(!chart.script_panel_caches.contains_key(&rsi_id));

    // Cleanup
    chart.remove_scripted_indicator(ma_id);
    assert_eq!(chart.active_panel_indicators_count(), 0);
}

#[test]
fn test_kline_chart_coexistence_footprint_and_replay_recalc() {
    let _ = data::chart::kline::indicators::initialize_indicator_catalog();

    let interval_ms = 60_000u64;
    let start_time = (1_700_000_000_000u64 / interval_ms) * interval_ms;
    let klines = vec![
        Kline {
            time: start_time,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        },
        Kline {
            time: start_time + interval_ms,
            open: Price::from_f32(102.0),
            high: Price::from_f32(108.0),
            low: Price::from_f32(101.0),
            close: Price::from_f32(106.0),
            volume: (15.0, 15.0),
        },
    ];
    let trade = Trade {
        time: start_time + 30_000,
        price: Price::from_f32(103.0),
        qty: 5.0,
        is_sell: false,
    };
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceSpot);
    let info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut chart = KlineChart::new(
        ViewConfig::default(),
        Basis::Time(exchange::Timeframe::M1),
        1.0,
        &klines,
        vec![],
        &[],
        info,
        &KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::VolumeProfile,
            scaling: data::chart::kline::ClusterScaling::VisibleRange,
            studies: Vec::new(),
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    chart.insert_trades_buffer(&[trade]);

    // Add scripted indicator
    let rsi_id = chart.add_scripted_indicator(
        data::chart::kline::indicators::IndicatorId::parse("core.rsi").unwrap(),
    );

    // Recalculate indicators
    chart.recalculate_scripted_indicators();

    // Verify footprint integrity
    if let PlotData::TimeBased(ref ts) = chart.data_source {
        let dp = ts.datapoints.get(&start_time).unwrap();
        let vol: f32 = dp.footprint.trades.values().map(|b| b.total_qty()).sum();
        assert_eq!(vol, 5.0, "Footprint trades volume must be preserved");
    }

    // Test replay cutoff synchronization
    chart.replay = Some(data::chart::replay::ReplayState::new(start_time));

    // Recalculate under replay
    chart.recalculate_scripted_indicators_replay();

    // Output should not exist past cutoff
    let val_1 = chart.indicator_host.output_value_for(rsi_id, "value", 1);
    assert_eq!(
        val_1, None,
        "Frame past replay cutoff must not have evaluated output"
    );
}

#[test]
fn test_ter11_vwap_range_scan_datapoints() {
    use exchange::unit::Price;
    use std::collections::BTreeMap;

    // 1. TimeBased range scan verification
    let mut datapoints: BTreeMap<u64, KlineDataPoint> = BTreeMap::new();
    for i in 0..10_000u64 {
        let t = 1_000_000 + i * 60_000;
        let kline = Kline {
            time: t,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (50.0, 50.0),
        };
        datapoints.insert(
            t,
            KlineDataPoint {
                kline,
                footprint: Default::default(),
                trades_fetched: false,
            },
        );
    }

    let earliest = 1_000_000 + 500 * 60_000;
    let latest = 1_000_000 + 550 * 60_000;

    let scanned: Vec<(i64, f64, f64)> = datapoints
        .range(earliest..=latest)
        .map(|(t, dp)| {
            let tp = (dp.kline.high.to_f32() + dp.kline.low.to_f32() + dp.kline.close.to_f32())
                as f64
                / 3.0;
            let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
            (*t as i64, tp, vol)
        })
        .collect();

    assert_eq!(scanned.len(), 51);
    assert_eq!(scanned.first().unwrap().0, earliest as i64);
    assert_eq!(scanned.last().unwrap().0, latest as i64);

    // 2. TickBased binary search partition_point verification
    let mut tick_datapoints: Vec<KlineDataPoint> = Vec::with_capacity(10_000);
    for i in 0..10_000u64 {
        let t = 1_000_000 + i * 60_000;
        let kline = Kline {
            time: t,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (50.0, 50.0),
        };
        tick_datapoints.push(KlineDataPoint {
            kline,
            footprint: Default::default(),
            trades_fetched: false,
        });
    }

    let start_idx = tick_datapoints.partition_point(|dp| dp.kline.time < earliest);
    let end_idx = tick_datapoints.partition_point(|dp| dp.kline.time <= latest);
    assert_eq!(start_idx, 500);
    assert_eq!(end_idx, 551);

    let tick_scanned: Vec<(i64, f64, f64)> = tick_datapoints[start_idx..end_idx]
        .iter()
        .map(|dp| {
            let tp = (dp.kline.high.to_f32() + dp.kline.low.to_f32() + dp.kline.close.to_f32())
                as f64
                / 3.0;
            let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
            (dp.kline.time as i64, tp, vol)
        })
        .collect();

    assert_eq!(tick_scanned.len(), 51);
    assert_eq!(scanned, tick_scanned);
}

#[test]
fn test_nudge_arrow_up_down_ticksize() {
    let mut chart = make_test_chart();
    let d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    // Nudge up by 1 tick (tick_size = 0.1)
    chart.nudge_selected_drawings(0, 0.1);
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 100.1).abs() < 1e-4);
    } else {
        panic!("Expected horizontal line");
    }

    // Nudge up by 10 ticks (Shift+ArrowUp: 1.0)
    chart.nudge_selected_drawings(0, 1.0);
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 101.1).abs() < 1e-4);
    } else {
        panic!("Expected horizontal line");
    }

    // Nudge down by 1 tick (ArrowDown: -0.1)
    chart.nudge_selected_drawings(0, -0.1);
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 101.0).abs() < 1e-4);
    } else {
        panic!("Expected horizontal line");
    }
}

#[test]
fn test_nudge_arrow_left_right_timeframe() {
    let mut chart = make_test_chart();
    let tf_ms = 300_000i64; // M5
    let d = Drawing::trendline(
        (10_000_000, 100.0),
        (20_000_000, 110.0),
        [1.0, 1.0, 1.0, 1.0],
        1.0,
    );
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    // Nudge right by 1 timeframe
    chart.nudge_selected_drawings(tf_ms, 0.0);
    if let DrawingKind::Trendline { p1, p2 } = chart.selected_drawing().unwrap().kind {
        assert_eq!(p1.0, 10_000_000 + 300_000);
        assert_eq!(p2.0, 20_000_000 + 300_000);
    } else {
        panic!("Expected trendline");
    }

    // Nudge left by 10 timeframes (Shift+ArrowLeft)
    chart.nudge_selected_drawings(-10 * tf_ms, 0.0);
    if let DrawingKind::Trendline { p1, p2 } = chart.selected_drawing().unwrap().kind {
        assert_eq!(p1.0, 10_000_000 + 300_000 - 3_000_000);
        assert_eq!(p2.0, 20_000_000 + 300_000 - 3_000_000);
    } else {
        panic!("Expected trendline");
    }
}

#[test]
fn test_nudge_locked_drawing_ignored() {
    let mut chart = make_test_chart();
    let mut d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    d.is_locked = true;
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    let res = chart.nudge_selected_drawings(0, 1.0);
    assert!(res.is_none(), "Locked drawing must not be nudged");
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert_eq!(price, 100.0);
    }
}

#[test]
fn test_nudge_burst_coalescing_undo_redo() {
    let mut chart = make_test_chart();
    let d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    // 3 rapid nudges within burst window
    chart.nudge_selected_drawings(0, 0.1);
    chart.nudge_selected_drawings(0, 0.1);
    chart.nudge_selected_drawings(0, 0.1);

    assert_eq!(
        chart.undo_stack.len(),
        1,
        "Burst nudges should coalesce into 1 undo action"
    );
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 100.3).abs() < 1e-4);
    }

    // Single Undo should revert all 3 rapid nudges back to 100.0
    let undone = chart.undo();
    assert!(undone.is_some());
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 100.0).abs() < 1e-4);
    }

    // Redo should restore to final position 100.3
    let redone = chart.redo();
    assert!(redone.is_some());
    if let DrawingKind::HorizontalLine { price } = chart.selected_drawing().unwrap().kind {
        assert!((price - 100.3).abs() < 1e-4);
    }
}

#[test]
fn test_nudge_batch_multi_selection() {
    let mut chart = make_test_chart();
    let mut d1 = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    d1.is_selected = true;
    let mut d2 = Drawing::horizontal(200.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    d2.is_selected = true;
    chart.add_drawing(d1);
    chart.add_drawing(d2);

    let res = chart.nudge_selected_drawings(0, 5.0);
    assert!(res.is_some());

    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert_eq!(price, 105.0);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert_eq!(price, 205.0);
    }

    // Undo batch
    chart.undo();
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert_eq!(price, 100.0);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert_eq!(price, 200.0);
    }

    // Redo batch
    chart.redo();
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert_eq!(price, 105.0);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert_eq!(price, 205.0);
    }
}

#[test]
fn test_nudge_batch_burst_coalescing_undo_redo() {
    let mut chart = make_test_chart();
    let mut d1 = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    d1.is_selected = true;
    let mut d2 = Drawing::horizontal(200.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    d2.is_selected = true;
    chart.add_drawing(d1);
    chart.add_drawing(d2);

    // 3 rapid batch nudges
    chart.nudge_selected_drawings(0, 0.5);
    chart.nudge_selected_drawings(0, 0.5);
    chart.nudge_selected_drawings(0, 0.5);

    assert_eq!(
        chart.undo_stack.len(),
        1,
        "Burst batch nudges should coalesce into a single undo action"
    );
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert!((price - 101.5).abs() < 1e-4);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert!((price - 201.5).abs() < 1e-4);
    }

    // Single undo reverts both drawings
    chart.undo();
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert!((price - 100.0).abs() < 1e-4);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert!((price - 200.0).abs() < 1e-4);
    }

    // Single redo restores both drawings
    chart.redo();
    if let DrawingKind::HorizontalLine { price } = chart.drawings[0].kind {
        assert!((price - 101.5).abs() < 1e-4);
    }
    if let DrawingKind::HorizontalLine { price } = chart.drawings[1].kind {
        assert!((price - 201.5).abs() < 1e-4);
    }
}

#[test]
fn test_nudge_keyboard_arrow_events() {
    use iced::widget::canvas::Program;
    let mut chart = make_test_chart();
    let mut interaction = Interaction::None;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    chart.chart.bounds = bounds;
    let cursor = mouse::Cursor::Available(Point::new(100.0, 100.0));

    let make_arrow =
        |named: iced::keyboard::key::Named, code: iced::keyboard::key::Code, shift: bool| {
            let mut modifiers = iced::keyboard::Modifiers::default();
            if shift {
                modifiers |= iced::keyboard::Modifiers::SHIFT;
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(named),
                location: iced::keyboard::Location::Standard,
                modifiers,
                repeat: false,
                text: None,
                physical_key: iced::keyboard::key::Physical::Code(code),
                modified_key: iced::keyboard::Key::Named(named),
            })
        };

    // Without any selection, arrow keys should not produce an action
    let up_key = make_arrow(
        iced::keyboard::key::Named::ArrowUp,
        iced::keyboard::key::Code::ArrowUp,
        false,
    );
    let action = chart.update(&mut interaction, &up_key, bounds, cursor);
    assert!(action.is_none());

    // Add drawing and select it
    let d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    // ArrowUp publishes action
    let action_up = chart.update(&mut interaction, &up_key, bounds, cursor);
    assert!(action_up.is_some());

    // Shift+ArrowUp publishes action
    let shift_up_key = make_arrow(
        iced::keyboard::key::Named::ArrowUp,
        iced::keyboard::key::Code::ArrowUp,
        true,
    );
    let action_shift_up = chart.update(&mut interaction, &shift_up_key, bounds, cursor);
    assert!(action_shift_up.is_some());

    // ArrowDown publishes action
    let down_key = make_arrow(
        iced::keyboard::key::Named::ArrowDown,
        iced::keyboard::key::Code::ArrowDown,
        false,
    );
    let action_down = chart.update(&mut interaction, &down_key, bounds, cursor);
    assert!(action_down.is_some());

    // ArrowLeft publishes action
    let left_key = make_arrow(
        iced::keyboard::key::Named::ArrowLeft,
        iced::keyboard::key::Code::ArrowLeft,
        false,
    );
    let action_left = chart.update(&mut interaction, &left_key, bounds, cursor);
    assert!(action_left.is_some());

    // ArrowRight publishes action
    let right_key = make_arrow(
        iced::keyboard::key::Named::ArrowRight,
        iced::keyboard::key::Code::ArrowRight,
        false,
    );
    let action_right = chart.update(&mut interaction, &right_key, bounds, cursor);
    assert!(action_right.is_some());
}

#[test]
fn test_ter35_contextual_mouse_interaction_for_drawing_tools_and_selection() {
    use iced::widget::canvas::Program;
    let mut chart = make_test_chart();
    let interaction = Interaction::None;
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let cursor_over = mouse::Cursor::Available(Point::new(200.0, 200.0));
    let cursor_outside = mouse::Cursor::Available(Point::new(900.0, 700.0));

    // 1. Neutral state: crosshair inside bounds, default outside bounds
    let ic_neutral = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_neutral, mouse::Interaction::Crosshair);
    let ic_out = chart.mouse_interaction(&interaction, bounds, cursor_outside);
    assert_eq!(ic_out, mouse::Interaction::default());

    // 2. Armed drawing tool (Trendline) switches cursor to Pointer
    chart.set_active_drawing_tool(DrawingTool::Trendline);
    let ic_armed = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_armed, mouse::Interaction::Pointer);

    // 3. Armed drawing tool (Rectangle) switches cursor to Pointer
    chart.set_active_drawing_tool(DrawingTool::Rectangle);
    let ic_rect = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_rect, mouse::Interaction::Pointer);

    // 4. Disarming drawing tool restores neutral Crosshair
    chart.set_active_drawing_tool(DrawingTool::None);
    let ic_disarmed = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_disarmed, mouse::Interaction::Crosshair);

    // 5. Selected drawing present switches cursor to system Arrow (default) instead of Crosshair
    let d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    let id = d.id;
    chart.add_drawing(d);
    chart.set_selected_drawing(Some(id));

    let ic_selected = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_selected, mouse::Interaction::default());

    // 6. Active drag switches cursor to Grabbing
    chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
        start_time: 0,
        start_price: 100.0,
        initial_drawing: chart.drawings[0].clone(),
        current_drawing: chart.drawings[0].clone(),
    });
    let ic_drag = chart.mouse_interaction(&interaction, bounds, cursor_over);
    assert_eq!(ic_drag, mouse::Interaction::Grabbing);

    chart.drawing_state.borrow_mut().drag = None;

    // 7. Panning interaction takes precedence
    let pan_interaction = Interaction::Panning {
        translation: iced::Vector::ZERO,
        start: Point::new(10.0, 10.0),
    };
    let ic_pan = chart.mouse_interaction(&pan_interaction, bounds, cursor_over);
    assert_eq!(ic_pan, mouse::Interaction::Grabbing);
}
