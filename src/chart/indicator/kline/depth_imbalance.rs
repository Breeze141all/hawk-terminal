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
use exchange::{Kline, Trade, depth::Depth};

use std::collections::{BTreeMap, VecDeque};
use std::ops::RangeInclusive;

/// Depth percentage band around mid price
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum DepthBand {
    #[default]
    TwoPercent,
    FivePercent,
    TenPercent,
}

impl DepthBand {
    pub const fn percentage(&self) -> f32 {
        match self {
            DepthBand::TwoPercent => 0.02,
            DepthBand::FivePercent => 0.05,
            DepthBand::TenPercent => 0.10,
        }
    }
}

/// Aggregation mode for depth imbalance calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum AggregationMode {
    #[default]
    Snapshot,
    Twap,
}

/// Imbalance regime based on critical thresholds (+-0.40)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthImbalanceRegime {
    BullishDominance, // >= +0.40
    BearishDominance, // <= -0.40
    Neutral,          // (-0.40, +0.40)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthImbalancePoint {
    pub ratio: f32, // in [-1.0, +1.0]
    pub bid_usd: f32,
    pub ask_usd: f32,
    pub total_usd: f32,
    pub mid_price: f32,
    pub regime: DepthImbalanceRegime,
}

impl DepthImbalancePoint {
    pub fn new(ratio: f32, bid_usd: f32, ask_usd: f32, total_usd: f32, mid_price: f32) -> Self {
        let regime = if ratio >= 0.40 {
            DepthImbalanceRegime::BullishDominance
        } else if ratio <= -0.40 {
            DepthImbalanceRegime::BearishDominance
        } else {
            DepthImbalanceRegime::Neutral
        };

        Self {
            ratio,
            bid_usd,
            ask_usd,
            total_usd,
            mid_price,
            regime,
        }
    }
}

/// Compute depth imbalance ratio from bids and asks within given depth percentage band.
///
/// Implements nominal USD weighting $V_i = P_i \times Q_i$, mid price derivation,
/// and edge case mitigation for zero volume, one-sided, or crossed books.
pub fn compute_depth_imbalance(
    bids: &[(f32, f32)],
    asks: &[(f32, f32)],
    depth_pct: f32,
) -> DepthImbalancePoint {
    let valid_bids: Vec<(f32, f32)> = bids
        .iter()
        .copied()
        .filter(|&(p, q)| p.is_finite() && q.is_finite() && p > 0.0 && q > 0.0)
        .collect();

    let valid_asks: Vec<(f32, f32)> = asks
        .iter()
        .copied()
        .filter(|&(p, q)| p.is_finite() && q.is_finite() && p > 0.0 && q > 0.0)
        .collect();

    if valid_bids.is_empty() && valid_asks.is_empty() {
        return DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, 0.0);
    }

    if valid_asks.is_empty() {
        let best_bid = valid_bids.iter().map(|&(p, _)| p).fold(0.0f32, f32::max);
        let total_bid_usd: f32 = valid_bids.iter().map(|&(p, q)| p * q).sum();
        return DepthImbalancePoint::new(1.0, total_bid_usd, 0.0, total_bid_usd, best_bid);
    }

    if valid_bids.is_empty() {
        let best_ask = valid_asks.iter().map(|&(p, _)| p).fold(f32::MAX, f32::min);
        let total_ask_usd: f32 = valid_asks.iter().map(|&(p, q)| p * q).sum();
        return DepthImbalancePoint::new(-1.0, 0.0, total_ask_usd, total_ask_usd, best_ask);
    }

    let mut best_bid = valid_bids.iter().map(|&(p, _)| p).fold(0.0f32, f32::max);
    let mut best_ask = valid_asks.iter().map(|&(p, _)| p).fold(f32::MAX, f32::min);

    // Crossed book handling: filter out intersecting levels
    if best_bid >= best_ask {
        let filtered_bids: Vec<(f32, f32)> = valid_bids
            .into_iter()
            .filter(|&(p, _)| p < best_ask)
            .collect();
        let filtered_asks: Vec<(f32, f32)> = valid_asks
            .into_iter()
            .filter(|&(p, _)| p > best_bid)
            .collect();

        if filtered_bids.is_empty() || filtered_asks.is_empty() {
            let mid = (best_bid + best_ask) / 2.0;
            return DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, mid);
        }

        best_bid = filtered_bids.iter().map(|&(p, _)| p).fold(0.0f32, f32::max);
        best_ask = filtered_asks
            .iter()
            .map(|&(p, _)| p)
            .fold(f32::MAX, f32::min);
    }

    let mid_price = (best_bid + best_ask) / 2.0;
    if mid_price <= 0.0 {
        return DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, 0.0);
    }

    let min_bid_price = mid_price * (1.0 - depth_pct.abs());
    let max_ask_price = mid_price * (1.0 + depth_pct.abs());

    let bid_usd: f32 = bids
        .iter()
        .filter(|&&(p, q)| {
            p.is_finite() && q.is_finite() && p >= min_bid_price && p <= mid_price && q > 0.0
        })
        .map(|&(p, q)| p * q)
        .sum();

    let ask_usd: f32 = asks
        .iter()
        .filter(|&&(p, q)| {
            p.is_finite() && q.is_finite() && p <= max_ask_price && p >= mid_price && q > 0.0
        })
        .map(|&(p, q)| p * q)
        .sum();

    let total_usd = bid_usd + ask_usd;
    let ratio = if total_usd <= 1e-6 {
        0.0
    } else {
        ((bid_usd - ask_usd) / total_usd).clamp(-1.0, 1.0)
    };

    DepthImbalancePoint::new(ratio, bid_usd, ask_usd, total_usd, mid_price)
}

