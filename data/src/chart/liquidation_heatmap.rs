//! Liquidation Heatmap
//!
//! Simulates liquidation zones based on volume-weighted leverage positions.
//! This is a simulation - exchanges don't provide actual liquidation data.
//!
//! The indicator calculates where liquidations would occur for positions opened
//! at recent price levels with various leverage settings, then accumulates
//! "strength" based on trading volume at those levels.

use exchange::util::Price;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Configuration for the liquidation heatmap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationHeatmapConfig {
    /// Leverage values to simulate (positive = longs, negative = shorts)
    /// Default: [5, 20, 100] which creates both long and short levels
    pub leverages: Vec<i32>,
    /// Buffer percentage added to liquidation distance (default: 0.5%)
    pub leverage_buffer: f32,
    /// Minimum strength threshold to display a zone (default: 2.0)
    pub threshold: f32,
    /// Strength multiplier (default: 0.5)
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
            leverages: vec![5, 20, 100],
            leverage_buffer: 0.5,
            threshold: 2.0,
            strength_multiplier: 0.5,
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

/// A single liquidation zone cell
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
    /// Active liquidation cells keyed by cell_id
    pub cells: FxHashMap<i64, LiquidationCell>,
    /// Current ATR value
    pub atr: f32,
    /// ATR RMA state (running moving average)
    atr_rma: f32,
    /// Volume SMA buffer for buy volume
    buy_vol_buffer: Vec<f32>,
    /// Volume SMA buffer for sell volume
    sell_vol_buffer: Vec<f32>,
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
            return 1.0; // Avoid division by zero
        }
        buffer.iter().sum::<f32>() / buffer.len() as f32
    }

    /// Update ATR with new price range
    fn update_atr(&mut self, high: f32, low: f32) {
        let price_range = high - low;
        self.price_ranges.push(price_range);

        // Keep only needed history
        if self.price_ranges.len() > self.config.auto_scale_length {
            self.price_ranges.remove(0);
        }

        // Calculate ATR using RMA
        if self.atr_rma == 0.0 && !self.price_ranges.is_empty() {
            // Initialize with SMA
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

        // Keep only needed history
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
        let mm = buffer + 0.01 / lev_abs; // Maintenance margin
        let offset = lev_distance + mm;

        if leverage > 0 {
            // Long position - liquidation is below entry
            // But we calculate where shorts would get liquidated (resistance)
            source_price * (1.0 + offset)
        } else {
            // Short position - liquidation is above entry
            // But we calculate where longs would get liquidated (support)
            source_price * (1.0 - offset)
        }
    }

    /// Round price to grid cell
    fn round_to_cell(&self, price: f32, cell_size: f32, is_resistance: bool) -> i64 {
        if cell_size <= 0.0 {
            return 0;
        }
        if is_resistance {
            // For resistance, floor to lower boundary
            (price / cell_size).floor() as i64
        } else {
            // For support, ceil to upper boundary
            (price / cell_size).ceil() as i64
        }
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

        // Update ATR
        self.update_atr(high, low);

        // Update volume buffers
        self.update_volume_buffers(buy_volume, sell_volume);

        // Need at least 2 bars for previous price
        if self.current_bar < 2 {
            return;
        }

        // Calculate cell size
        let cell_size = self.atr * self.config.auto_scale;
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

        // Process each leverage
        for &base_leverage in &self.config.leverages.clone() {
            // Positive leverage = long positions, liquidation creates resistance
            let liq_price_resistance =
                Self::calculate_liquidation_price(long_source, base_leverage, self.config.leverage_buffer);
            let cell_id_res = self.round_to_cell(liq_price_resistance, cell_size, true);

            // Add or update resistance cell
            self.add_or_update_cell(cell_id_res, cell_size, sell_ratio, true, time);

            // Negative leverage = short positions, liquidation creates support
            let liq_price_support =
                Self::calculate_liquidation_price(short_source, -base_leverage, self.config.leverage_buffer);
            let cell_id_sup = self.round_to_cell(liq_price_support, cell_size, false);

            // Add or update support cell
            self.add_or_update_cell(cell_id_sup, cell_size, buy_ratio, false, time);
        }

        // Check for zone touches and apply fading
        self.process_touches_and_fading(high, low, time);
    }

    /// Add or update a cell
    fn add_or_update_cell(
        &mut self,
        cell_id: i64,
        cell_size: f32,
        ratio: f32,
        is_resistance: bool,
        time: u64,
    ) {
        let cell_price = cell_id as f32 * cell_size;
        let half_cell = cell_size / 2.0;

        self.cells
            .entry(cell_id)
            .and_modify(|cell| {
                cell.strength += ratio;
                cell.count += 1;
            })
            .or_insert_with(|| LiquidationCell {
                cell_id,
                top: Price::from_f32(cell_price + half_cell),
                bottom: Price::from_f32(cell_price - half_cell),
                strength: ratio,
                count: 1,
                created_at_bar: self.current_bar,
                is_resistance,
            });
    }

    /// Process zone touches and apply fading
    fn process_touches_and_fading(&mut self, high: f32, low: f32, _time: u64) {
        let fade_start = self.config.fade_start as u64;
        let fade_amount = self.config.fade_amount;
        let current_bar = self.current_bar;

        // Collect cells to remove
        let cells_to_remove: Vec<i64> = self
            .cells
            .iter()
            .filter(|(_, cell)| {
                let cell_price = cell.bottom.to_f32() + (cell.top.to_f32() - cell.bottom.to_f32()) / 2.0;

                // Check if price touched the zone
                if cell.is_resistance && high >= cell_price {
                    return true; // Resistance touched by high
                }
                if !cell.is_resistance && low <= cell_price {
                    return true; // Support touched by low
                }

                false
            })
            .map(|(id, _)| *id)
            .collect();

        // Remove touched cells
        for id in cells_to_remove {
            self.cells.remove(&id);
        }

        // Apply fading
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
                self.cells.remove(&id);
            }
        }
    }

    /// Get visible cells sorted by distance from current price
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
                    let cell_price = cell.bottom.to_f32() + (cell.top.to_f32() - cell.bottom.to_f32()) / 2.0;
                    let distance_pct = ((cell_price - current_price) / current_price).abs() * 100.0;
                    if distance_pct > max_distance {
                        return false;
                    }
                }

                true
            })
            .collect();

        // Sort by distance from current price
        visible.sort_by(|a, b| {
            let a_price = a.bottom.to_f32() + (a.top.to_f32() - a.bottom.to_f32()) / 2.0;
            let b_price = b.bottom.to_f32() + (b.top.to_f32() - b.bottom.to_f32()) / 2.0;
            let a_dist = (a_price - current_price).abs();
            let b_dist = (b_price - current_price).abs();
            a_dist.partial_cmp(&b_dist).unwrap_or(std::cmp::Ordering::Equal)
        });

        visible.truncate(limit);
        visible
    }

    /// Rebuild heatmap from historical kline data
    pub fn rebuild_from_klines(&mut self, klines: &[(u64, f32, f32, f32, f32, f32, f32)]) {
        self.clear();

        for &(time, open, high, low, close, buy_vol, sell_vol) in klines {
            self.on_candle(time, open, high, low, close, buy_vol, sell_vol);
        }
    }
}

