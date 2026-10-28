use super::*;

fn make_test_chart() -> crate::chart::kline::KlineChart {
    let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
    crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &data::chart::KlineChartKind::Candles,
        None,
    )
}

#[test]
fn test_single_active_toolbar_pane() {
    let mut dashboard = Dashboard::default();
    let main_window_id = window::Id::unique();

    // 1. Initially default panes are Starter; active_kline_pane returns None
    assert_eq!(dashboard.active_kline_pane(main_window_id), None);

    // 2. Set one pane to uninitialized Kline content
    let (first_pane, _) = dashboard.panes.iter().next().unwrap();
    let first_pane = *first_pane;
    if let Some(state) = dashboard.panes.get_mut(first_pane) {
        state.content = pane::Content::Kline {
            chart: None,
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };
    }

    // Active kline pane finds the uninitialized Kline pane as fallback
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, first_pane))
    );

    // 3. Set another pane to initialized Kline chart
    let second_pane = dashboard
        .panes
        .iter()
        .find(|(p, _)| **p != first_pane)
        .map(|(p, _)| *p)
        .unwrap();
    if let Some(state) = dashboard.panes.get_mut(second_pane) {
        state.content = pane::Content::Kline {
            chart: Some(make_test_chart()),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };
    }

    // Active kline pane should prefer the initialized Kline chart when neither is focused
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, second_pane))
    );

    // 3b. Focusing the uninitialized pane takes priority over initialized pane
    dashboard.set_focus(main_window_id, main_window_id, first_pane);
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, first_pane))
    );

    // 4. Initialize first pane as well; when first pane is focused, focus takes priority
    if let Some(state) = dashboard.panes.get_mut(first_pane) {
        state.content = pane::Content::Kline {
            chart: Some(make_test_chart()),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };
    }
    dashboard.set_focus(main_window_id, main_window_id, first_pane);
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, first_pane))
    );

    // 5. If focus switches to a non-Kline pane, last_focused_kline preserves the toolbar on first_pane
    let third_pane = dashboard
        .panes
        .iter()
        .find(|(p, _)| **p != first_pane && **p != second_pane)
        .map(|(p, _)| *p)
        .unwrap();
    dashboard.focus = Some((main_window_id, third_pane));
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, first_pane))
    );

    // 6. When second_pane receives focus, the single toolbar transfers to second_pane
    dashboard.set_focus(main_window_id, main_window_id, second_pane);
    assert_eq!(
        dashboard.active_kline_pane(main_window_id),
        Some((main_window_id, second_pane))
    );
}

#[test]
fn test_drawing_isolation_and_cross_pane_sync() {
    let mut dashboard = Dashboard::default();
    let main_window_id = window::Id::unique();

    // 1. Setup two panes with BTCUSDT kline charts
    let panes: Vec<pane_grid::Pane> = dashboard.panes.iter().map(|(p, _)| *p).collect();
    let p1 = panes[0];
    let p2 = panes[1];

    let p1_id = dashboard.panes.get(p1).unwrap().unique_id();
    let p2_id = dashboard.panes.get(p2).unwrap().unique_id();
    assert_ne!(p1_id, p2_id);

    dashboard.panes.get_mut(p1).unwrap().content = pane::Content::Kline {
        chart: Some(make_test_chart()),
        indicators: Default::default(),
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Candles,
    };
    dashboard.panes.get_mut(p2).unwrap().content = pane::Content::Kline {
        chart: Some(make_test_chart()),
        indicators: Default::default(),
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Candles,
    };

    // 2. Add an unsynced drawing to pane 1
    let mut drawing = data::chart::drawing::Drawing::horizontal(65000.0, [1.0, 1.0, 1.0, 1.0], 1.0);
    drawing.pane_id = Some(p1_id);
    drawing.is_synced = false;
    let drawing_id = drawing.id;

    if let pane::Content::Kline { chart: Some(c), .. } =
        &mut dashboard.panes.get_mut(p1).unwrap().content
    {
        c.add_drawing(drawing.clone());
    }

    // Verify pane 1 has 1 drawing and pane 2 has 0 drawings (isolated)
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.drawings.len(), 1);
    }
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p2).unwrap().content {
        assert_eq!(c.drawings.len(), 0);
    }

    // 3. Sync drawing to all charts of the same ticker
    drawing.is_synced = true;
    dashboard.sync_drawing_to_panes(main_window_id, "BTCUSDT", &drawing, p1_id);

    // Verify both panes now have the drawing
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.drawings.len(), 1);
        assert!(c.drawings[0].is_synced);
    }
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p2).unwrap().content {
        assert_eq!(c.drawings.len(), 1);
        assert_eq!(c.drawings[0].id, drawing_id);
        assert!(c.drawings[0].is_synced);
    }

    // 4. Unsync from other panes
    dashboard.unsync_drawing_from_other_panes(main_window_id, "BTCUSDT", drawing_id, p1_id);

    // Verify pane 1 retains drawing, pane 2 no longer has it
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.drawings.len(), 1);
    }
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p2).unwrap().content {
        assert_eq!(c.drawings.len(), 0);
    }

    // 5. Re-sync and then delete synced drawing
    dashboard.sync_drawing_to_panes(main_window_id, "BTCUSDT", &drawing, p1_id);
    dashboard.delete_synced_drawing_from_panes(main_window_id, "BTCUSDT", drawing_id);

    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.drawings.len(), 0);
    }
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p2).unwrap().content {
        assert_eq!(c.drawings.len(), 0);
    }
}

