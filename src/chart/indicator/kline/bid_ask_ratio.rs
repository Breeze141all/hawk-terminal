use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{
            PlotTooltip,
            bar::{BarClass, BarPlot, Baseline},
        },
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use data::util::format_with_commas;
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Debug, Clone, Copy)]
pub struct BidAskRatioPoint {
    pub ratio: f32,
    pub buy_vol: f32,
    pub sell_vol: f32,
    pub buy_pct: f32,
    pub delta: f32,
}

pub struct BidAskRatioIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, BidAskRatioPoint>,
}

impl BidAskRatioIndicator {
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
        let tooltip = |pt: &BidAskRatioPoint, _next: Option<&BidAskRatioPoint>| {
            let ratio_line = format!("Bid/Ask Ratio: {:.2}x", pt.ratio);
            let share_line = format!(
                "Buyer: {:.1}% | Seller: {:.1}%",
                pt.buy_pct,
                100.0 - pt.buy_pct
            );
            let buy_line = format!("Buy Volume: {}", format_with_commas(pt.buy_vol));
            let sell_line = format!("Sell Volume: {}", format_with_commas(pt.sell_vol));
            let delta_sign = if pt.delta >= 0.0 { "+" } else { "" };
            let delta_line = format!("Delta: {}{}", delta_sign, format_with_commas(pt.delta));

            PlotTooltip::new(format!(
                "{ratio_line}\n{share_line}\n{buy_line}\n{sell_line}\n{delta_line}"
            ))
        };

        let bar_kind = |pt: &BidAskRatioPoint| BarClass::Overlay {
            overlay: pt.ratio - 1.0,
        };

        let value_fn = |pt: &BidAskRatioPoint| pt.ratio;

        let plot = BarPlot::new(value_fn, bar_kind)
            .baseline(Baseline::Zero)
            .bar_width_factor(0.85)
            .padding(0.1)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }

    fn compute_point(buy: f32, sell: f32) -> BidAskRatioPoint {
        let b = buy.max(0.0);
        let s = sell.max(0.0);
        let total = b + s;
        let delta = b - s;

        let (ratio, buy_pct) = if total <= 0.0 {
            (1.0, 50.0)
        } else if s <= 0.0 {
            (10.0, 100.0)
        } else if b <= 0.0 {
            (0.0, 0.0)
        } else {
            let r = (b / s).clamp(0.0, 50.0);
            let pct = (b / total) * 100.0;
            (r, pct)
        };

        BidAskRatioPoint {
            ratio,
            buy_vol: b,
            sell_vol: s,
            buy_pct,
            delta,
        }
    }
}

impl Default for BidAskRatioIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for BidAskRatioIndicator {
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
        match source {
            PlotData::TimeBased(timeseries) => {
                for (time, dp) in &timeseries.datapoints {
                    let pt = Self::compute_point(dp.kline.volume.0, dp.kline.volume.1);
                    self.data.insert(*time, pt);
                }
            }
            PlotData::TickBased(tickseries) => {
                for (idx, dp) in tickseries.datapoints.iter().enumerate() {
                    let pt = Self::compute_point(dp.kline.volume.0, dp.kline.volume.1);
                    self.data.insert(idx as u64, pt);
                }
            }
        }
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for k in klines {
            let pt = Self::compute_point(k.volume.0, k.volume.1);
            self.data.insert(k.time, pt);
        }
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        match source {
            PlotData::TimeBased(_) => return,
            PlotData::TickBased(tickseries) => {
                let start_idx = old_dp_len.saturating_sub(1);
                for (idx, dp) in tickseries.datapoints.iter().enumerate().skip(start_idx) {
                    let pt = Self::compute_point(dp.kline.volume.0, dp.kline.volume.1);
                    self.data.insert(idx as u64, pt);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bid_ask_ratio_computation() {
        // Balanced: buy == sell
        let p_balanced = BidAskRatioIndicator::compute_point(50.0, 50.0);
        assert_eq!(p_balanced.ratio, 1.0);
        assert_eq!(p_balanced.buy_pct, 50.0);
        assert_eq!(p_balanced.delta, 0.0);

        // Buyer dominated: buy = 80, sell = 20
        let p_bull = BidAskRatioIndicator::compute_point(80.0, 20.0);
        assert_eq!(p_bull.ratio, 4.0);
        assert_eq!(p_bull.buy_pct, 80.0);
        assert_eq!(p_bull.delta, 60.0);

        // Seller dominated: buy = 20, sell = 80
        let p_bear = BidAskRatioIndicator::compute_point(20.0, 80.0);
        assert_eq!(p_bear.ratio, 0.25);
        assert_eq!(p_bear.buy_pct, 20.0);
        assert_eq!(p_bear.delta, -60.0);

        // Zero volume
        let p_zero = BidAskRatioIndicator::compute_point(0.0, 0.0);
        assert_eq!(p_zero.ratio, 1.0);
        assert_eq!(p_zero.buy_pct, 50.0);

        // Zero sell
        let p_no_sell = BidAskRatioIndicator::compute_point(100.0, 0.0);
        assert_eq!(p_no_sell.ratio, 10.0);
        assert_eq!(p_no_sell.buy_pct, 100.0);
    }
}
