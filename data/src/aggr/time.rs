use std::collections::BTreeMap;

use crate::chart::Basis;
use crate::chart::heatmap::HeatmapDataPoint;
use crate::chart::kline::{ClusterKind, KlineDataPoint, KlineTrades, NPoc};
use rustc_hash::FxHashSet;

use exchange::util::{Price, PriceStep};
use exchange::{Kline, Timeframe, Trade};

pub trait DataPoint {
    fn add_trade(&mut self, trade: &Trade, step: PriceStep);

    fn clear_trades(&mut self);

    fn last_trade_time(&self) -> Option<u64>;

    fn first_trade_time(&self) -> Option<u64>;

    fn last_price(&self) -> Price;

    fn kline(&self) -> Option<&Kline>;

    fn value_high(&self) -> Price;

    fn value_low(&self) -> Price;
}

pub struct TimeSeries<D: DataPoint> {
    pub datapoints: BTreeMap<u64, D>,
    pub interval: Timeframe,
    pub tick_size: PriceStep,
}

impl<D: DataPoint> TimeSeries<D> {
    pub fn base_price(&self) -> Price {
        self.datapoints
            .values()
            .last()
            .map_or(Price::from_f32(0.0), DataPoint::last_price)
    }

    pub fn latest_timestamp(&self) -> Option<u64> {
        self.datapoints.keys().last().copied()
    }

    pub fn latest_kline(&self) -> Option<&Kline> {
        self.datapoints.values().last().and_then(|dp| dp.kline())
    }

    pub fn price_scale(&self, lookback: usize) -> (Price, Price) {
        let mut iter = self.datapoints.iter().rev().take(lookback);

        if let Some((_, first)) = iter.next() {
            let mut high = first.value_high();
            let mut low = first.value_low();

            for (_, dp) in iter {
                let value_high = dp.value_high();
                let value_low = dp.value_low();
                if value_high > high {
                    high = value_high;
                }
                if value_low < low {
                    low = value_low;
                }
            }

            (high, low)
        } else {
            (Price::from_f32(0.0), Price::from_f32(0.0))
        }
    }

    pub fn volume_data<'a>(&'a self) -> BTreeMap<u64, (f32, f32)>
    where
        BTreeMap<u64, (f32, f32)>: From<&'a TimeSeries<D>>,
    {
        self.into()
    }

    pub fn timerange(&self) -> (u64, u64) {
        let earliest = self.datapoints.keys().next().copied().unwrap_or(0);
        let latest = self.datapoints.keys().last().copied().unwrap_or(0);

        (earliest, latest)
    }

    pub fn min_max_price_in_range_prices(
        &self,
        earliest: u64,
        latest: u64,
    ) -> Option<(Price, Price)> {
        if earliest > latest {
            return None;
        }

        let mut it = self.datapoints.range(earliest..=latest);

        let (_, first) = it.next()?;
        let mut min_price = first.value_low();
        let mut max_price = first.value_high();

        for (_, dp) in it {
            let low = dp.value_low();
            let high = dp.value_high();
            if low < min_price {
                min_price = low;
            }
            if high > max_price {
                max_price = high;
            }
        }

        Some((min_price, max_price))
    }

    pub fn min_max_price_in_range(&self, earliest: u64, latest: u64) -> Option<(f32, f32)> {
        self.min_max_price_in_range_prices(earliest, latest)
            .map(|(min_p, max_p)| (min_p.to_f32(), max_p.to_f32()))
    }

    pub fn clear_trades(&mut self) {
        for data_point in self.datapoints.values_mut() {
            data_point.clear_trades();
        }
    }

    pub fn check_kline_integrity(
        &self,
        earliest: u64,
        latest: u64,
        interval: u64,
    ) -> Option<Vec<u64>> {
        if earliest >= latest || interval == 0 {
            return None;
        }

        let mut time = earliest;
        let mut missing_count = 0;

        while time < latest {
            if !self.datapoints.contains_key(&time) {
                missing_count += 1;
                break;
            }
            time += interval;
        }

        if missing_count > 0 {
            let mut missing_keys = Vec::with_capacity(((latest - earliest) / interval) as usize);
            let mut time = earliest;

            while time < latest {
                if !self.datapoints.contains_key(&time) {
                    missing_keys.push(time);
                }
                time += interval;
            }

            log::warn!(
                "Integrity check failed: missing {} klines",
                missing_keys.len()
            );
            return Some(missing_keys);
        }

        None
    }
}

