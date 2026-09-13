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
        let high = self.high.to_f32() as f64;
        let low = self.low.to_f32() as f64;
        let close = self.close.to_f32() as f64;
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
        let high = self.high.to_f32() as f64;
        let low = self.low.to_f32() as f64;
        let close = self.close.to_f32() as f64;
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
        let high = self.kline.high.to_f32() as f64;
        let low = self.kline.low.to_f32() as f64;
        let close = self.kline.close.to_f32() as f64;
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
        let high = self.kline.high.to_f32() as f64;
        let low = self.kline.low.to_f32() as f64;
        let close = self.kline.close.to_f32() as f64;
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
}
