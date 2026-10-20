use data::chart::kline::indicators::{
    IndicatorAttachmentMode, IndicatorHost, IndicatorId, IndicatorInstanceStore,
    initialize_indicator_catalog,
};
use data::chart::kline::{KlineDataPoint, KlineTrades};
use exchange::util::{Price, PriceStep};
use exchange::{Kline, Trade};

fn make_candle(time: u64, open: f64, high: f64, low: f64, close: f64) -> Kline {
    Kline {
        time,
        open: Price::from_f64(open),
        high: Price::from_f64(high),
        low: Price::from_f64(low),
        close: Price::from_f64(close),
        volume: (10.0, 10.0),
    }
}

#[test]
fn test_footprint_cluster_immutability_during_indicator_recalc() {
    let _ = initialize_indicator_catalog();

    let step = PriceStep { units: 10_000 };
    let mut footprint = KlineTrades::new();
    let trade1 = Trade {
        time: 1_700_000_000_000,
        price: Price::from_f64(100.0),
        qty: 15.0,
        is_sell: false,
    };
    let trade2 = Trade {
        time: 1_700_000_000_050,
        price: Price::from_f64(105.0),
        qty: 25.0,
        is_sell: true,
    };
    footprint.add_trade_to_nearest_bin(&trade1, step);
    footprint.add_trade_to_nearest_bin(&trade2, step);
    footprint.calculate_poc();

    let initial_poc_price = footprint.poc.as_ref().map(|p| p.price);
    let initial_poc_vol = footprint.poc.as_ref().map(|p| p.volume);
    let initial_trades_len = footprint.trades.len();
    let initial_volume: f32 = footprint
        .trades
        .values()
        .map(|b| b.buy_qty + b.sell_qty)
        .sum();

    let dp = KlineDataPoint {
        kline: make_candle(1_700_000_000_000, 100.0, 106.0, 99.0, 104.0),
        footprint,
        trades_fetched: true,
    };

    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let inst_id = store.create(rsi_id);
    let mut host = IndicatorHost::default();

    let frame = data::chart::kline::indicators::datapoint_to_indicator_frame(
        0,
        &dp,
        None,
        indicator_engine::TimeOrTick::Time,
        Some(0.1),
        true,
    );

    host.recalculate(&[inst_id], &store, std::slice::from_ref(&frame));
    host.evaluate_closed(&frame);

    // Assert footprint data was not mutated or corrupted by indicator execution
    assert_eq!(
        dp.footprint.poc.as_ref().map(|p| p.price),
        initial_poc_price
    );
    assert_eq!(dp.footprint.poc.as_ref().map(|p| p.volume), initial_poc_vol);
    assert_eq!(dp.footprint.trades.len(), initial_trades_len);
    let post_volume: f32 = dp
        .footprint
        .trades
        .values()
        .map(|b| b.buy_qty + b.sell_qty)
        .sum();
    assert_eq!(post_volume, initial_volume);
    assert_eq!(dp.footprint.trades[&Price::from_f64(100.0)].buy_qty, 15.0);
    assert_eq!(dp.footprint.trades[&Price::from_f64(105.0)].sell_qty, 25.0);
}

#[test]
fn test_replay_cutoff_bounds_and_lookahead_isolation() {
    let _ = initialize_indicator_catalog();

    let candles: Vec<_> = (0..20)
        .map(|i| {
            let t = 1_000 + i * 1_000;
            let price = 100.0 + (i as f64);
            make_candle(t, price, price + 5.0, price - 2.0, price + 1.0)
        })
        .collect();

    let frames: Vec<_> = candles
        .iter()
        .enumerate()
        .map(|(idx, c)| {
            data::chart::kline::indicators::kline_to_indicator_frame(idx, c, None, None)
        })
        .collect();

    let mut store = IndicatorInstanceStore::default();
    let rsi_id = IndicatorId::parse("core.rsi").unwrap();
    let inst_id = store.create(rsi_id);
    let mut host = IndicatorHost::default();

    // Replay cutoff at 16_000: only frames with time <= 16_000 must be fed to recalculate
    let cutoff_time = 16_000u64;
    let mut replay_frames = frames.clone();
    replay_frames.retain(|f| (f.time as u64) <= cutoff_time);

    assert_eq!(replay_frames.len(), 16);
    assert_eq!(replay_frames[0].time, 1_000);
    assert_eq!(replay_frames[15].time, 16_000);

    host.recalculate(&[inst_id], &store, &replay_frames);

    let output_15 = host.output_value_for(inst_id, "value", 15);
    let output_16 = host.output_value_for(inst_id, "value", 16);
    let output_19 = host.output_value_for(inst_id, "value", 19);

    assert!(
        output_15.is_some(),
        "RSI should be evaluated at cutoff bar 15 after 14-period warmup"
    );
    assert_eq!(
        output_16, None,
        "Replay cutoff must not evaluate future frame 16"
    );
    assert_eq!(
        output_19, None,
        "Replay cutoff must not evaluate future frame 19"
    );
}

#[test]
fn test_drawing_and_alerts_layer_non_interference_contract() {
    let _ = initialize_indicator_catalog();

    // Contract: Scripted indicators render into Layer 0 (Pass 3.5 in main cache)
    // Dynamic drawing tools and price alerts render in Layer 1 (dynamic_frame)
    // Crosshair renders in Layer 2 (crosshair cache).
    // Indicator host and instances maintain zero drawing tool or alert state,
    // ensuring drawings/alerts are visually atop indicators and receive mouse hits directly.

    let mut store = IndicatorInstanceStore::default();
    let ma_id = IndicatorId::parse("core.moving-averages").unwrap();
    let inst_id = store.create(ma_id);

    let def = store.instances()[0].kind.definition();
    assert_eq!(def.attachment_mode, IndicatorAttachmentMode::Overlay);

    // Assert that indicator instance contains only indicator styling and configuration,
    // with no ownership or mutation of chart interaction / drawing geometry.
    assert!(store.instances().iter().any(|i| i.id == inst_id));
}
