use exchange::Trade;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BigTradesConfig {
    pub min_usd: f64,
    pub min_radius: f32,
    pub max_radius: f32,
    pub scale: f32,
    pub opacity: f32,
}

impl Default for BigTradesConfig {
    fn default() -> Self {
        Self {
            min_usd: 100_000.0,
            min_radius: 4.0,
            max_radius: 28.0,
            scale: 5.0,
            opacity: 0.65,
        }
    }
}

impl BigTradesConfig {
    pub fn new(min_usd: f64, min_radius: f32, max_radius: f32, scale: f32, opacity: f32) -> Self {
        Self {
            min_usd,
            min_radius,
            max_radius,
            scale,
            opacity,
        }
    }

    /// Computes logarithmic bubble radius for a given USD value.
    /// Formula: r = min_radius + scale * ln(1 + (usd - min_usd) / min_usd)
    /// clamped within [min_radius, max_radius].
    pub fn compute_radius(&self, usd_value: f64) -> f32 {
        if usd_value < self.min_usd || self.min_usd <= 0.0 {
            return self.min_radius;
        }
        let excess_ratio = (usd_value - self.min_usd) / self.min_usd;
        let log_scaled = self.min_radius + self.scale * (1.0 + excess_ratio).ln() as f32;
        log_scaled.clamp(self.min_radius, self.max_radius)
    }

    /// Finds visible trades within time range [start_time, end_time] using O(log N) binary search
    pub fn filter_visible_trades<'a>(
        &self,
        trades: &'a [Trade],
        start_time: u64,
        end_time: u64,
    ) -> impl Iterator<Item = &'a Trade> {
        let left_idx = trades.partition_point(|t| t.time < start_time);
        let right_idx = trades.partition_point(|t| t.time <= end_time);

        trades[left_idx..right_idx]
            .iter()
            .filter(move |t| (t.price.to_f64() * t.qty as f64) >= self.min_usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::util::Price;

    #[test]
    fn test_ter21_big_trades_logarithmic_radius_scaling() {
        let cfg = BigTradesConfig {
            min_usd: 100_000.0,
            min_radius: 5.0,
            max_radius: 30.0,
            scale: 6.0,
            opacity: 0.7,
        };

        // Exactly at threshold -> min_radius
        assert_eq!(cfg.compute_radius(100_000.0), 5.0);
        // Below threshold -> min_radius
        assert_eq!(cfg.compute_radius(50_000.0), 5.0);

        // $200k -> 5 + 6 * ln(1 + 1) = 5 + 6 * 0.69315 = 9.158
        let r_200k = cfg.compute_radius(200_000.0);
        assert!((r_200k - 9.158).abs() < 0.01);

        // $1M -> 5 + 6 * ln(1 + 9) = 5 + 6 * 2.30258 = 18.815
        let r_1m = cfg.compute_radius(1_000_000.0);
        assert!((r_1m - 18.815).abs() < 0.01);

        // Huge value -> clamped to max_radius (30.0)
        let r_huge = cfg.compute_radius(100_000_000.0);
        assert_eq!(r_huge, 30.0);
    }

    #[test]
    fn test_ter21_big_trades_partition_point_binary_search() {
        let cfg = BigTradesConfig {
            min_usd: 100_000.0,
            ..Default::default()
        };

        // 100 trades across time 1000..10000
        let mut trades = Vec::new();
        for i in 0..100 {
            trades.push(Trade {
                time: 1000 + i * 100,
                is_sell: i % 2 == 0,
                price: Price::from_f64(50_000.0),
                qty: if i % 10 == 0 { 3.0 } else { 0.1 }, // 3.0 * $50k = $150k (big trade)
            });
        }

        // Query visible window [3000, 7000]
        let visible: Vec<&Trade> = cfg.filter_visible_trades(&trades, 3000, 7000).collect();

        // Big trades in [3000..7000]:
        // i = 20 (time 3000), i = 30 (time 4000), i = 40 (time 5000), i = 50 (time 6000), i = 60 (time 7000)
        for t in &visible {
            assert!(t.time >= 3000 && t.time <= 7000);
            assert!(t.price.to_f64() * t.qty as f64 >= 100_000.0);
        }
        assert_eq!(visible.len(), 5);
    }
}