pub struct DepthImbalanceIndicator {
    cache: Caches,
    pub band: DepthBand,
    pub mode: AggregationMode,
    pub data: BTreeMap<u64, DepthImbalancePoint>,
    twap_window: VecDeque<f32>,
    twap_window_size: usize,
}

impl DepthImbalanceIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            band: DepthBand::TwoPercent,
            mode: AggregationMode::Snapshot,
            data: BTreeMap::new(),
            twap_window: VecDeque::new(),
            twap_window_size: 10,
        }
    }

    #[allow(dead_code)]
    pub fn with_band(mut self, band: DepthBand) -> Self {
        self.band = band;
        self
    }

    #[allow(dead_code)]
    pub fn with_mode(mut self, mode: AggregationMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn on_depth_update(&mut self, time: u64, depth: &Depth) {
        let bids: Vec<(f32, f32)> = depth
            .bids
            .iter()
            .map(|(&p, &q)| (p.to_f32_lossy(), q))
            .collect();
        let asks: Vec<(f32, f32)> = depth
            .asks
            .iter()
            .map(|(&p, &q)| (p.to_f32_lossy(), q))
            .collect();

        let mut pt = compute_depth_imbalance(&bids, &asks, self.band.percentage());

        if matches!(self.mode, AggregationMode::Twap) {
            self.twap_window.push_back(pt.ratio);
            if self.twap_window.len() > self.twap_window_size {
                self.twap_window.pop_front();
            }
            let sum: f32 = self.twap_window.iter().sum();
            let twap_ratio = sum / (self.twap_window.len() as f32);
            pt = DepthImbalancePoint::new(
                twap_ratio,
                pt.bid_usd,
                pt.ask_usd,
                pt.total_usd,
                pt.mid_price,
            );
        }

        self.data.insert(time, pt);
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |pt: &DepthImbalancePoint, _next: Option<&DepthImbalancePoint>| {
            let regime_str = match pt.regime {
                DepthImbalanceRegime::BullishDominance => "Bullish Imbalance (>= +0.40)",
                DepthImbalanceRegime::BearishDominance => "Bearish Imbalance (<= -0.40)",
                DepthImbalanceRegime::Neutral => "Balanced Book",
            };
            let ratio_pct = pt.ratio * 100.0;
            let ratio_sign = if pt.ratio >= 0.0 { "+" } else { "" };
            let ratio_line = format!(
                "Depth Imbalance: {}{:.1}% ({:+.2})",
                ratio_sign, ratio_pct, pt.ratio
            );
            let regime_line = format!("Regime: {regime_str}");
            let bid_line = format!("Bid Depth: ${}", format_with_commas(pt.bid_usd));
            let ask_line = format!("Ask Depth: ${}", format_with_commas(pt.ask_usd));
            let mid_line = format!("Mid Price: ${:.2}", pt.mid_price);

            PlotTooltip::new(format!(
                "{ratio_line}\n{regime_line}\n{bid_line}\n{ask_line}\n{mid_line}"
            ))
        };

        let bar_kind = |pt: &DepthImbalancePoint| match pt.regime {
            DepthImbalanceRegime::BullishDominance => {
                BarClass::Colored(iced::Color::from_rgb(0.12, 0.85, 0.45))
            }
            DepthImbalanceRegime::BearishDominance => {
                BarClass::Colored(iced::Color::from_rgb(0.95, 0.22, 0.25))
            }
            DepthImbalanceRegime::Neutral => {
                BarClass::Colored(iced::Color::from_rgba(0.55, 0.58, 0.65, 0.60))
            }
        };

        let value_fn = |pt: &DepthImbalancePoint| pt.ratio;

        let plot = BarPlot::new(value_fn, bar_kind)
            .baseline(Baseline::Zero)
            .bar_width_factor(0.85)
            .padding(0.1)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }
}

