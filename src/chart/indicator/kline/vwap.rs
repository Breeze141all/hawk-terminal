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
    kline::KlineDataPoint,
    vwap::{VwapPeriod, VwapPoint, VwapTracker},
};
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

pub struct VwapIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, VwapPoint>,
    tracker: VwapTracker,
}

impl VwapIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            tracker: VwapTracker::new(VwapPeriod::Daily),
        }
    }

    #[allow(dead_code)]
    pub fn with_period(period: VwapPeriod) -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            tracker: VwapTracker::new(period),
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |pt: &VwapPoint, _next: Option<&VwapPoint>| {
            let vwap_line = format!("VWAP: {:.2}", pt.vwap);
            let s1_line = format!("+1σ: {:.2} | -1σ: {:.2}", pt.upper_1sigma, pt.lower_1sigma);
            let s2_line = format!("+2σ: {:.2} | -2σ: {:.2}", pt.upper_2sigma, pt.lower_2sigma);
            let s3_line = format!("+3σ: {:.2} | -3σ: {:.2}", pt.upper_3sigma, pt.lower_3sigma);
            let sigma_line = format!("Std Dev (σ): {:.2}", pt.sigma);

            PlotTooltip::new(format!(
                "{vwap_line}\n{s1_line}\n{s2_line}\n{s3_line}\n{sigma_line}"
            ))
        };

        let value_fn = |pt: &VwapPoint| pt.vwap as f32;

        let plot = LinePlot::new(value_fn)
            .stroke_width(1.5)
            .show_points(true)
            .point_radius_factor(0.2)
            .padding(0.08)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }
}

impl Default for VwapIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for VwapIndicator {
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
        self.tracker = VwapTracker::new(self.tracker.period);

        match source {
            PlotData::TimeBased(timeseries) => {
                for (time, dp) in &timeseries.datapoints {
                    let high = dp.kline.high.to_f32() as f64;
                    let low = dp.kline.low.to_f32() as f64;
                    let close = dp.kline.close.to_f32() as f64;
                    let tp = (high + low + close) / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;

                    if let Some(pt) = self.tracker.on_data_point(*time as i64, tp, vol) {
                        self.data.insert(*time, pt);
                    }
                }
            }
            PlotData::TickBased(tickseries) => {
                for (idx, dp) in tickseries.datapoints.iter().enumerate() {
                    let high = dp.kline.high.to_f32() as f64;
                    let low = dp.kline.low.to_f32() as f64;
                    let close = dp.kline.close.to_f32() as f64;
                    let tp = (high + low + close) / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;

                    if let Some(pt) = self.tracker.on_data_point(dp.kline.time as i64, tp, vol) {
                        self.data.insert(idx as u64, pt);
                    }
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

            if let Some(pt) = self.tracker.on_data_point(kline.time as i64, tp, vol) {
                self.data.insert(kline.time, pt);
            }
        }
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        trades: &[Trade],
        _old_dp_len: usize,
        _source: &PlotData<KlineDataPoint>,
    ) {
        for trade in trades {
            let price = trade.price.to_f32() as f64;
            let qty = trade.qty as f64;
            if let Some(pt) = self.tracker.on_data_point(trade.time as i64, price, qty) {
                self.data.insert(trade.time, pt);
            }
        }
        self.clear_all_caches();
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }
}
