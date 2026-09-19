use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::{FetchCtx, KlineIndicatorImpl},
        plot::{
            PlotTooltip,
            bar::{BarClass, BarPlot, Baseline},
        },
    },
};

use super::open_interest::OpenInterestIndicator;

use data::chart::{
    PlotData,
    kline::{KlineDataPoint, PositionFlowColors},
};
use data::util::format_with_commas;
use exchange::fetcher::FetchRange;
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionRegime {
    NewLongs,
    NewShorts,
    LongForcedClose,
    ShortForcedClose,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionFlowPoint {
    pub regime: PositionRegime,
    pub volume: f32,
    pub delta_price: f32,
    pub delta_oi: Option<f32>,
    pub buy_vol: f32,
    pub sell_vol: f32,
}

pub struct PositionFlowIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, PositionFlowPoint>,
    raw_oi: BTreeMap<u64, f32>,
    cached_candles: Vec<(u64, f32, f32, f32, f32)>,
    pub colors: PositionFlowColors,
}

impl PositionFlowIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            raw_oi: BTreeMap::new(),
            cached_candles: Vec::new(),
            colors: PositionFlowColors::default(),
        }
    }

    pub fn set_colors(&mut self, colors: PositionFlowColors) {
        self.colors = colors;
        self.clear_all_caches();
    }

    fn classify_candle(
        delta_p: f32,
        delta_oi: Option<f32>,
        buy_vol: f32,
        sell_vol: f32,
    ) -> (PositionRegime, Option<f32>) {
        if let Some(doi) = delta_oi
            && doi.abs() > 1e-4
        {
            let regime = if delta_p >= 0.0 {
                if doi >= 0.0 {
                    PositionRegime::NewLongs
                } else {
                    PositionRegime::ShortForcedClose
                }
            } else if doi >= 0.0 {
                PositionRegime::NewShorts
            } else {
                PositionRegime::LongForcedClose
            };
            return (regime, Some(doi));
        }

        // Fallback using taker volume delta
        let delta_v = buy_vol - sell_vol;
        let regime = if delta_p >= 0.0 {
            if delta_v >= 0.0 {
                PositionRegime::NewLongs
            } else {
                PositionRegime::ShortForcedClose
            }
        } else if delta_v <= 0.0 {
            PositionRegime::NewShorts
        } else {
            PositionRegime::LongForcedClose
        };
        (regime, None)
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let colors = self.colors;
        let tooltip = move |pt: &PositionFlowPoint, _next: Option<&PositionFlowPoint>| {
            let regime_name = match pt.regime {
                PositionRegime::NewLongs => "Regime: New Longs (Buildup)",
                PositionRegime::NewShorts => "Regime: New Shorts (Buildup)",
                PositionRegime::LongForcedClose => "Regime: Long Forced Close (Liquidation)",
                PositionRegime::ShortForcedClose => "Regime: Short Forced Close (Liquidation)",
            };
            let vol_line = format!("Candle Volume: {}", format_with_commas(pt.volume));
            let p_sign = if pt.delta_price >= 0.0 { "+" } else { "" };
            let price_line = format!("Δ Price: {}{:.2}", p_sign, pt.delta_price);
            let oi_line = if let Some(doi) = pt.delta_oi {
                let oi_sign = if doi >= 0.0 { "+" } else { "" };
                format!("Δ OI: {}{}", oi_sign, format_with_commas(doi))
            } else {
                "Δ OI: N/A (Volume Delta Proxy)".to_string()
            };
            let flow_line = format!(
                "Buy/Sell: {} / {}",
                format_with_commas(pt.buy_vol),
                format_with_commas(pt.sell_vol)
            );

            PlotTooltip::new(format!(
                "{regime_name}\n{vol_line}\n{price_line}\n{oi_line}\n{flow_line}"
            ))
        };

        let bar_kind = move |pt: &PositionFlowPoint| {
            let rgb = match pt.regime {
                PositionRegime::NewLongs => colors.new_longs_rgb(),
                PositionRegime::NewShorts => colors.new_shorts_rgb(),
                PositionRegime::LongForcedClose => colors.long_forced_close_rgb(),
                PositionRegime::ShortForcedClose => colors.short_forced_close_rgb(),
            };
            let color = iced::Color::from_rgb(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            );
            BarClass::Colored(color)
        };

        let value_fn = |pt: &PositionFlowPoint| pt.volume;

        let plot = BarPlot::new(value_fn, bar_kind)
            .baseline(Baseline::Zero)
            .bar_width_factor(0.85)
            .padding(0.08)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }

    fn compute_points(&mut self) {
        self.data.clear();
        if self.cached_candles.is_empty() {
            return;
        }

        for (i, &(time, open, close, buy, sell)) in self.cached_candles.iter().enumerate() {
            let delta_p = if i > 0 {
                close - self.cached_candles[i - 1].2
            } else {
                close - open
            };

            let delta_oi = if let Some(&curr_oi) = self.raw_oi.get(&time) {
                if i > 0 {
                    let prev_time = self.cached_candles[i - 1].0;
                    self.raw_oi
                        .get(&prev_time)
                        .map(|&prev_oi| curr_oi - prev_oi)
                } else {
                    None
                }
            } else {
                None
            };

            let (regime, doi) = Self::classify_candle(delta_p, delta_oi, buy, sell);
            let total_vol = if buy == -1.0 { sell } else { buy + sell };

            self.data.insert(
                time,
                PositionFlowPoint {
                    regime,
                    volume: total_vol,
                    delta_price: delta_p,
                    delta_oi: doi,
                    buy_vol: buy.max(0.0),
                    sell_vol: sell.max(0.0),
                },
            );
        }
    }
}