impl Default for DepthImbalanceIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for DepthImbalanceIndicator {
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
        self.twap_window.clear();
        match source {
            PlotData::TimeBased(ts) => {
                for &time in ts.datapoints.keys() {
                    self.data
                        .insert(time, DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, 0.0));
                }
            }
            PlotData::TickBased(tb) => {
                for idx in 0..tb.datapoints.len() {
                    self.data.insert(
                        idx as u64,
                        DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, 0.0),
                    );
                }
            }
        }
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for k in klines {
            self.data.entry(k.time).or_insert_with(|| {
                DepthImbalancePoint::new(0.0, 0.0, 0.0, 0.0, k.close.to_f32_lossy())
            });
        }
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        if self.data.is_empty() {
            self.rebuild_from_source(source);
        }
    }

    fn on_depth(&mut self, time: u64, depth: &Depth) {
        self.on_depth_update(time, depth);
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
    fn test_compute_depth_imbalance_symmetric_balanced() {
        let bids = vec![(99.0, 10.0), (98.0, 10.0)];
        let asks = vec![(101.0, 10.0), (102.0, 10.0)];
        // Mid price = (99 + 101) / 2 = 100
        // 2% band: bids >= 98, asks <= 102
        // bid USD = 99*10 + 98*10 = 1970
        // ask USD = 101*10 + 102*10 = 2030
        let pt = compute_depth_imbalance(&bids, &asks, 0.02);
        assert_eq!(pt.mid_price, 100.0);
        assert_eq!(pt.bid_usd, 1970.0);
        assert_eq!(pt.ask_usd, 2030.0);
        let expected_ratio = (1970.0 - 2030.0) / (1970.0 + 2030.0);
        assert!((pt.ratio - expected_ratio).abs() < 1e-4);
        assert_eq!(pt.regime, DepthImbalanceRegime::Neutral);
    }

    #[test]
    fn test_compute_depth_imbalance_strong_bullish_imbalance() {
        let bids = vec![(99.0, 50.0), (98.0, 50.0)];
        let asks = vec![(101.0, 5.0), (102.0, 5.0)];
        let pt = compute_depth_imbalance(&bids, &asks, 0.05);
        assert!(pt.ratio >= 0.40);
        assert_eq!(pt.regime, DepthImbalanceRegime::BullishDominance);
    }

    #[test]
    fn test_compute_depth_imbalance_strong_bearish_imbalance() {
        let bids = vec![(99.0, 5.0), (98.0, 5.0)];
        let asks = vec![(101.0, 50.0), (102.0, 50.0)];
        let pt = compute_depth_imbalance(&bids, &asks, 0.05);
        assert!(pt.ratio <= -0.40);
        assert_eq!(pt.regime, DepthImbalanceRegime::BearishDominance);
    }

    #[test]
    fn test_compute_depth_imbalance_one_sided_books() {
        // Only bids
        let bids = vec![(100.0, 10.0)];
        let pt_bids_only = compute_depth_imbalance(&bids, &[], 0.05);
        assert_eq!(pt_bids_only.ratio, 1.0);
        assert_eq!(pt_bids_only.regime, DepthImbalanceRegime::BullishDominance);

        // Only asks
        let asks = vec![(100.0, 10.0)];
        let pt_asks_only = compute_depth_imbalance(&[], &asks, 0.05);
        assert_eq!(pt_asks_only.ratio, -1.0);
        assert_eq!(pt_asks_only.regime, DepthImbalanceRegime::BearishDominance);

        // Both empty
        let pt_empty = compute_depth_imbalance(&[], &[], 0.05);
        assert_eq!(pt_empty.ratio, 0.0);
        assert_eq!(pt_empty.regime, DepthImbalanceRegime::Neutral);
    }

    #[test]
    fn test_compute_depth_imbalance_crossed_book_sanitization() {
        // Crossed book: best bid (105.0) > best ask (95.0)
        let bids = vec![(105.0, 1.0), (90.0, 5.0)];
        let asks = vec![(95.0, 1.0), (110.0, 5.0)];
        let pt = compute_depth_imbalance(&bids, &asks, 0.10);
        assert!(pt.ratio.is_finite());
        assert!(pt.ratio >= -1.0 && pt.ratio <= 1.0);
    }

    #[test]
    fn test_twap_aggregation_mode() {
        let mut indi = DepthImbalanceIndicator::new()
            .with_mode(AggregationMode::Twap)
            .with_band(DepthBand::TwoPercent);

        let mut depth1 = Depth::default();
        depth1.bids.insert(exchange::Price::from_f32(99.0), 10.0);
        depth1.asks.insert(exchange::Price::from_f32(101.0), 10.0);
        indi.on_depth_update(1000, &depth1);

        let pt1 = indi.data.get(&1000).unwrap();
        assert_eq!(pt1.ratio, pt1.ratio); // valid number

        // Now strong bid imbalance
        let mut depth2 = Depth::default();
        depth2.bids.insert(exchange::Price::from_f32(99.0), 100.0);
        depth2.asks.insert(exchange::Price::from_f32(101.0), 1.0);
        indi.on_depth_update(2000, &depth2);

        let pt2 = indi.data.get(&2000).unwrap();
        // Since TWAP smoothed with previous balanced sample, pt2 ratio is smoothed
        assert!(pt2.ratio > 0.0);
    }
}