#[test]
fn test_alert_isolation_between_panes_same_symbol() {
    let mut dashboard = Dashboard::default();
    let main_window_id = window::Id::unique();

    let test_symbol = format!("TEST_{}", &uuid::Uuid::new_v4().to_string()[..8]);

    // Cleanup any potential pre-existing alerts for this test symbol or legacy TESTALERT
    for a in data::AlertStore::for_symbol(&test_symbol) {
        data::AlertStore::remove(a.id);
    }
    for a in data::AlertStore::for_symbol("TESTALERT") {
        data::AlertStore::remove(a.id);
    }

    // 1. Setup two panes with the same ticker: Pane 1 (TPO) and Pane 2 (Footprint)
    let panes: Vec<pane_grid::Pane> = dashboard.panes.iter().map(|(p, _)| *p).collect();
    let p1 = panes[0];
    let p2 = panes[1];

    let p1_id = dashboard.panes.get(p1).unwrap().unique_id();
    let p2_id = dashboard.panes.get(p2).unwrap().unique_id();
    assert_ne!(p1_id, p2_id);

    let ticker_info = exchange::TickerInfo::new(
        exchange::Ticker::new(&test_symbol, exchange::adapter::Exchange::BinanceLinear),
        0.1,
        0.001,
        None,
    );

    let stream = exchange::adapter::StreamKind::Kline {
        ticker_info,
        timeframe: exchange::Timeframe::M15,
    };

    let tpo_kind = data::chart::KlineChartKind::Tpo {
        show_candles: true,
        show_letters: true,
        show_ib: true,
        show_va: true,
        show_poc: true,
        show_single_prints: true,
        tick_step: Default::default(),
        period: Default::default(),
        clusters: vec![],
        split_sessions: vec![],
        color_scheme: Default::default(),
        ib_color: Default::default(),
        poc_color: Default::default(),
        single_prints_color: Default::default(),
    };

    let footprint_kind = data::chart::KlineChartKind::Footprint {
        clusters: Default::default(),
        scaling: Default::default(),
        studies: vec![],
        show_bottom_volume: false,
        aggregate: false,
        aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
        history_lookback_days: None,
    };

    let chart_p1 = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M15),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &tpo_kind,
        None,
    );

    let chart_p2 = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M15),
        1.0,
        &[],
        Vec::new(),
        &[],
        ticker_info,
        &footprint_kind,
        None,
    );

    dashboard.panes.get_mut(p1).unwrap().streams =
        exchange::adapter::ResolvedStream::Ready(vec![stream]);
    dashboard.panes.get_mut(p1).unwrap().content = pane::Content::Kline {
        chart: Some(chart_p1),
        indicators: Default::default(),
        layout: Default::default(),
        kind: tpo_kind,
    };

    dashboard.panes.get_mut(p2).unwrap().streams =
        exchange::adapter::ResolvedStream::Ready(vec![stream]);
    dashboard.panes.get_mut(p2).unwrap().content = pane::Content::Kline {
        chart: Some(chart_p2),
        indicators: Default::default(),
        layout: Default::default(),
        kind: footprint_kind,
    };

    // 2. Add an alert on Pane 1 (TPO)
    let alert_price = 65000.0;
    dashboard
        .panes
        .get_mut(p1)
        .unwrap()
        .update(pane::Event::AddPriceAlert(alert_price));

    // Verify Pane 1 has 1 alert, Pane 2 has 0 alerts
    let p1_alerts_len = match &dashboard.panes.get(p1).unwrap().content {
        pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
        _ => 0,
    };
    let p2_alerts_len = match &dashboard.panes.get(p2).unwrap().content {
        pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
        _ => 0,
    };
    assert_eq!(p1_alerts_len, 1);
    assert_eq!(p2_alerts_len, 0);

    // Verify AlertStore has it bound to p1_id
    let for_p1 = data::AlertStore::for_symbol_and_pane(&test_symbol, p1_id);
    let for_p2 = data::AlertStore::for_symbol_and_pane(&test_symbol, p2_id);
    assert_eq!(for_p1.len(), 1);
    assert_eq!(for_p1[0].pane_id, Some(p1_id));
    assert_eq!(for_p2.len(), 0);

    // 3. Simulate kline reload / app startup on Pane 2 (Footprint)
    dashboard.panes.get_mut(p2).unwrap().insert_hist_klines(
        None,
        exchange::Timeframe::M15,
        ticker_info,
        &[],
    );

    // Pane 2 must STILL have 0 alerts
    let p2_alerts_len_after = match &dashboard.panes.get(p2).unwrap().content {
        pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
        _ => 0,
    };
    assert_eq!(p2_alerts_len_after, 0);

    // 4. Initial price below alert (64000.0)
    let kline_init = exchange::Kline {
        time: 1700000000000,
        open: exchange::util::Price::from_f32(64000.0),
        high: exchange::util::Price::from_f32(64100.0),
        low: exchange::util::Price::from_f32(63900.0),
        close: exchange::util::Price::from_f32(64000.0),
        volume: (10.0, 10.0),
    };
    let _ = dashboard.update_latest_klines(&stream, &kline_init, main_window_id);

    // Next price crossing 65000.0
    let kline_trigger = exchange::Kline {
        time: 1700000060000,
        open: exchange::util::Price::from_f32(64000.0),
        high: exchange::util::Price::from_f32(65200.0),
        low: exchange::util::Price::from_f32(64000.0),
        close: exchange::util::Price::from_f32(65100.0),
        volume: (10.0, 10.0),
    };
    let _ = dashboard.update_latest_klines(&stream, &kline_trigger, main_window_id);

    // Pane 1 must receive the notification and have its alert marked Triggered
    let p1_notifications = &dashboard.panes.get(p1).unwrap().notifications;
    let p2_notifications = &dashboard.panes.get(p2).unwrap().notifications;
    assert_eq!(p1_notifications.len(), 1);
    assert!(p1_notifications[0].body.contains("Price Alert"));
    assert_eq!(
        p2_notifications.len(),
        0,
        "Pane 2 must not receive notification for Pane 1's alert"
    );

    // Clean up AlertStore for this test symbol
    for a in data::AlertStore::for_symbol(&test_symbol) {
        data::AlertStore::remove(a.id);
    }
    let after_remove = data::AlertStore::for_symbol(&test_symbol);
    assert!(after_remove.is_empty());
}

