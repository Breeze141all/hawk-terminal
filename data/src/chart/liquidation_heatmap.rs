//! Liquidation Heatmap
//!
//! Simulates 2D liquidation zones (Time x Price) based on volume-weighted leverage positions.
//!
//! The indicator tracks where liquidations occur for positions opened across historical candles
//! at various leverage settings (5x, 10x, 25x, 50x, 100x), forming horizontal segments over time.
//! When market price crosses an active liquidation zone, that zone is terminated (liquidated),
//! matching real-world liquidation heatmaps (Kingfisher / Coinglass).

use exchange::util::Price;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Configuration for the liquidation heatmap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationHeatmapConfig {
    /// Leverage values to simulate (positive = longs, negative = shorts)
    /// Default: [5, 10, 25, 50, 100]
    pub leverages: Vec<i32>,
    /// Buffer percentage added to liquidation distance (default: 0.5%)
    pub leverage_buffer: f32,
    /// Minimum strength threshold to display a zone (default: 1.0)
    pub threshold: f32,
    /// Strength multiplier (default: 1.0)
    pub strength_multiplier: f32,
    /// Grid cell size as ATR multiplier (default: 0.19)
    pub auto_scale: f32,
    /// ATR calculation period (default: 200)
    pub auto_scale_length: usize,
    /// Volume smoothing period (default: 1)
    pub volume_length: usize,
    /// Strength fade per bar after fadeStart (default: 0)
    pub fade_amount: f32,
    /// Bars before fading begins (default: 100)
    pub fade_start: usize,
    /// Maximum distance from current price to show zones (0 = unlimited)
    pub max_distance_pct: f32,
    /// Use logarithmic volume ratios
    pub use_log: bool,
    /// Price source for liquidation calculation
    pub origin_mode: OriginMode,
    /// Whether the heatmap is enabled
    pub enabled: bool,
}

impl Default for LiquidationHeatmapConfig {
    fn default() -> Self {
        Self {
            leverages: vec![5, 10, 25, 50, 100],
            leverage_buffer: 0.5,
            threshold: 1.0,
            strength_multiplier: 1.0,
            auto_scale: 0.19,
            auto_scale_length: 200,
            volume_length: 1,
            fade_amount: 0.0,
            fade_start: 100,
            max_distance_pct: 0.0,
            use_log: false,
            origin_mode: OriginMode::HighLow,
            enabled: false,
        }
    }
}

/// Price source for liquidation calculation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OriginMode {
    /// Use high for resistance, low for support
    #[default]
    HighLow,
    /// Use close price
    Close,
    /// Use OHLC4 average
    Ohlc4,
}

impl std::fmt::Display for OriginMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OriginMode::HighLow => write!(f, "High/Low"),
            OriginMode::Close => write!(f, "Close"),
            OriginMode::Ohlc4 => write!(f, "OHLC4"),
        }
    }
}

impl OriginMode {
    pub const ALL: [OriginMode; 3] = [OriginMode::HighLow, OriginMode::Close, OriginMode::Ohlc4];
}

/// A 2D liquidation segment spanning across time at a price level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationSegment {
    /// Cell ID (price level bucket)
    pub cell_id: i64,
    /// Top boundary of the cell
    pub top: Price,
    /// Bottom boundary of the cell
    pub bottom: Price,
    /// Timestamp when this segment was created (candle time)
    pub start_time: u64,
    /// Timestamp when this segment ended (touched/liquidated). None if still active.
    pub end_time: Option<u64>,
    /// Accumulated strength (volume density)
    pub strength: f32,
    /// True = resistance (short liquidations above price), False = support (long liquidations below price)
    pub is_resistance: bool,
}

