use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::kline::KlineDataPoint;
use exchange::Kline;

/// Supported VWAP anchor periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VwapPeriod {
    #[default]
    Daily,
    Weekly,
    Monthly,
    Session(i64),
}

/// Trait abstracting candle input for VWAP calculation.
pub trait VwapCandle {
    fn time_ms(&self) -> i64;
    fn typical_price(&self) -> f64;
    fn volume(&self) -> f64;
}

impl VwapCandle for Kline {
    #[inline]
    fn time_ms(&self) -> i64 {
        self.time as i64
    }

    #[inline]
    fn typical_price(&self) -> f64 {
        let high = self.high.to_f64();
        let low = self.low.to_f64();
        let close = self.close.to_f64();
        (high + low + close) / 3.0
    }

    #[inline]
    fn volume(&self) -> f64 {
        (self.volume.0 + self.volume.1) as f64
    }
}

impl VwapCandle for &Kline {
    #[inline]
    fn time_ms(&self) -> i64 {
        self.time as i64
    }

    #[inline]
    fn typical_price(&self) -> f64 {
        let high = self.high.to_f64();
        let low = self.low.to_f64();
        let close = self.close.to_f64();
        (high + low + close) / 3.0
    }

    #[inline]
    fn volume(&self) -> f64 {
        (self.volume.0 + self.volume.1) as f64
    }
}

impl VwapCandle for KlineDataPoint {
    #[inline]
    fn time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn typical_price(&self) -> f64 {
        let high = self.kline.high.to_f64();
        let low = self.kline.low.to_f64();
        let close = self.kline.close.to_f64();
        (high + low + close) / 3.0
    }

    #[inline]
    fn volume(&self) -> f64 {
        (self.kline.volume.0 + self.kline.volume.1) as f64
    }
}

impl VwapCandle for &KlineDataPoint {
    #[inline]
    fn time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn typical_price(&self) -> f64 {
        let high = self.kline.high.to_f64();
        let low = self.kline.low.to_f64();
        let close = self.kline.close.to_f64();
        (high + low + close) / 3.0
    }

    #[inline]
    fn volume(&self) -> f64 {
        (self.kline.volume.0 + self.kline.volume.1) as f64
    }
}

impl VwapCandle for (i64, f64, f64) {
    #[inline]
    fn time_ms(&self) -> i64 {
        self.0
    }

    #[inline]
    fn typical_price(&self) -> f64 {
        self.1
    }

    #[inline]
    fn volume(&self) -> f64 {
        self.2
    }
}

/// Calculated VWAP point with ±1σ, ±2σ, and ±3σ standard deviation bands.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VwapPoint {
    pub vwap: f64,
    pub upper_1sigma: f64,
    pub lower_1sigma: f64,
    pub upper_2sigma: f64,
    pub lower_2sigma: f64,
    pub upper_3sigma: f64,
    pub lower_3sigma: f64,
    pub sigma: f64,
}

/// O(1) incremental accumulator for VWAP calculations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VwapAccumulator {
    pub cum_vol: f64,
    pub cum_pv: f64,
    pub cum_pv2: f64,
    pub count: usize,
    pub current_bucket: i64,
}

impl VwapAccumulator {
    pub fn new(initial_bucket: i64) -> Self {
        Self {
            cum_vol: 0.0,
            cum_pv: 0.0,
            cum_pv2: 0.0,
            count: 0,
            current_bucket: initial_bucket,
        }
    }

    pub fn reset(&mut self, new_bucket: i64) {
        self.cum_vol = 0.0;
        self.cum_pv = 0.0;
        self.cum_pv2 = 0.0;
        self.count = 0;
        self.current_bucket = new_bucket;
    }

    /// O(1) incremental update.
    pub fn update(&mut self, price: f64, volume: f64) -> Option<VwapPoint> {
        let v = volume.max(0.0);
        if !v.is_finite() || !price.is_finite() || v <= 0.0 {
            return self.current_point();
        }

        self.cum_vol += v;
        self.cum_pv += price * v;
        self.cum_pv2 += price * price * v;
        self.count += 1;

        self.current_point()
    }