impl TimeSeries<KlineDataPoint> {
    pub fn new(interval: Timeframe, tick_size: PriceStep, klines: &[Kline]) -> Self {
        let mut timeseries = Self {
            datapoints: BTreeMap::new(),
            interval,
            tick_size,
        };

        timeseries.insert_klines(klines);
        timeseries
    }

    pub fn with_trades(&self, trades: &[Trade]) -> TimeSeries<KlineDataPoint> {
        let mut new_series = Self {
            datapoints: self.datapoints.clone(),
            interval: self.interval,
            tick_size: self.tick_size,
        };

        new_series.insert_trades_or_create_bucket(trades);
        new_series
    }

    pub fn insert_klines(&mut self, klines: &[Kline]) {
        for kline in klines {
            let entry = self
                .datapoints
                .entry(kline.time)
                .or_insert_with(|| KlineDataPoint {
                    kline: *kline,
                    footprint: KlineTrades::new(),
                    trades_fetched: false,
                });

            entry.kline = *kline;
        }

        self.update_poc_status();
    }

    pub fn insert_trades_or_create_bucket(&mut self, buffer: &[Trade]) {
        if buffer.is_empty() {
            return;
        }
        let aggr_time = self.interval.to_milliseconds();
        let mut updated_times = FxHashSet::default();

        buffer.iter().for_each(|trade| {
            let rounded_time = (trade.time / aggr_time) * aggr_time;

            updated_times.insert(rounded_time);

            let entry = self
                .datapoints
                .entry(rounded_time)
                .or_insert_with(|| KlineDataPoint {
                    kline: Kline {
                        time: rounded_time,
                        open: trade.price,
                        high: trade.price,
                        low: trade.price,
                        close: trade.price,
                        volume: (0.0, 0.0),
                    },
                    footprint: KlineTrades::new(),
                    trades_fetched: true,
                });

            entry.add_trade(trade, self.tick_size);
        });

        for time in updated_times {
            if let Some(data_point) = self.datapoints.get_mut(&time) {
                data_point.calculate_poc();
            }
        }
    }

    pub fn insert_trades_existing_buckets(&mut self, buffer: &[Trade]) {
        if buffer.is_empty() {
            return;
        }
        let aggr_time = self.interval.to_milliseconds();
        let mut updated_times: FxHashSet<u64> = FxHashSet::default();
        let min_trade_time = buffer.first().map(|t| t.time).unwrap_or(0);
        let max_trade_time = buffer.last().map(|t| t.time).unwrap_or(0);

        for trade in buffer {
            let rounded_time = (trade.time / aggr_time) * aggr_time;

            if let Some(entry) = self.datapoints.get_mut(&rounded_time) {
                updated_times.insert(rounded_time);
                entry
                    .footprint
                    .add_trade_to_nearest_bin(trade, self.tick_size);
            }
        }

        if aggr_time > 0 && max_trade_time >= min_trade_time && min_trade_time > 0 {
            let start_bucket = (min_trade_time / aggr_time) * aggr_time;
            let end_bucket = (max_trade_time / aggr_time) * aggr_time;
            for (_, dp) in self.datapoints.range_mut(start_bucket..=end_bucket) {
                if !dp.footprint.is_empty() || (dp.kline.volume.0 + dp.kline.volume.1) == 0.0 {
                    dp.trades_fetched = true;
                }
            }
        }

        for time in updated_times {
            if let Some(data_point) = self.datapoints.get_mut(&time) {
                data_point.calculate_poc();
            }
        }
    }

    pub fn insert_preaggregated_footprint(&mut self, dps: Vec<(u64, KlineDataPoint)>) {
        if dps.is_empty() {
            return;
        }
        for (time, mut new_dp) in dps {
            if let Some(existing) = self.datapoints.get_mut(&time) {
                if !new_dp.footprint.is_empty() {
                    existing.footprint = new_dp.footprint;
                    existing.trades_fetched = true;
                }
            } else {
                if new_dp.footprint.is_empty()
                    && (new_dp.kline.volume.0 + new_dp.kline.volume.1) > 0.0
                {
                    new_dp.trades_fetched = false;
                }
                self.datapoints.insert(time, new_dp);
            }
        }
        self.update_poc_status();
    }

