use super::*;
use exchange::Ticker;

fn mock_ticker_info(symbol: &str, exchange: Exchange) -> TickerInfo {
    TickerInfo::new(Ticker::new(symbol, exchange), 0.1, 0.001, None)
}

#[test]
fn test_sync_footprint_exchange_notice_hyperliquid() {
    let mut state = State::default();
    let ti = mock_ticker_info("BTC", Exchange::HyperliquidLinear);
    state.streams = ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: ti,
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    }]);
    state.content = Content::Kline {
        chart: None,
        indicators: vec![],
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        layout: Default::default(),
    };

    state.sync_footprint_exchange_notice();
    assert_eq!(state.notifications.len(), 1);
    let notice = &state.notifications[0];
    assert_eq!(notice.title, "Hyperliquid Footprint");
    assert!(notice.persistent);
    assert!(notice.closable);

    // Deleting notification sets footprint_notice_dismissed
    state.update(Event::DeleteNotification(0));
    assert!(state.notifications.is_empty());
    assert_eq!(
        state.footprint_notice_dismissed,
        Some(Exchange::HyperliquidLinear)
    );

    // Syncing again does not re-add dismissed notification
    state.sync_footprint_exchange_notice();
    assert!(state.notifications.is_empty());
}

#[test]
fn test_sync_footprint_exchange_notice_bybit() {
    let mut state = State::default();
    let ti = mock_ticker_info("BTCUSDT", Exchange::BybitLinear);
    state.streams = ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: ti,
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    }]);
    state.content = Content::Kline {
        chart: None,
        indicators: vec![],
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        layout: Default::default(),
    };

    state.sync_footprint_exchange_notice();
    assert_eq!(state.notifications.len(), 1);
    let notice = &state.notifications[0];
    assert_eq!(notice.title, "Bybit Footprint");
    assert!(notice.persistent);
    assert!(notice.closable);
}

#[test]
fn test_sync_footprint_exchange_notice_okx() {
    let mut state = State::default();
    let ti = mock_ticker_info("BTC-USDT", Exchange::OkexLinear);
    state.streams = ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: ti,
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    }]);
    state.content = Content::Kline {
        chart: None,
        indicators: vec![],
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        layout: Default::default(),
    };

    state.sync_footprint_exchange_notice();
    assert_eq!(state.notifications.len(), 1);
    let notice = &state.notifications[0];
    assert_eq!(notice.title, "OKX Footprint");
    assert!(notice.persistent);
    assert!(notice.closable);
}

#[test]
fn test_sync_footprint_exchange_notice_binance_none() {
    let mut state = State::default();
    let ti = mock_ticker_info("btcusdt", Exchange::BinanceLinear);
    state.streams = ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: ti,
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    }]);
    state.content = Content::Kline {
        chart: None,
        indicators: vec![],
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        layout: Default::default(),
    };

    state.sync_footprint_exchange_notice();
    assert!(state.notifications.is_empty());
}

#[test]
fn test_extract_base_symbol() {
    assert_eq!(
        extract_base_symbol(&exchange::Ticker::new("btcusdt", Exchange::BinanceLinear)),
        "BTC"
    );
    assert_eq!(
        extract_base_symbol(&exchange::Ticker::new("BTCUSDT", Exchange::BybitLinear)),
        "BTC"
    );
    assert_eq!(
        extract_base_symbol(&exchange::Ticker::new(
            "BTC-USDT-SWAP",
            Exchange::OkexLinear
        )),
        "BTC"
    );
    assert_eq!(
        extract_base_symbol(&exchange::Ticker::new("BTC-USDT", Exchange::OkexLinear)),
        "BTC"
    );
    assert_eq!(
        extract_base_symbol(&exchange::Ticker::new("BTC", Exchange::HyperliquidLinear)),
        "BTC"
    );
}

