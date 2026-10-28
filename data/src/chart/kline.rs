use exchange::{
    Exchange, Kline, Timeframe, Trade,
    util::{Price, PriceStep},
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::tpo::{SessionCluster, SessionPeriod};
use crate::aggr::time::DataPoint;

pub mod big_trades;
pub mod indicators;

pub use big_trades::BigTradesConfig;

pub use indicators::adapter::{
    datapoint_to_indicator_frame, kline_to_indicator_frame, kline_to_indicator_frame_with_context,
    plot_data_to_indicator_frames,
};
pub use indicators::composition;
pub use indicators::core::{
    IndicatorHost, IndicatorInputRequirement, IndicatorInputSource, IndicatorOutputRef,
    OpenInterestSeries, OutputChannelSpec, OutputValue, OutputValueType,
};
pub use indicators::feed_capabilities;
pub use indicators::instance::{
    IndicatorInstance, IndicatorInstanceId, IndicatorInstanceStore, IndicatorStyleOverrides,
    LineStrokeStyle, SourceBindingProblem,
};
pub use indicators::scripting::ScriptIndicatorRuntime;
pub use indicators::{
    AvailabilityContext, BaseSeriesSource, CatalogInstallError, IndicatorAuthor,
    IndicatorAvailability, IndicatorCatalog, IndicatorCatalogPaths, IndicatorColorSpec,
    IndicatorConfig, IndicatorConfigError, IndicatorDefinition, IndicatorDefinitionError,
    IndicatorFillGeometry, IndicatorFillSpec, IndicatorHistoryRequirement, IndicatorId,
    IndicatorIdError, IndicatorInputAvailability, IndicatorInputCoverage,
    IndicatorInputCoverageState, IndicatorMarkSpec, IndicatorOutputLayerSpec,
    IndicatorOverlayChannelSpec, IndicatorOverlaySource, IndicatorPanelRecipe,
    IndicatorReadoutScope, IndicatorReadoutSpec, IndicatorRegistration, IndicatorSegmentSpec,
    IndicatorSetting, IndicatorSettingChoice, IndicatorSettingControl, IndicatorSettingValue,
    IndicatorSettingsContext, IndicatorSourceInput, IndicatorUnsupportedReason, TableColumnColor,
    TableColumnFormat, TableColumnSpec, TableColumnValue, all_indicators, availability, catalog,
    declared_input_requirements, display_name, indicator_definition, initialize_indicator_catalog,
    initialize_indicator_catalog_with_paths, is_supported_for_market, panel_recipe,
    requires_trade_data, table_columns, unsupported_reason,
};

#[derive(Debug, Clone)]
pub struct KlineDataPoint {
    pub kline: Kline,
    pub footprint: KlineTrades,
    pub trades_fetched: bool,
}

impl KlineDataPoint {
    pub fn max_cluster_qty(&self, cluster_kind: ClusterKind, highest: Price, lowest: Price) -> f32 {
        match cluster_kind {
            ClusterKind::BidAsk | ClusterKind::Table => {
                self.footprint.max_qty_by(highest, lowest, f32::max)
            }
            ClusterKind::DeltaProfile => self
                .footprint
                .max_qty_by(highest, lowest, |buy, sell| (buy - sell).abs()),
            ClusterKind::VolumeProfile => {
                self.footprint
                    .max_qty_by(highest, lowest, |buy, sell| buy + sell)
            }
        }
    }

    pub fn add_trade(&mut self, trade: &Trade, step: PriceStep) {
        if self.kline.open.to_f32() == 0.0 {
            self.kline.open = trade.price;
        }
        if self.kline.high.to_f32() == 0.0 {
            self.kline.high = trade.price;
        } else {
            self.kline.high = self.kline.high.max(trade.price);
        }
        if self.kline.low.to_f32() == 0.0 {
            self.kline.low = trade.price;
        } else {
            self.kline.low = self.kline.low.min(trade.price);
        }
        self.kline.close = trade.price;

        if trade.is_sell {
            self.kline.volume.1 += trade.qty;
        } else {
            self.kline.volume.0 += trade.qty;
        }

        self.footprint.add_trade_to_nearest_bin(trade, step);
    }

    pub fn poc_price(&self) -> Option<Price> {
        self.footprint.poc_price()
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        self.footprint.set_poc_status(status);
    }

    pub fn clear_trades(&mut self) {
        self.footprint.clear();
    }

    pub fn calculate_poc(&mut self) {
        self.footprint.calculate_poc();
    }

    pub fn last_trade_time(&self) -> Option<u64> {
        self.footprint.last_trade_t()
    }

    pub fn first_trade_time(&self) -> Option<u64> {
        self.footprint.first_trade_t()
    }

    pub fn is_trades_coverage_complete(
        &self,
        interval_ms: u64,
        tick_size: PriceStep,
        now_ms: u64,
    ) -> bool {
        let total_vol = self.kline.volume.0 + self.kline.volume.1;
        if total_vol == 0.0 {
            return true;
        }
        if self.footprint.is_empty() {
            return false;
        }

        let candle_start = self.kline.time;
        let candle_end = candle_start.saturating_add(interval_ms);
        if candle_end > now_ms {
            return false;
        }

        let tolerance = (interval_ms / 10).clamp(500, 120_000).min(interval_ms / 2);
        let first_t = self.footprint.first_trade_t().unwrap_or(0);
        let last_t = self.footprint.last_trade_t().unwrap_or(0);

        let start_ok = first_t <= candle_start.saturating_add(tolerance);
        let end_ok = last_t >= candle_end.saturating_sub(tolerance);
        if !start_ok || !end_ok {
            return false;
        }

        let has_kline_extremes = self.kline.high.units > 0 && self.kline.low.units > 0;
        if !has_kline_extremes {
            return true;
        }

        let step_units = tick_size.units.max(1);
        let tolerance_units = step_units.saturating_mul(2);
        let low_bound = self
            .kline
            .low
            .round_to_step(tick_size)
            .units
            .saturating_add(tolerance_units);
        let high_bound = self
            .kline
            .high
            .round_to_step(tick_size)
            .units
            .saturating_sub(tolerance_units);

        match (self.footprint.min_price(), self.footprint.max_price()) {
            (Some(min_p), Some(max_p)) => min_p.units <= low_bound && max_p.units >= high_bound,
            _ => false,
        }
    }
}

impl DataPoint for KlineDataPoint {
    fn add_trade(&mut self, trade: &Trade, step: PriceStep) {
        self.add_trade(trade, step);
    }

    fn clear_trades(&mut self) {
        self.clear_trades();
    }

    fn last_trade_time(&self) -> Option<u64> {
        self.last_trade_time()
    }

    fn first_trade_time(&self) -> Option<u64> {
        self.first_trade_time()
    }

    fn last_price(&self) -> Price {
        self.kline.close
    }

    fn kline(&self) -> Option<&Kline> {
        Some(&self.kline)
    }

    fn value_high(&self) -> Price {
        self.kline.high
    }

    fn value_low(&self) -> Price {
        self.kline.low
    }
}

/// Volume traded at one price level of a footprint.
///
/// Quantities are summed in `f64`: a busy level can reach tens of thousands of base units,
/// where an `f32` step (~0.004 above 32k) would silently drop 0.001-size trades. Readers
/// that draw or compare get `f32` through `total_qty`/`delta_qty` or an explicit cast.
#[derive(Debug, Clone, Default)]
pub struct GroupedTrades {
    pub buy_qty: f64,
    pub sell_qty: f64,
    pub first_time: u64,
    pub last_time: u64,
    pub buy_count: usize,
    pub sell_count: usize,
}

impl GroupedTrades {
    fn new(trade: &Trade) -> Self {
        Self {
            buy_qty: if trade.is_sell {
                0.0
            } else {
                f64::from(trade.qty)
            },
            sell_qty: if trade.is_sell {
                f64::from(trade.qty)
            } else {
                0.0
            },
            first_time: trade.time,
            last_time: trade.time,
            buy_count: if trade.is_sell { 0 } else { 1 },
            sell_count: if trade.is_sell { 1 } else { 0 },
        }
    }

    fn add_trade(&mut self, trade: &Trade) {
        if trade.is_sell {
            self.sell_qty += f64::from(trade.qty);
            self.sell_count += 1;
        } else {
            self.buy_qty += f64::from(trade.qty);
            self.buy_count += 1;
        }
        self.last_time = trade.time;
    }

    pub fn total_qty(&self) -> f32 {
        (self.buy_qty + self.sell_qty) as f32
    }

    /// Computed in `f64` before narrowing, so a small delta between two large sides survives.
    pub fn delta_qty(&self) -> f32 {
        (self.buy_qty - self.sell_qty) as f32
    }
}

#[derive(Debug, Clone, Default)]
pub struct KlineTrades {
    pub trades: FxHashMap<Price, GroupedTrades>,
    pub poc: Option<PointOfControl>,
    cached_first_time: Option<u64>,
    cached_last_time: Option<u64>,
}

impl KlineTrades {
    pub fn new() -> Self {
        Self {
            trades: FxHashMap::default(),
            poc: None,
            cached_first_time: None,
            cached_last_time: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }

    pub fn len(&self) -> usize {
        self.trades.len()
    }

    pub fn first_trade_t(&self) -> Option<u64> {
        self.cached_first_time
    }

    pub fn last_trade_t(&self) -> Option<u64> {
        self.cached_last_time
    }

    pub fn trade_summary(&self) -> Option<indicator_engine::TradeAggregate> {
        if self.trades.is_empty() {
            return None;
        }

        let mut buy_count = 0u64;
        let mut sell_count = 0u64;
        let mut buy_qty = 0.0f64;
        let mut sell_qty = 0.0f64;

        for group in self.trades.values() {
            buy_count = buy_count.saturating_add(group.buy_count as u64);
            sell_count = sell_count.saturating_add(group.sell_count as u64);
            buy_qty += group.buy_qty;
            sell_qty += group.sell_qty;
        }

        Some(indicator_engine::TradeAggregate {
            buy_qty,
            sell_qty,
            buy_count,
            sell_count,
        })
    }

    pub fn min_price(&self) -> Option<Price> {
        self.trades.keys().copied().min()
    }

    pub fn max_price(&self) -> Option<Price> {
        self.trades.keys().copied().max()
    }

    /// Add trade to the bin at the step multiple computed with side-based rounding.
    /// Intended for order-book ladder/quotes; Floor for sells, ceil for buys.
    /// Introduces side bias at bin edges and should not be used for OHLC/footprint aggregation
    pub fn add_trade_to_side_bin(&mut self, trade: &Trade, step: PriceStep) {
        let price = trade.price.round_to_side_step(trade.is_sell, step);

        self.trades
            .entry(price)
            .and_modify(|group| group.add_trade(trade))
            .or_insert_with(|| GroupedTrades::new(trade));

        self.cached_first_time = Some(
            self.cached_first_time
                .map_or(trade.time, |t| t.min(trade.time)),
        );
        self.cached_last_time = Some(
            self.cached_last_time
                .map_or(trade.time, |t| t.max(trade.time)),
        );
    }

    /// Add trade to the bin at the nearest step multiple (side-agnostic).
    /// Ties (exactly half a step) round up to the higher multiple.
    /// Intended for footprint/OHLC trade aggregation
    pub fn add_trade_to_nearest_bin(&mut self, trade: &Trade, step: PriceStep) {
        let price = trade.price.round_to_step(step);

        self.trades
            .entry(price)
            .and_modify(|group| group.add_trade(trade))
            .or_insert_with(|| GroupedTrades::new(trade));

        self.cached_first_time = Some(
            self.cached_first_time
                .map_or(trade.time, |t| t.min(trade.time)),
        );
        self.cached_last_time = Some(
            self.cached_last_time
                .map_or(trade.time, |t| t.max(trade.time)),
        );
    }

    pub fn max_qty_by<F>(&self, highest: Price, lowest: Price, f: F) -> f32
    where
        F: Fn(f32, f32) -> f32,
    {
        let mut max_qty: f32 = 0.0;
        for (price, group) in &self.trades {
            if *price >= lowest && *price <= highest {
                max_qty = max_qty.max(f(group.buy_qty as f32, group.sell_qty as f32));
            }
        }
        max_qty
    }

    pub fn calculate_poc(&mut self) {
        if self.trades.is_empty() {
            return;
        }

        let mut max_volume = 0.0;
        let mut poc_price = Price::from_f32(0.0);

        for (price, group) in &self.trades {
            let total_volume = group.total_qty();
            if total_volume > max_volume {
                max_volume = total_volume;
                poc_price = *price;
            }
        }

        self.poc = Some(PointOfControl {
            price: poc_price,
            volume: max_volume,
            status: NPoc::default(),
        });
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        if let Some(poc) = &mut self.poc {
            poc.status = status;
        }
    }

    pub fn poc_price(&self) -> Option<Price> {
        self.poc.map(|poc| poc.price)
    }

    pub fn clear(&mut self) {
        self.trades.clear();
        self.poc = None;
        self.cached_first_time = None;
        self.cached_last_time = None;
    }

    /// Generates synthetic / simulated footprint clusters for a Kline when historical raw trades
    /// are unavailable (e.g. during historical Market Replay).
    /// Distributes buy and sell volumes across price levels using a volume profile centered on the body/VWAP,
    /// with delta distribution reflecting buy/sell aggression.
    pub fn simulate_from_kline(kline: &Kline, step: PriceStep) -> Self {
        let mut footprint = Self::new();
        let total_buy = kline.volume.0;
        let total_sell = kline.volume.1;
        let total_vol = total_buy + total_sell;
        if total_vol <= 0.0 {
            return footprint;
        }

        let step_units = step.units.max(1);
        let low_binned = (kline.low.units / step_units) * step_units;
        let high_binned = ((kline.high.units + step_units - 1) / step_units) * step_units;
        let count = ((high_binned - low_binned) / step_units + 1).clamp(1, 500) as usize;

        let mut prices = Vec::with_capacity(count);
        let mut curr_units = low_binned;
        while curr_units <= high_binned && prices.len() < 500 {
            prices.push(Price { units: curr_units });
            curr_units += step_units;
        }
        if prices.is_empty() {
            prices.push(kline.low.round_to_step(step));
        }

        let center_units = (kline.open.units + kline.close.units) / 2;
        let span_units = (high_binned - low_binned).max(step_units) as f32;
        let sigma = (span_units / 3.0).max(step_units as f32);

        let mut weights: Vec<f32> = prices
            .iter()
            .map(|p| {
                let diff = (p.units - center_units) as f32;
                (-0.5 * (diff / sigma).powi(2)).exp()
            })
            .collect();
        let sum_w: f32 = weights.iter().sum();
        let norm_factor = if sum_w > 0.0 {
            1.0 / sum_w
        } else {
            1.0 / weights.len() as f32
        };
        for w in &mut weights {
            *w *= norm_factor;
        }

        let overall_buy_ratio = (total_buy / total_vol).clamp(0.05, 0.95);
        let mut raw_buys = Vec::with_capacity(prices.len());
        let mut raw_sells = Vec::with_capacity(prices.len());

        for (i, p) in prices.iter().enumerate() {
            let diff = (p.units - center_units) as f32;
            let p_norm = (diff / span_units).clamp(-0.5, 0.5);
            let level_buy_ratio = (overall_buy_ratio + p_norm * 0.3).clamp(0.05, 0.95);
            let level_vol = total_vol * weights[i];
            raw_buys.push(level_vol * level_buy_ratio);
            raw_sells.push(level_vol * (1.0 - level_buy_ratio));
        }

        let sum_raw_buy: f32 = raw_buys.iter().sum();
        let sum_raw_sell: f32 = raw_sells.iter().sum();
        let buy_scale = if sum_raw_buy > 0.0 {
            total_buy / sum_raw_buy
        } else {
            1.0
        };
        let sell_scale = if sum_raw_sell > 0.0 {
            total_sell / sum_raw_sell
        } else {
            1.0
        };

        for (i, p) in prices.iter().enumerate() {
            let level_buy = raw_buys[i] * buy_scale;
            let level_sell = raw_sells[i] * sell_scale;

            let group = footprint.trades.entry(*p).or_default();
            group.buy_qty = f64::from(level_buy);
            group.sell_qty = f64::from(level_sell);
            group.first_time = kline.time;
            group.last_time = kline.time;
            group.buy_count =
                (level_buy * 5.0).max(if level_buy > 0.0 { 1.0 } else { 0.0 }) as usize;
            group.sell_count =
                (level_sell * 5.0).max(if level_sell > 0.0 { 1.0 } else { 0.0 }) as usize;
        }

        footprint.calculate_poc();
        footprint.cached_first_time = Some(kline.time);
        footprint.cached_last_time = Some(kline.time);
        footprint
    }
}

const fn default_true() -> bool {
    true
}

const fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum TpoTickStep {
    #[default]
    Auto,
    X1,
    X2,
    X5,
    X10,
    X25,
    X50,
    X100,
}

impl TpoTickStep {
    pub const ALL: &'static [Self] = &[
        Self::Auto,
        Self::X1,
        Self::X2,
        Self::X5,
        Self::X10,
        Self::X25,
        Self::X50,
        Self::X100,
    ];

    pub fn multiplier(&self) -> u32 {
        match self {
            Self::Auto => 0,
            Self::X1 => 1,
            Self::X2 => 2,
            Self::X5 => 5,
            Self::X10 => 10,
            Self::X25 => 25,
            Self::X50 => 50,
            Self::X100 => 100,
        }
    }
}

impl std::fmt::Display for TpoTickStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "Auto"),
            Self::X1 => write!(f, "1x (Raw Tick)"),
            Self::X2 => write!(f, "2x"),
            Self::X5 => write!(f, "5x"),
            Self::X10 => write!(f, "10x"),
            Self::X25 => write!(f, "25x"),
            Self::X50 => write!(f, "50x"),
            Self::X100 => write!(f, "100x"),
        }
    }
}