    /// O(1) calculation of current VWAP and standard deviation bands.
    pub fn current_point(&self) -> Option<VwapPoint> {
        if self.cum_vol <= 0.0 {
            return None;
        }

        let vwap = self.cum_pv / self.cum_vol;
        let raw_variance = (self.cum_pv2 / self.cum_vol - vwap * vwap).max(0.0);
        let threshold = 1e-10 * vwap.powi(2);
        let variance = if self.count <= 1 || raw_variance <= threshold {
            0.0
        } else {
            raw_variance
        };
        let sigma = variance.sqrt();

        Some(VwapPoint {
            vwap,
            upper_1sigma: vwap + sigma,
            lower_1sigma: vwap - sigma,
            upper_2sigma: vwap + 2.0 * sigma,
            lower_2sigma: vwap - 2.0 * sigma,
            upper_3sigma: vwap + 3.0 * sigma,
            lower_3sigma: vwap - 3.0 * sigma,
            sigma,
        })
    }
}

/// Calculates anchor bucket start timestamp in milliseconds.
pub fn get_period_bucket(time_ms: i64, period: VwapPeriod) -> i64 {
    match period {
        VwapPeriod::Session(duration_ms) => {
            let dur = duration_ms.max(1);
            time_ms.div_euclid(dur) * dur
        }
        VwapPeriod::Daily => {
            const DAY_MS: i64 = 86_400_000;
            time_ms.div_euclid(DAY_MS) * DAY_MS
        }
        VwapPeriod::Weekly => {
            const DAY_MS: i64 = 86_400_000;
            const WEEK_MS: i64 = 7 * DAY_MS;
            // 1970-01-01 was Thursday. Thursday (0) + 3 days = Sunday. Monday was -3 days (-259_200_000 ms).
            let shifted = time_ms + 3 * DAY_MS;
            shifted.div_euclid(WEEK_MS) * WEEK_MS - 3 * DAY_MS
        }
        VwapPeriod::Monthly => {
            if let Some(dt) = DateTime::from_timestamp_millis(time_ms) {
                let year = dt.year();
                let month = dt.month();
                Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
                    .single()
                    .map(|d| d.timestamp_millis())
                    .unwrap_or_else(|| time_ms.div_euclid(30 * 86_400_000) * (30 * 86_400_000))
            } else {
                time_ms.div_euclid(30 * 86_400_000) * (30 * 86_400_000)
            }
        }
    }
}

/// Real-time VWAP tracker supporting dynamic streaming updates.
#[derive(Debug, Clone)]
pub struct VwapTracker {
    pub period: VwapPeriod,
    pub accumulator: VwapAccumulator,
}

impl VwapTracker {
    pub fn new(period: VwapPeriod) -> Self {
        Self {
            period,
            accumulator: VwapAccumulator::new(i64::MIN),
        }
    }

    /// Feeds a new price and volume point at a given timestamp in O(1).
    pub fn on_data_point(&mut self, time_ms: i64, price: f64, volume: f64) -> Option<VwapPoint> {
        let bucket = get_period_bucket(time_ms, self.period);
        if self.accumulator.current_bucket != bucket {
            self.accumulator.reset(bucket);
        }
        self.accumulator.update(price, volume)
    }

    pub fn current_point(&self) -> Option<VwapPoint> {
        self.accumulator.current_point()
    }
}

/// Batch calculates VWAP series across a slice of candles.
pub fn calculate_vwap_series<C: VwapCandle>(
    candles: &[C],
    period: VwapPeriod,
) -> Vec<Option<VwapPoint>> {
    let mut out = Vec::with_capacity(candles.len());
    let mut tracker = VwapTracker::new(period);

    for c in candles {
        let pt = tracker.on_data_point(c.time_ms(), c.typical_price(), c.volume());
        out.push(pt);
    }

    out
}

