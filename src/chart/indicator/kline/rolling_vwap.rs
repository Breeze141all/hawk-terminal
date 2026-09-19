use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{PlotTooltip, line::LinePlot},
    },
};

use data::chart::{
    PlotData,
    kline::{KlineDataPoint, RollingVwapColors},
    vwap::{MultiRollingVwapPoint, MultiRollingVwapTracker},
};
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

pub struct RollingVwapIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, MultiRollingVwapPoint>,
    tracker: MultiRollingVwapTracker,
    pub colors: RollingVwapColors,
}

impl RollingVwapIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            tracker: MultiRollingVwapTracker::new(),
            colors: RollingVwapColors::default(),
        }
    }

    #[allow(dead_code)]
    pub fn with_window_hours(_hours: u32) -> Self {
        Self::new()
    }

    pub fn set_window_hours(&mut self, _hours: u32, _source: &PlotData<KlineDataPoint>) {
        // Multi-period rolling VWAP tracks fixed 7d, 30d, 90d, and 365d windows.
    }

    pub fn set_colors(&mut self, colors: RollingVwapColors) {
        self.colors = colors;
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |pt: &MultiRollingVwapPoint, _next: Option<&MultiRollingVwapPoint>| {
            let mut lines = Vec::with_capacity(5);
            lines.push("Rolling VWAP:".to_string());
            if let Some(d7) = pt.d7 {
                lines.push(format!("  7D: {:.2} (σ: {:.2})", d7.vwap, d7.sigma));
            }
            if let Some(d30) = pt.d30 {
                lines.push(format!(" 30D: {:.2} (σ: {:.2})", d30.vwap, d30.sigma));
            }
            if let Some(d90) = pt.d90 {
                lines.push(format!(" 90D: {:.2} (σ: {:.2})", d90.vwap, d90.sigma));
            }
            if let Some(d365) = pt.d365 {
                lines.push(format!("365D: {:.2} (σ: {:.2})", d365.vwap, d365.sigma));
            }

            PlotTooltip::new(lines.join("\n"))
        };

        let value_fn = |pt: &MultiRollingVwapPoint| {
            pt.d7
                .or(pt.d30)
                .or(pt.d90)
                .or(pt.d365)
                .map(|p| p.vwap as f32)
                .unwrap_or(0.0)
        };

        let plot = LinePlot::new(value_fn)
            .stroke_width(1.5)
            .show_points(true)
            .point_radius_factor(0.2)
            .padding(0.08)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }
}

impl Default for RollingVwapIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for RollingVwapIndicator {
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

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data.clear();
        self.tracker = MultiRollingVwapTracker::new();

        match source {
            PlotData::TimeBased(timeseries) => {
                for (time, dp) in &timeseries.datapoints {
                    let high = dp.kline.high.to_f32() as f64;
                    let low = dp.kline.low.to_f32() as f64;
                    let close = dp.kline.close.to_f32() as f64;
                    let tp = (high + low + close) / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;

                    let pt = self.tracker.on_data_point(*time as i64, tp, vol);
                    self.data.insert(*time, pt);
                }
            }
            PlotData::TickBased(tickseries) => {
                for (idx, dp) in tickseries.datapoints.iter().enumerate() {
                    let high = dp.kline.high.to_f32() as f64;
                    let low = dp.kline.low.to_f32() as f64;
                    let close = dp.kline.close.to_f32() as f64;
                    let tp = (high + low + close) / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;

                    let pt = self.tracker.on_data_point(dp.kline.time as i64, tp, vol);
                    self.data.insert(idx as u64, pt);
                }
            }
        }
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for kline in klines {
            let high = kline.high.to_f32() as f64;
            let low = kline.low.to_f32() as f64;
            let close = kline.close.to_f32() as f64;
            let tp = (high + low + close) / 3.0;
            let vol = (kline.volume.0 + kline.volume.1) as f64;

            let pt = self.tracker.on_data_point(kline.time as i64, tp, vol);
            self.data.insert(kline.time, pt);
        }
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        match source {
            PlotData::TimeBased(_) => (),
            PlotData::TickBased(_) => self.rebuild_from_source(source),
        }
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