    pub fn mark_trades_fetched(&mut self, from_time: u64, to_time: u64) {
        if from_time > to_time {
            return;
        }
        let aggr_time = self.interval.to_milliseconds();
        if aggr_time == 0 {
            return;
        }
        let rounded_from = (from_time / aggr_time) * aggr_time;
        if rounded_from > to_time {
            return;
        }

        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        for (&time, dp) in self.datapoints.range_mut(rounded_from..=to_time) {
            let candle_end = time.saturating_add(aggr_time);
            if candle_end <= to_time
                || (candle_end > now_ms && to_time >= now_ms.saturating_sub(120_000))
            {
                dp.trades_fetched = true;
            }
        }
    }

    pub fn insert_realtime_trades(&mut self, buffer: &[Trade]) {
        if buffer.is_empty() {
            return;
        }
        let aggr_time = self.interval.to_milliseconds();
        let mut updated_times: FxHashSet<u64> = FxHashSet::default();
        let latest_ts = self.latest_timestamp();

        for trade in buffer {
            let rounded_time = (trade.time / aggr_time) * aggr_time;

            if let Some(entry) = self.datapoints.get_mut(&rounded_time) {
                updated_times.insert(rounded_time);
                entry.add_trade(trade, self.tick_size);
            } else if latest_ts.is_none_or(|latest| rounded_time >= latest) {
                updated_times.insert(rounded_time);
                let is_near_candle_start = trade.time.saturating_sub(rounded_time) <= 120_000;
                let entry = self
                    .datapoints
                    .entry(rounded_time)
                    .or_insert_with(|| KlineDataPoint {
                        kline: Kline {
                            time: rounded_time,
                            open: trade.price,
                            high: trade.price,
                            low: trade.price,
                            close: trade.price,
                            volume: (0.0, 0.0),
                        },
                        footprint: KlineTrades::new(),
                        trades_fetched: is_near_candle_start,
                    });
                entry.add_trade(trade, self.tick_size);
            }
        }

        for time in updated_times {
            if let Some(data_point) = self.datapoints.get_mut(&time) {
                data_point.calculate_poc();
            }
        }
    }

    pub fn change_tick_size(&mut self, tick_size: f32, raw_trades: &[Trade]) {
        self.tick_size = PriceStep::from_f32(tick_size);
        for dp in self.datapoints.values_mut() {
            dp.clear_trades();
            dp.trades_fetched = false;
        }

        if !raw_trades.is_empty() {
            self.insert_trades_existing_buckets(raw_trades);
        }
    }