/// View modes supported by TPO charting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    CandlesOnly,
    #[serde(alias = "PureTpo")]
    TpoOnly,
    Combined,
}

impl ViewMode {
    #[allow(non_upper_case_globals)]
    pub const PureTpo: ViewMode = ViewMode::TpoOnly;
    pub const PURE_TPO: ViewMode = ViewMode::TpoOnly;

    #[inline(always)]
    pub fn is_pure_tpo(self) -> bool {
        matches!(self, ViewMode::TpoOnly)
    }

    #[inline(always)]
    pub fn should_render_candles(self) -> bool {
        matches!(self, ViewMode::CandlesOnly | ViewMode::Combined)
    }

    #[inline(always)]
    pub fn should_render_tpo(self) -> bool {
        matches!(self, ViewMode::TpoOnly | ViewMode::Combined)
    }

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::CandlesOnly => "Candles",
            ViewMode::TpoOnly => "TPO",
            ViewMode::Combined => "Combined",
        }
    }
}

pub fn default_aggregate_exchanges() -> Vec<Exchange> {
    vec![
        Exchange::BinanceLinear,
        Exchange::BybitLinear,
        Exchange::OkexLinear,
        Exchange::HyperliquidLinear,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum KlineChartKind {
    #[default]
    Candles,
    Footprint {
        clusters: ClusterKind,
        #[serde(default)]
        scaling: ClusterScaling,
        studies: Vec<FootprintStudy>,
        #[serde(default)]
        show_bottom_volume: bool,
        #[serde(default)]
        aggregate: bool,
        #[serde(default = "default_aggregate_exchanges")]
        aggregate_exchanges: Vec<Exchange>,
        #[serde(default)]
        history_lookback_days: Option<u32>,
    },
    Tpo {
        #[serde(default = "default_true")]
        show_candles: bool,
        #[serde(default = "default_true")]
        show_letters: bool,
        #[serde(default = "default_true")]
        show_ib: bool,
        #[serde(default = "default_true")]
        show_va: bool,
        #[serde(default = "default_true")]
        show_poc: bool,
        #[serde(default = "default_true")]
        show_single_prints: bool,
        #[serde(default)]
        tick_step: TpoTickStep,
        #[serde(default)]
        period: SessionPeriod,
        #[serde(default)]
        clusters: Vec<SessionCluster>,
        #[serde(default)]
        split_sessions: Vec<i64>,
        #[serde(default)]
        color_scheme: TpoColorScheme,
        #[serde(default)]
        ib_color: TpoElementColor,
        #[serde(default)]
        poc_color: TpoElementColor,
        #[serde(default)]
        single_prints_color: TpoElementColor,
    },
}

impl KlineChartKind {
    pub fn view_mode(&self) -> ViewMode {
        match self {
            KlineChartKind::Candles => ViewMode::CandlesOnly,
            KlineChartKind::Footprint { .. } => ViewMode::CandlesOnly,
            KlineChartKind::Tpo { show_candles, .. } => {
                if *show_candles {
                    ViewMode::Combined
                } else {
                    ViewMode::TpoOnly
                }
            }
        }
    }

    /// Effective aggregation period honoring the configured period for TPO.
    pub fn effective_tpo_period(&self) -> SessionPeriod {
        match self {
            KlineChartKind::Tpo { period, .. } => *period,
            _ => SessionPeriod::Daily,
        }
    }

    pub fn history_lookback_days(&self) -> Option<u32> {
        match self {
            KlineChartKind::Footprint {
                history_lookback_days,
                ..
            } => *history_lookback_days,
            _ => None,
        }
    }

    pub fn set_history_lookback_days(&mut self, days: Option<u32>) {
        if let KlineChartKind::Footprint {
            history_lookback_days,
            ..
        } = self
        {
            *history_lookback_days = days;
        }
    }

    pub fn effective_history_lookback_days(&self) -> u32 {
        self.history_lookback_days()
            .unwrap_or_else(exchange::fetcher::trade_history_lookback_days)
    }

    pub fn min_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 0.1,
            KlineChartKind::Candles => 0.6,
            KlineChartKind::Tpo { .. } => 0.1,
        }
    }

    pub fn max_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 4.0,
            KlineChartKind::Candles => 2.5,
            KlineChartKind::Tpo { .. } => 2.0,
        }
    }

    pub fn max_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 2500.0,
            KlineChartKind::Candles => 16.0,
            KlineChartKind::Tpo { .. } => 120.0,
        }
    }

    pub fn min_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 2.0,
            KlineChartKind::Candles => 1.0,
            KlineChartKind::Tpo { .. } => 1.0,
        }
    }

    pub fn max_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 90.0,
            KlineChartKind::Candles => 8.0,
            KlineChartKind::Tpo { .. } => 30.0,
        }
    }

    pub fn min_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 1.0,
            KlineChartKind::Candles => 0.001,
            KlineChartKind::Tpo { .. } => 0.0001,
        }
    }

    pub fn default_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 80.0,
            KlineChartKind::Candles => 4.0,
            KlineChartKind::Tpo { .. } => 8.0,
        }
    }

    pub fn allows_indicator(&self, indicator: super::indicator::KlineIndicator) -> bool {
        !matches!(
            (self, indicator),
            (
                KlineChartKind::Candles,
                super::indicator::KlineIndicator::BarAnalysis
            )
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum ClusterKind {
    #[default]
    BidAsk,
    VolumeProfile,
    DeltaProfile,
    Table,
}

impl ClusterKind {
    pub const ALL: [ClusterKind; 4] = [
        ClusterKind::BidAsk,
        ClusterKind::VolumeProfile,
        ClusterKind::DeltaProfile,
        ClusterKind::Table,
    ];

    pub fn min_footprint_width(self) -> f32 {
        match self {
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => 50.0,
            ClusterKind::BidAsk => 80.0,
            ClusterKind::Table => 100.0,
        }
    }
}

impl std::fmt::Display for ClusterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterKind::BidAsk => write!(f, "Bid/Ask"),
            ClusterKind::VolumeProfile => write!(f, "Volume Profile"),
            ClusterKind::DeltaProfile => write!(f, "Delta Profile"),
            ClusterKind::Table => write!(f, "Table"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FootprintSummary {
    pub buy: f32,
    pub sell: f32,
    pub total: f32,
    pub delta: f32,
    pub delta_pct: f64,
}

impl FootprintSummary {
    pub fn new(buy: f32, sell: f32) -> Self {
        let total = buy + sell;
        let delta = buy - sell;
        let total_f = total as f64;
        let delta_pct = if total_f > 0.0 {
            (delta as f64 / total_f) * 100.0
        } else {
            0.0
        };

        Self {
            buy,
            sell,
            total,
            delta,
            delta_pct,
        }
    }

    pub fn from_trades(footprint: &KlineTrades) -> Option<Self> {
        if footprint.trades.is_empty() {
            return None;
        }

        let (buy, sell) = footprint
            .trades
            .values()
            .fold((0.0f64, 0.0f64), |(buy, sell), group| {
                (buy + group.buy_qty, sell + group.sell_qty)
            });

        Some(Self::new(buy as f32, sell as f32))
    }
}

fn default_new_longs_color() -> TpoElementColor {
    TpoElementColor::Green
}

fn default_new_shorts_color() -> TpoElementColor {
    TpoElementColor::Red
}

fn default_long_forced_close_color() -> TpoElementColor {
    TpoElementColor::Magenta
}

fn default_short_forced_close_color() -> TpoElementColor {
    TpoElementColor::Yellow
}

fn default_rolling_vwap_hours() -> u32 {
    24
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct PositionFlowColors {
    #[serde(default = "default_new_longs_color")]
    pub new_longs: TpoElementColor,
    #[serde(default = "default_new_shorts_color")]
    pub new_shorts: TpoElementColor,
    #[serde(default = "default_long_forced_close_color")]
    pub long_forced_close: TpoElementColor,
    #[serde(default = "default_short_forced_close_color")]
    pub short_forced_close: TpoElementColor,
}

impl Default for PositionFlowColors {
    fn default() -> Self {
        Self {
            new_longs: default_new_longs_color(),
            new_shorts: default_new_shorts_color(),
            long_forced_close: default_long_forced_close_color(),
            short_forced_close: default_short_forced_close_color(),
        }
    }
}

impl PositionFlowColors {
    pub fn new_longs_rgb(&self) -> [u8; 3] {
        self.new_longs.to_rgb().unwrap_or([0, 220, 130])
    }

    pub fn new_shorts_rgb(&self) -> [u8; 3] {
        self.new_shorts.to_rgb().unwrap_or([255, 60, 60])
    }

    pub fn long_forced_close_rgb(&self) -> [u8; 3] {
        self.long_forced_close.to_rgb().unwrap_or([240, 65, 180])
    }

    pub fn short_forced_close_rgb(&self) -> [u8; 3] {
        self.short_forced_close.to_rgb().unwrap_or([255, 220, 40])
    }
}

fn default_rolling_vwap_7d_color() -> TpoElementColor {
    TpoElementColor::Cyan
}

fn default_rolling_vwap_30d_color() -> TpoElementColor {
    TpoElementColor::Yellow
}

fn default_rolling_vwap_90d_color() -> TpoElementColor {
    TpoElementColor::Orange
}

fn default_rolling_vwap_365d_color() -> TpoElementColor {
    TpoElementColor::Magenta
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct RollingVwapColors {
    #[serde(default = "default_rolling_vwap_7d_color")]
    pub d7: TpoElementColor,
    #[serde(default = "default_rolling_vwap_30d_color")]
    pub d30: TpoElementColor,
    #[serde(default = "default_rolling_vwap_90d_color")]
    pub d90: TpoElementColor,
    #[serde(default = "default_rolling_vwap_365d_color")]
    pub d365: TpoElementColor,
}

impl Default for RollingVwapColors {
    fn default() -> Self {
        Self {
            d7: default_rolling_vwap_7d_color(),
            d30: default_rolling_vwap_30d_color(),
            d90: default_rolling_vwap_90d_color(),
            d365: default_rolling_vwap_365d_color(),
        }
    }
}

impl RollingVwapColors {
    pub fn d7_rgb(&self) -> [u8; 3] {
        self.d7.to_rgb().unwrap_or([0, 230, 255])
    }

    pub fn d30_rgb(&self) -> [u8; 3] {
        self.d30.to_rgb().unwrap_or([255, 214, 0])
    }

    pub fn d90_rgb(&self) -> [u8; 3] {
        self.d90.to_rgb().unwrap_or([255, 115, 26])
    }

    pub fn d365_rgb(&self) -> [u8; 3] {
        self.d365.to_rgb().unwrap_or([217, 38, 242])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub position_flow_colors: PositionFlowColors,
    #[serde(default)]
    pub rolling_vwap_colors: RollingVwapColors,
    #[serde(default = "default_rolling_vwap_hours")]
    pub rolling_vwap_window_hours: u32,
    #[serde(default = "default_true")]
    pub rolling_vwap_show_7d: bool,
    #[serde(default = "default_true")]
    pub rolling_vwap_show_30d: bool,
    #[serde(default = "default_true")]
    pub rolling_vwap_show_90d: bool,
    #[serde(default = "default_true")]
    pub rolling_vwap_show_365d: bool,
    #[serde(default = "default_false")]
    pub rolling_vwap_show_panel: bool,
    #[serde(default = "default_true")]
    pub liq_show_bands: bool,
    #[serde(default = "default_true")]
    pub liq_show_histogram: bool,
    #[serde(default = "default_true")]
    pub magnet_mode: bool,
    #[serde(default = "default_true")]
    pub alert_drag_handle_only: bool,
    #[serde(default = "default_false")]
    pub show_footprint_summary: bool,
    #[serde(default = "default_false")]
    pub data_labels_always_visible: bool,
}

static USER_DEFAULT_CONFIG: std::sync::LazyLock<std::sync::RwLock<Option<Config>>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(None));

pub fn set_user_default_kline_config(config: Config) {
    if let Ok(mut lock) = USER_DEFAULT_CONFIG.write() {
        *lock = Some(config);
    }
}

pub fn user_default_kline_config() -> Option<Config> {
    USER_DEFAULT_CONFIG.read().ok().and_then(|lock| *lock)
}

impl Config {
    pub fn factory_default() -> Self {
        Self {
            position_flow_colors: PositionFlowColors::default(),
            rolling_vwap_colors: RollingVwapColors::default(),
            rolling_vwap_window_hours: default_rolling_vwap_hours(),
            rolling_vwap_show_7d: true,
            rolling_vwap_show_30d: true,
            rolling_vwap_show_90d: true,
            rolling_vwap_show_365d: true,
            rolling_vwap_show_panel: false,
            liq_show_bands: true,
            liq_show_histogram: true,
            magnet_mode: true,
            alert_drag_handle_only: true,
            show_footprint_summary: false,
            data_labels_always_visible: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        user_default_kline_config().unwrap_or_else(Self::factory_default)
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum ClusterScaling {
    #[default]
    /// Scale based on the maximum quantity in the visible range.
    VisibleRange,
    /// Blend global VisibleRange and per-cluster Individual using a weight in [0.0, 1.0].
    /// weight = fraction of global contribution (1.0 == all-global, 0.0 == all-individual).
    Hybrid { weight: f32 },
    /// Scale based only on the maximum quantity inside the datapoint (per-candle).
    Datapoint,
}

impl ClusterScaling {
    pub const ALL: [ClusterScaling; 3] = [
        ClusterScaling::VisibleRange,
        ClusterScaling::Hybrid { weight: 0.2 },
        ClusterScaling::Datapoint,
    ];
}

impl std::fmt::Display for ClusterScaling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterScaling::VisibleRange => write!(f, "Visible Range"),
            ClusterScaling::Hybrid { weight } => write!(f, "Hybrid (weight: {:.2})", weight),
            ClusterScaling::Datapoint => write!(f, "Per-candle"),
        }
    }
}

impl std::cmp::Eq for ClusterScaling {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum ClusterSearchSide {
    #[default]
    Both,
    BuyOnly,
    SellOnly,
}

impl ClusterSearchSide {
    pub const ALL: [ClusterSearchSide; 3] = [
        ClusterSearchSide::Both,
        ClusterSearchSide::BuyOnly,
        ClusterSearchSide::SellOnly,
    ];
}

impl std::fmt::Display for ClusterSearchSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterSearchSide::Both => write!(f, "Both"),
            ClusterSearchSide::BuyOnly => write!(f, "Buy Only"),
            ClusterSearchSide::SellOnly => write!(f, "Sell Only"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum HighlightStyle {
    #[default]
    Border,
    Fill,
    Circle,
    Ring,
    Concentric,
    Triangle,
    Square,
    ClassicCircle,
    ClassicTriangle,
    ClassicSquare,
}

impl HighlightStyle {
    pub const ALL: [HighlightStyle; 10] = [
        HighlightStyle::Border,
        HighlightStyle::Fill,
        HighlightStyle::Circle,
        HighlightStyle::Ring,
        HighlightStyle::Concentric,
        HighlightStyle::Triangle,
        HighlightStyle::Square,
        HighlightStyle::ClassicCircle,
        HighlightStyle::ClassicTriangle,
        HighlightStyle::ClassicSquare,
    ];
}

impl std::fmt::Display for HighlightStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightStyle::Border => write!(f, "Border"),
            HighlightStyle::Fill => write!(f, "Fill"),
            HighlightStyle::Circle => write!(f, "Circle (Bubble)"),
            HighlightStyle::Ring => write!(f, "Ring"),
            HighlightStyle::Concentric => write!(f, "Concentric"),
            HighlightStyle::Triangle => write!(f, "Triangle (Styled)"),
            HighlightStyle::Square => write!(f, "Square (Styled)"),
            HighlightStyle::ClassicCircle => write!(f, "Dot (Classic)"),
            HighlightStyle::ClassicTriangle => write!(f, "Triangle (Classic)"),
            HighlightStyle::ClassicSquare => write!(f, "Square (Classic)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum HighlightColor {
    #[default]
    Amber,
    Cyan,
    Magenta,
    Green,
    Red,
    White,
    Purple,
    Orange,
    Custom([u8; 3]),
}

impl HighlightColor {
    pub const ALL: [HighlightColor; 9] = [
        HighlightColor::Amber,
        HighlightColor::Cyan,
        HighlightColor::Magenta,
        HighlightColor::Green,
        HighlightColor::Red,
        HighlightColor::White,
        HighlightColor::Purple,
        HighlightColor::Orange,
        HighlightColor::Custom([255, 180, 0]),
    ];

    pub fn to_rgb(self) -> [f32; 3] {
        match self {
            HighlightColor::Amber => [1.0, 0.75, 0.0],
            HighlightColor::Cyan => [0.0, 0.88, 1.0],
            HighlightColor::Magenta => [1.0, 0.20, 0.80],
            HighlightColor::Green => [0.15, 0.85, 0.35],
            HighlightColor::Red => [1.0, 0.25, 0.25],
            HighlightColor::White => [1.0, 1.0, 1.0],
            HighlightColor::Purple => [0.68, 0.36, 0.96],
            HighlightColor::Orange => [1.0, 0.50, 0.05],
            HighlightColor::Custom([r, g, b]) => {
                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
            }
        }
    }
}

impl std::fmt::Display for HighlightColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightColor::Amber => write!(f, "Amber"),
            HighlightColor::Cyan => write!(f, "Cyan"),
            HighlightColor::Magenta => write!(f, "Magenta"),
            HighlightColor::Green => write!(f, "Green"),
            HighlightColor::Red => write!(f, "Red"),
            HighlightColor::White => write!(f, "White"),
            HighlightColor::Purple => write!(f, "Purple"),
            HighlightColor::Orange => write!(f, "Orange"),
            HighlightColor::Custom([r, g, b]) => write!(f, "Custom RGB ({r}, {g}, {b})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TpoColorScheme {
    #[default]
    Classic,
    Theme,
    Cyan,
    Emerald,
    Amber,
    Purple,
    Red,
    Blue,
    Monochrome,
    Custom([u8; 3]),
}

impl TpoColorScheme {
    pub const ALL: [TpoColorScheme; 10] = [
        TpoColorScheme::Classic,
        TpoColorScheme::Theme,
        TpoColorScheme::Cyan,
        TpoColorScheme::Emerald,
        TpoColorScheme::Amber,
        TpoColorScheme::Purple,
        TpoColorScheme::Red,
        TpoColorScheme::Blue,
        TpoColorScheme::Monochrome,
        TpoColorScheme::Custom([32, 164, 243]),
    ];

    pub fn label(self) -> String {
        match self {
            Self::Classic => "Classic (Rainbow)".to_string(),
            Self::Theme => "Match Theme".to_string(),
            Self::Cyan => "Neon Cyan".to_string(),
            Self::Emerald => "Emerald Green".to_string(),
            Self::Amber => "Amber Gold".to_string(),
            Self::Purple => "Violet Purple".to_string(),
            Self::Red => "Coral Red".to_string(),
            Self::Blue => "Sky Blue".to_string(),
            Self::Monochrome => "Monochrome".to_string(),
            Self::Custom(_) => "Custom RGB".to_string(),
        }
    }

    pub fn custom_rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::Custom(rgb) => Some(rgb),
            _ => None,
        }
    }
}

impl std::fmt::Display for TpoColorScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TpoElementColor {
    #[default]
    Auto,
    Theme,
    Amber,
    Yellow,
    Orange,
    Red,
    Green,
    Cyan,
    Blue,
    Purple,
    Magenta,
    White,
    Custom([u8; 3]),
}

impl TpoElementColor {
    pub const ALL: [TpoElementColor; 13] = [
        TpoElementColor::Auto,
        TpoElementColor::Theme,
        TpoElementColor::Amber,
        TpoElementColor::Yellow,
        TpoElementColor::Orange,
        TpoElementColor::Red,
        TpoElementColor::Green,
        TpoElementColor::Cyan,
        TpoElementColor::Blue,
        TpoElementColor::Purple,
        TpoElementColor::Magenta,
        TpoElementColor::White,
        TpoElementColor::Custom([255, 180, 0]),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Default)",
            Self::Theme => "Match Theme",
            Self::Amber => "Amber",
            Self::Yellow => "Yellow",
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Cyan => "Cyan",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Magenta => "Magenta",
            Self::White => "White",
            Self::Custom(_) => "Custom RGB",
        }
    }

    pub fn to_rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::Amber => Some([250, 188, 46]),
            Self::Yellow => Some([255, 220, 40]),
            Self::Orange => Some([255, 130, 30]),
            Self::Red => Some([235, 75, 75]),
            Self::Green => Some([40, 210, 110]),
            Self::Cyan => Some([20, 215, 235]),
            Self::Blue => Some([50, 140, 245]),
            Self::Purple => Some([170, 85, 225]),
            Self::Magenta => Some([240, 65, 180]),
            Self::White => Some([240, 240, 240]),
            Self::Custom(rgb) => Some(rgb),
            Self::Auto | Self::Theme => None,
        }
    }

    pub fn custom_rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::Custom(rgb) => Some(rgb),
            _ => None,
        }
    }
}