/// Real-time rolling window VWAP tracker supporting sliding window updates in O(1) amortized.
#[derive(Debug, Clone)]
pub struct RollingVwapTracker {
    /// Window duration in milliseconds (e.g. 24 * 3600 * 1000 = 86_400_000 for 24h)
    pub window_ms: i64,
    /// Sliding window buffer of (timestamp_ms, price, volume)
    history: std::collections::VecDeque<(i64, f64, f64)>,
    cum_vol: f64,
    cum_pv: f64,
    cum_pv2: f64,
}

impl RollingVwapTracker {
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms: window_ms.max(1),
            history: std::collections::VecDeque::new(),
            cum_vol: 0.0,
            cum_pv: 0.0,
            cum_pv2: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.cum_vol = 0.0;
        self.cum_pv = 0.0;
        self.cum_pv2 = 0.0;
    }

    /// Feeds a new price and volume point at a given timestamp in O(1) amortized.
    pub fn on_data_point(&mut self, time_ms: i64, price: f64, volume: f64) -> Option<VwapPoint> {
        let v = volume.max(0.0);
        if !v.is_finite() || !price.is_finite() || v <= 0.0 {
            self.evict_expired(time_ms);
            return self.current_point();
        }

        self.history.push_back((time_ms, price, v));
        self.cum_vol += v;
        self.cum_pv += price * v;
        self.cum_pv2 += price * price * v;

        self.evict_expired(time_ms);
        self.current_point()
    }

    fn evict_expired(&mut self, current_time_ms: i64) {
        let cutoff = current_time_ms.saturating_sub(self.window_ms);
        while let Some(&(t, p, v)) = self.history.front() {
            if t < cutoff {
                self.history.pop_front();
                self.cum_vol -= v;
                self.cum_pv -= p * v;
                self.cum_pv2 -= p * p * v;
            } else {
                break;
            }
        }

        if self.history.is_empty() || self.cum_vol <= 1e-9 {
            self.cum_vol = 0.0;
            self.cum_pv = 0.0;
            self.cum_pv2 = 0.0;
        }
    }

    pub fn current_point(&self) -> Option<VwapPoint> {
        if self.cum_vol <= 0.0 || self.history.is_empty() {
            return None;
        }

        let vwap = self.cum_pv / self.cum_vol;
        let raw_variance = (self.cum_pv2 / self.cum_vol - vwap * vwap).max(0.0);
        let threshold = 1e-10 * vwap.powi(2);
        let variance = if self.history.len() <= 1 || raw_variance <= threshold {
            0.0
        } else {
            raw_variance
        };
        let sigma = variance.sqrt();

        Some(VwapPoint {
            vwap,
            upper_1sigma: vwap + sigma,
            lower_1sigma: vwap - sigma,
            upper_2sigma: vwap + 2.0 * sigma,
            lower_2sigma: vwap - 2.0 * sigma,
            upper_3sigma: vwap + 3.0 * sigma,
            lower_3sigma: vwap - 3.0 * sigma,
            sigma,
        })
    }
}

/// Batch calculates Rolling VWAP series across a slice of candles.
pub fn calculate_rolling_vwap_series<C: VwapCandle>(
    candles: &[C],
    window_ms: i64,
) -> Vec<Option<VwapPoint>> {
    let mut out = Vec::with_capacity(candles.len());
    let mut tracker = RollingVwapTracker::new(window_ms);

    for c in candles {
        let pt = tracker.on_data_point(c.time_ms(), c.typical_price(), c.volume());
        out.push(pt);
    }

    out
}

/// Calculated Multi-period Rolling VWAP point (7d, 30d, 90d, 365d).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct MultiRollingVwapPoint {
    pub d7: Option<VwapPoint>,
    pub d30: Option<VwapPoint>,
    pub d90: Option<VwapPoint>,
    pub d365: Option<VwapPoint>,
}

/// Real-time multi-period rolling VWAP tracker (7d, 30d, 90d, 365d).
#[derive(Debug, Clone)]
pub struct MultiRollingVwapTracker {
    pub tracker_7d: RollingVwapTracker,
    pub tracker_30d: RollingVwapTracker,
    pub tracker_90d: RollingVwapTracker,
    pub tracker_365d: RollingVwapTracker,
}

