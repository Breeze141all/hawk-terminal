use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{PlotTooltip, line::LinePlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use data::util::format_with_commas;
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Debug, Clone, Copy)]
pub struct CvdPoint {
    pub cvd: f32,
    pub delta: f32,
    pub buy_vol: f32,
    pub sell_vol: f32,
}

pub struct CvdIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, CvdPoint>,
}

impl CvdIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |pt: &CvdPoint, _next: Option<&CvdPoint>| {
            let cvd_sign = if pt.cvd >= 0.0 { "+" } else { "" };
            let delta_sign = if pt.delta >= 0.0 { "+" } else { "" };
            let cvd_line = format!("Cumulative CVD: {}{}", cvd_sign, format_with_commas(pt.cvd));
            let delta_line = format!("Bar Delta: {}{}", delta_sign, format_with_commas(pt.delta));
            let buy_line = format!("Buy Volume: {}", format_with_commas(pt.buy_vol));
            let sell_line = format!("Sell Volume: {}", format_with_commas(pt.sell_vol));

            PlotTooltip::new(format!("{cvd_line}\n{delta_line}\n{buy_line}\n{sell_line}"))
        };

        let value_fn = |pt: &CvdPoint| pt.cvd;

        let plot = LinePlot::new(value_fn)
            .stroke_width(1.5)
            .show_points(true)
            .point_radius_factor(0.2)
            .padding(0.08)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }

    fn recalculate_from_pairs(&mut self, pairs: Vec<(u64, f32, f32)>) {
        self.data.clear();
        let mut cum_delta = 0.0f32;
        for (time, buy, sell) in pairs {
            let delta = if buy == -1.0 { 0.0 } else { buy - sell };
            cum_delta += delta;
            self.data.insert(
                time,
                CvdPoint {
                    cvd: cum_delta,
                    delta,
                    buy_vol: buy.max(0.0),
                    sell_vol: sell.max(0.0),
                },
            );
        }
    }
}

impl Default for CvdIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for CvdIndicator {
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
        let pairs: Vec<(u64, f32, f32)> = match source {
            PlotData::TimeBased(timeseries) => timeseries
                .datapoints
                .iter()
                .map(|(t, dp)| (*t, dp.kline.volume.0, dp.kline.volume.1))
                .collect(),
            PlotData::TickBased(tickseries) => tickseries
                .datapoints
                .iter()
                .enumerate()
                .map(|(idx, dp)| (idx as u64, dp.kline.volume.0, dp.kline.volume.1))
                .collect(),
        };
        self.recalculate_from_pairs(pairs);
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, _klines: &[Kline]) {
        // Full recalculation maintains cumulative delta continuity
        // In practice, klines are inserted into data_source, and callers invoke rebuild or insert
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cvd_sequential_accumulation() {
        let mut cvd = CvdIndicator::new();
        let pairs = vec![
            (1000u64, 100.0f32, 60.0f32), // delta = +40, cvd = +40
            (2000u64, 50.0f32, 80.0f32),  // delta = -30, cvd = +10
            (3000u64, 70.0f32, 70.0f32),  // delta = 0,   cvd = +10
            (4000u64, 20.0f32, 50.0f32),  // delta = -30, cvd = -20
        ];
        cvd.recalculate_from_pairs(pairs);

        assert_eq!(cvd.data[&1000].cvd, 40.0);
        assert_eq!(cvd.data[&1000].delta, 40.0);

        assert_eq!(cvd.data[&2000].cvd, 10.0);
        assert_eq!(cvd.data[&2000].delta, -30.0);

        assert_eq!(cvd.data[&3000].cvd, 10.0);
        assert_eq!(cvd.data[&3000].delta, 0.0);

        assert_eq!(cvd.data[&4000].cvd, -20.0);
        assert_eq!(cvd.data[&4000].delta, -30.0);
    }
}