impl std::fmt::Display for TpoElementColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

const fn default_cluster_search_id() -> u8 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub enum FootprintStudy {
    NPoC {
        lookback: usize,
    },
    Imbalance {
        threshold: usize,
        color_scale: Option<usize>,
        ignore_zeros: bool,
    },
    ClusterSearch {
        #[serde(default = "default_cluster_search_id")]
        id: u8,
        min_volume: f32,
        min_delta: f32,
        side: ClusterSearchSide,
        style: HighlightStyle,
        color: HighlightColor,
    },
    BigTrades {
        min_usd: f64,
        min_radius: f32,
        max_radius: f32,
        scale: f32,
        opacity: f32,
    },
}

impl Eq for FootprintStudy {}

impl FootprintStudy {
    pub fn is_same_type(&self, other: &Self) -> bool {
        match (self, other) {
            (FootprintStudy::NPoC { .. }, FootprintStudy::NPoC { .. }) => true,
            (FootprintStudy::Imbalance { .. }, FootprintStudy::Imbalance { .. }) => true,
            (
                FootprintStudy::ClusterSearch { id: a, .. },
                FootprintStudy::ClusterSearch { id: b, .. },
            ) => a == b,
            (FootprintStudy::BigTrades { .. }, FootprintStudy::BigTrades { .. }) => true,
            _ => false,
        }
    }
}

