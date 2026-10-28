use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::{FetchCtx, KlineIndicatorImpl},
        plot::{PlotTooltip, line::LinePlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use data::util::format_with_commas;
use exchange::fetcher::FetchRange;
use exchange::{Kline, SpotKline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// Market absorption regime identified from cross-market delta divergence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DivergenceRegime {
    #[default]
    Neutral,
    /// Spot buying absorbing perp short selling (limit bid support)
    BullishSpotAbsorption,
    /// Spot selling into perp aggressive long buying (limit ask resistance)
    BearishSpotAbsorption,
    /// Aggressive perp shorting while spot remains resilient (squeeze potential)
    PerpSqueezeRisk,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CvdDivergencePoint {
    pub spot_cvd: f32,
    pub perp_cvd: f32,
    pub spread: f32, // spot_cvd - perp_cvd
    pub spot_delta: f32,
    pub perp_delta: f32,
    pub regime: DivergenceRegime,
}

impl CvdDivergencePoint {
    pub fn new(spot_cvd: f32, perp_cvd: f32, spot_delta: f32, perp_delta: f32) -> Self {
        let spread = spot_cvd - perp_cvd;

        let regime = if perp_delta <= -50.0 && spot_delta >= 0.0 {
            DivergenceRegime::PerpSqueezeRisk
        } else if spot_delta > 0.0 && perp_delta < 0.0 {
            DivergenceRegime::BullishSpotAbsorption
        } else if spot_delta < 0.0 && perp_delta > 0.0 {
            DivergenceRegime::BearishSpotAbsorption
        } else if spread > 0.0 && spot_cvd > 0.0 && perp_cvd <= 0.0 {
            DivergenceRegime::BullishSpotAbsorption
        } else if spread < 0.0 && spot_cvd < 0.0 && perp_cvd >= 0.0 {
            DivergenceRegime::BearishSpotAbsorption
        } else {
            DivergenceRegime::Neutral
        };

        Self {
            spot_cvd,
            perp_cvd,
            spread,
            spot_delta,
            perp_delta,
            regime,
        }
    }
}

pub struct CvdDivergenceIndicator {
    cache: Caches,
    pub data: BTreeMap<u64, CvdDivergencePoint>,
    perp_deltas: BTreeMap<u64, f32>,
    spot_deltas: BTreeMap<u64, f32>,
    spot_klines: BTreeMap<u64, f32>,
}

impl CvdDivergenceIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            perp_deltas: BTreeMap::new(),
            spot_deltas: BTreeMap::new(),
            spot_klines: BTreeMap::new(),
        }
    }

    /// Record a perp bar delta (buy volume - sell volume)
    pub fn insert_perp_delta(&mut self, time: u64, buy: f32, sell: f32) {
        let delta = if buy < 0.0 { 0.0 } else { buy - sell.max(0.0) };
        self.perp_deltas.insert(time, delta);
    }

    /// Record a spot bar delta (buy volume - sell volume)
    #[allow(dead_code)]
    pub fn insert_spot_delta(&mut self, time: u64, buy: f32, sell: f32) {
        let delta = if buy < 0.0 { 0.0 } else { buy - sell.max(0.0) };
        self.spot_deltas.insert(time, delta);
    }

    /// Recalculate cumulative CVDs and divergence spread across synchronized timestamps
    pub fn recalculate(&mut self) {
        self.data.clear();

        // Collect all sorted timestamps across perp and spot
        let mut timestamps: Vec<u64> = self
            .perp_deltas
            .keys()
            .chain(self.spot_deltas.keys())
            .chain(self.spot_klines.keys())
            .copied()
            .collect();
        timestamps.sort_unstable();
        timestamps.dedup();

        // Running totals in f64 for the same reason as the CVD indicator.
        let mut cum_perp = 0.0f64;
        let mut cum_spot = 0.0f64;

        for time in timestamps {
            let p_delta = self.perp_deltas.get(&time).copied().unwrap_or(0.0);
            let s_delta = self.spot_deltas.get(&time).copied().unwrap_or(0.0);

            cum_perp += f64::from(p_delta);
            cum_spot += f64::from(s_delta);

            let pt = CvdDivergencePoint::new(cum_spot as f32, cum_perp as f32, s_delta, p_delta);
            self.data.insert(time, pt);
        }

        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |pt: &CvdDivergencePoint, _next: Option<&CvdDivergencePoint>| {
            let regime_str = match pt.regime {
                DivergenceRegime::BullishSpotAbsorption => "Bullish Spot Absorption (Accumulation)",
                DivergenceRegime::BearishSpotAbsorption => "Bearish Spot Absorption (Distribution)",
                DivergenceRegime::PerpSqueezeRisk => "Perp Short Squeeze Risk",
                DivergenceRegime::Neutral => "Neutral / Aligned",
            };
            let spread_sign = if pt.spread >= 0.0 { "+" } else { "" };
            let spot_sign = if pt.spot_cvd >= 0.0 { "+" } else { "" };
            let perp_sign = if pt.perp_cvd >= 0.0 { "+" } else { "" };

            let spread_line = format!(
                "CVD Spread (Spot - Perp): {}{}",
                spread_sign,
                format_with_commas(pt.spread)
            );
            let spot_line = format!("Spot CVD: {}{}", spot_sign, format_with_commas(pt.spot_cvd));
            let perp_line = format!("Perp CVD: {}{}", perp_sign, format_with_commas(pt.perp_cvd));
            let regime_line = format!("Regime: {regime_str}");

            PlotTooltip::new(format!(
                "{spread_line}\n{spot_line}\n{perp_line}\n{regime_line}"
            ))
        };

        let value_fn = |pt: &CvdDivergencePoint| pt.spread;

        let plot = LinePlot::new(value_fn)
            .stroke_width(1.5)
            .show_points(true)
            .point_radius_factor(0.2)
            .padding(0.08)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }
}