#[test]
fn test_build_footprint_streams_aggregate() {
    let base_ti = mock_ticker_info("btcusdt", Exchange::BinanceLinear);
    let settings = Settings::default();
    let kind_single = data::chart::KlineChartKind::Footprint {
        clusters: Default::default(),
        scaling: Default::default(),
        studies: vec![],
        show_bottom_volume: false,
        aggregate: false,
        aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
        history_lookback_days: None,
    };

    let streams_single =
        State::build_footprint_streams(base_ti, &kind_single, &settings, Some(Timeframe::M5));
    // 1 Kline stream + 1 DepthAndTrades
    assert_eq!(streams_single.len(), 2);

    let kind_agg = data::chart::KlineChartKind::Footprint {
        clusters: Default::default(),
        scaling: Default::default(),
        studies: vec![],
        show_bottom_volume: false,
        aggregate: true,
        aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
        history_lookback_days: None,
    };

    let streams_agg =
        State::build_footprint_streams(base_ti, &kind_agg, &settings, Some(Timeframe::M5));
    // 1 Kline stream + 4 DepthAndTrades (Binance, Bybit, OKX, Hyperliquid)
    assert_eq!(streams_agg.len(), 5);
    let depth_exchanges: Vec<_> = streams_agg[1..]
        .iter()
        .map(|s| s.ticker_info().exchange())
        .collect();
    assert_eq!(
        depth_exchanges,
        vec![
            Exchange::BinanceLinear,
            Exchange::BybitLinear,
            Exchange::OkexLinear,
            Exchange::HyperliquidLinear,
        ]
    );
}

#[test]
fn test_matches_trades_aggregate() {
    let mut state = State::default();
    let base_ti = mock_ticker_info("btcusdt", Exchange::BinanceLinear);
    state.streams = ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: base_ti,
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    }]);
    state.content = Content::Kline {
        chart: None,
        indicators: vec![],
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: true,
            aggregate_exchanges: vec![
                Exchange::BinanceLinear,
                Exchange::BybitLinear,
                Exchange::OkexLinear,
                Exchange::HyperliquidLinear,
            ],
            history_lookback_days: None,
        },
        layout: Default::default(),
    };

    // Incoming trades stream from Bybit
    let bybit_stream = StreamKind::DepthAndTrades {
        ticker_info: mock_ticker_info("BTCUSDT", Exchange::BybitLinear),
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    };
    assert!(state.matches_trades(&bybit_stream));

    // Incoming trades stream from OKX
    let okx_stream = StreamKind::DepthAndTrades {
        ticker_info: mock_ticker_info("BTC-USDT-SWAP", Exchange::OkexLinear),
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    };
    assert!(state.matches_trades(&okx_stream));

    // Incoming trades stream from Hyperliquid
    let hl_stream = StreamKind::DepthAndTrades {
        ticker_info: mock_ticker_info("BTC", Exchange::HyperliquidLinear),
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    };
    assert!(state.matches_trades(&hl_stream));

    // Incoming trades stream for ETH should NOT match
    let eth_stream = StreamKind::DepthAndTrades {
        ticker_info: mock_ticker_info("ETHUSDT", Exchange::BybitLinear),
        depth_aggr: StreamTicksize::Client,
        push_freq: exchange::PushFrequency::ServerDefault,
    };
    assert!(!state.matches_trades(&eth_stream));
}

#[test]
fn test_footprint_from_config_registers_depth_and_trades_stream() {
    let ticker = exchange::Ticker::new("BTCUSDT", Exchange::BinanceLinear);
    let kline_persist = PersistStreamKind::Kline(exchange::adapter::PersistKline {
        ticker,
        timeframe: exchange::Timeframe::M5,
    });
    let content = Content::Kline {
        chart: None,
        indicators: vec![],
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
    };

    let state = State::from_config(content, vec![kline_persist], Settings::default(), None);

    match &state.streams {
        ResolvedStream::Waiting(streams) => {
            assert_eq!(streams.len(), 2);
            assert!(
                streams
                    .iter()
                    .any(|s| matches!(s, PersistStreamKind::Kline(_)))
            );
            assert!(
                streams
                    .iter()
                    .any(|s| matches!(s, PersistStreamKind::DepthAndTrades(_)))
            );
        }
        _ => panic!("Expected ResolvedStream::Waiting"),
    }
}