/// A single liquidation zone cell (snapshot for active price levels)
#[derive(Debug, Clone)]
pub struct LiquidationCell {
    /// Unique identifier (price level as i64 units)
    pub cell_id: i64,
    /// Top boundary of the cell
    pub top: Price,
    /// Bottom boundary of the cell
    pub bottom: Price,
    /// Accumulated strength
    pub strength: f32,
    /// Number of accumulations
    pub count: u32,
    /// Bar index when created
    pub created_at_bar: u64,
    /// True = resistance (above price), False = support (below price)
    pub is_resistance: bool,
}

impl LiquidationCell {
    /// Calculate color ratio for rendering (0.0 to 1.0)
    pub fn color_ratio(&self, strength_multiplier: f32) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let ratio = (self.strength / self.count as f32) * (strength_multiplier / 100.0);
        ratio.clamp(0.01, 1.0)
    }
}

/// Main liquidation heatmap state
#[derive(Debug, Clone)]
pub struct LiquidationHeatmap {
    /// Active liquidation cells keyed by cell_id (current active snapshot)
    pub cells: FxHashMap<i64, LiquidationCell>,
    /// Historical 2D segments (time-bounded horizontal liquidation bands)
    pub segments: Vec<LiquidationSegment>,
    /// Index in `segments` of the currently active segment for each cell_id
    pub active_indices: FxHashMap<i64, usize>,
    /// Stable price bin size (computed once per dataset/session)
    pub bin_size: f32,
    /// Current ATR value
    pub atr: f32,
    /// ATR RMA state (running moving average)
    atr_rma: f32,
    /// Volume SMA buffer for buy volume
    pub buy_vol_buffer: Vec<f32>,
    /// Volume SMA buffer for sell volume
    pub sell_vol_buffer: Vec<f32>,
    /// Current bar index
    pub current_bar: u64,
    /// Price ranges for ATR calculation
    price_ranges: Vec<f32>,
    /// Configuration
    pub config: LiquidationHeatmapConfig,
}

impl Default for LiquidationHeatmap {
    fn default() -> Self {
        Self::new(LiquidationHeatmapConfig::default())
    }
}

impl LiquidationHeatmap {
    pub fn new(config: LiquidationHeatmapConfig) -> Self {
        Self {
            cells: FxHashMap::default(),
            segments: Vec::with_capacity(512),
            active_indices: FxHashMap::default(),
            bin_size: 0.0,
            atr: 0.0,
            atr_rma: 0.0,
            buy_vol_buffer: Vec::new(),
            sell_vol_buffer: Vec::new(),
            current_bar: 0,
            price_ranges: Vec::new(),
            config,
        }
    }

    /// Reset all state
    pub fn clear(&mut self) {
        self.cells.clear();
        self.segments.clear();
        self.active_indices.clear();
        self.bin_size = 0.0;
        self.atr = 0.0;
        self.atr_rma = 0.0;
        self.buy_vol_buffer.clear();
        self.sell_vol_buffer.clear();
        self.current_bar = 0;
        self.price_ranges.clear();
    }

    /// Calculate RMA (Running Moving Average) - Wilder's smoothing
    /// RMA[i] = (RMA[i-1] * (length - 1) + value[i]) / length
    fn calculate_rma(prev_rma: f32, value: f32, length: usize) -> f32 {
        if length == 0 {
            return value;
        }
        let len_f = length as f32;
        (prev_rma * (len_f - 1.0) + value) / len_f
    }

    /// Calculate SMA of a buffer
    fn calculate_sma(buffer: &[f32]) -> f32 {
        if buffer.is_empty() {
            return 1.0;
        }
        buffer.iter().sum::<f32>() / buffer.len() as f32
    }

    /// Update ATR with new price range
    fn update_atr(&mut self, high: f32, low: f32) {
        let price_range = high - low;
        self.price_ranges.push(price_range);

        if self.price_ranges.len() > self.config.auto_scale_length {
            self.price_ranges.remove(0);
        }

        if self.atr_rma == 0.0 && !self.price_ranges.is_empty() {
            self.atr_rma = Self::calculate_sma(&self.price_ranges);
        } else {
            self.atr_rma =
                Self::calculate_rma(self.atr_rma, price_range, self.config.auto_scale_length);
        }
        self.atr = self.atr_rma;
    }