impl Default for MultiRollingVwapTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiRollingVwapTracker {
    pub const DAY_MS: i64 = 86_400_000;
    pub const WINDOW_7D_MS: i64 = 7 * Self::DAY_MS;
    pub const WINDOW_30D_MS: i64 = 30 * Self::DAY_MS;
    pub const WINDOW_90D_MS: i64 = 90 * Self::DAY_MS;
    pub const WINDOW_365D_MS: i64 = 365 * Self::DAY_MS;

    pub fn new() -> Self {
        Self {
            tracker_7d: RollingVwapTracker::new(Self::WINDOW_7D_MS),
            tracker_30d: RollingVwapTracker::new(Self::WINDOW_30D_MS),
            tracker_90d: RollingVwapTracker::new(Self::WINDOW_90D_MS),
            tracker_365d: RollingVwapTracker::new(Self::WINDOW_365D_MS),
        }
    }

    pub fn reset(&mut self) {
        self.tracker_7d.reset();
        self.tracker_30d.reset();
        self.tracker_90d.reset();
        self.tracker_365d.reset();
    }

    /// Feeds a new price and volume point at a given timestamp in O(1) amortized.
    pub fn on_data_point(
        &mut self,
        time_ms: i64,
        price: f64,
        volume: f64,
    ) -> MultiRollingVwapPoint {
        MultiRollingVwapPoint {
            d7: self.tracker_7d.on_data_point(time_ms, price, volume),
            d30: self.tracker_30d.on_data_point(time_ms, price, volume),
            d90: self.tracker_90d.on_data_point(time_ms, price, volume),
            d365: self.tracker_365d.on_data_point(time_ms, price, volume),
        }
    }

    pub fn current_point(&self) -> MultiRollingVwapPoint {
        MultiRollingVwapPoint {
            d7: self.tracker_7d.current_point(),
            d30: self.tracker_30d.current_point(),
            d90: self.tracker_90d.current_point(),
            d365: self.tracker_365d.current_point(),
        }
    }
}

