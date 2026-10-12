use crate::chart::kline::KlineDataPoint;
use exchange::TickMultiplier;
use exchange::util::{Price, PriceStep};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VolumeSearchSortCol {
    #[default]
    MaxVolume,
    Time,
    PocPrice,
    BarRange,
    TotalVolume,
    Delta,
    BuyVol,
    SellVol,
    MaxDelta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortOrder {
    #[default]
    Descending,
    Ascending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeSearchRow {
    pub index: usize,
    pub timestamp: u64,
    pub max_volume: f32,
    pub poc_price: Price,
    pub bar_range_ticks: u32,
    pub total_volume: f32,
    pub total_delta: f32,
    pub buy_volume: f32,
    pub sell_volume: f32,
    pub max_delta: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeSearchQuery {
    pub start_time: u64,
    pub end_time: u64,
    pub min_volume: f32,
    pub tick_multiplier: TickMultiplier,
    pub base_tick_size: PriceStep,
    pub start_minute_of_day: u16,
    pub end_minute_of_day: u16,
}

impl Default for VolumeSearchQuery {
    fn default() -> Self {
        Self {
            start_time: 0,
            end_time: u64::MAX,
            min_volume: 0.0,
            tick_multiplier: TickMultiplier(1),
            base_tick_size: PriceStep::from_f32(0.1),
            start_minute_of_day: 0,
            end_minute_of_day: 1439,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VolumeSearchSummary {
    pub count: usize,
    pub avg_max_volume: f32,
    pub avg_total_volume: f32,
    pub avg_delta: f32,
    pub avg_bar_range: f32,
}

/// Executes volume cluster search across candles within the specified time range.
/// Aggregates trades at `base_tick_size * tick_multiplier`.
/// Complexity: O(K * M + K log K) where K = candles count, M = average distinct price levels per candle.
pub fn search_volume_candles(
    datapoints: &BTreeMap<u64, KlineDataPoint>,
    query: &VolumeSearchQuery,
) -> (Vec<VolumeSearchRow>, VolumeSearchSummary) {
    let mut rows = Vec::new();
    let effective_step = PriceStep {
        units: query
            .base_tick_size
            .units
            .saturating_mul(query.tick_multiplier.0 as i64)
            .max(1),
    };
    let step_units = effective_step.units.max(1);

    for (t, dp) in datapoints.range(query.start_time..=query.end_time) {
        // Time of day filter (0..1439 minutes)
        let minute_of_day = ((*t / 60_000) % 1440) as u16;
        if query.start_minute_of_day <= query.end_minute_of_day {
            if minute_of_day < query.start_minute_of_day || minute_of_day > query.end_minute_of_day
            {
                continue;
            }
        } else {
            // Wrapped over midnight (e.g. 22:00 - 04:00)
            if minute_of_day < query.start_minute_of_day && minute_of_day > query.end_minute_of_day
            {
                continue;
            }
        }

        let mut binned: FxHashMap<Price, (f32, f32)> = FxHashMap::default();
        if !dp.footprint.is_empty() {
            for (price, grouped) in &dp.footprint.trades {
                let rounded = price.round_to_step(effective_step);
                let entry = binned.entry(rounded).or_insert((0.0, 0.0));
                entry.0 += grouped.buy_qty;
                entry.1 += grouped.sell_qty;
            }
        }

        let mut max_vol = 0.0_f32;
        let mut poc_price = dp.kline.close;
        let mut max_cluster_delta = 0.0_f32;
        let mut candle_buy_vol = 0.0_f32;
        let mut candle_sell_vol = 0.0_f32;

        if !binned.is_empty() {
            for (p, (buy_vol, sell_vol)) in &binned {
                let total = buy_vol + sell_vol;
                candle_buy_vol += buy_vol;
                candle_sell_vol += sell_vol;
                let cluster_delta = buy_vol - sell_vol;
                if cluster_delta.abs() > max_cluster_delta.abs() {
                    max_cluster_delta = cluster_delta;
                }
                if total > max_vol {
                    max_vol = total;
                    poc_price = *p;
                }
            }
        } else {
            // Fallback to kline volumes if footprint empty
            candle_buy_vol = dp.kline.volume.0;
            candle_sell_vol = dp.kline.volume.1;
            max_vol = candle_buy_vol + candle_sell_vol;
            poc_price = dp.kline.close;
        }

        let total_vol = candle_buy_vol + candle_sell_vol;
        if max_vol < query.min_volume {
            continue;
        }

        let bar_range_ticks =
            (dp.kline.high.units.saturating_sub(dp.kline.low.units) / step_units).max(0) as u32;

        rows.push(VolumeSearchRow {
            index: 0,
            timestamp: *t,
            max_volume: max_vol,
            poc_price,
            bar_range_ticks,
            total_volume: total_vol,
            total_delta: candle_buy_vol - candle_sell_vol,
            buy_volume: candle_buy_vol,
            sell_volume: candle_sell_vol,
            max_delta: max_cluster_delta,
        });
    }

    let summary = compute_summary(&rows);
    (rows, summary)
}

pub fn sort_rows(rows: &mut [VolumeSearchRow], col: VolumeSearchSortCol, order: SortOrder) {
    rows.sort_by(|a, b| {
        let cmp = match col {
            VolumeSearchSortCol::MaxVolume => a
                .max_volume
                .partial_cmp(&b.max_volume)
                .unwrap_or(std::cmp::Ordering::Equal),
            VolumeSearchSortCol::Time => a.timestamp.cmp(&b.timestamp),
            VolumeSearchSortCol::PocPrice => a.poc_price.cmp(&b.poc_price),
            VolumeSearchSortCol::BarRange => a.bar_range_ticks.cmp(&b.bar_range_ticks),
            VolumeSearchSortCol::TotalVolume => a
                .total_volume
                .partial_cmp(&b.total_volume)
                .unwrap_or(std::cmp::Ordering::Equal),
            VolumeSearchSortCol::Delta => a
                .total_delta
                .abs()
                .partial_cmp(&b.total_delta.abs())
                .unwrap_or(std::cmp::Ordering::Equal),
            VolumeSearchSortCol::BuyVol => a
                .buy_volume
                .partial_cmp(&b.buy_volume)
                .unwrap_or(std::cmp::Ordering::Equal),
            VolumeSearchSortCol::SellVol => a
                .sell_volume
                .partial_cmp(&b.sell_volume)
                .unwrap_or(std::cmp::Ordering::Equal),
            VolumeSearchSortCol::MaxDelta => a
                .max_delta
                .abs()
                .partial_cmp(&b.max_delta.abs())
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        match order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        }
    });

    for (i, row) in rows.iter_mut().enumerate() {
        row.index = i + 1;
    }
}

pub fn compute_summary(rows: &[VolumeSearchRow]) -> VolumeSearchSummary {
    if rows.is_empty() {
        return VolumeSearchSummary::default();
    }
    let n = rows.len() as f32;
    let sum_max_vol: f32 = rows.iter().map(|r| r.max_volume).sum();
    let sum_total_vol: f32 = rows.iter().map(|r| r.total_volume).sum();
    let sum_delta: f32 = rows.iter().map(|r| r.total_delta).sum();
    let sum_bar_range: f32 = rows.iter().map(|r| r.bar_range_ticks as f32).sum();

    VolumeSearchSummary {
        count: rows.len(),
        avg_max_volume: sum_max_vol / n,
        avg_total_volume: sum_total_vol / n,
        avg_delta: sum_delta / n,
        avg_bar_range: sum_bar_range / n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::kline::KlineTrades;
    use exchange::{Kline, Trade};

    fn make_test_dp(
        time: u64,
        high: f32,
        low: f32,
        close: f32,
        trades: &[(f32, f32, bool)],
    ) -> KlineDataPoint {
        let step = PriceStep::from_f32(0.1);
        let mut dp = KlineDataPoint {
            kline: Kline {
                time,
                open: Price::from_f32(low),
                high: Price::from_f32(high),
                low: Price::from_f32(low),
                close: Price::from_f32(close),
                volume: (0.0, 0.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: true,
        };

        for (price, qty, is_sell) in trades {
            dp.add_trade(
                &Trade {
                    time,
                    price: Price::from_f32(*price),
                    qty: *qty,
                    is_sell: *is_sell,
                },
                step,
            );
        }
        dp
    }

    #[test]
    fn test_volume_search_poc_and_tick_multiplier() {
        let mut datapoints = BTreeMap::new();
        // Candle 1: High=2740, Low=2730, clusters at 2732 (vol 100), 2734 (vol 500)
        let dp1 = make_test_dp(
            1726927200000, // 21.09.2026 14:00 UTC
            2740.0,
            2730.0,
            2735.0,
            &[
                (2732.0, 40.0, false),
                (2732.0, 60.0, true),
                (2734.0, 300.0, false),
                (2734.0, 200.0, true),
            ],
        );
        datapoints.insert(dp1.kline.time, dp1);

        // Candle 2: High=2750, Low=2735, clusters at 2738 (vol 800)
        let dp2 = make_test_dp(
            1726930800000,
            2750.0,
            2735.0,
            2745.0,
            &[(2738.0, 500.0, false), (2738.0, 300.0, true)],
        );
        datapoints.insert(dp2.kline.time, dp2);

        let query = VolumeSearchQuery {
            start_time: 0,
            end_time: u64::MAX,
            min_volume: 200.0,
            tick_multiplier: TickMultiplier(1),
            base_tick_size: PriceStep::from_f32(0.1),
            start_minute_of_day: 0,
            end_minute_of_day: 1439,
        };

        let (mut rows, summary) = search_volume_candles(&datapoints, &query);
        assert_eq!(rows.len(), 2);
        assert_eq!(summary.count, 2);

        // Sort descending by Max Volume
        sort_rows(
            &mut rows,
            VolumeSearchSortCol::MaxVolume,
            SortOrder::Descending,
        );
        assert_eq!(rows[0].max_volume, 800.0);
        assert!((rows[0].poc_price.to_f32() - 2738.0).abs() < 1e-2);
        assert_eq!(rows[1].max_volume, 500.0);
        assert!((rows[1].poc_price.to_f32() - 2734.0).abs() < 1e-2);
    }

    #[test]
    fn test_volume_search_min_volume_cutoff() {
        let mut datapoints = BTreeMap::new();
        let dp = make_test_dp(1000, 100.0, 90.0, 95.0, &[(95.0, 50.0, false)]);
        datapoints.insert(1000, dp);

        let query = VolumeSearchQuery {
            min_volume: 100.0, // greater than 50.0
            ..Default::default()
        };

        let (rows, summary) = search_volume_candles(&datapoints, &query);
        assert_eq!(rows.len(), 0);
        assert_eq!(summary.count, 0);
    }
}
