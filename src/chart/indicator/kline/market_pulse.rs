//! MTM Tension Index (Market Pulse) Indicator
//!
//! A composite oscillator (0-100) that aggregates volatility, volume,
//! funding rates, and basis spreads to identify market "tension" states.
//!
//! Signal Logic: A BUY/ALERT signal is generated when the index breaches
//! the upper threshold (88.86), indicating a potential reversal.

use crate::chart::{
    Basis, Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::{FetchCtx, KlineIndicatorImpl},
        plot::{PlotTooltip, mtm::MtmPlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::adapter::Exchange;
use exchange::fetcher::FetchRange;
use exchange::{FundingRate, Kline, SpotKline, Timeframe, Trade};

use iced::widget::{center, text};
use std::{collections::BTreeMap, ops::RangeInclusive};

/// Configuration for MTM Tension Index calculation
pub struct MtmConfig {
    /// Window size for rolling Z-Score calculations
    pub lookback: usize,
    /// Trigger signal when index > threshold
    pub threshold: f32,
    /// Component weights
    pub weight_volatility: f32,
    pub weight_volume: f32,
    pub weight_funding: f32,
    pub weight_basis: f32,
}

impl Default for MtmConfig {
    fn default() -> Self {
        Self {
            lookback: 23,
            threshold: 88.86,
            weight_volatility: 0.113,
            weight_volume: 0.094,
            weight_funding: 0.512,
            weight_basis: 0.281,
        }
    }
}

pub struct MarketPulseIndicator {
    cache: Caches,
    config: MtmConfig,
    /// Computed MTM index values by timestamp (stores just the index for plotting)
    pub data: BTreeMap<u64, f32>,
    /// Raw input data for calculations
    raw_volatility: BTreeMap<u64, f32>, // High - Low
    raw_volume: BTreeMap<u64, f32>,
    raw_funding: BTreeMap<u64, f32>, // Absolute funding rate
    raw_basis: BTreeMap<u64, f32>,   // Absolute basis
    /// Spot klines for basis calculation
    spot_klines: BTreeMap<u64, f32>,
    /// Funding rates data
    funding_rates: BTreeMap<u64, f32>,
    /// Futures close prices for basis calculation
    futures_closes: BTreeMap<u64, f32>,
}

impl MarketPulseIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            config: MtmConfig::default(),
            data: BTreeMap::new(),
            raw_volatility: BTreeMap::new(),
            raw_volume: BTreeMap::new(),
            raw_funding: BTreeMap::new(),
            raw_basis: BTreeMap::new(),
            spot_klines: BTreeMap::new(),
            funding_rates: BTreeMap::new(),
            futures_closes: BTreeMap::new(),
        }
    }

    /// Recalculate MTM index from raw data
    fn recalculate(&mut self) {
        self.data.clear();

        // Collect all timestamps from volatility/volume (main kline data)
        let timestamps: Vec<u64> = self.raw_volatility.keys().copied().collect();

        if timestamps.len() < self.config.lookback {
            return; // Not enough data
        }

        for (i, &time) in timestamps.iter().enumerate() {
            if i < self.config.lookback - 1 {
                continue; // Need enough history for lookback
            }

            // Calculate Z-scores for each component
            let vol_score = self.calc_zscore_score(&self.raw_volatility, &timestamps, i);
            let volume_score = self.calc_zscore_score(&self.raw_volume, &timestamps, i);
            let funding_score = self.calc_zscore_score(&self.raw_funding, &timestamps, i);
            let basis_score = self.calc_zscore_score(&self.raw_basis, &timestamps, i);

            // Weighted aggregation
            let mtm_index = self.config.weight_volatility * vol_score
                + self.config.weight_volume * volume_score
                + self.config.weight_funding * funding_score
                + self.config.weight_basis * basis_score;

            // Clamp to 0-100
            let mtm_final = mtm_index.clamp(0.0, 100.0);

            self.data.insert(time, mtm_final);
        }

        self.cache.clear_all();
    }

    /// Calculate Z-score and convert to 0-100 scale
    fn calc_zscore_score(
        &self,
        data: &BTreeMap<u64, f32>,
        timestamps: &[u64],
        current_idx: usize,
    ) -> f32 {
        let lookback = self.config.lookback;
        let start_idx = current_idx.saturating_sub(lookback - 1);

        // Collect values for lookback window
        let values: Vec<f32> = timestamps[start_idx..=current_idx]
            .iter()
            .filter_map(|t| data.get(t).copied())
            .collect();

        if values.is_empty() {
            return 50.0; // Neutral score if no data
        }

        let current_value = *values.last().unwrap_or(&0.0);

        // Calculate mean and std dev
        let n = values.len() as f32;
        let mean = values.iter().sum::<f32>() / n;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n;
        let std_dev = variance.sqrt();

        // Calculate Z-score with epsilon to avoid division by zero
        let epsilon = 1e-8;
        let z_score = (current_value - mean) / (std_dev + epsilon);

        // Convert Z-score to 0-100 scale: S = 50 + (25 * clip(Z, -3, 3))
        let z_clipped = z_score.clamp(-3.0, 3.0);
        50.0 + (25.0 * z_clipped)
    }

    /// Update raw data from klines
    fn update_from_klines(&mut self, klines: &[Kline]) {
        for kline in klines {
            // Volatility: High - Low (True Range simplified)
            let volatility = (kline.high - kline.low).to_f32();
            self.raw_volatility.insert(kline.time, volatility);

            // Volume: total volume
            let volume = kline.volume.0 + kline.volume.1;
            self.raw_volume.insert(kline.time, volume);

            // Store futures close for basis calculation
            self.futures_closes.insert(kline.time, kline.close.to_f32());
        }
    }

    /// Update basis from spot klines (basis = |futures - spot|)
    fn update_basis(&mut self) {
        for (&time, &futures_close) in &self.futures_closes {
            if let Some(&spot_close) = self.spot_klines.get(&time) {
                let basis = (futures_close - spot_close).abs();
                self.raw_basis.insert(time, basis);
            } else {
                // If no spot data, use 0 basis
                self.raw_basis.insert(time, 0.0);
            }
        }
    }

    /// Forward-fill funding rates to kline timestamps
    /// Funding rates occur every 8 hours, so we forward-fill to all kline timestamps
    fn forward_fill_funding(&mut self) {
        if self.funding_rates.is_empty() {
            // No funding data yet - use neutral value (0) which gives z-score of 0
            for &time in self.raw_volatility.keys() {
                self.raw_funding.insert(time, 0.0);
            }
            return;
        }

        let timestamps: Vec<u64> = self.raw_volatility.keys().copied().collect();

        // Use binary search for efficient forward-fill
        let mut last_funding = 0.0f32;

        for &time in &timestamps {
            // Find the most recent funding rate <= time using range query
            if let Some((&_ft, &rate)) = self.funding_rates.range(..=time).next_back() {
                last_funding = rate;
            }
            self.raw_funding.insert(time, last_funding.abs());
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        match main_chart.basis {
            Basis::Time(timeframe) => {
                let exchange = main_chart.ticker_info.exchange();
                if !Self::is_supported_exchange(exchange) {
                    return center(text(format!(
                        "Market Pulse is not available for {exchange}"
                    )))
                    .into();
                }

                if !Self::is_supported_timeframe(timeframe) {
                    return center(text(format!(
                        "Market Pulse is not available on {timeframe} timeframe"
                    )))
                    .into();
                }

                let (earliest, latest) = visible_range.clone().into_inner();
                if latest < earliest {
                    return iced::widget::row![].into();
                }
            }
            Basis::Tick(_) => {
                return center(text("Market Pulse is not available for tick charts.")).into();
            }
        }

        if self.data.is_empty() {
            return center(text("Loading Market Pulse data...")).into();
        }

        let threshold = self.config.threshold;

        let tooltip = move |value: &f32, _next: Option<&f32>| {
            let signal_text = if *value > threshold { " [ALERT]" } else { "" };
            PlotTooltip::new(format!("MTM Index: {:.2}{}", value, signal_text))
        };

        let value_fn = |v: &f32| *v;

        let plot = MtmPlot::new(value_fn, threshold)
            .stroke_width(1.5)
            .with_tooltip(tooltip);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }

    pub fn is_supported_exchange(exchange: Exchange) -> bool {
        // Only Binance linear perps for now (has funding rate API)
        exchange == Exchange::BinanceLinear
    }

    pub fn is_supported_timeframe(timeframe: Timeframe) -> bool {
        // Support 5m to 4h timeframes
        timeframe >= Timeframe::M5 && timeframe <= Timeframe::H4
    }

    /// Insert funding rate data (used via on_funding_rates trait method)
    #[allow(dead_code)]
    pub fn insert_funding_rates(&mut self, rates: &[FundingRate]) {
        for rate in rates {
            self.funding_rates.insert(rate.time, rate.rate);
        }
        self.forward_fill_funding();
        self.recalculate();
    }

    /// Insert spot kline data for basis calculation (used via on_spot_klines trait method)
    #[allow(dead_code)]
    pub fn insert_spot_klines(&mut self, klines: &[SpotKline]) {
        for kline in klines {
            self.spot_klines.insert(kline.time, kline.close);
        }
        self.update_basis();
        self.recalculate();
    }
}