impl FootprintStudy {
    pub const ALL: [FootprintStudy; 7] = [
        FootprintStudy::NPoC { lookback: 80 },
        FootprintStudy::Imbalance {
            threshold: 200,
            color_scale: Some(400),
            ignore_zeros: true,
        },
        FootprintStudy::BigTrades {
            min_usd: 100_000.0,
            min_radius: 4.0,
            max_radius: 28.0,
            scale: 5.0,
            opacity: 0.65,
        },
        FootprintStudy::ClusterSearch {
            id: 1,
            min_volume: 100.0,
            min_delta: 50.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Border,
            color: HighlightColor::Amber,
        },
        FootprintStudy::ClusterSearch {
            id: 2,
            min_volume: 300.0,
            min_delta: 150.0,
            side: ClusterSearchSide::BuyOnly,
            style: HighlightStyle::Circle,
            color: HighlightColor::Cyan,
        },
        FootprintStudy::ClusterSearch {
            id: 3,
            min_volume: 500.0,
            min_delta: 250.0,
            side: ClusterSearchSide::SellOnly,
            style: HighlightStyle::Triangle,
            color: HighlightColor::Red,
        },
        FootprintStudy::ClusterSearch {
            id: 4,
            min_volume: 1000.0,
            min_delta: 500.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Square,
            color: HighlightColor::White,
        },
    ];
}