impl Default for CvdDivergenceIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for CvdDivergenceIndicator {
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
        if ctx.kline_latest == 0 || ctx.prefetch_earliest >= ctx.kline_latest {
            return None;
        }

        if self.spot_klines.is_empty() {
            return Some(FetchRange::SpotKline(
                ctx.prefetch_earliest,
                ctx.kline_latest,
            ));
        }

        None
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.perp_deltas.clear();
        match source {
            PlotData::TimeBased(ts) => {
                for (time, dp) in &ts.datapoints {
                    self.insert_perp_delta(*time, dp.kline.volume_f32().0, dp.kline.volume_f32().1);
                }
            }
            PlotData::TickBased(tb) => {
                for (idx, dp) in tb.datapoints.iter().enumerate() {
                    self.insert_perp_delta(
                        idx as u64,
                        dp.kline.volume_f32().0,
                        dp.kline.volume_f32().1,
                    );
                }
            }
        }
        self.recalculate();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for k in klines {
            self.insert_perp_delta(k.time, k.volume_f32().0, k.volume_f32().1);
        }
        self.recalculate();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        self.rebuild_from_source(source);
    }

    fn on_spot_klines(&mut self, klines: &[SpotKline]) {
        let mut prev_close: Option<f32> = None;
        for sk in klines {
            self.spot_klines.insert(sk.time, sk.close);
            if let Some(prev) = prev_close {
                let delta = sk.close - prev;
                // Spot delta proxy based on spot price movement
                self.spot_deltas.insert(sk.time, delta);
            }
            prev_close = Some(sk.close);
        }
        self.recalculate();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cvd_divergence_point_regime_classification() {
        // Bullish spot absorption: spot delta positive, perp delta negative
        let pt_bull = CvdDivergencePoint::new(100.0, -50.0, 20.0, -15.0);
        assert_eq!(pt_bull.spread, 150.0);
        assert_eq!(pt_bull.regime, DivergenceRegime::BullishSpotAbsorption);

        // Bearish spot absorption: spot delta negative, perp delta positive
        let pt_bear = CvdDivergencePoint::new(-80.0, 60.0, -10.0, 25.0);
        assert_eq!(pt_bear.spread, -140.0);
        assert_eq!(pt_bear.regime, DivergenceRegime::BearishSpotAbsorption);

        // Perp squeeze risk: massive perp shorting, spot flat or buying
        let pt_squeeze = CvdDivergencePoint::new(5.0, -120.0, 1.0, -60.0);
        assert_eq!(pt_squeeze.regime, DivergenceRegime::PerpSqueezeRisk);

        // Neutral alignment: both positive
        let pt_neutral = CvdDivergencePoint::new(50.0, 45.0, 10.0, 8.0);
        assert_eq!(pt_neutral.regime, DivergenceRegime::Neutral);
    }

    #[test]
    fn test_cvd_divergence_recalculate_spread_accumulation() {
        let mut indi = CvdDivergenceIndicator::new();

        // T1: Spot = +10, Perp = +5
        indi.insert_spot_delta(1000, 20.0, 10.0);
        indi.insert_perp_delta(1000, 15.0, 10.0);

        // T2: Spot = +30 (cum 40), Perp = -20 (cum -15)
        indi.insert_spot_delta(2000, 40.0, 10.0);
        indi.insert_perp_delta(2000, 10.0, 30.0);

        indi.recalculate();

        let pt1 = indi.data.get(&1000).unwrap();
        assert_eq!(pt1.spot_cvd, 10.0);
        assert_eq!(pt1.perp_cvd, 5.0);
        assert_eq!(pt1.spread, 5.0);

        let pt2 = indi.data.get(&2000).unwrap();
        assert_eq!(pt2.spot_cvd, 40.0);
        assert_eq!(pt2.perp_cvd, -15.0);
        assert_eq!(pt2.spread, 55.0);
        assert_eq!(pt2.regime, DivergenceRegime::BullishSpotAbsorption);
    }

    #[test]
    fn test_on_spot_klines_proxy_delta_generation() {
        let mut indi = CvdDivergenceIndicator::new();
        let spot_klines = vec![
            SpotKline {
                time: 1000,
                close: 100.0,
            },
            SpotKline {
                time: 2000,
                close: 105.0,
            },
            SpotKline {
                time: 3000,
                close: 102.0,
            },
        ];

        indi.insert_perp_delta(1000, 10.0, 10.0);
        indi.insert_perp_delta(2000, 5.0, 15.0);
        indi.insert_perp_delta(3000, 20.0, 10.0);

        indi.on_spot_klines(&spot_klines);

        assert!(indi.data.contains_key(&2000));
        let pt2 = indi.data.get(&2000).unwrap();
        assert_eq!(pt2.spot_delta, 5.0); // 105 - 100
        assert_eq!(pt2.perp_delta, -10.0); // 5 - 15
        assert_eq!(pt2.regime, DivergenceRegime::BullishSpotAbsorption);
    }
}