impl KlineIndicatorImpl for MarketPulseIndicator {
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
        if !Self::is_supported_exchange(exchange) || !Self::is_supported_timeframe(ctx.timeframe) {
            return None;
        }

        // Secondary data requires loaded futures klines and valid timerange
        if ctx.kline_latest == 0
            || self.futures_closes.is_empty()
            || ctx.prefetch_earliest >= ctx.kline_latest
        {
            return None;
        }

        // First priority: funding rates
        if self.funding_rates.is_empty() {
            return Some(FetchRange::FundingRate(
                ctx.prefetch_earliest,
                ctx.kline_latest,
            ));
        }

        // Second priority: spot klines for basis calculation
        if self.spot_klines.is_empty() {
            return Some(FetchRange::SpotKline(
                ctx.prefetch_earliest,
                ctx.kline_latest,
            ));
        }

        None
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        // Extract klines from source
        let klines: Vec<Kline> = match source {
            PlotData::TimeBased(ts) => ts.datapoints.values().map(|dp| dp.kline).collect(),
            PlotData::TickBased(ta) => ta.datapoints.iter().map(|dp| dp.kline).collect(),
        };

        self.update_from_klines(&klines);
        self.update_basis();
        self.forward_fill_funding();
        self.recalculate();
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        self.update_from_klines(klines);
        self.update_basis();
        self.forward_fill_funding();
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
        // Clear all data and rebuild
        self.raw_volatility.clear();
        self.raw_volume.clear();
        self.raw_funding.clear();
        self.raw_basis.clear();
        self.data.clear();
        self.futures_closes.clear();

        self.rebuild_from_source(source);
    }

    fn on_open_interest(&mut self, _data: &[exchange::OpenInterest]) {
        // OI is not used in current MTM formula (weight = 0)
    }

    fn on_funding_rates(&mut self, rates: &[exchange::FundingRate]) {
        for rate in rates {
            self.funding_rates.insert(rate.time, rate.rate);
        }
        self.forward_fill_funding();
        self.recalculate();
    }

    fn on_spot_klines(&mut self, klines: &[exchange::SpotKline]) {
        for kline in klines {
            self.spot_klines.insert(kline.time, kline.close);
        }
        self.update_basis();
        self.recalculate();
    }
}
