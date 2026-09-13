use exchange::{
    Kline, Timeframe, Trade,
    util::{Price, PriceStep},
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::tpo::{SessionCluster, SessionPeriod};
use crate::aggr::time::DataPoint;

#[derive(Clone)]
pub struct KlineDataPoint {
    pub kline: Kline,
    pub footprint: KlineTrades,
    pub trades_fetched: bool,
}

impl KlineDataPoint {
    pub fn max_cluster_qty(&self, cluster_kind: ClusterKind, highest: Price, lowest: Price) -> f32 {
        match cluster_kind {
            ClusterKind::BidAsk => self.footprint.max_qty_by(highest, lowest, f32::max),
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

#[derive(Debug, Clone, Default)]
pub struct GroupedTrades {
    pub buy_qty: f32,
    pub sell_qty: f32,
    pub first_time: u64,
    pub last_time: u64,
    pub buy_count: usize,
    pub sell_count: usize,
}

impl GroupedTrades {
    fn new(trade: &Trade) -> Self {
        Self {
            buy_qty: if trade.is_sell { 0.0 } else { trade.qty },
            sell_qty: if trade.is_sell { trade.qty } else { 0.0 },
            first_time: trade.time,
            last_time: trade.time,
            buy_count: if trade.is_sell { 0 } else { 1 },
            sell_count: if trade.is_sell { 1 } else { 0 },
        }
    }

    fn add_trade(&mut self, trade: &Trade) {
        if trade.is_sell {
            self.sell_qty += trade.qty;
            self.sell_count += 1;
        } else {
            self.buy_qty += trade.qty;
            self.buy_count += 1;
        }
        self.last_time = trade.time;
    }

    pub fn total_qty(&self) -> f32 {
        self.buy_qty + self.sell_qty
    }

    pub fn delta_qty(&self) -> f32 {
        self.buy_qty - self.sell_qty
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

    pub fn first_trade_t(&self) -> Option<u64> {
        self.cached_first_time
    }

    pub fn last_trade_t(&self) -> Option<u64> {
        self.cached_last_time
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
                max_qty = max_qty.max(f(group.buy_qty, group.sell_qty));
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
}

const fn default_true() -> bool {
    true
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
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum ClusterKind {
    #[default]
    BidAsk,
    VolumeProfile,
    DeltaProfile,
}

impl ClusterKind {
    pub const ALL: [ClusterKind; 3] = [
        ClusterKind::BidAsk,
        ClusterKind::VolumeProfile,
        ClusterKind::DeltaProfile,
    ];
}

impl std::fmt::Display for ClusterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterKind::BidAsk => write!(f, "Bid/Ask"),
            ClusterKind::VolumeProfile => write!(f, "Volume Profile"),
            ClusterKind::DeltaProfile => write!(f, "Delta Profile"),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Deserialize, Serialize)]
pub struct Config {}

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
    Triangle,
    Square,
}

impl HighlightStyle {
    pub const ALL: [HighlightStyle; 5] = [
        HighlightStyle::Border,
        HighlightStyle::Fill,
        HighlightStyle::Circle,
        HighlightStyle::Triangle,
        HighlightStyle::Square,
    ];
}

impl std::fmt::Display for HighlightStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightStyle::Border => write!(f, "Border"),
            HighlightStyle::Fill => write!(f, "Fill"),
            HighlightStyle::Circle => write!(f, "Circle"),
            HighlightStyle::Triangle => write!(f, "Triangle"),
            HighlightStyle::Square => write!(f, "Square"),
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
}

impl HighlightColor {
    pub const ALL: [HighlightColor; 6] = [
        HighlightColor::Amber,
        HighlightColor::Cyan,
        HighlightColor::Magenta,
        HighlightColor::Green,
        HighlightColor::Red,
        HighlightColor::White,
    ];

    pub fn to_rgb(self) -> [f32; 3] {
        match self {
            HighlightColor::Amber => [1.0, 0.75, 0.0],
            HighlightColor::Cyan => [0.0, 0.88, 1.0],
            HighlightColor::Magenta => [1.0, 0.20, 0.80],
            HighlightColor::Green => [0.15, 0.85, 0.35],
            HighlightColor::Red => [1.0, 0.25, 0.25],
            HighlightColor::White => [1.0, 1.0, 1.0],
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
        }
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
            _ => false,
        }
    }
}

impl FootprintStudy {
    pub const ALL: [FootprintStudy; 6] = [
        FootprintStudy::NPoC { lookback: 80 },
        FootprintStudy::Imbalance {
            threshold: 200,
            color_scale: Some(400),
            ignore_zeros: true,
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
            raw.extend_from_slice(&group.buy_qty.to_le_bytes());
            raw.extend_from_slice(&group.sell_qty.to_le_bytes());
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
                    buy_qty,
                    sell_qty,
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

        datapoints.push((
            timestamp,
            KlineDataPoint {
                kline,
                footprint,
                trades_fetched,
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
    let file_name = format!("{symbol_upper}-fp-{}.bin", date.format("%Y-%m-%d"));
    base_data_path
        .join("footprint")
        .join(&symbol_upper)
        .join(&timeframe_str)
        .join(step_units.to_string())
        .join(file_name)
}

pub fn load_daily_footprint(path: &Path) -> Option<Vec<(u64, KlineDataPoint)>> {
    if !path.exists() {
        return None;
    }
    match std::fs::read(path) {
        Ok(bytes) => match decode_footprint_datapoints(&bytes) {
            Ok(dps) => Some(dps),
            Err(e) => {
                log::warn!("Corrupted footprint cache {:?}: {}, removing", path, e);
                let _ = std::fs::remove_file(path);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to read footprint cache {:?}: {}", path, e);
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
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    if let Err(e) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