#[test]
fn test_dashboard_toggle_replay_play_pause() {
    let mut p1_state = pane::State::default();
    let chart = make_test_chart();
    assert!(!chart.is_replay_active());
    p1_state.content = pane::Content::Kline {
        chart: Some(chart),
        indicators: Default::default(),
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Candles,
    };

    let (panes, p1) = pane_grid::State::new(p1_state);
    let win_id = window::Id::unique();

    let mut dashboard = Dashboard {
        panes,
        focus: Some((win_id, p1)),
        last_focused_kline: Some((win_id, p1)),
        streams: UniqueStreams::default(),
        popout: HashMap::new(),
        layout_id: uuid::Uuid::new_v4(),
    };

    // When replay is not active, toggle returns false
    assert!(!dashboard.toggle_replay_play_pause_active());

    // Activate replay on pane
    if let pane::Content::Kline { chart: Some(c), .. } =
        &mut dashboard.panes.get_mut(p1).unwrap().content
    {
        c.toggle_replay();
        assert!(c.is_replay_active());
    }

    // Toggle play/pause via dashboard (Space hotkey logic)
    assert!(dashboard.toggle_replay_play_pause_active());
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert!(c.replay_state().unwrap().is_playing);
    }

    // Toggle again to pause
    assert!(dashboard.toggle_replay_play_pause_active());
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert!(!c.replay_state().unwrap().is_playing);
    }

    // When a modal is open on the pane (e.g. Settings), space does not trigger replay
    dashboard.panes.get_mut(p1).unwrap().modal = Some(crate::modal::pane::Modal::Settings);
    assert!(!dashboard.toggle_replay_play_pause_active());
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert!(!c.replay_state().unwrap().is_playing);
    }
}

