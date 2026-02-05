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
        let total_points = self.datapoints.len();
        if total_points == 0 {
            return;
        }

        // O(n) approach: iterate backwards once, tracking cumulative price range
        // and the first index where each price level was touched.
        //
        // For each datapoint (from end to start), we track the cumulative high/low
        // range seen so far. When we encounter a POC, we check if it falls within
        // that range. If so, we need to find when it was first touched.
        //
        // We maintain a "first touch index" by tracking when prices first entered
        // the cumulative range as we scan backwards.

        // First, precompute low/high for each datapoint
        let bounds: Vec<(Price, Price)> = self
            .datapoints
            .iter()
            .map(|dp| {
                let low = dp.kline.low.round_to_side_step(true, self.tick_size);
                let high = dp.kline.high.round_to_side_step(false, self.tick_size);
                (low, high)
            })
            .collect();

        // Track cumulative range and when each boundary was established
        let mut cumulative_low: Option<Price> = None;
        let mut cumulative_high: Option<Price> = None;
        let mut low_established_at: usize = total_points;
        let mut high_established_at: usize = total_points;

        // Process from end to start
        for i in (0..total_points).rev() {
            let (dp_low, dp_high) = bounds[i];

            // Update cumulative range (looking at i+1 onwards, i.e., "future" candles)
            // We check the POC at index i against the range from i+1 to end

            // Check if this datapoint has a POC and determine its status
            if let Some(poc_price) = self.datapoints[i].poc_price() {
                let npoc = if let (Some(cum_low), Some(cum_high)) = (cumulative_low, cumulative_high)
                {
                    if cum_low <= poc_price && cum_high >= poc_price {
                        // POC was touched - find the first touch index
                        // The first touch is the earliest index where the price was covered
                        let first_touch = if poc_price <= bounds[low_established_at].1
                            && poc_price >= bounds[low_established_at].0
                        {
                            low_established_at
                        } else if poc_price <= bounds[high_established_at].1
                            && poc_price >= bounds[high_established_at].0
                        {
                            high_established_at
                        } else {
                            // Need to find first touch by scanning forward from i+1
                            // This is a fallback for edge cases
                            let search_end = high_established_at.max(low_established_at);
                            bounds[(i + 1)..=search_end]
                                .iter()
                                .enumerate()
                                .find(|(_, (low, high))| *low <= poc_price && *high >= poc_price)
                                .map_or(total_points, |(offset, _)| i + 1 + offset)
                        };

                        if first_touch < total_points {
                            // Convert to reversed index for rendering
                            let reversed_idx = (total_points - 1) - first_touch;
                            let mut n = NPoc::default();
                            n.filled(reversed_idx as u64);
                            n
                        } else {
                            NPoc::Naked
                        }
                    } else {
                        NPoc::Naked
                    }
                } else {
                    // No future candles to check against (this is the last candle)
                    NPoc::default()
                };

                self.datapoints[i].set_poc_status(npoc);
            }

            // Now update cumulative range to include this candle for the next iteration
            match cumulative_low {
                Some(cl) if dp_low < cl => {
                    cumulative_low = Some(dp_low);
                    low_established_at = i;
                }
                None => {
                    cumulative_low = Some(dp_low);
                    low_established_at = i;
                }
                _ => {}
            }

            match cumulative_high {
                Some(ch) if dp_high > ch => {
                    cumulative_high = Some(dp_high);
                    high_established_at = i;
                }
                None => {
                    cumulative_high = Some(dp_high);
                    high_established_at = i;
                }
                _ => {}
            }
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
