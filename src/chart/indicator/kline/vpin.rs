//! VPIN (Volume-Synchronized Probability of Informed Trading) Indicator
//!
//! Measures the probability of informed trading by analyzing order flow imbalance
//! in volume-synchronized buckets rather than time-based intervals.
//!
//! Formula: VPIN = sum(|Buy_i - Sell_i|) / sum(Volume_i) over N buckets
//!
//! High VPIN (>0.5) indicates potential informed trading activity and
//! often precedes significant price moves.

use crate::chart::{
    Basis, Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{PlotTooltip, mtm::MtmPlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::{Kline, Trade};

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// VPIN configuration
pub struct VpinConfig {
    /// Number of volume buckets for VPIN calculation
    pub num_buckets: usize,
    /// Alert threshold (VPIN > threshold = high toxicity)
    pub threshold: f32,
    /// Auto-calibrate bucket size based on recent volume
    pub auto_calibrate: bool,
    /// Lookback periods for auto-calibration
    pub calibration_lookback: usize,
    /// EMA smoothing factor (0 = no smoothing, higher = more smoothing)
    pub ema_periods: usize,
}

impl Default for VpinConfig {
    fn default() -> Self {
        Self {
            num_buckets: 20,       // Balance between sensitivity and noise
            threshold: 0.40,
            auto_calibrate: true,
            calibration_lookback: 50,
            ema_periods: 5,        // Smooth out noise
        }
    }
}

/// A volume bucket containing buy/sell breakdown
#[derive(Debug, Clone, Default)]
struct VolumeBucket {
    buy_volume: f32,
    sell_volume: f32,
    start_time: u64,
    end_time: u64,
}

impl VolumeBucket {
    fn total(&self) -> f32 {
        self.buy_volume + self.sell_volume
    }

    fn imbalance(&self) -> f32 {
        (self.buy_volume - self.sell_volume).abs()
    }
}

pub struct VpinIndicator {
    cache: Caches,
    config: VpinConfig,
    /// VPIN values by timestamp (mapped to kline timestamps for display)
    pub data: BTreeMap<u64, f32>,
    /// Volume buckets for calculation
    buckets: Vec<VolumeBucket>,
    /// Current bucket being filled
    current_bucket: VolumeBucket,
    /// Target volume per bucket (auto-calibrated)
    bucket_volume: f32,
    /// Raw kline data for recalculation
    kline_data: BTreeMap<u64, (f32, f32)>, // timestamp -> (buy_vol, sell_vol)
}

impl VpinIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            config: VpinConfig::default(),
            data: BTreeMap::new(),
            buckets: Vec::new(),
            current_bucket: VolumeBucket::default(),
            bucket_volume: 0.0,
            kline_data: BTreeMap::new(),
        }
    }

    /// Auto-calibrate bucket volume based on recent trading activity
    fn calibrate_bucket_volume(&mut self) {
        if !self.config.auto_calibrate || self.kline_data.is_empty() {
            return;
        }

        // Calculate average volume per kline over lookback period
        let recent_volumes: Vec<f32> = self
            .kline_data
            .values()
            .rev()
            .take(self.config.calibration_lookback)
            .map(|(buy, sell)| buy + sell)
            .collect();

        if recent_volumes.is_empty() {
            return;
        }

        let avg_volume = recent_volumes.iter().sum::<f32>() / recent_volumes.len() as f32;

        // Bucket volume = avg_kline_volume * klines_per_bucket
        // ~1.0 klines per bucket for balanced sensitivity
        let klines_per_bucket = 1.0;
        self.bucket_volume = (avg_volume * klines_per_bucket).max(0.1);
    }

    /// Rebuild VPIN from kline data
    fn recalculate(&mut self) {
        self.data.clear();
        self.buckets.clear();
        self.current_bucket = VolumeBucket::default();

        if self.kline_data.is_empty() {
            return;
        }

        // Calibrate bucket volume first
        self.calibrate_bucket_volume();

        if self.bucket_volume <= 0.0 {
            return;
        }

        // Process klines into volume buckets
        // Collect first to avoid borrow conflict
        let kline_entries: Vec<(u64, f32, f32)> = self
            .kline_data
            .iter()
            .map(|(&t, &(b, s))| (t, b, s))
            .collect();

        for (time, buy_vol, sell_vol) in kline_entries {
            self.add_volume_to_buckets(time, buy_vol, sell_vol);
        }

        // Calculate VPIN for each timestamp
        self.calculate_vpin_values();

        self.cache.clear_all();
    }

    /// Add volume to buckets, potentially creating new ones
    fn add_volume_to_buckets(&mut self, time: u64, buy_vol: f32, sell_vol: f32) {
        let mut remaining_buy = buy_vol;
        let mut remaining_sell = sell_vol;

        if self.current_bucket.start_time == 0 {
            self.current_bucket.start_time = time;
        }

        while remaining_buy > 0.0 || remaining_sell > 0.0 {
            let current_total = self.current_bucket.total();
            let space_left = self.bucket_volume - current_total;

            if space_left <= 0.0 {
                // Bucket is full, finalize it
                self.current_bucket.end_time = time;
                self.buckets.push(self.current_bucket.clone());
                self.current_bucket = VolumeBucket {
                    start_time: time,
                    ..Default::default()
                };
                continue;
            }

            // Calculate how much to add to this bucket
            let total_remaining = remaining_buy + remaining_sell;
            let to_add = total_remaining.min(space_left);

            if total_remaining > 0.0 {
                // Proportionally split between buy and sell
                let buy_ratio = remaining_buy / total_remaining;
                let add_buy = to_add * buy_ratio;
                let add_sell = to_add * (1.0 - buy_ratio);

                self.current_bucket.buy_volume += add_buy;
                self.current_bucket.sell_volume += add_sell;

                remaining_buy -= add_buy;
                remaining_sell -= add_sell;

                // Handle floating point precision
                if remaining_buy < 0.001 {
                    remaining_buy = 0.0;
                }
                if remaining_sell < 0.001 {
                    remaining_sell = 0.0;
                }
            } else {
                break;
            }
        }

        self.current_bucket.end_time = time;
    }

    /// Calculate VPIN values and map to timestamps
    fn calculate_vpin_values(&mut self) {
        let n = self.config.num_buckets;

        if self.buckets.len() < n {
            return;
        }

        // First pass: calculate raw VPIN values
        let mut raw_values: Vec<(u64, f32)> = Vec::new();

        for i in (n - 1)..self.buckets.len() {
            let window = &self.buckets[(i + 1 - n)..=i];

            let total_imbalance: f32 = window.iter().map(|b| b.imbalance()).sum();
            let total_volume: f32 = window.iter().map(|b| b.total()).sum();

            let vpin = if total_volume > 0.0 {
                total_imbalance / total_volume
            } else {
                0.0
            };

            let timestamp = self.buckets[i].end_time;
            raw_values.push((timestamp, vpin));
        }

        if raw_values.is_empty() {
            return;
        }

        // Apply EMA smoothing to reduce noise
        let ema_periods = self.config.ema_periods;
        let alpha = if ema_periods > 0 {
            2.0 / (ema_periods as f32 + 1.0)
        } else {
            1.0 // No smoothing
        };

        let mut smoothed_values: Vec<(u64, f32)> = Vec::with_capacity(raw_values.len());
        let mut ema = raw_values[0].1;

        for (timestamp, raw_vpin) in &raw_values {
            ema = alpha * raw_vpin + (1.0 - alpha) * ema;
            smoothed_values.push((*timestamp, ema));
        }

        // Second pass: normalize to use full 0-1 range based on recent min/max
        // This makes the indicator more visually responsive
        let values_only: Vec<f32> = smoothed_values.iter().map(|(_, v)| *v).collect();
        let min_vpin = values_only.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_vpin = values_only.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let range = (max_vpin - min_vpin).max(0.01);

        for (timestamp, smoothed_vpin) in smoothed_values {
            // Normalize to 0-1 range, then scale to emphasize variations
            // Keep some baseline (don't start from 0)
            let normalized = (smoothed_vpin - min_vpin) / range;
            // Map to 0.2-0.8 range to show relative changes
            let scaled = 0.2 + (normalized * 0.6);
            // But also preserve absolute high values
            let final_vpin = scaled.max(smoothed_vpin);

            self.data.insert(timestamp, final_vpin);
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        match main_chart.basis {
            Basis::Time(_) => {}
            Basis::Tick(_) => {
                return iced::widget::center(iced::widget::text(
                    "VPIN is not available for tick charts",
                ))
                .into();
            }
        }

        if self.data.is_empty() {
            return iced::widget::center(iced::widget::text("Calculating VPIN...")).into();
        }

        let threshold = self.config.threshold;

        let tooltip = move |value: &f32, _next: Option<&f32>| {
            let pct = *value * 100.0;
            let signal = if *value > threshold {
                " [HIGH TOXICITY]"
            } else if *value > threshold * 0.8 {
                " [Elevated]"
            } else {
                ""
            };
            PlotTooltip::new(format!("VPIN: {:.1}%{}", pct, signal))
        };

        // Scale VPIN (0-1) to 0-100 for display
        let value_fn = |v: &f32| *v * 100.0;

        let plot = MtmPlot::new(value_fn, threshold * 100.0)
            .stroke_width(1.5)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }
}

impl KlineIndicatorImpl for VpinIndicator {
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
        self.kline_data.clear();

        match source {
            PlotData::TimeBased(ts) => {
                for (&time, dp) in &ts.datapoints {
                    self.kline_data.insert(time, (dp.kline.volume.0, dp.kline.volume.1));
                }
            }
            PlotData::TickBased(ta) => {
                for (idx, dp) in ta.datapoints.iter().enumerate() {
                    self.kline_data
                        .insert(idx as u64, (dp.kline.volume.0, dp.kline.volume.1));
                }
            }
        }

        self.recalculate();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for kline in klines {
            self.kline_data.insert(kline.time, (kline.volume.0, kline.volume.1));
        }
        self.recalculate();
    }

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        // Rebuild from source on trade updates
        self.rebuild_from_source(source);
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild_from_source(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.kline_data.clear();
        self.data.clear();
        self.buckets.clear();
        self.current_bucket = VolumeBucket::default();
        self.bucket_volume = 0.0;

        self.rebuild_from_source(source);
    }
}