#[test]
fn test_stale_trade_fetch_discarded_on_ticker_mismatch() {
    let mut p1_state = pane::State::default();
    let eth_ticker = exchange::Ticker::new("ETHUSDT", exchange::adapter::Exchange::BinanceLinear);
    let eth_info = exchange::TickerInfo::new(eth_ticker, 0.01, 0.001, None);
    let chart = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::H1),
        1.0,
        &[],
        Vec::new(),
        &[],
        eth_info,
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
    p1_state.content = pane::Content::Kline {
        chart: Some(chart),
        indicators: Default::default(),
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

    let (panes, p1) = pane_grid::State::new(p1_state);
    let win_id = window::Id::unique();

    let mut dashboard = Dashboard {
        panes,
        focus: Some((win_id, p1)),
        last_focused_kline: Some((win_id, p1)),
        streams: UniqueStreams::default(),
        popout: HashMap::new(),
        layout_id: uuid::Uuid::new_v4(),
    };

    let p1_uuid = dashboard.panes.get(p1).unwrap().unique_id();

    // Create SOL trade and try inserting it into ETH pane
    let sol_ticker = exchange::Ticker::new("SOLUSDT", exchange::adapter::Exchange::BinanceLinear);
    let sol_info = exchange::TickerInfo::new(sol_ticker, 0.01, 0.001, None);
    let sol_trade = exchange::Trade {
        time: 1700000000000,
        price: exchange::util::Price::from_f32(120.0),
        qty: 1.0,
        is_sell: false,
    };

    // 1. insert_fetched_trades with SOL must drop the trade and return Ok(())
    let res = dashboard.insert_fetched_trades(win_id, p1_uuid, sol_info, &[sol_trade]);
    assert!(res.is_ok());

    // Verify ETH chart buffer did NOT accept SOL trade
    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.raw_trades().len(), 0);
    }

    // 2. update Message::TradeFetchBatchDone with SOL must be ignored
    let main_win = Window::new(win_id);
    let layout_id = dashboard.layout_id;
    let (_task, event) = dashboard.update(
        Message::TradeFetchBatchDone(p1_uuid, sol_info, 1700000000000, 1700000001000),
        &main_win,
        &layout_id,
    );
    assert!(event.is_none());

    // 3. insert_fetched_trades with matching ETH trade is accepted
    let eth_trade = exchange::Trade {
        time: 1700000000000,
        price: exchange::util::Price::from_f32(2700.0),
        qty: 2.0,
        is_sell: false,
    };
    let res2 = dashboard.insert_fetched_trades(win_id, p1_uuid, eth_info, &[eth_trade]);
    assert!(res2.is_ok());

    if let pane::Content::Kline { chart: Some(c), .. } = &dashboard.panes.get(p1).unwrap().content {
        assert_eq!(c.raw_trades().len(), 1);
    }
}