/// Interpolate between heatmap colors based on strength ratio
pub fn interpolate_color(ratio: f32) -> (f32, f32, f32, f32) {
    // Color palette from specification (in 0-1 range)
    let colors: [(f32, f32, f32, f32); 5] = [
        (66.0 / 255.0, 3.0 / 255.0, 81.0 / 255.0, 0.0),      // 0.0 - transparent purple
        (63.0 / 255.0, 56.0 / 255.0, 113.0 / 255.0, 0.5),    // 0.25 - dark blue
        (38.0 / 255.0, 130.0 / 255.0, 140.0 / 255.0, 0.65),  // 0.5 - teal
        (76.0 / 255.0, 152.0 / 255.0, 134.0 / 255.0, 0.8),   // 0.75 - green
        (240.0 / 255.0, 218.0 / 255.0, 24.0 / 255.0, 0.95),  // 1.0 - yellow
    ];

    let ratio = ratio.clamp(0.0, 1.0);

    // Find the two colors to interpolate between
    let scaled = ratio * 4.0;
    let idx = (scaled.floor() as usize).min(3);
    let t = scaled - idx as f32;

    let c1 = colors[idx];
    let c2 = colors[idx + 1];

    // Linear interpolation
    (
        c1.0 + (c2.0 - c1.0) * t,
        c1.1 + (c2.1 - c1.1) * t,
        c1.2 + (c2.2 - c1.2) * t,
        c1.3 + (c2.3 - c1.3) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liquidation_price_calculation() {
        // Test long liquidation (5x leverage)
        let liq = LiquidationHeatmap::calculate_liquidation_price(100.0, 5, 0.5);
        assert!(liq > 100.0); // Liquidation above entry for resistance

        // Test short liquidation (5x leverage)
        let liq = LiquidationHeatmap::calculate_liquidation_price(100.0, -5, 0.5);
        assert!(liq < 100.0); // Liquidation below entry for support
    }

    #[test]
    fn test_rma_calculation() {
        let rma = LiquidationHeatmap::calculate_rma(10.0, 20.0, 5);
        // (10 * 4 + 20) / 5 = 60/5 = 12
        assert!((rma - 12.0).abs() < 0.001);
    }

    #[test]
    fn test_color_interpolation() {
        let (r, g, b, a) = interpolate_color(0.0);
        assert!(a < 0.1); // Should be nearly transparent

        let (r, g, b, a) = interpolate_color(1.0);
        assert!(a > 0.9); // Should be nearly opaque
        assert!(r > 0.8); // Should be yellow-ish
    }
}