impl Default for PositionFlowIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for PositionFlowIndicator {
    fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }

    fn clear_crosshair_caches(&mut self) {
        self.cache.clear_crosshair();
    }

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        self.indicator_elem(chart, visible_range)
    }

    fn fetch_range(&mut self, ctx: &FetchCtx) -> Option<FetchRange> {
        let exchange = ctx.main_chart.ticker_info.exchange();
        let is_supported = OpenInterestIndicator::is_supported_exchange(exchange)
            && OpenInterestIndicator::is_supported_timeframe(ctx.timeframe);

        if !is_supported || ctx.kline_latest == 0 || ctx.visible_earliest == 0 {
            return None;
        }

        let oi_earliest = self
            .raw_oi
            .keys()
            .next()
            .copied()
            .unwrap_or(ctx.kline_latest);
        let oi_latest = self.raw_oi.keys().next_back().copied().unwrap_or(u64::MIN);

        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let thirty_days_ago = now_ms.saturating_sub(exchange::adapter::OI_RETENTION_MS);

        if ctx.visible_earliest < oi_earliest && oi_earliest > thirty_days_ago {
            let prefetch_start = ctx.prefetch_earliest.max(thirty_days_ago);
            if prefetch_start < oi_earliest {
                return Some(FetchRange::OpenInterest(prefetch_start, oi_earliest));
            }
        }

        if oi_latest < ctx.kline_latest
            && ctx.kline_latest.saturating_sub(oi_latest) >= ctx.timeframe.to_milliseconds()
        {
            return Some(FetchRange::OpenInterest(
                oi_latest.max(ctx.prefetch_earliest),
                ctx.kline_latest,
            ));
        }

        None
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.cached_candles = match source {
            PlotData::TimeBased(timeseries) => timeseries
                .datapoints
                .iter()
                .map(|(t, dp)| {
                    (
                        *t,
                        dp.kline.open.to_f32(),
                        dp.kline.close.to_f32(),
                        dp.kline.volume.0,
                        dp.kline.volume.1,
                    )
                })
                .collect(),
            PlotData::TickBased(tickseries) => tickseries
                .datapoints
                .iter()
                .enumerate()
                .map(|(idx, dp)| {
                    (
                        idx as u64,
                        dp.kline.open.to_f32(),
                        dp.kline.close.to_f32(),
                        dp.kline.volume.0,
                        dp.kline.volume.1,
                    )
                })
                .collect(),
        };

        self.compute_points();
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, _klines: &[Kline]) {
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        self.rebuild_from_source(source);
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn on_open_interest(&mut self, data: &[exchange::OpenInterest]) {
        self.raw_oi
            .extend(data.iter().map(|oi| (oi.time, oi.value)));
        self.compute_points();
        self.clear_all_caches();
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_flow_regime_classification_with_oi() {
        // Price UP + OI UP => NewLongs
        let (r1, doi1) = PositionFlowIndicator::classify_candle(5.0, Some(100.0), 10.0, 5.0);
        assert_eq!(r1, PositionRegime::NewLongs);
        assert_eq!(doi1, Some(100.0));

        // Price DOWN + OI UP => NewShorts
        let (r2, doi2) = PositionFlowIndicator::classify_candle(-5.0, Some(100.0), 5.0, 10.0);
        assert_eq!(r2, PositionRegime::NewShorts);
        assert_eq!(doi2, Some(100.0));

        // Price DOWN + OI DOWN => LongForcedClose
        let (r3, doi3) = PositionFlowIndicator::classify_candle(-5.0, Some(-100.0), 5.0, 10.0);
        assert_eq!(r3, PositionRegime::LongForcedClose);
        assert_eq!(doi3, Some(-100.0));

        // Price UP + OI DOWN => ShortForcedClose
        let (r4, doi4) = PositionFlowIndicator::classify_candle(5.0, Some(-100.0), 10.0, 5.0);
        assert_eq!(r4, PositionRegime::ShortForcedClose);
        assert_eq!(doi4, Some(-100.0));
    }

    #[test]
    fn test_position_flow_regime_classification_fallback() {
        // Price UP + Delta UP => NewLongs
        let (r1, doi1) = PositionFlowIndicator::classify_candle(2.0, None, 100.0, 50.0);
        assert_eq!(r1, PositionRegime::NewLongs);
        assert_eq!(doi1, None);

        // Price DOWN + Delta DOWN => NewShorts
        let (r2, doi2) = PositionFlowIndicator::classify_candle(-2.0, None, 50.0, 100.0);
        assert_eq!(r2, PositionRegime::NewShorts);
        assert_eq!(doi2, None);

        // Price DOWN + Delta UP => LongForcedClose (absorption)
        let (r3, doi3) = PositionFlowIndicator::classify_candle(-2.0, None, 100.0, 50.0);
        assert_eq!(r3, PositionRegime::LongForcedClose);
        assert_eq!(doi3, None);

        // Price UP + Delta DOWN => ShortForcedClose (absorption)
        let (r4, doi4) = PositionFlowIndicator::classify_candle(2.0, None, 50.0, 100.0);
        assert_eq!(r4, PositionRegime::ShortForcedClose);
        assert_eq!(doi4, None);
    }

    #[test]
    fn test_position_flow_on_open_interest_recomputation() {
        let mut pf = PositionFlowIndicator::new();
        pf.cached_candles = vec![
            (1000, 100.0, 105.0, 10.0, 5.0),
            (2000, 105.0, 110.0, 15.0, 5.0),
        ];
        pf.compute_points();

        // Initially without OI: volume delta fallback
        let pt0 = pf.data.get(&2000).unwrap();
        assert_eq!(pt0.delta_oi, None);

        // Feed Open Interest
        pf.on_open_interest(&[
            exchange::OpenInterest {
                time: 1000,
                value: 500.0,
            },
            exchange::OpenInterest {
                time: 2000,
                value: 650.0,
            },
        ]);

        // After on_open_interest: delta_oi is recomputed to 150.0
        let pt1 = pf.data.get(&2000).unwrap();
        assert_eq!(pt1.delta_oi, Some(150.0));
        assert_eq!(pt1.regime, PositionRegime::NewLongs);
    }
}