/// Batch calculates Multi-Period Rolling VWAP series across a slice of candles.
pub fn calculate_multi_rolling_vwap_series<C: VwapCandle>(
    candles: &[C],
) -> Vec<MultiRollingVwapPoint> {
    let mut out = Vec::with_capacity(candles.len());
    let mut tracker = MultiRollingVwapTracker::new();

    for c in candles {
        let pt = tracker.on_data_point(c.time_ms(), c.typical_price(), c.volume());
        out.push(pt);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vwap_single_candle_zero_variance() {
        let candles = vec![(1_000_000i64, 100.0f64, 50.0f64)];
        let res = calculate_vwap_series(&candles, VwapPeriod::Daily);
        let pt = res[0].expect("VWAP point exists");
        assert_eq!(pt.vwap, 100.0);
        assert_eq!(pt.sigma, 0.0);
        assert_eq!(pt.upper_1sigma, 100.0);
        assert_eq!(pt.lower_1sigma, 100.0);
        assert_eq!(pt.upper_2sigma, 100.0);
        assert_eq!(pt.lower_2sigma, 100.0);
        assert_eq!(pt.upper_3sigma, 100.0);
        assert_eq!(pt.lower_3sigma, 100.0);
    }

    #[test]
    fn test_vwap_incremental_matches_batch() {
        let candles = vec![
            (1_000i64, 100.0f64, 10.0f64),
            (2_000i64, 110.0f64, 20.0f64),
            (3_000i64, 105.0f64, 15.0f64),
        ];

        let batch = calculate_vwap_series(&candles, VwapPeriod::Daily);

        let mut tracker = VwapTracker::new(VwapPeriod::Daily);
        for (i, c) in candles.iter().enumerate() {
            let inc = tracker.on_data_point(c.0, c.1, c.2);
            assert_eq!(inc, batch[i]);
        }

        // Mathematical verification for 2nd candle:
        // cum_vol = 30
        // cum_pv = 100*10 + 110*20 = 1000 + 2200 = 3200
        // vwap = 3200 / 30 = 106.66666666666667
        let p1 = batch[1].unwrap();
        assert!((p1.vwap - 3200.0 / 30.0).abs() < 1e-10);
        assert!(p1.sigma > 0.0);
        assert_eq!(p1.upper_1sigma, p1.vwap + p1.sigma);
        assert_eq!(p1.lower_1sigma, p1.vwap - p1.sigma);
        assert_eq!(p1.upper_2sigma, p1.vwap + 2.0 * p1.sigma);
        assert_eq!(p1.lower_2sigma, p1.vwap - 2.0 * p1.sigma);
        assert_eq!(p1.upper_3sigma, p1.vwap + 3.0 * p1.sigma);
        assert_eq!(p1.lower_3sigma, p1.vwap - 3.0 * p1.sigma);
    }

    #[test]
    fn test_vwap_daily_session_reset() {
        const DAY_MS: i64 = 86_400_000;
        let candles = vec![
            (DAY_MS - 1000, 100.0, 10.0), // Day 0
            (DAY_MS + 1000, 200.0, 10.0), // Day 1
        ];

        let res = calculate_vwap_series(&candles, VwapPeriod::Daily);
        assert_eq!(res[0].unwrap().vwap, 100.0);
        assert_eq!(res[1].unwrap().vwap, 200.0); // Reset at Day 1 00:00 UTC
    }

    #[test]
    fn test_vwap_weekly_session_reset() {
        // 1970-01-01 was Thursday (day 0)
        // 1970-01-04 was Sunday (day 3)
        // 1970-01-05 was Monday (day 4)
        const DAY_MS: i64 = 86_400_000;
        let sun = 3 * DAY_MS + 1000;
        let mon = 4 * DAY_MS + 1000;

        let b_sun = get_period_bucket(sun, VwapPeriod::Weekly);
        let b_mon = get_period_bucket(mon, VwapPeriod::Weekly);

        assert_ne!(b_sun, b_mon);
        assert_eq!(b_mon, 4 * DAY_MS); // Monday 00:00 UTC
    }

    #[test]
    fn test_vwap_flat_price_multi_candle_zero_variance() {
        let candles = vec![
            (1_000i64, 50.0, 10.0),
            (2_000i64, 50.0, 20.0),
            (3_000i64, 50.0, 30.0),
        ];

        let res = calculate_vwap_series(&candles, VwapPeriod::Daily);
        for pt in res {
            let p = pt.unwrap();
            assert_eq!(p.vwap, 50.0);
            assert_eq!(p.sigma, 0.0);
            assert_eq!(p.upper_1sigma, 50.0);
            assert_eq!(p.lower_1sigma, 50.0);
        }
    }

    #[test]
    fn test_rolling_vwap_sliding_window_eviction() {
        const WINDOW_MS: i64 = 60_000; // 1 minute
        let mut tracker = RollingVwapTracker::new(WINDOW_MS);

        // Point 1 at t=0
        let p1 = tracker.on_data_point(0, 100.0, 10.0).unwrap();
        assert_eq!(p1.vwap, 100.0);

        // Point 2 at t=30,000 (within window)
        // cum_vol = 30, cum_pv = 100*10 + 200*20 = 5000 -> vwap = 5000 / 30 = 166.666...
        let p2 = tracker.on_data_point(30_000, 200.0, 20.0).unwrap();
        assert!((p2.vwap - 5000.0 / 30.0).abs() < 1e-10);

        // Point 3 at t=70,000 (Point 1 at t=0 is now expired since 70,000 - 60,000 = 10,000 > 0)
        // Window contains Point 2 (200.0 * 20.0) and Point 3 (300.0 * 10.0)
        // cum_vol = 30, cum_pv = 4000 + 3000 = 7000 -> vwap = 7000 / 30 = 233.333...
        let p3 = tracker.on_data_point(70_000, 300.0, 10.0).unwrap();
        assert!((p3.vwap - 7000.0 / 30.0).abs() < 1e-10);

        // Point 4 at t=150,000 (both Point 1, 2, 3 expired)
        // Window contains only Point 4 (400.0 * 5.0)
        let p4 = tracker.on_data_point(150_000, 400.0, 5.0).unwrap();
        assert_eq!(p4.vwap, 400.0);
        assert_eq!(p4.sigma, 0.0);
    }

    #[test]
    fn test_rolling_vwap_flat_price_zero_variance() {
        let candles = vec![
            (1_000i64, 100.0, 10.0),
            (2_000i64, 100.0, 20.0),
            (3_000i64, 100.0, 30.0),
        ];
        let res = calculate_rolling_vwap_series(&candles, 5_000);
        for pt in res {
            let p = pt.unwrap();
            assert_eq!(p.vwap, 100.0);
            assert_eq!(p.sigma, 0.0);
            assert_eq!(p.upper_1sigma, 100.0);
            assert_eq!(p.lower_1sigma, 100.0);
        }
    }

    #[test]
    fn test_multi_rolling_vwap_windows() {
        const DAY_MS: i64 = 86_400_000;
        let mut tracker = MultiRollingVwapTracker::new();

        // Day 0: Initial point
        let pt0 = tracker.on_data_point(0, 100.0, 10.0);
        assert_eq!(pt0.d7.unwrap().vwap, 100.0);
        assert_eq!(pt0.d30.unwrap().vwap, 100.0);
        assert_eq!(pt0.d90.unwrap().vwap, 100.0);
        assert_eq!(pt0.d365.unwrap().vwap, 100.0);

        // Day 10: 7-day window has expired Day 0 point, while 30d, 90d, 365d still contain it
        let pt10 = tracker.on_data_point(10 * DAY_MS, 200.0, 10.0);
        assert_eq!(pt10.d7.unwrap().vwap, 200.0); // Only Day 10 in 7d window
        assert_eq!(pt10.d30.unwrap().vwap, 150.0); // Both Day 0 and Day 10 in 30d
        assert_eq!(pt10.d90.unwrap().vwap, 150.0); // Both in 90d
        assert_eq!(pt10.d365.unwrap().vwap, 150.0); // Both in 365d

        // Day 40: 30-day window has expired Day 0 point, but 90d and 365d keep it
        let pt40 = tracker.on_data_point(40 * DAY_MS, 300.0, 10.0);
        assert_eq!(pt40.d7.unwrap().vwap, 300.0);
        // In 30d: Day 10 (200 * 10) and Day 40 (300 * 10) -> sum = 5000 / 20 = 250
        assert_eq!(pt40.d30.unwrap().vwap, 250.0);
        // In 90d: Day 0 (1000) + Day 10 (2000) + Day 40 (3000) -> sum = 6000 / 30 = 200
        assert_eq!(pt40.d90.unwrap().vwap, 200.0);
        assert_eq!(pt40.d365.unwrap().vwap, 200.0);

        // Day 100: 90-day window cutoff is 100 - 90 = 10. Day 0 expired, but Day 10, Day 40, Day 100 are present:
        // (200*10 + 300*10 + 400*10) / 30 = 9000 / 30 = 300.0
        let pt100 = tracker.on_data_point(100 * DAY_MS, 400.0, 10.0);
        assert_eq!(pt100.d7.unwrap().vwap, 400.0);
        assert_eq!(pt100.d30.unwrap().vwap, 400.0);
        assert_eq!(pt100.d90.unwrap().vwap, 300.0);
        // In 365d: Day 0 (1000) + Day 10 (2000) + Day 40 (3000) + Day 100 (4000) -> 10000 / 40 = 250
        assert_eq!(pt100.d365.unwrap().vwap, 250.0);

        // Batch helper matches incremental
        let candles = vec![
            (0i64, 100.0, 10.0),
            (10 * DAY_MS, 200.0, 10.0),
            (40 * DAY_MS, 300.0, 10.0),
            (100 * DAY_MS, 400.0, 10.0),
        ];
        let series = calculate_multi_rolling_vwap_series(&candles);
        assert_eq!(series.len(), 4);
        assert_eq!(series[3].d365.unwrap().vwap, 250.0);
    }
}
