use crate::aggr;
use crate::chart::kline::{ClusterKind, KlineTrades, NPoc};
use exchange::util::{Price, PriceStep};
use exchange::{Kline, Trade};
use rustc_hash::FxHashSet;

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TickAccumulation {
    pub tick_count: usize,
    pub kline: Kline,
    pub footprint: KlineTrades,
}

impl TickAccumulation {
    pub fn new(trade: &Trade, step: PriceStep) -> Self {
        let mut footprint = KlineTrades::new();
        footprint.add_trade_to_nearest_bin(trade, step);

        let kline = Kline {
            time: trade.time,
            open: trade.price,
            high: trade.price,
            low: trade.price,
            close: trade.price,
            volume: (
                if trade.is_sell { 0.0 } else { trade.qty },
                if trade.is_sell { trade.qty } else { 0.0 },
            ),
        };

        Self {
            tick_count: 1,
            kline,
            footprint,
        }
    }

    pub fn update_with_trade(&mut self, trade: &Trade, step: PriceStep) {
        self.tick_count += 1;
        self.kline.high = self.kline.high.max(trade.price);
        self.kline.low = self.kline.low.min(trade.price);
        self.kline.close = trade.price;

        if trade.is_sell {
            self.kline.volume.1 += trade.qty;
        } else {
            self.kline.volume.0 += trade.qty;
        }

        self.add_trade(trade, step);
    }

    fn add_trade(&mut self, trade: &Trade, step: PriceStep) {
        self.footprint.add_trade_to_nearest_bin(trade, step);
    }

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

    pub fn is_full(&self, interval: aggr::TickCount) -> bool {
        self.tick_count >= interval.0 as usize
    }

    pub fn poc_price(&self) -> Option<Price> {
        self.footprint.poc_price()
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        self.footprint.set_poc_status(status);
    }

    pub fn calculate_poc(&mut self) {
        self.footprint.calculate_poc();
    }
}

pub struct TickAggr {
    pub datapoints: Vec<TickAccumulation>,
    pub interval: aggr::TickCount,
    pub tick_size: PriceStep,
}

impl TickAggr {
    pub const MAX_RETAINED_DATAPOINTS: usize = 10_000;

    pub fn enforce_retention_limit(&mut self) {
        if self.datapoints.len() > Self::MAX_RETAINED_DATAPOINTS {
            let excess = self.datapoints.len() - Self::MAX_RETAINED_DATAPOINTS;
            self.datapoints.drain(0..excess);
        }
    }

    pub fn new(interval: aggr::TickCount, tick_size: PriceStep, raw_trades: &[Trade]) -> Self {
        let mut tick_aggr = Self {
            datapoints: Vec::new(),
            interval,
            tick_size,
        };

        if !raw_trades.is_empty() {
            tick_aggr.insert_trades(raw_trades);
        }

        tick_aggr
    }

    pub fn change_tick_size(&mut self, tick_size: f32, raw_trades: &[Trade]) {
        self.tick_size = PriceStep::from_f32(tick_size);

        self.datapoints.clear();

        if !raw_trades.is_empty() {
            self.insert_trades(raw_trades);
        }
    }

    /// return latest data point and its index
    pub fn latest_dp(&self) -> Option<(&TickAccumulation, usize)> {
        self.datapoints
            .last()
            .map(|dp| (dp, self.datapoints.len() - 1))
    }

    pub fn volume_data(&self) -> BTreeMap<u64, (f32, f32)> {
        self.into()
    }

    pub fn insert_trades(&mut self, buffer: &[Trade]) {
        let mut updated_indices = FxHashSet::default();

        for trade in buffer {
            if self.datapoints.is_empty() {
                self.datapoints
                    .push(TickAccumulation::new(trade, self.tick_size));
                updated_indices.insert(0);
            } else {
                let last_idx = self.datapoints.len() - 1;

                if self.datapoints[last_idx].is_full(self.interval) {
                    self.datapoints
                        .push(TickAccumulation::new(trade, self.tick_size));
                    updated_indices.insert(self.datapoints.len() - 1);
                } else {
                    self.datapoints[last_idx].update_with_trade(trade, self.tick_size);
                    updated_indices.insert(last_idx);
                }
            }
        }

        for idx in updated_indices {
            if idx < self.datapoints.len() {
                self.datapoints[idx].calculate_poc();
            }
        }

        self.update_poc_status();
    }