impl std::fmt::Display for FootprintStudy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FootprintStudy::NPoC { .. } => write!(f, "Naked Point of Control"),
            FootprintStudy::Imbalance { .. } => write!(f, "Imbalance"),
            FootprintStudy::ClusterSearch { id, .. } => write!(f, "Cluster Search {}", id),
            FootprintStudy::BigTrades { .. } => write!(f, "Big Trades"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PointOfControl {
    pub price: Price,
    pub volume: f32,
    pub status: NPoc,
}

impl Default for PointOfControl {
    fn default() -> Self {
        Self {
            price: Price::from_f32(0.0),
            volume: 0.0,
            status: NPoc::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NPoc {
    #[default]
    None,
    Naked,
    Filled {
        at: u64,
    },
}

impl NPoc {
    pub fn filled(&mut self, at: u64) {
        *self = NPoc::Filled { at };
    }

    pub fn unfilled(&mut self) {
        *self = NPoc::Naked;
    }
}

pub const FOOTPRINT_CACHE_MAGIC: &[u8; 4] = b"FPCB";

pub fn encode_footprint_datapoints(datapoints: &[(u64, KlineDataPoint)]) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(FOOTPRINT_CACHE_MAGIC);
    raw.extend_from_slice(&1u16.to_le_bytes());
    raw.extend_from_slice(&(datapoints.len() as u32).to_le_bytes());

    for (timestamp, dp) in datapoints {
        raw.extend_from_slice(&timestamp.to_le_bytes());
        raw.extend_from_slice(&dp.kline.time.to_le_bytes());
        raw.extend_from_slice(&dp.kline.open.units.to_le_bytes());
        raw.extend_from_slice(&dp.kline.high.units.to_le_bytes());
        raw.extend_from_slice(&dp.kline.low.units.to_le_bytes());
        raw.extend_from_slice(&dp.kline.close.units.to_le_bytes());
        raw.extend_from_slice(&dp.kline.volume.0.to_le_bytes());
        raw.extend_from_slice(&dp.kline.volume.1.to_le_bytes());
        raw.push(if dp.trades_fetched { 1 } else { 0 });

        raw.extend_from_slice(&dp.footprint.first_trade_t().unwrap_or(0).to_le_bytes());
        raw.extend_from_slice(&dp.footprint.last_trade_t().unwrap_or(0).to_le_bytes());

        if let Some(poc) = &dp.footprint.poc {
            raw.push(1);
            raw.extend_from_slice(&poc.price.units.to_le_bytes());
            raw.extend_from_slice(&poc.volume.to_le_bytes());
            match poc.status {
                NPoc::None => {
                    raw.push(0);
                    raw.extend_from_slice(&0u64.to_le_bytes());
                }
                NPoc::Naked => {
                    raw.push(1);
                    raw.extend_from_slice(&0u64.to_le_bytes());
                }
                NPoc::Filled { at } => {
                    raw.push(2);
                    raw.extend_from_slice(&at.to_le_bytes());
                }
            }
        } else {
            raw.push(0);
        }

        let num_clusters = dp.footprint.trades.len();
        raw.extend_from_slice(&(num_clusters as u32).to_le_bytes());

        for (price, group) in &dp.footprint.trades {
            raw.extend_from_slice(&price.units.to_le_bytes());
            // The cache format stores 4-byte f32 quantities; keep it so existing caches load.
            raw.extend_from_slice(&(group.buy_qty as f32).to_le_bytes());
            raw.extend_from_slice(&(group.sell_qty as f32).to_le_bytes());
            raw.extend_from_slice(&group.first_time.to_le_bytes());
            raw.extend_from_slice(&group.last_time.to_le_bytes());
            raw.extend_from_slice(&(group.buy_count as u32).to_le_bytes());
            raw.extend_from_slice(&(group.sell_count as u32).to_le_bytes());
        }
    }

    lz4_flex::compress_prepend_size(&raw)
}

pub fn decode_footprint_datapoints(
    compressed: &[u8],
) -> Result<Vec<(u64, KlineDataPoint)>, String> {
    let decompressed = lz4_flex::decompress_size_prepended(compressed)
        .map_err(|e| format!("Decompress footprint cache failed: {e}"))?;

    if decompressed.len() < 10 {
        return Err("Footprint cache data too short".into());
    }

    if &decompressed[0..4] != FOOTPRINT_CACHE_MAGIC {
        return Err("Footprint cache magic mismatch".into());
    }

    let _version = u16::from_le_bytes(decompressed[4..6].try_into().unwrap());
    let count = u32::from_le_bytes(decompressed[6..10].try_into().unwrap()) as usize;

    const MIN_CANDLE_HEADER_BYTES: usize = 66;
    let max_possible_count = decompressed.len().saturating_sub(10) / MIN_CANDLE_HEADER_BYTES;
    if count > max_possible_count {
        return Err(format!(
            "Corrupt footprint cache: count {count} exceeds buffer bounds"
        ));
    }

    let mut cursor = 10;
    let mut datapoints = Vec::with_capacity(count);

    for _ in 0..count {
        if cursor + 8 + 8 + 8 + 8 + 8 + 8 + 4 + 4 + 1 + 8 + 8 + 1 > decompressed.len() {
            return Err("Unexpected EOF in candle header".into());
        }

        let timestamp = u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let kline_time = u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let open_units = i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let high_units = i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let low_units = i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let close_units = i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let vol_buy = f32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        let vol_sell = f32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        let trades_fetched = decompressed[cursor] != 0;
        cursor += 1;

        let first_t = u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let last_t = u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;

        let has_poc = decompressed[cursor] != 0;
        cursor += 1;

        let poc = if has_poc {
            if cursor + 8 + 4 + 1 + 8 > decompressed.len() {
                return Err("Unexpected EOF in POC".into());
            }
            let poc_price_units =
                i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let poc_volume =
                f32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let npoc_tag = decompressed[cursor];
            cursor += 1;
            let npoc_filled_at =
                u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;

            let status = match npoc_tag {
                1 => NPoc::Naked,
                2 => NPoc::Filled { at: npoc_filled_at },
                _ => NPoc::None,
            };

            Some(PointOfControl {
                price: Price {
                    units: poc_price_units,
                },
                volume: poc_volume,
                status,
            })
        } else {
            None
        };

        if cursor + 4 > decompressed.len() {
            return Err("Unexpected EOF in clusters count".into());
        }
        let clusters_count =
            u32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        const CLUSTER_RECORD_SIZE: usize = 40;
        if clusters_count > (decompressed.len() - cursor) / CLUSTER_RECORD_SIZE {
            return Err("Corrupted cache: clusters count exceeds available buffer".into());
        }

        let mut trades_map = FxHashMap::default();
        trades_map.reserve(clusters_count);

        for _ in 0..clusters_count {
            if cursor + 8 + 4 + 4 + 8 + 8 + 4 + 4 > decompressed.len() {
                return Err("Unexpected EOF in cluster entry".into());
            }
            let price_units =
                i64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let buy_qty = f32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let sell_qty = f32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let first_time =
                u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let last_time =
                u64::from_le_bytes(decompressed[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let buy_count =
                u32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let sell_count =
                u32::from_le_bytes(decompressed[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;

            trades_map.insert(
                Price { units: price_units },
                GroupedTrades {
                    buy_qty: f64::from(buy_qty),
                    sell_qty: f64::from(sell_qty),
                    first_time,
                    last_time,
                    buy_count,
                    sell_count,
                },
            );
        }

        let footprint = KlineTrades {
            trades: trades_map,
            poc,
            cached_first_time: if first_t == 0 { None } else { Some(first_t) },
            cached_last_time: if last_t == 0 { None } else { Some(last_t) },
        };

        let kline = Kline {
            time: kline_time,
            open: Price { units: open_units },
            high: Price { units: high_units },
            low: Price { units: low_units },
            close: Price { units: close_units },
            volume: (vol_buy, vol_sell),
        };

        let valid_fetched =
            trades_fetched && (!footprint.is_empty() || (vol_buy + vol_sell) == 0.0);

        datapoints.push((
            timestamp,
            KlineDataPoint {
                kline,
                footprint,
                trades_fetched: valid_fetched,
            },
        ));
    }

    Ok(datapoints)
}

pub fn footprint_cache_path(
    base_data_path: &Path,
    symbol: &str,
    timeframe: Timeframe,
    step: PriceStep,
    date: chrono::NaiveDate,
) -> PathBuf {
    let symbol_upper = symbol.to_uppercase();
    let timeframe_str = timeframe.to_string();
    let step_units = step.units;
    let file_name = format!("{symbol_upper}-fp-{}.fst", date.format("%Y-%m-%d"));
    base_data_path
        .join("footprint")
        .join(&symbol_upper)
        .join(&timeframe_str)
        .join(step_units.to_string())
        .join(file_name)
}

pub fn load_daily_footprint(path: &Path) -> Option<Vec<(u64, KlineDataPoint)>> {
    let target_path = if path.exists() {
        path.to_path_buf()
    } else {
        let bin_path = path.with_extension("bin");
        if bin_path.exists() {
            bin_path
        } else {
            return None;
        }
    };
    match std::fs::read(&target_path) {
        Ok(bytes) => match decode_footprint_datapoints(&bytes) {
            Ok(dps) => {
                let has_empty_volume_candles = dps.iter().any(|(_, dp)| {
                    (dp.kline.volume.0 + dp.kline.volume.1) > 0.0 && dp.footprint.is_empty()
                });
                if has_empty_volume_candles {
                    log::warn!(
                        "Footprint cache {:?} has missing candle clusters, removing incomplete cache",
                        target_path
                    );
                    let _ = std::fs::remove_file(&target_path);
                    return None;
                }
                Some(dps)
            }
            Err(e) => {
                log::warn!(
                    "Corrupted footprint cache {:?}: {}, removing",
                    target_path,
                    e
                );
                let _ = std::fs::remove_file(&target_path);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to read footprint cache {:?}: {}", target_path, e);
            None
        }
    }
}

pub fn save_daily_footprint(
    path: &Path,
    datapoints: &[(u64, KlineDataPoint)],
) -> Result<(), std::io::Error> {
    if datapoints.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let encoded = encode_footprint_datapoints(datapoints);
    let temp_name = format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_path = path.with_extension(temp_name);
    std::fs::write(&temp_path, &encoded)?;
    let mut last_err = None;
    let mut delay = std::time::Duration::from_millis(10);
    let mut success = false;
    for attempt in 0..3 {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        match std::fs::rename(&temp_path, path) {
            Ok(()) => {
                success = true;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < 2 {
                    std::thread::sleep(delay);
                    delay *= 2;
                }
            }
        }
    }
    if !success {
        let _ = std::fs::remove_file(&temp_path);
        return Err(last_err
            .unwrap_or_else(|| std::io::Error::other("Failed to rename footprint cache file")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_footprint_level_keeps_small_trades_on_a_large_level() {
        let step = PriceStep {
            units: 100_00000000,
        };
        let trade = |qty: f32, is_sell: bool| Trade {
            time: 1726210800000,
            is_sell,
            price: Price {
                units: 58000_00000000,
            },
            qty,
        };

        let mut footprint = KlineTrades::new();
        footprint.add_trade_to_nearest_bin(&trade(50_000.0, false), step);
        for _ in 0..1_000_000 {
            footprint.add_trade_to_nearest_bin(&trade(0.001, false), step);
        }
        footprint.add_trade_to_nearest_bin(&trade(50_000.0, true), step);

        let level = footprint.trades.values().next().unwrap();
        // The only error left is 0.001's own f32 representation, not the summing.
        let expected_buy = 50_000.0 + 1_000_000.0 * f64::from(0.001f32);
        assert!(
            (level.buy_qty - expected_buy).abs() < 1e-6,
            "buy {}",
            level.buy_qty
        );
        // With f32 sums every 0.001 added to 50k rounded away and the delta read 0.
        assert!(
            (level.delta_qty() - 1000.0).abs() < 0.01,
            "delta {}",
            level.delta_qty()
        );
    }

    #[test]
    fn test_tpo_chart_kind_defaults_and_scaling() {
        let tpo = KlineChartKind::Tpo {
            show_candles: true,
            show_letters: true,
            show_ib: true,
            show_va: true,
            show_poc: true,
            show_single_prints: true,
            tick_step: TpoTickStep::Auto,
            period: SessionPeriod::Daily,
            clusters: vec![],
            split_sessions: vec![],
            color_scheme: TpoColorScheme::Classic,
            ib_color: TpoElementColor::Auto,
            poc_color: TpoElementColor::Auto,
            single_prints_color: TpoElementColor::Auto,
        };

        assert_eq!(tpo.min_cell_width(), 1.0);
        assert_eq!(tpo.default_cell_width(), 8.0);
        assert_eq!(tpo.max_cell_width(), 120.0);
        assert_eq!(tpo.min_scaling(), 0.1);
        assert_eq!(tpo.max_scaling(), 2.0);
        assert_eq!(tpo.view_mode(), ViewMode::Combined);
        assert_eq!(tpo.effective_tpo_period(), SessionPeriod::Daily);

        let combined_weekly = KlineChartKind::Tpo {
            show_candles: true,
            show_letters: true,
            show_ib: true,
            show_va: true,
            show_poc: true,
            show_single_prints: true,
            tick_step: TpoTickStep::Auto,
            period: SessionPeriod::Weekly,
            clusters: vec![],
            split_sessions: vec![],
            color_scheme: TpoColorScheme::Classic,
            ib_color: TpoElementColor::Auto,
            poc_color: TpoElementColor::Auto,
            single_prints_color: TpoElementColor::Auto,
        };
        assert_eq!(combined_weekly.view_mode(), ViewMode::Combined);
        assert_eq!(
            combined_weekly.effective_tpo_period(),
            SessionPeriod::Weekly
        );

        let pure_tpo = KlineChartKind::Tpo {
            show_candles: false,
            show_letters: true,
            show_ib: true,
            show_va: true,
            show_poc: true,
            show_single_prints: true,
            tick_step: TpoTickStep::Auto,
            period: SessionPeriod::Weekly,
            clusters: vec![SessionCluster::new(vec![100, 200])],
            split_sessions: vec![100],
            color_scheme: TpoColorScheme::Classic,
            ib_color: TpoElementColor::Auto,
            poc_color: TpoElementColor::Auto,
            single_prints_color: TpoElementColor::Auto,
        };
        assert_eq!(pure_tpo.view_mode(), ViewMode::TpoOnly);
        assert_eq!(pure_tpo.effective_tpo_period(), SessionPeriod::Weekly);

        let json = serde_json::to_string(&tpo).unwrap();
        let deserialized: KlineChartKind = serde_json::from_str(&json).unwrap();
        assert_eq!(tpo, deserialized);

        // Backward compatibility: JSON without period and clusters defaults properly
        let legacy_json = r#"{"Tpo":{"show_candles":true,"show_letters":true,"show_ib":true,"show_va":true,"show_poc":true,"show_single_prints":true,"tick_step":"Auto"}}"#;
        let legacy_deserialized: KlineChartKind = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(legacy_deserialized, tpo);
    }

    #[test]
    fn test_footprint_scaling_and_cluster_search() {
        let footprint = KlineChartKind::Footprint {
            clusters: ClusterKind::BidAsk,
            scaling: ClusterScaling::VisibleRange,
            studies: vec![FootprintStudy::ClusterSearch {
                id: 1,
                min_volume: 500.0,
                min_delta: 200.0,
                side: ClusterSearchSide::BuyOnly,
                style: HighlightStyle::Border,
                color: HighlightColor::Amber,
            }],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: default_aggregate_exchanges(),
            history_lookback_days: None,
        };

        assert_eq!(footprint.min_cell_width(), 2.0);
        assert_eq!(footprint.default_cell_width(), 80.0);
        assert_eq!(footprint.max_cell_width(), 2500.0);
        assert_eq!(footprint.min_scaling(), 0.1);
        assert_eq!(footprint.max_scaling(), 4.0);

        let json = serde_json::to_string(&footprint).unwrap();
        let deserialized: KlineChartKind = serde_json::from_str(&json).unwrap();
        assert_eq!(footprint, deserialized);
    }

    #[test]
    fn test_footprint_datapoints_binary_roundtrip() {
        let mut dp = KlineDataPoint {
            kline: Kline {
                time: 1726210800000,
                open: Price {
                    units: 58000_00000000,
                },
                high: Price {
                    units: 58500_00000000,
                },
                low: Price {
                    units: 57900_00000000,
                },
                close: Price {
                    units: 58300_00000000,
                },
                volume: (15.5, 12.3),
            },
            footprint: KlineTrades::new(),
            trades_fetched: true,
        };

        dp.footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 1726210800100,
                is_sell: false,
                price: Price {
                    units: 58000_00000000,
                },
                qty: 10.0,
            },
            PriceStep {
                units: 100_00000000,
            },
        );

        dp.footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 1726210800200,
                is_sell: true,
                price: Price {
                    units: 58100_00000000,
                },
                qty: 5.5,
            },
            PriceStep {
                units: 100_00000000,
            },
        );

        dp.calculate_poc();
        dp.set_poc_status(NPoc::Filled { at: 1726214400000 });

        let original = vec![(1726210800000, dp)];
        let encoded = encode_footprint_datapoints(&original);
        let decoded = decode_footprint_datapoints(&encoded).expect("decode failed");

        assert_eq!(decoded.len(), original.len());
        let (t_orig, dp_orig) = &original[0];
        let (t_dec, dp_dec) = &decoded[0];

        assert_eq!(t_orig, t_dec);
        assert_eq!(dp_orig.kline.time, dp_dec.kline.time);
        assert_eq!(dp_orig.kline.open.units, dp_dec.kline.open.units);
        assert_eq!(dp_orig.kline.high.units, dp_dec.kline.high.units);
        assert_eq!(dp_orig.kline.low.units, dp_dec.kline.low.units);
        assert_eq!(dp_orig.kline.close.units, dp_dec.kline.close.units);
        assert_eq!(dp_orig.kline.volume, dp_dec.kline.volume);
        assert_eq!(dp_orig.trades_fetched, dp_dec.trades_fetched);

        let poc_orig = dp_orig.footprint.poc.as_ref().unwrap();
        let poc_dec = dp_dec.footprint.poc.as_ref().unwrap();
        assert_eq!(poc_orig.price.units, poc_dec.price.units);
        assert_eq!(poc_orig.volume, poc_dec.volume);
        assert_eq!(poc_orig.status, poc_dec.status);

        assert_eq!(
            dp_orig.footprint.trades.len(),
            dp_dec.footprint.trades.len()
        );
        for (price, group_orig) in &dp_orig.footprint.trades {
            let group_dec = dp_dec.footprint.trades.get(price).expect("price missing");
            assert_eq!(group_orig.buy_qty, group_dec.buy_qty);
            assert_eq!(group_orig.sell_qty, group_dec.sell_qty);
            assert_eq!(group_orig.buy_count, group_dec.buy_count);
            assert_eq!(group_orig.sell_count, group_dec.sell_count);
        }
    }

    #[test]
    fn test_load_daily_footprint_rejects_incomplete_cache() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_incomplete_fp_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fp_file = temp_dir.join("BTCUSDT-fp-2026-09-16.bin");

        // Create a datapoint with volume > 0 but empty footprint (incomplete)
        let dp = KlineDataPoint {
            kline: Kline {
                time: 1789516800000,
                open: Price::from_f32(60000.0),
                high: Price::from_f32(61000.0),
                low: Price::from_f32(59000.0),
                close: Price::from_f32(60500.0),
                volume: (100.0, 100.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: true,
        };

        let dps = vec![(1789516800000, dp)];
        save_daily_footprint(&fp_file, &dps).expect("save failed");
        assert!(fp_file.exists());

        // load_daily_footprint must detect empty footprint with volume > 0, reject it, and delete the file
        let loaded = load_daily_footprint(&fp_file);
        assert!(loaded.is_none());
        assert!(!fp_file.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_is_trades_coverage_complete_validates_extremes_and_gap() {
        let step = PriceStep::from_f32(0.1);
        let interval_ms = 1_800_000; // 30m
        let candle_time = 1_700_000_000_000; // past closed candle
        let now_ms = candle_time + interval_ms + 100_000;

        let mut dp = KlineDataPoint {
            kline: Kline {
                time: candle_time,
                open: Price::from_f32(83000.0),
                high: Price::from_f32(83443.0),
                low: Price::from_f32(82500.1),
                close: Price::from_f32(82800.0),
                volume: (100.0, 100.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: false,
        };

        // 1. Incomplete footprint (bare wick below 82660.0, low is 82500.1)
        dp.add_trade(
            &Trade {
                time: candle_time + 1000,
                is_sell: false,
                price: Price::from_f32(83443.0),
                qty: 1.0,
            },
            step,
        );
        dp.add_trade(
            &Trade {
                time: candle_time + interval_ms - 1000,
                is_sell: true,
                price: Price::from_f32(82660.0), // Does not reach 82500.1
                qty: 1.0,
            },
            step,
        );

        assert!(
            !dp.is_trades_coverage_complete(interval_ms, step, now_ms),
            "Footprint terminating at 82660.0 must NOT be complete when kline low is 82500.1"
        );

        // 2. Add trade covering the low wick down to 82500.1
        dp.add_trade(
            &Trade {
                time: candle_time + 500_000,
                is_sell: true,
                price: Price::from_f32(82500.1),
                qty: 1.0,
            },
            step,
        );

        assert!(
            dp.is_trades_coverage_complete(interval_ms, step, now_ms),
            "Footprint covering both high and low extremes and timestamps must be complete"
        );

        // 3. Active/forming candle (candle_end > now_ms) must never be finalized as complete
        let active_now_ms = candle_time + interval_ms / 2;
        assert!(
            !dp.is_trades_coverage_complete(interval_ms, step, active_now_ms),
            "Active candle must not be finalized as complete"
        );
    }

    #[test]
    fn test_dat04_footprint_cache_corrupted_count_bounds() {
        let mut raw = Vec::new();
        raw.extend_from_slice(FOOTPRINT_CACHE_MAGIC);
        raw.extend_from_slice(&1u16.to_le_bytes());
        raw.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());

        let compressed = lz4_flex::compress_prepend_size(&raw);
        let result = decode_footprint_datapoints(&compressed);
        match result {
            Err(err) => assert!(err.contains("exceeds buffer bounds")),
            Ok(_) => panic!("expected Err on corrupted count"),
        }
    }

    #[test]
    fn test_simulate_from_kline_distribution_and_poc() {
        let kline = Kline {
            time: 1_700_000_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (60.0, 40.0), // 60 buy, 40 sell -> total 100
        };
        let step = PriceStep::from_f32(1.0);
        let footprint = KlineTrades::simulate_from_kline(&kline, step);

        assert!(!footprint.is_empty());
        assert!(footprint.trades.len() >= 10);

        // Sum of volumes should be close to 100.0
        let total_vol: f32 = footprint.trades.values().map(|g| g.total_qty()).sum();
        assert!(
            (total_vol - 100.0).abs() < 1e-2,
            "total volume must equal 100.0, got {total_vol}"
        );

        // Total buy must be ~60, total sell must be ~40
        let total_buy: f64 = footprint.trades.values().map(|g| g.buy_qty).sum();
        let total_sell: f64 = footprint.trades.values().map(|g| g.sell_qty).sum();
        assert!(
            (total_buy - 60.0).abs() < 1e-1,
            "total buy must equal ~60, got {total_buy}"
        );
        assert!(
            (total_sell - 40.0).abs() < 1e-1,
            "total sell must equal ~40, got {total_sell}"
        );

        // POC must exist and be within [90, 110]
        let poc = footprint.poc.as_ref().expect("POC must exist");
        let poc_price = poc.price.to_f32_lossy();
        assert!(
            (90.0..=110.0).contains(&poc_price),
            "POC {poc_price} out of range"
        );
        assert!(poc.volume > 0.0);
    }

    #[test]
    fn test_ter13_table_cluster_and_footprint_summary() {
        assert_eq!(ClusterKind::ALL.len(), 4);
        assert!(ClusterKind::ALL.contains(&ClusterKind::Table));
        assert_eq!(ClusterKind::Table.to_string(), "Table");
        assert_eq!(ClusterKind::Table.min_footprint_width(), 100.0);

        let kline = Kline {
            time: 1_700_000_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (75.0, 25.0),
        };
        let step = PriceStep::from_f32(1.0);
        let footprint = KlineTrades::simulate_from_kline(&kline, step);

        let summary = FootprintSummary::from_trades(&footprint).expect("summary from trades");
        assert!((summary.total - 100.0).abs() < 1e-1);
        assert!(summary.delta > 0.0);
        assert!(summary.delta_pct > 0.0);

        let dp = KlineDataPoint {
            kline,
            footprint,
            trades_fetched: true,
        };
        let max_qty_table = dp.max_cluster_qty(
            ClusterKind::Table,
            Price::from_f32(110.0),
            Price::from_f32(90.0),
        );
        let max_qty_bidask = dp.max_cluster_qty(
            ClusterKind::BidAsk,
            Price::from_f32(110.0),
            Price::from_f32(90.0),
        );
        assert_eq!(max_qty_table, max_qty_bidask);
    }

    #[test]
    fn test_dat11_save_daily_footprint_retry_and_write() {
        let temp_dir = std::env::temp_dir().join(format!("hawk_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let target_path = temp_dir.join("test_fp.bin");

        let kline = Kline {
            time: 1_700_000_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (50.0, 50.0),
        };
        let step = PriceStep::from_f32(1.0);
        let footprint = KlineTrades::simulate_from_kline(&kline, step);
        let dp = KlineDataPoint {
            kline,
            footprint,
            trades_fetched: true,
        };
        let datapoints = vec![(1_700_000_000, dp)];

        // Initial save
        save_daily_footprint(&target_path, &datapoints).expect("save must succeed");
        assert!(target_path.exists());

        // Overwrite save (exercises removal and atomic rename)
        save_daily_footprint(&target_path, &datapoints).expect("overwrite save must succeed");
        assert!(target_path.exists());

        let _ = std::fs::remove_file(&target_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
