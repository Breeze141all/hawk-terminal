//! Net OI Long / Net OI Short Indicator
//!
//! Tracks market pressure by analyzing correlation between price changes
//! and Open Interest changes:
//!
//! **Net Longs:**
//! - Price up + OI up = Opening longs (accumulate)
//! - Price down + OI down = Closing longs (reduce)
//!
//! **Net Shorts:**
//! - Price down + OI up = Opening shorts (accumulate)
//! - Price up + OI down = Closing shorts (reduce)
//!
//! **Net Position (displayed):**
//! - Net Longs - Net Shorts
//! - Positive = Longs dominant
//! - Negative = Shorts dominant
//!
//! Data Source: bitcoincounterflow.com API (BTC only)

use crate::chart::{
    Basis, Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::{FetchCtx, KlineIndicatorImpl},
        plot::{PlotTooltip, dual_area::DualAreaPlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::adapter::bitcoincounterflow;
use exchange::fetcher::{FetchRange, NetOiInterval};
use exchange::{Kline, NetOiDataPoint, Timeframe, Trade};

use iced::widget::{center, text};
use std::{collections::BTreeMap, ops::RangeInclusive};

/// Net OI indicator state
pub struct NetOiIndicator {
    cache: Caches,
    /// Cumulative Net Longs values by timestamp
    net_longs: BTreeMap<u64, f32>,
    /// Cumulative Net Shorts values by timestamp
    net_shorts: BTreeMap<u64, f32>,
    /// Net Position = Net Longs - Net Shorts (what we display)
    pub data: BTreeMap<u64, NetOiValue>,
    /// Raw data from API
    raw_data: Vec<NetOiDataPoint>,
    /// Whether data has been fetched
    data_fetched: bool,
}

/// Combined value for display showing both components
#[derive(Clone, Copy)]
pub struct NetOiValue {
    pub net_longs: f32,
    pub net_shorts: f32,
}

impl NetOiIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            net_longs: BTreeMap::new(),
            net_shorts: BTreeMap::new(),
            data: BTreeMap::new(),
            raw_data: Vec::new(),
            data_fetched: false,
        }
    }

    /// Recalculate Net Longs and Net Shorts from raw data
    fn recalculate(&mut self) {
        self.net_longs.clear();
        self.net_shorts.clear();
        self.data.clear();

        if self.raw_data.len() < 2 {
            return;
        }

        // Sort by time
        self.raw_data.sort_by_key(|d| d.time);

        let mut cumulative_longs = 0.0f32;
        let mut cumulative_shorts = 0.0f32;

        // First point starts at 0
        let first = &self.raw_data[0];
        self.net_longs.insert(first.time, 0.0);
        self.net_shorts.insert(first.time, 0.0);
        self.data.insert(
            first.time,
            NetOiValue {
                net_longs: 0.0,
                net_shorts: 0.0,
            },
        );

        for i in 1..self.raw_data.len() {
            let prev = &self.raw_data[i - 1];
            let curr = &self.raw_data[i];

            let delta_price = curr.price - prev.price;

            // Calculate OI change in percentage
            let delta_oi_pct = if prev.open_interest != 0.0 {
                ((curr.open_interest - prev.open_interest) / prev.open_interest * 100.0) as f32
            } else {
                0.0
            };

            // Net Longs logic
            if delta_price > 0.0 && delta_oi_pct > 0.0 {
                // Price up + OI up = Opening longs
                cumulative_longs += delta_oi_pct;
            } else if delta_price < 0.0 && delta_oi_pct < 0.0 {
                // Price down + OI down = Closing longs
                cumulative_longs += delta_oi_pct; // delta_oi_pct is negative
            }

            // Net Shorts logic
            if delta_price < 0.0 && delta_oi_pct > 0.0 {
                // Price down + OI up = Opening shorts
                cumulative_shorts += delta_oi_pct;
            } else if delta_price > 0.0 && delta_oi_pct < 0.0 {
                // Price up + OI down = Closing shorts
                cumulative_shorts += delta_oi_pct; // delta_oi_pct is negative
            }

            self.net_longs.insert(curr.time, cumulative_longs);
            self.net_shorts.insert(curr.time, cumulative_shorts);
            self.data.insert(
                curr.time,
                NetOiValue {
                    net_longs: cumulative_longs,
                    net_shorts: cumulative_shorts,
                },
            );
        }

        self.cache.clear_all();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        match main_chart.basis {
            Basis::Time(timeframe) => {
                // Check if symbol is BTC
                let (symbol, _) = main_chart.ticker_info.ticker.to_full_symbol_and_type();
                if !bitcoincounterflow::is_btc_symbol(&symbol) {
                    return center(text("Net OI is only available for BTC")).into();
                }

                // Check supported timeframes
                if NetOiInterval::from_timeframe(timeframe).is_none() {
                    return center(text(format!(
                        "Net OI is not available on {} timeframe\n(Use 15m, 30m, 1h, 2h, 4h, or 1d)",
                        timeframe
                    )))
                    .into();
                }

                let (earliest, latest) = visible_range.clone().into_inner();
                if latest < earliest {
                    return iced::widget::row![].into();
                }
            }
            Basis::Tick(_) => {
                return center(text("Net OI is not available for tick charts.")).into();
            }
        }

        if self.data.is_empty() {
            return center(text("Loading Net OI data...")).into();
        }

        // Tooltip showing both values
        let tooltip = |value: &NetOiValue, _next: Option<&NetOiValue>| {
            PlotTooltip::new(format!(
                "Net Longs: {:.2}%\nNet Shorts: {:.2}%",
                value.net_longs, value.net_shorts
            ))
        };

        // Value functions for each series
        let longs_fn = |v: &NetOiValue| v.net_longs;
        let shorts_fn = |v: &NetOiValue| v.net_shorts;

        // Dual area plot: green for longs, red for shorts
        let plot = DualAreaPlot::new(longs_fn, shorts_fn)
            .with_tooltip(tooltip)
            .stroke_width(1.5)
            .with_zero_line(true)
            .fill_alpha(0.25)
            .padding(0.15);

        indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
    }

    /// Get the appropriate days parameter based on visible range
    fn get_fetch_days(timeframe: Timeframe) -> u16 {
        // Use more data for longer timeframes
        match timeframe {
            Timeframe::M15 | Timeframe::M30 => 7,
            Timeframe::H1 | Timeframe::H2 => 21,
            Timeframe::H4 => 90,
            Timeframe::D1 => 365,
            _ => 21,
        }
    }
}