    pub fn update_poc_status(&mut self) {
        self.enforce_retention_limit();

        let total_points = self.datapoints.len();
        if total_points == 0 {
            return;
        }

        // First, precompute low/high and poc_price for each datapoint
        let bounds: Vec<(Price, Price, Option<Price>)> = self
            .datapoints
            .iter()
            .map(|dp| {
                let low = dp.kline.low.round_to_side_step(true, self.tick_size);
                let high = dp.kline.high.round_to_side_step(false, self.tick_size);
                (low, high, dp.poc_price())
            })
            .collect();

        let mut active_unfilled: Vec<(usize, Price)> = Vec::with_capacity(64);
        let mut status_updates: Vec<(usize, NPoc)> = Vec::with_capacity(total_points);

        for (i, (low, high, poc_price)) in bounds.into_iter().enumerate() {
            let mut j = 0;
            while j < active_unfilled.len() {
                let (candle_idx, poc) = active_unfilled[j];
                if low <= poc && poc <= high {
                    // Chronologically earliest touch by candle i
                    let reversed_idx = (total_points - 1) - i;
                    let mut n = NPoc::default();
                    n.filled(reversed_idx as u64);
                    status_updates.push((candle_idx, n));
                    active_unfilled.swap_remove(j);
                } else {
                    j += 1;
                }
            }

            if let Some(poc) = poc_price {
                active_unfilled.push((i, poc));
            }
        }

        for (candle_idx, _) in active_unfilled {
            if candle_idx == total_points - 1 {
                status_updates.push((candle_idx, NPoc::default()));
            } else {
                status_updates.push((candle_idx, NPoc::Naked));
            }
        }

        for (candle_idx, npoc) in status_updates {
            self.datapoints[candle_idx].set_poc_status(npoc);
        }
    }

    pub fn min_max_price_in_range_prices(
        &self,
        earliest: usize,
        latest: usize,
    ) -> Option<(Price, Price)> {
        if earliest > latest {
            return None;
        }

        let mut min_p: Option<Price> = None;
        let mut max_p: Option<Price> = None;

        self.datapoints
            .iter()
            .rev()
            .enumerate()
            .filter(|(idx, _)| *idx >= earliest && *idx <= latest)
            .for_each(|(_, dp)| {
                let low = dp.kline.low;
                let high = dp.kline.high;

                min_p = Some(match min_p {
                    Some(value) => value.min(low),
                    None => low,
                });
                max_p = Some(match max_p {
                    Some(value) => value.max(high),
                    None => high,
                });
            });

        match (min_p, max_p) {
            (Some(low), Some(high)) => Some((low, high)),
            _ => None,
        }
    }

    pub fn min_max_price_in_range(&self, earliest: usize, latest: usize) -> Option<(f32, f32)> {
        self.min_max_price_in_range_prices(earliest, latest)
            .map(|(min_p, max_p)| (min_p.to_f32(), max_p.to_f32()))
    }

    pub fn max_qty_idx_range(
        &self,
        cluster_kind: ClusterKind,
        earliest: usize,
        latest: usize,
        highest: Price,
        lowest: Price,
    ) -> f32 {
        let mut max_cluster_qty: f32 = 0.0;

        self.datapoints
            .iter()
            .rev()
            .enumerate()
            .filter(|(index, _)| *index <= latest && *index >= earliest)
            .for_each(|(_, dp)| {
                max_cluster_qty =
                    max_cluster_qty.max(dp.max_cluster_qty(cluster_kind, highest, lowest));
            });

        max_cluster_qty
    }
}

impl From<&TickAggr> for BTreeMap<u64, (f32, f32)> {
    /// Converts datapoints into a map of timestamps and volume data
    fn from(tick_aggr: &TickAggr) -> Self {
        tick_aggr
            .datapoints
            .iter()
            .enumerate()
            .map(|(idx, dp)| (idx as u64, (dp.kline.volume.0, dp.kline.volume.1)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dat02_ticks_chronological_first_touch() {
        let step = PriceStep::from_f32(1.0);
        let interval = aggr::TickCount(1);
        let mut aggr = TickAggr::new(interval, step, &[]);

        let trades = vec![
            Trade {
                time: 1000,
                price: Price::from_f32(100.0),
                qty: 10.0,
                is_sell: false,
            },
            Trade {
                time: 2000,
                price: Price::from_f32(100.0),
                qty: 5.0,
                is_sell: true,
            },
            Trade {
                time: 3000,
                price: Price::from_f32(10.0),
                qty: 2.0,
                is_sell: true,
            },
        ];
        aggr.insert_trades(&trades);

        let npoc = aggr.datapoints[0].footprint.poc.as_ref().unwrap().status;
        assert_eq!(npoc, NPoc::Filled { at: 1 });
    }

    #[test]
    fn test_dat03_ticks_retention_limit() {
        let step = PriceStep::from_f32(1.0);
        let interval = aggr::TickCount(1);
        let mut aggr = TickAggr::new(interval, step, &[]);

        let total_trades = TickAggr::MAX_RETAINED_DATAPOINTS + 50;
        let mut trades = Vec::with_capacity(total_trades);
        for i in 0..total_trades {
            trades.push(Trade {
                time: (i as u64 + 1) * 1000,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            });
        }
        aggr.insert_trades(&trades);

        assert_eq!(aggr.datapoints.len(), TickAggr::MAX_RETAINED_DATAPOINTS);
    }
}