    /// Update volume buffers
    fn update_volume_buffers(&mut self, buy_volume: f32, sell_volume: f32) {
        self.buy_vol_buffer.push(buy_volume);
        self.sell_vol_buffer.push(sell_volume);

        let max_len = self.config.volume_length.max(1);
        if self.buy_vol_buffer.len() > max_len {
            self.buy_vol_buffer.remove(0);
        }
        if self.sell_vol_buffer.len() > max_len {
            self.sell_vol_buffer.remove(0);
        }
    }

    /// Calculate liquidation price for a given leverage
    pub fn calculate_liquidation_price(source_price: f32, leverage: i32, buffer_pct: f32) -> f32 {
        let lev_abs = (leverage.abs() as f32).max(1.0);
        let lev_distance = 1.0 / lev_abs;
        let buffer = buffer_pct / 100.0;
        let mm = buffer + 0.01 / lev_abs;
        let offset = lev_distance + mm;

        if leverage > 0 {
            // Short liquidation level (resistance above current price)
            source_price * (1.0 + offset)
        } else {
            // Long liquidation level (support below current price)
            source_price * (1.0 - offset)
        }
    }

    /// Calculate a clean, discrete price bin size for fixed-grid heatmap tiles
    pub fn calculate_bin_size(price: f32) -> f32 {
        let p = if price.is_finite() && price > 0.0 {
            price
        } else {
            100.0
        };
        let target = (p * 0.0015).max(1e-8);
        let exponent = 10.0_f32.powf(target.log10().floor());
        let fraction = target / exponent;
        let nice_mult = if fraction < 1.5 {
            1.0
        } else if fraction < 3.5 {
            2.0
        } else if fraction < 7.5 {
            5.0
        } else {
            10.0
        };
        nice_mult * exponent
    }

    /// Round price to fixed grid cell index
    fn round_to_cell(&self, price: f32, cell_size: f32) -> i64 {
        if cell_size <= 0.0 {
            return 0;
        }
        (price / cell_size).floor() as i64
    }

    /// Process a new candle
    pub fn on_candle(
        &mut self,
        time: u64,
        open: f32,
        high: f32,
        low: f32,
        close: f32,
        buy_volume: f32,
        sell_volume: f32,
    ) {
        if !self.config.enabled {
            return;
        }

        self.current_bar += 1;

        // Ensure stable, fixed bin size
        if self.bin_size <= 0.0 {
            self.bin_size = Self::calculate_bin_size(close);
        }
        let cell_size = self.bin_size;

        // 1. Process zone touches with current candle high/low BEFORE adding new levels
        self.process_touches_and_fading(high, low, time);

        // Update ATR
        self.update_atr(high, low);

        // Update volume buffers
        self.update_volume_buffers(buy_volume, sell_volume);

        // Need at least 2 bars for previous price
        if self.current_bar < 2 {
            return;
        }

        if cell_size <= 0.0 {
            return;
        }

        // Calculate volume ratios
        let avg_buy = Self::calculate_sma(&self.buy_vol_buffer).max(0.001);
        let avg_sell = Self::calculate_sma(&self.sell_vol_buffer).max(0.001);

        let mut buy_ratio = buy_volume / avg_buy;
        let mut sell_ratio = sell_volume / avg_sell;

        if self.config.use_log {
            buy_ratio = (buy_ratio + 1.0).ln();
            sell_ratio = (sell_ratio + 1.0).ln();
        }

        // Get source prices based on origin mode
        let (long_source, short_source) = match self.config.origin_mode {
            OriginMode::HighLow => (high, low),
            OriginMode::Close => (close, close),
            OriginMode::Ohlc4 => {
                let ohlc4 = (open + high + low + close) / 4.0;
                (ohlc4, ohlc4)
            }
        };

        // 2. Process each leverage: shorts liquidated above, longs liquidated below
        for &base_leverage in &self.config.leverages.clone() {
            // Short liquidation level (resistance above current price)
            let liq_price_resistance = Self::calculate_liquidation_price(
                long_source,
                base_leverage,
                self.config.leverage_buffer,
            );
            let cell_id_res = self.round_to_cell(liq_price_resistance, cell_size);
            self.add_or_update_cell(cell_id_res, cell_size, sell_ratio, true, time);

            // Long liquidation level (support below current price)
            let liq_price_support = Self::calculate_liquidation_price(
                short_source,
                -base_leverage,
                self.config.leverage_buffer,
            );
            let cell_id_sup = self.round_to_cell(liq_price_support, cell_size);
            self.add_or_update_cell(cell_id_sup, cell_size, buy_ratio, false, time);
        }
    }