#[test]
fn test_fetch_range_trades_falls_back_to_chart_ticker_info() {
    let mut p1_state = pane::State::default();
    let btc_ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
    let btc_info = exchange::TickerInfo::new(btc_ticker, 0.1, 0.001, None);
    let chart = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        btc_info,
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
    p1_state.content = pane::Content::Kline {
        chart: Some(chart),
        indicators: Default::default(),
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

    // Explicitly set streams to only contain Kline stream (no DepthAndTrades)
    p1_state.streams = exchange::adapter::ResolvedStream::Ready(vec![StreamKind::Kline {
        ticker_info: btc_info,
        timeframe: exchange::Timeframe::M5,
    }]);

    let layout_id = uuid::Uuid::new_v4();
    let req_id = uuid::Uuid::new_v4();
    let fetch = FetchRange::Trades(1700000000000, 1700000060000);

    // request_fetch should NOT return Task::none() or drop request,
    // and should synchronize footprint streams so DepthAndTrades is registered.
    let _task = request_fetch(&mut p1_state, layout_id, req_id, fetch, None);

    // Verify DepthAndTrades is now in p1_state.streams
    let has_depth_and_trades = p1_state.streams.find_ready_map(|s| {
        if let StreamKind::DepthAndTrades { ticker_info, .. } = s {
            Some(*ticker_info)
        } else {
            None
        }
    });
    assert_eq!(has_depth_and_trades, Some(btc_info));
}

#[test]
fn test_fetch_range_trades_explicit_stream_priority() {
    let btc_ticker = exchange::Ticker::new("BTCUSDT", Exchange::BinanceLinear);
    let btc_info = TickerInfo::new(btc_ticker, 0.1, 0.001, None);
    let bybit_ticker = exchange::Ticker::new("BTCUSDT", Exchange::BybitLinear);
    let bybit_info = TickerInfo::new(bybit_ticker, 0.1, 0.001, None);

    let mut p1_state = pane::State::default();
    let chart = crate::chart::kline::KlineChart::new(
        data::chart::ViewConfig::default(),
        data::chart::Basis::Time(exchange::Timeframe::M5),
        1.0,
        &[],
        Vec::new(),
        &[],
        btc_info,
        &data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: true,
            aggregate_exchanges: vec![Exchange::BinanceLinear, Exchange::BybitLinear],
            history_lookback_days: None,
        },
        None,
    );
    p1_state.content = pane::Content::Kline {
        chart: Some(chart),
        indicators: Default::default(),
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: true,
            aggregate_exchanges: vec![Exchange::BinanceLinear, Exchange::BybitLinear],
            history_lookback_days: None,
        },
    };

    p1_state.streams = exchange::adapter::ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
        ticker_info: btc_info,
        depth_aggr: StreamTicksize::Client,
        push_freq: PushFrequency::ServerDefault,
    }]);

    let explicit_stream = StreamKind::DepthAndTrades {
        ticker_info: bybit_info,
        depth_aggr: StreamTicksize::Client,
        push_freq: PushFrequency::ServerDefault,
    };

    let layout_id = uuid::Uuid::new_v4();
    let req_id = uuid::Uuid::new_v4();
    let fetch = FetchRange::Trades(1700000000000, 1700000060000);

    // When explicit stream is passed, request_fetch must not panic or drop request
    let _task = request_fetch(
        &mut p1_state,
        layout_id,
        req_id,
        fetch,
        Some(explicit_stream),
    );
    assert!(p1_state.streams.matches_trades(&explicit_stream));
}

#[test]
fn test_ter06_popout_window_grid_routing_isolation() {
    let (main_panes, _) = pane_grid::State::new(pane::State::default());
    let (popout_panes, popout_p1) = pane_grid::State::new(pane::State::default());

    let popout_win_id = window::Id::unique();

    let mut popout_map = HashMap::new();
    popout_map.insert(popout_win_id, (popout_panes, WindowSpec::default()));

    let mut dashboard = Dashboard {
        panes: main_panes,
        focus: None,
        last_focused_kline: None,
        streams: UniqueStreams::default(),
        popout: popout_map,
        layout_id: uuid::Uuid::new_v4(),
    };

    // Split pane in popout window
    let (new_popout_pane, _) = dashboard
        .popout
        .get_mut(&popout_win_id)
        .unwrap()
        .0
        .split(pane_grid::Axis::Horizontal, popout_p1, pane::State::new())
        .unwrap();

    assert_eq!(dashboard.popout.get(&popout_win_id).unwrap().0.len(), 2);
    assert_eq!(
        dashboard.panes.len(),
        1,
        "Main grid must remain unaffected by popout split"
    );

    dashboard
        .popout
        .get_mut(&popout_win_id)
        .unwrap()
        .0
        .maximize(new_popout_pane);

    assert_eq!(
        dashboard.panes.len(),
        1,
        "Main grid must remain unaffected by popout maximize"
    );
}

