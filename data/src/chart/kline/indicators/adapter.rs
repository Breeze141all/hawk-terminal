use crate::chart::PlotData;
use crate::chart::kline::{KlineDataPoint, KlineTrades};
use exchange::Kline;
use indicator_engine::{ChartFrame, IndicatorFrame, InstrumentContext, TimeOrTick};
use std::sync::Arc;

pub fn kline_to_indicator_frame(
    index: usize,
    kline: &Kline,
    footprint: Option<&KlineTrades>,
    open_interest: Option<f64>,
) -> IndicatorFrame {
    kline_to_indicator_frame_with_context(
        index,
        kline,
        footprint,
        open_interest,
        TimeOrTick::Time,
        None,
        true,
    )
}

pub fn kline_to_indicator_frame_with_context(
    index: usize,
    kline: &Kline,
    footprint: Option<&KlineTrades>,
    open_interest: Option<f64>,
    basis: TimeOrTick,
    min_tick: Option<f64>,
    trade_data_enabled: bool,
) -> IndicatorFrame {
    let trade = if trade_data_enabled {
        footprint.and_then(|fp| fp.trade_summary())
    } else {
        None
    };

    let (volume, buy_qty, sell_qty, directional_volume_available) =
        if Kline::buy_sell(kline.volume_f32()).is_some() {
            let buy = kline.volume.0;
            let sell = kline.volume.1;
            let vol = buy + sell;
            if vol == 0.0 {
                if let Some(t) = trade.as_ref() {
                    let vol = t.buy_qty + t.sell_qty;
                    (Some(vol), t.buy_qty, t.sell_qty, true)
                } else {
                    (Some(0.0), 0.0, 0.0, false)
                }
            } else {
                (Some(vol), buy, sell, true)
            }
        } else {
            let vol = kline.volume.1;
            if let Some(ref t) = trade {
                (Some(vol), t.buy_qty, t.sell_qty, true)
            } else {
                (Some(vol), 0.0, 0.0, false)
            }
        };

    IndicatorFrame {
        index,
        time: kline.time as i64,
        basis,
        instrument: InstrumentContext { min_tick },
        chart: ChartFrame {
            open: Some(kline.open.to_f64()),
            high: Some(kline.high.to_f64()),
            low: Some(kline.low.to_f64()),
            close: Some(kline.close.to_f64()),
            volume,
            directional_volume_available,
            buy_qty,
            sell_qty,
            trade,
        },
        open_interest,
        series_inputs: Arc::from([]),
    }
}

pub fn datapoint_to_indicator_frame(
    index: usize,
    dp: &KlineDataPoint,
    open_interest: Option<f64>,
    basis: TimeOrTick,
    min_tick: Option<f64>,
    trade_data_enabled: bool,
) -> IndicatorFrame {
    let has_trades = dp.trades_fetched || !dp.footprint.is_empty();
    let footprint = if has_trades {
        Some(&dp.footprint)
    } else {
        None
    };
    kline_to_indicator_frame_with_context(
        index,
        &dp.kline,
        footprint,
        open_interest,
        basis,
        min_tick,
        trade_data_enabled && has_trades,
    )
}

pub fn plot_data_to_indicator_frames(
    plot_data: &PlotData<KlineDataPoint>,
    open_interest: Option<f64>,
    min_tick: Option<f64>,
    trade_data_enabled: bool,
) -> Vec<IndicatorFrame> {
    match plot_data {
        PlotData::TimeBased(time_series) => time_series
            .datapoints
            .values()
            .enumerate()
            .map(|(index, dp)| {
                datapoint_to_indicator_frame(
                    index,
                    dp,
                    open_interest,
                    TimeOrTick::Time,
                    min_tick,
                    trade_data_enabled,
                )
            })
            .collect(),
        PlotData::TickBased(tick_aggr) => tick_aggr
            .datapoints
            .iter()
            .enumerate()
            .map(|(index, tick_acc)| {
                kline_to_indicator_frame_with_context(
                    index,
                    &tick_acc.kline,
                    Some(&tick_acc.footprint),
                    open_interest,
                    TimeOrTick::Tick,
                    min_tick,
                    trade_data_enabled,
                )
            })
            .collect(),
    }
}