    /// Add or update a cell and manage 2D timeline segments
    fn add_or_update_cell(
        &mut self,
        cell_id: i64,
        cell_size: f32,
        ratio: f32,
        is_resistance: bool,
        time: u64,
    ) {
        let bottom = Price::from_f32(cell_id as f32 * cell_size);
        let top = Price::from_f32((cell_id + 1) as f32 * cell_size);

        // Update active cells map for histogram / current state
        let new_strength = if let Some(cell) = self.cells.get_mut(&cell_id) {
            cell.strength += ratio;
            cell.count += 1;
            cell.strength
        } else {
            self.cells.insert(
                cell_id,
                LiquidationCell {
                    cell_id,
                    top,
                    bottom,
                    strength: ratio,
                    count: 1,
                    created_at_bar: self.current_bar,
                    is_resistance,
                },
            );
            ratio
        };

        // Manage 2D timeline segments
        if let Some(&seg_idx) = self.active_indices.get(&cell_id)
            && seg_idx < self.segments.len()
        {
            if self.segments[seg_idx].start_time == time {
                self.segments[seg_idx].strength += ratio;
                return;
            }
            // Close preceding segment when strength increases on a new bar
            self.segments[seg_idx].end_time = Some(time);
        }

        let new_seg_idx = self.segments.len();
        self.segments.push(LiquidationSegment {
            cell_id,
            top,
            bottom,
            start_time: time,
            end_time: None,
            strength: new_strength,
            is_resistance,
        });
        self.active_indices.insert(cell_id, new_seg_idx);

        // Bound segment history: drain oldest chronologically from the front (FIFO), preserving recent 15,000
        if self.segments.len() > 15000 {
            let drain_count = 3000;
            self.segments.drain(0..drain_count);
            self.active_indices.clear();
            for (idx, seg) in self.segments.iter().enumerate() {
                if seg.end_time.is_none() {
                    self.active_indices.insert(seg.cell_id, idx);
                }
            }
        }
    }

    /// Process zone touches and apply fading
    fn process_touches_and_fading(&mut self, high: f32, low: f32, time: u64) {
        let fade_start = self.config.fade_start as u64;
        let fade_amount = self.config.fade_amount;
        let current_bar = self.current_bar;

        let mut to_liquidate: Vec<i64> = Vec::new();
        for (&id, cell) in &self.cells {
            let top_f = cell.top.to_f32();
            let bottom_f = cell.bottom.to_f32();

            // Touched if candle range reaches into the cell, or crosses it
            let touched = if cell.is_resistance {
                high >= bottom_f
            } else {
                low <= top_f
            } || (high >= bottom_f && low <= top_f);

            if touched {
                to_liquidate.push(id);
            }
        }

        for id in to_liquidate {
            if let Some(idx) = self.active_indices.remove(&id)
                && idx < self.segments.len()
            {
                self.segments[idx].end_time = Some(time);
            }
            self.cells.remove(&id);
        }

        // Apply fading if configured
        if fade_amount > 0.0 {
            let mut faded_out: Vec<i64> = Vec::new();
            for (id, cell) in self.cells.iter_mut() {
                if current_bar > cell.created_at_bar + fade_start {
                    cell.strength -= fade_amount.min(cell.strength);
                    if cell.strength <= 0.0 {
                        faded_out.push(*id);
                    }
                }
            }
            for id in faded_out {
                if let Some(idx) = self.active_indices.remove(&id)
                    && idx < self.segments.len()
                {
                    self.segments[idx].end_time = Some(time);
                }
                self.cells.remove(&id);
            }
        }
    }