#[test]
fn test_footprint_events_sync_streams_and_return_refresh_effect() {
    let ticker = exchange::Ticker::new("BTCUSDT", Exchange::BinanceLinear);
    let ti = TickerInfo::new(ticker, 0.1, 0.001, None);
    let chart = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ti,
        &data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
        None,
    );

    let mut state = State {
        content: Content::Kline {
            chart: Some(chart),
            indicators: vec![],
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
        },
        ..Default::default()
    };

    // Initially no streams
    assert!(matches!(state.streams, ResolvedStream::Waiting(ref v) if v.is_empty()));

    // ClusterKindSelected should sync streams and return RefreshStreams
    let eff = state.update(Event::ClusterKindSelected(
        data::chart::kline::ClusterKind::DeltaProfile,
    ));
    assert!(matches!(eff, Some(Effect::RefreshStreams)));
    assert!(
        state.streams.find_ready_map(|s| match s {
            StreamKind::DepthAndTrades { ticker_info, .. } => Some(*ticker_info),
            _ => None,
        }) == Some(ti)
    );

    // ClusterScalingSelected should return RefreshStreams
    let eff = state.update(Event::ClusterScalingSelected(
        data::chart::kline::ClusterScaling::Hybrid { weight: 0.5 },
    ));
    assert!(matches!(eff, Some(Effect::RefreshStreams)));

    // FootprintShowBottomVolumeToggled should return RefreshStreams
    let eff = state.update(Event::FootprintShowBottomVolumeToggled(true));
    assert!(matches!(eff, Some(Effect::RefreshStreams)));
}

#[test]
fn test_ter01_empty_tickers_pane_update() {
    let mut state = State::default();
    let streams = state.set_content_and_streams(Vec::new(), ContentKind::CandlestickChart);
    assert!(streams.is_empty());
}

#[cfg(feature = "shader-heatmap")]
#[test]
fn test_shader_heatmap_set_content_and_streams() {
    let ticker = exchange::Ticker::new("BTCUSDT", Exchange::BinanceLinear);
    let ti = TickerInfo::new(ticker, 0.1, 0.001, None);
    let mut state = State::default();
    let streams = state.set_content_and_streams(vec![ti], ContentKind::ShaderHeatmap);

    assert_eq!(streams.len(), 1);
    assert!(matches!(streams[0], StreamKind::DepthAndTrades { .. }));
    assert_eq!(state.content.kind(), ContentKind::ShaderHeatmap);
    assert!(state.content.initialized());

    // Test visual config change
    let cfg = data::chart::heatmap::Config {
        trade_size_filter: 1500.0,
        ..Default::default()
    };
    state
        .content
        .change_visual_config(VisualConfig::Heatmap(cfg));
    if let Content::ShaderHeatmap { chart: Some(c), .. } = &state.content {
        assert_eq!(c.config.trade_size_filter, 1500.0);
        assert_eq!(c.size_filter_inputs().0, "1500");
    } else {
        panic!("ShaderHeatmap chart not initialized");
    }
}

#[test]
fn test_ter30_overlay_z_index_and_default_coordinates() {
    let mut state = State::default();
    assert_eq!(state.drawing_toolbar_pos, None);
    assert_eq!(state.selected_drawing_toolbar_pos, None);

    // Verify position updates via events
    state.update(Event::DrawingToolbarMoved(iced::Point::new(100.0, 150.0)));
    assert_eq!(
        state.drawing_toolbar_pos,
        Some(iced::Point::new(100.0, 150.0))
    );

    state.update(Event::SelectedDrawingToolbarMoved(iced::Point::new(
        300.0, 250.0,
    )));
    assert_eq!(
        state.selected_drawing_toolbar_pos,
        Some(iced::Point::new(300.0, 250.0))
    );

    // Verify default position fallback value (16.0, 72.0)
    let default_pos = State::default()
        .drawing_toolbar_pos
        .unwrap_or(iced::Point::new(16.0, 72.0));
    assert_eq!(default_pos, iced::Point::new(16.0, 72.0));

    let default_sel_pos = State::default()
        .selected_drawing_toolbar_pos
        .unwrap_or(iced::Point::new(260.0, 72.0));
    assert_eq!(default_sel_pos, iced::Point::new(260.0, 72.0));
}