impl KlineIndicatorImpl for NetOiIndicator {
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
        // Check if BTC symbol
        let (symbol, _) = ctx.main_chart.ticker_info.ticker.to_full_symbol_and_type();
        if !bitcoincounterflow::is_btc_symbol(&symbol) {
            return None;
        }

        // Check timeframe support
        let interval = NetOiInterval::from_timeframe(ctx.timeframe)?;

        // Only fetch once
        if self.data_fetched {
            return None;
        }

        let days = Self::get_fetch_days(ctx.timeframe);
        Some(FetchRange::NetOiData { days, interval })
    }

    fn rebuild_from_source(&mut self, _source: &PlotData<KlineDataPoint>) {
        // Net OI data comes from external API, not from kline source
        self.clear_all_caches();
    }

    fn on_insert_klines(&mut self, _klines: &[Kline]) {}

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        _source: &PlotData<KlineDataPoint>,
    ) {
    }

    fn on_ticksize_change(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_basis_change(&mut self, _source: &PlotData<KlineDataPoint>) {
        // Reset data on timeframe change
        self.raw_data.clear();
        self.net_longs.clear();
        self.net_shorts.clear();
        self.data.clear();
        self.data_fetched = false;
        self.cache.clear_all();
    }

    fn on_open_interest(&mut self, _data: &[exchange::OpenInterest]) {}

    fn on_funding_rates(&mut self, _rates: &[exchange::FundingRate]) {}

    fn on_spot_klines(&mut self, _klines: &[exchange::SpotKline]) {}

    fn on_net_oi_data(&mut self, data: &[NetOiDataPoint]) {
        self.raw_data.extend_from_slice(data);
        self.data_fetched = true;
        self.recalculate();
    }
}