    /// Get visible cells sorted by distance from current price (for histogram)
    pub fn visible_cells(&self, current_price: f32, limit: usize) -> Vec<&LiquidationCell> {
        let threshold = self.config.threshold;
        let max_distance = self.config.max_distance_pct;

        let mut visible: Vec<_> = self
            .cells
            .values()
            .filter(|cell| {
                if cell.strength <= threshold {
                    return false;
                }

                if max_distance > 0.0 {
                    let cell_price =
                        cell.bottom.to_f32() + (cell.top.to_f32() - cell.bottom.to_f32()) / 2.0;
                    let distance_pct = ((cell_price - current_price) / current_price).abs() * 100.0;
                    if distance_pct > max_distance {
                        return false;
                    }
                }

                true
            })
            .collect();

        visible.sort_by(|a, b| {
            let a_price = a.bottom.to_f32() + (a.top.to_f32() - a.bottom.to_f32()) / 2.0;
            let b_price = b.bottom.to_f32() + (b.top.to_f32() - b.bottom.to_f32()) / 2.0;
            let a_dist = (a_price - current_price).abs();
            let b_dist = (b_price - current_price).abs();
            a_dist
                .partial_cmp(&b_dist)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        visible.truncate(limit);
        visible
    }

    /// Get all historical and active 2D segments
    pub fn segments(&self) -> &[LiquidationSegment] {
        &self.segments
    }

    /// Maximum segment strength across all recorded segments
    pub fn max_segment_strength(&self) -> f32 {
        self.segments
            .iter()
            .map(|s| s.strength)
            .fold(0.0_f32, f32::max)
            .max(1.0)
    }

    /// Rebuild heatmap from historical kline data
    pub fn rebuild_from_klines(&mut self, klines: &[(u64, f32, f32, f32, f32, f32, f32)]) {
        self.clear();
        if let Some(first) = klines.first() {
            self.bin_size = Self::calculate_bin_size(first.4);
        }

        for &(time, open, high, low, close, buy_vol, sell_vol) in klines {
            self.on_candle(time, open, high, low, close, buy_vol, sell_vol);
        }
    }
}

/// Piecewise multi-stop linear color interpolation
pub fn interpolate_stops(colors: &[(f32, f32, f32, f32)], ratio: f32) -> (f32, f32, f32, f32) {
    if colors.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let ratio = ratio.clamp(0.0, 1.0);
    let n = (colors.len() - 1) as f32;
    let scaled = ratio * n;
    let idx = (scaled.floor() as usize).min(colors.len() - 2);
    let t = scaled - idx as f32;

    let c1 = colors[idx];
    let c2 = colors[idx + 1];

    (
        c1.0 + (c2.0 - c1.0) * t,
        c1.1 + (c2.1 - c1.1) * t,
        c1.2 + (c2.2 - c1.2) * t,
        c1.3 + (c2.3 - c1.3) * t,
    )
}

/// Interpolate between heatmap colors based on strength ratio and active theme (dark/light)
pub fn interpolate_color_themed(ratio: f32, is_dark: bool) -> (f32, f32, f32, f32) {
    let ratio = ratio.clamp(0.0, 1.0);
    if is_dark {
        // Dark theme: navy blue -> cyan -> green -> yellow -> orange -> crimson red
        let colors: [(f32, f32, f32, f32); 6] = [
            (10.0 / 255.0, 25.0 / 255.0, 60.0 / 255.0, 0.40),
            (0.0 / 255.0, 180.0 / 255.0, 235.0 / 255.0, 0.65),
            (0.0 / 255.0, 225.0 / 255.0, 110.0 / 255.0, 0.75),
            (1.0, 235.0 / 255.0, 50.0 / 255.0, 0.85),
            (1.0, 140.0 / 255.0, 20.0 / 255.0, 0.90),
            (1.0, 30.0 / 255.0, 50.0 / 255.0, 0.95),
        ];
        interpolate_stops(&colors, ratio)
    } else {
        // Light theme: light pastel blue -> rich blue -> forest green -> amber orange -> deep crimson
        let colors: [(f32, f32, f32, f32); 6] = [
            (180.0 / 255.0, 225.0 / 255.0, 250.0 / 255.0, 0.40),
            (2.0 / 255.0, 136.0 / 255.0, 209.0 / 255.0, 0.65),
            (46.0 / 255.0, 125.0 / 255.0, 50.0 / 255.0, 0.75),
            (230.0 / 255.0, 81.0 / 255.0, 0.0 / 255.0, 0.85),
            (198.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 0.90),
            (136.0 / 255.0, 14.0 / 255.0, 79.0 / 255.0, 0.95),
        ];
        interpolate_stops(&colors, ratio)
    }
}

/// Backward compatibility: defaults to dark theme interpolation
pub fn interpolate_color(ratio: f32) -> (f32, f32, f32, f32) {
    interpolate_color_themed(ratio, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liquidation_price_calculation() {
        let liq = LiquidationHeatmap::calculate_liquidation_price(100.0, 5, 0.5);
        assert!(liq > 100.0);

        let liq = LiquidationHeatmap::calculate_liquidation_price(100.0, -5, 0.5);
        assert!(liq < 100.0);
    }

    #[test]
    fn test_rma_calculation() {
        let rma = LiquidationHeatmap::calculate_rma(10.0, 20.0, 5);
        assert!((rma - 12.0).abs() < 0.001);
    }

    #[test]
    fn test_color_interpolation_dark_and_light() {
        let (_r, _g, _b, a) = interpolate_color_themed(0.0, true);
        assert!(a >= 0.35);

        let (r, _g, _b, a) = interpolate_color_themed(1.0, true);
        assert!(a > 0.9);
        assert!(r > 0.8); // Bright red in dark theme

        let (r, _g, _b, a) = interpolate_color_themed(1.0, false);
        assert!(a > 0.9);
        assert!(r > 0.4); // Crimson in light theme
    }

    #[test]
    fn test_liquidation_segments_and_truncation() {
        let mut heatmap = LiquidationHeatmap::new(LiquidationHeatmapConfig {
            enabled: true,
            leverages: vec![10], // 10x leverage
            leverage_buffer: 0.0,
            threshold: 0.1,
            ..Default::default()
        });

        // Candle 1 at time 1000: price around 100, volume 100/100
        heatmap.on_candle(1000, 100.0, 101.0, 99.0, 100.0, 100.0, 100.0);
        // Candle 2 at time 2000: price around 100
        heatmap.on_candle(2000, 100.0, 101.0, 99.0, 100.0, 100.0, 100.0);

        assert!(!heatmap.segments().is_empty());
        let active_count_before = heatmap
            .segments()
            .iter()
            .filter(|s| s.end_time.is_none())
            .count();
        assert!(active_count_before > 0);

        // Candle 3 at time 3000: huge spike upward to 150 (liquidating short resistance levels)
        heatmap.on_candle(3000, 100.0, 150.0, 100.0, 140.0, 50.0, 50.0);

        // Check that touched resistance levels have end_time == Some(3000)
        let closed_at_3000 = heatmap
            .segments()
            .iter()
            .filter(|s| s.is_resistance && s.end_time == Some(3000))
            .count();
        assert!(closed_at_3000 > 0);
    }
}