    pub fn update_poc_status(&mut self) {
        if self.datapoints.is_empty() {
            return;
        }

        // O(n) approach: iterate backwards once, tracking cumulative price range
        // and the first time where each price level was touched.
        //
        // Collect keys and bounds first to avoid borrow issues with BTreeMap
        let entries: Vec<(u64, Price, Price, Option<Price>)> = self
            .datapoints
            .iter()
            .map(|(&time, dp)| {
                let low = dp.kline.low.round_to_side_step(true, self.tick_size);
                let high = dp.kline.high.round_to_side_step(false, self.tick_size);
                (time, low, high, dp.poc_price())
            })
            .collect();

        let total_points = entries.len();

        // Track cumulative range and when each boundary was established
        let mut cumulative_low: Option<Price> = None;
        let mut cumulative_high: Option<Price> = None;
        let mut low_established_at: usize = total_points;
        let mut high_established_at: usize = total_points;

        // Collect status updates to apply later
        let mut status_updates: Vec<(u64, NPoc)> = Vec::new();

        // Process from end to start
        for i in (0..total_points).rev() {
            let (time, dp_low, dp_high, poc_price) = entries[i];

            // Check if this datapoint has a POC and determine its status
            if let Some(poc_price) = poc_price {
                let npoc = if let (Some(cum_low), Some(cum_high)) =
                    (cumulative_low, cumulative_high)
                {
                    if cum_low <= poc_price && cum_high >= poc_price {
                        // POC was touched - find the first touch time
                        let first_touch_time = if poc_price <= entries[low_established_at].2
                            && poc_price >= entries[low_established_at].1
                        {
                            Some(entries[low_established_at].0)
                        } else if poc_price <= entries[high_established_at].2
                            && poc_price >= entries[high_established_at].1
                        {
                            Some(entries[high_established_at].0)
                        } else {
                            // Fallback: scan forward from i+1 to find first touch
                            let search_end = high_established_at.max(low_established_at);
                            entries[(i + 1)..=search_end]
                                .iter()
                                .find(|(_, low, high, _)| *low <= poc_price && *high >= poc_price)
                                .map(|(time, _, _, _)| *time)
                        };

                        if let Some(touch_time) = first_touch_time {
                            let mut n = NPoc::default();
                            n.filled(touch_time);
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

                status_updates.push((time, npoc));
            }

            // Update cumulative range to include this candle for the next iteration
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

        // Apply all status updates
        for (time, npoc) in status_updates {
            if let Some(data_point) = self.datapoints.get_mut(&time) {
                data_point.set_poc_status(npoc);
            }
        }
    }

    pub fn suggest_trade_fetch_range(
        &self,
        visible_earliest: u64,
        visible_latest: u64,
    ) -> Option<(u64, u64)> {
        if self.datapoints.is_empty() || visible_earliest >= visible_latest {
            return None;
        }

        let interval_ms = self.interval.to_milliseconds();
        if interval_ms == 0 {
            return None;
        }

        let aligned_earliest = (visible_earliest / interval_ms) * interval_ms;
        let aligned_latest = (visible_latest / interval_ms) * interval_ms;

        // 1. Priority 1: Unfetched candles within the visible range [aligned_earliest, aligned_latest]
        let mut visible_unfetched = self
            .datapoints
            .range(aligned_earliest..=aligned_latest)
            .filter(|(_, dp)| !dp.trades_fetched && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0)
            .map(|(&t, _)| t);

        if let Some(first_unfetched) = visible_unfetched.next() {
            let last_unfetched = visible_unfetched.next_back().unwrap_or(first_unfetched);
            let fetch_from = first_unfetched;
            let fetch_to = last_unfetched.saturating_add(interval_ms);
            return Some((fetch_from, fetch_to));
        }

        // 2. Priority 2: Prefetch earlier candles preceding the visible range
        let prefetch_earliest = aligned_earliest.saturating_sub(7 * 24 * 3600 * 1000);
        let aligned_prefetch_earliest = (prefetch_earliest / interval_ms) * interval_ms;
        let mut past_unfetched = self
            .datapoints
            .range(aligned_prefetch_earliest..aligned_earliest)
            .filter(|(_, dp)| !dp.trades_fetched && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0)
            .map(|(&t, _)| t);

        if let Some(first_unfetched) = past_unfetched.next() {
            let last_unfetched = past_unfetched.next_back().unwrap_or(first_unfetched);
            let fetch_from = first_unfetched;
            let fetch_to = last_unfetched.saturating_add(interval_ms);
            return Some((fetch_from, fetch_to));
        }

        // 3. Priority 3: Prefetch candles succeeding the visible range
        let prefetch_latest = aligned_latest.saturating_add(7 * 24 * 3600 * 1000);
        let aligned_prefetch_latest = (prefetch_latest / interval_ms) * interval_ms;
        let mut future_unfetched = self
            .datapoints
            .range(aligned_latest.saturating_add(interval_ms)..=aligned_prefetch_latest)
            .filter(|(_, dp)| !dp.trades_fetched && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0)
            .map(|(&t, _)| t);

        if let Some(first_unfetched) = future_unfetched.next() {
            let last_unfetched = future_unfetched.next_back().unwrap_or(first_unfetched);
            let fetch_from = first_unfetched;
            let fetch_to = last_unfetched.saturating_add(interval_ms);
            return Some((fetch_from, fetch_to));
        }

        None
    }

    pub fn max_qty_ts_range(
        &self,
        cluster_kind: ClusterKind,
        earliest: u64,
        latest: u64,
        highest: Price,
        lowest: Price,
    ) -> f32 {
        if earliest > latest {
            return 0.0;
        }

        let mut max_cluster_qty: f32 = 0.0;

        self.datapoints
            .range(earliest..=latest)
            .for_each(|(_, dp)| {
                max_cluster_qty =
                    max_cluster_qty.max(dp.max_cluster_qty(cluster_kind, highest, lowest));
            });

        max_cluster_qty
    }
}

pub fn aggregate_trades_for_day(
    trades: &[Trade],
    interval: Timeframe,
    step: PriceStep,
) -> Vec<(u64, KlineDataPoint)> {
    if trades.is_empty() {
        return Vec::new();
    }
    let interval_ms = interval.to_milliseconds();
    if interval_ms == 0 {
        return Vec::new();
    }

    let min_trade_t = trades.first().map(|t| t.time).unwrap_or(0);
    let max_trade_t = trades.last().map(|t| t.time).unwrap_or(0);
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;

    let mut map: BTreeMap<u64, KlineDataPoint> = BTreeMap::new();

    for trade in trades {
        let rounded_time = (trade.time / interval_ms) * interval_ms;
        let entry = map.entry(rounded_time).or_insert_with(|| KlineDataPoint {
            kline: Kline {
                time: rounded_time,
                open: trade.price,
                high: trade.price,
                low: trade.price,
                close: trade.price,
                volume: (0.0, 0.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: false,
        });

        entry.add_trade(trade, step);
    }

    let today_date = chrono::Utc::now().date_naive();
    let today_midnight = today_date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    for (&time, dp) in map.iter_mut() {
        dp.calculate_poc();
        let candle_end = time.saturating_add(interval_ms);
        let is_historical_day = time < today_midnight && candle_end <= today_midnight;
        if is_historical_day
            || (min_trade_t <= time && max_trade_t >= candle_end)
            || (candle_end > now_ms && max_trade_t >= now_ms.saturating_sub(120_000))
        {
            dp.trades_fetched = true;
        }
    }

    map.into_iter().collect()
}

impl TimeSeries<HeatmapDataPoint> {
    pub fn new(basis: Basis, tick_size: PriceStep) -> Self {
        let timeframe = match basis {
            Basis::Time(interval) => interval,
            Basis::Tick(_) => unimplemented!(),
        };

        Self {
            datapoints: BTreeMap::new(),
            interval: timeframe,
            tick_size,
        }
    }

    pub fn max_trade_qty_and_aggr_volume(&self, earliest: u64, latest: u64) -> (f32, f32) {
        if earliest > latest {
            return (0.0, 0.0);
        }

        let mut max_trade_qty = 0.0f32;
        let mut max_aggr_volume = 0.0f32;

        self.datapoints
            .range(earliest..=latest)
            .for_each(|(_, dp)| {
                let (mut buy_volume, mut sell_volume) = (0.0, 0.0);

                dp.grouped_trades.iter().for_each(|trade| {
                    max_trade_qty = max_trade_qty.max(trade.qty);

                    if trade.is_sell {
                        sell_volume += trade.qty;
                    } else {
                        buy_volume += trade.qty;
                    }
                });

                max_aggr_volume = max_aggr_volume.max(buy_volume + sell_volume);
            });

        (max_trade_qty, max_aggr_volume)
    }
}

impl From<&TimeSeries<KlineDataPoint>> for BTreeMap<u64, (f32, f32)> {
    /// Converts datapoints into a map of timestamps and volume data
    fn from(timeseries: &TimeSeries<KlineDataPoint>) -> Self {
        timeseries
            .datapoints
            .iter()
            .map(|(time, dp)| (*time, (dp.kline.volume.0, dp.kline.volume.1)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::util::Price;

    #[test]
    fn test_kline_datapoint_realtime_trade_update() {
        let mut dp = KlineDataPoint {
            kline: Kline {
                time: 1_000_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(100.0),
                low: Price::from_f32(100.0),
                close: Price::from_f32(100.0),
                volume: (0.0, 0.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: false,
        };

        let step = PriceStep::from_f32(0.5);

        // Incoming Buy trade at 105.0 with qty 2.5
        let trade_buy = Trade {
            time: 1_000_100,
            price: Price::from_f32(105.0),
            qty: 2.5,
            is_sell: false,
        };
        dp.add_trade(&trade_buy, step);

        assert_eq!(dp.kline.high.to_f32(), 105.0);
        assert_eq!(dp.kline.low.to_f32(), 100.0);
        assert_eq!(dp.kline.close.to_f32(), 105.0);
        assert_eq!(dp.kline.volume.0, 2.5);
        assert_eq!(dp.kline.volume.1, 0.0);

        // Incoming Sell trade at 95.0 with qty 1.5
        let trade_sell = Trade {
            time: 1_000_200,
            price: Price::from_f32(95.0),
            qty: 1.5,
            is_sell: true,
        };
        dp.add_trade(&trade_sell, step);

        assert_eq!(dp.kline.high.to_f32(), 105.0);
        assert_eq!(dp.kline.low.to_f32(), 95.0);
        assert_eq!(dp.kline.close.to_f32(), 95.0);
        assert_eq!(dp.kline.volume.0, 2.5);
        assert_eq!(dp.kline.volume.1, 1.5);
    }

    #[test]
    fn test_timeseries_insert_realtime_trades_rollover() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M15, step, &[]);

        let t1 = 15 * 60 * 1000; // First 15m interval
        let trade1 = Trade {
            time: t1 + 5000,
            price: Price::from_f32(100.0),
            qty: 1.0,
            is_sell: false,
        };
        ts.insert_realtime_trades(&[trade1]);

        assert_eq!(ts.datapoints.len(), 1);
        let dp1 = ts.datapoints.get(&t1).unwrap();
        assert_eq!(dp1.kline.open.to_f32(), 100.0);
        assert_eq!(dp1.kline.close.to_f32(), 100.0);

        // Trade in the NEXT 15m interval
        let t2 = 30 * 60 * 1000;
        let trade2 = Trade {
            time: t2 + 1000,
            price: Price::from_f32(108.0),
            qty: 3.0,
            is_sell: true,
        };
        ts.insert_realtime_trades(&[trade2]);

        assert_eq!(ts.datapoints.len(), 2);
        let dp2 = ts.datapoints.get(&t2).unwrap();
        assert_eq!(dp2.kline.open.to_f32(), 108.0);
        assert_eq!(dp2.kline.close.to_f32(), 108.0);
        assert_eq!(dp2.kline.volume.1, 3.0);
    }

    #[test]
    fn test_suggest_trade_fetch_range_when_all_empty() {
        let step = PriceStep::from_f32(1.0);
        let klines = vec![
            Kline {
                time: 300_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(99.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: 600_000,
                open: Price::from_f32(102.0),
                high: Price::from_f32(107.0),
                low: Price::from_f32(101.0),
                close: Price::from_f32(106.0),
                volume: (15.0, 15.0),
            },
        ];
        let ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);

        // All datapoints have empty footprint trades
        let range = ts.suggest_trade_fetch_range(200_000, 800_000);
        assert_eq!(range, Some((300_000, 900_000)));
    }

    #[test]
    fn test_suggest_trade_fetch_range_stops_after_mark_fetched() {
        let step = PriceStep::from_f32(1.0);
        let klines = vec![
            Kline {
                time: 300_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(99.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: 600_000,
                open: Price::from_f32(102.0),
                high: Price::from_f32(107.0),
                low: Price::from_f32(101.0),
                close: Price::from_f32(106.0),
                volume: (15.0, 15.0),
            },
        ];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);

        let range = ts.suggest_trade_fetch_range(200_000, 800_000);
        assert_eq!(range, Some((300_000, 900_000)));

        // After marking the range fetched, no more gap should be suggested
        ts.mark_trades_fetched(300_000, 900_000);
        let next_range = ts.suggest_trade_fetch_range(200_000, 800_000);
        assert_eq!(next_range, None);
    }

    #[test]
    fn test_zero_volume_candle_is_not_suggested_as_gap() {
        let step = PriceStep::from_f32(1.0);
        let klines = vec![Kline {
            time: 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(100.0),
            low: Price::from_f32(100.0),
            close: Price::from_f32(100.0),
            volume: (0.0, 0.0),
        }];
        let ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);
        assert_eq!(ts.suggest_trade_fetch_range(200_000, 800_000), None);
    }

    #[test]
    fn test_midnight_rollover_footprint() {
        let step = PriceStep::from_f32(0.1);
        let t_sep11 = 1789171200000 - 900_000;
        let t_sep12 = 1789171200000;
        let klines = vec![
            Kline {
                time: t_sep11,
                open: Price::from_f32(77000.0),
                high: Price::from_f32(77100.0),
                low: Price::from_f32(76900.0),
                close: Price::from_f32(77050.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: t_sep12,
                open: Price::from_f32(77050.0),
                high: Price::from_f32(77200.0),
                low: Price::from_f32(77000.0),
                close: Price::from_f32(77150.0),
                volume: (15.0, 15.0),
            },
        ];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M15, step, &klines);

        let trade1 = Trade {
            time: t_sep11 + 1000,
            price: Price::from_f32(77000.0),
            qty: 1.0,
            is_sell: false,
        };
        ts.insert_trades_existing_buckets(&[trade1]);
        assert!(
            !ts.datapoints
                .get(&t_sep11)
                .unwrap()
                .footprint
                .trades
                .is_empty()
        );

        let trade2 = Trade {
            time: t_sep12 + 1000,
            price: Price::from_f32(77100.0),
            qty: 2.0,
            is_sell: true,
        };
        ts.insert_trades_existing_buckets(&[trade2]);
        assert!(
            !ts.datapoints
                .get(&t_sep12)
                .unwrap()
                .footprint
                .trades
                .is_empty()
        );

        ts.mark_trades_fetched(t_sep11, t_sep12 + 900_000);
        assert_eq!(
            ts.suggest_trade_fetch_range(t_sep11, t_sep12 + 900_000),
            None
        );
    }

    #[test]
    fn test_insert_trades_existing_buckets_does_not_poison_earlier_candles() {
        let step = PriceStep::from_f32(1.0);
        let klines = vec![
            Kline {
                time: 0,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(99.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: 300_000,
                open: Price::from_f32(102.0),
                high: Price::from_f32(107.0),
                low: Price::from_f32(101.0),
                close: Price::from_f32(106.0),
                volume: (15.0, 15.0),
            },
            Kline {
                time: 600_000,
                open: Price::from_f32(106.0),
                high: Price::from_f32(110.0),
                low: Price::from_f32(105.0),
                close: Price::from_f32(108.0),
                volume: (20.0, 20.0),
            },
        ];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);

        // Insert trades ONLY for the latest candle at 600_000
        let trade = Trade {
            time: 605_000,
            price: Price::from_f32(107.0),
            qty: 5.0,
            is_sell: false,
        };
        ts.insert_trades_existing_buckets(&[trade]);

        // 600_000 should be marked fetched
        assert!(ts.datapoints.get(&600_000).unwrap().trades_fetched);

        // Earlier candles at 0 and 300_000 MUST remain unfetched!
        assert!(!ts.datapoints.get(&0).unwrap().trades_fetched);
        assert!(!ts.datapoints.get(&300_000).unwrap().trades_fetched);

        // If user scrolls to past range 0..400_000, it MUST suggest fetching 0..600_000 (aligned candle end)
        let suggested = ts.suggest_trade_fetch_range(0, 400_000);
        assert_eq!(suggested, Some((0, 600_000)));
    }

    #[test]
    fn test_daily_footprint_cache_integration() {
        let step = PriceStep::from_f32(10.0);
        let trades = vec![
            Trade {
                time: 1726210800000,
                price: Price::from_f32(58000.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: 1726210805000,
                price: Price::from_f32(58010.0),
                qty: 2.5,
                is_sell: true,
            },
        ];

        let dps = aggregate_trades_for_day(&trades, Timeframe::H1, step);
        assert_eq!(dps.len(), 1);

        let temp_dir = std::env::temp_dir().join(format!(
            "test_fp_cache_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fp_file = temp_dir.join("BTCUSDT-fp-2026-09-12.bin");

        crate::chart::kline::save_daily_footprint(&fp_file, &dps).expect("save failed");
        assert!(fp_file.exists());

        let loaded = crate::chart::kline::load_daily_footprint(&fp_file).expect("load failed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, dps[0].0);
        assert_eq!(loaded[0].1.kline.open, dps[0].1.kline.open);
        assert_eq!(loaded[0].1.kline.close, dps[0].1.kline.close);
        assert_eq!(
            loaded[0].1.footprint.trades.len(),
            dps[0].1.footprint.trades.len()
        );

        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::H1, step, &[]);
        ts.insert_preaggregated_footprint(loaded);
        assert_eq!(ts.datapoints.len(), 1);
        assert!(ts.datapoints.values().next().unwrap().trades_fetched);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