#[test]
fn test_ter14_auto_inherit_pane_config_on_init() {
    let win_id = window::Id::unique();

    // Pane 1: existing configured Kline pane with DeltaProfile
    let mut p1_state = pane::State::default();
    let mut chart1 = make_test_chart();
    chart1.set_cluster_kind(data::chart::kline::ClusterKind::DeltaProfile);
    p1_state.settings.visual_config = Some(data::layout::pane::VisualConfig::Kline(
        data::chart::kline::Config {
            rolling_vwap_window_hours: 42,
            ..Default::default()
        },
    ));
    p1_state.content = pane::Content::Kline {
        chart: Some(chart1),
        indicators: Default::default(),
        layout: Default::default(),
        kind: data::chart::KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::DeltaProfile,
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        },
    };

    // Pane 2: newly created placeholder pane
    let p2_state = pane::State::default();

    let (mut panes, p1) = pane_grid::State::new(p1_state);
    let (p2, _) = panes
        .split(pane_grid::Axis::Horizontal, p1, p2_state)
        .unwrap();

    let mut dashboard = Dashboard {
        panes,
        focus: Some((win_id, p2)),
        last_focused_kline: Some((win_id, p1)),
        streams: UniqueStreams::default(),
        popout: HashMap::new(),
        layout_id: uuid::Uuid::new_v4(),
    };

    let ticker = exchange::Ticker::new("ETHUSDT", exchange::adapter::Exchange::BinanceLinear);
    let ti = exchange::TickerInfo::new(ticker, 0.01, 0.001, None);

    // Initialize pane 2 as FootprintChart
    let _ = dashboard.init_pane(
        win_id,
        win_id,
        p2,
        ti,
        data::layout::pane::ContentKind::FootprintChart,
    );

    let p2_state = dashboard.panes.get(p2).unwrap();
    // Check visual config was inherited
    match &p2_state.settings.visual_config {
        Some(data::layout::pane::VisualConfig::Kline(cfg)) => {
            assert_eq!(cfg.rolling_vwap_window_hours, 42);
        }
        _ => {
            panic!("Expected inherited VisualConfig::Kline with rolling_vwap_window_hours = 42")
        }
    }
    // Check cluster kind was inherited
    assert_eq!(
        p2_state.content.clusters(),
        Some(data::chart::kline::ClusterKind::DeltaProfile)
    );
}

#[test]
fn test_dat17_dashboard_layout_id_propagation_and_clone_isolation() {
    let dash = Dashboard::default();
    let original_layout_id = dash.layout_id;

    // Verify layout_id is propagated to all default panes
    for (_, pane_state) in dash.panes.iter() {
        assert_eq!(pane_state.layout_id, Some(original_layout_id));
    }

    // Clone layout simulation as in src/main.rs
    let mut ser_dashboard = data::Dashboard::from(&dash);

    fn reset_pane_ids(pane: &mut data::Pane) {
        match pane {
            data::Pane::Split { a, b, .. } => {
                reset_pane_ids(a);
                reset_pane_ids(b);
            }
            data::Pane::KlineChart { settings, .. }
            | data::Pane::HeatmapChart { settings, .. }
            | data::Pane::ShaderHeatmap { settings, .. }
            | data::Pane::ComparisonChart { settings, .. }
            | data::Pane::TimeAndSales { settings, .. }
            | data::Pane::Ladder { settings, .. } => {
                settings.pane_id = Some(uuid::Uuid::new_v4());
            }
            data::Pane::Starter { .. } => {}
        }
    }

    reset_pane_ids(&mut ser_dashboard.pane);
    let new_uid = uuid::Uuid::new_v4();
    assert_ne!(new_uid, original_layout_id);

    let cloned_dash = Dashboard::from_config(
        crate::layout::configuration(ser_dashboard.pane),
        vec![],
        new_uid,
    );

    assert_eq!(cloned_dash.layout_id, new_uid);
    for (_, p_state) in cloned_dash.panes.iter() {
        assert_eq!(p_state.layout_id, Some(new_uid));
        // Ensure no pane ID collision with original dashboard
        for (_, orig_state) in dash.panes.iter() {
            assert_ne!(p_state.id, orig_state.id);
        }
    }
}
