use std::collections::BTreeMap;

use crate::chart::Basis;
use crate::chart::heatmap::HeatmapDataPoint;
use crate::chart::kline::{ClusterKind, KlineDataPoint, KlineTrades, NPoc};
use rustc_hash::{FxHashMap, FxHashSet};

use exchange::util::{Price, PriceStep};
use exchange::{Exchange, Kline, Timeframe, Trade};

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
    pub exchange: Option<Exchange>,
}

impl<D: DataPoint> TimeSeries<D> {
    pub const MAX_RETAINED_DATAPOINTS: usize = 60_000;

    pub fn enforce_retention_limit(&mut self) {
        if self.datapoints.len() > Self::MAX_RETAINED_DATAPOINTS {
            let excess = self.datapoints.len() - Self::MAX_RETAINED_DATAPOINTS;
            let keys_to_remove: Vec<u64> = self.datapoints.keys().take(excess).copied().collect();
            for k in keys_to_remove {
                self.datapoints.remove(&k);
            }
        }
    }

    pub fn set_exchange(&mut self, exchange: Exchange) {
        self.exchange = Some(exchange);
    }

    pub fn with_exchange(mut self, exchange: Exchange) -> Self {
        self.exchange = Some(exchange);
        self
    }

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

    /// Ensures a datapoint bucket exists at `rounded_t` and ingests all trades into it.
    pub fn ingest_trades_bucket(
        &mut self,
        rounded_t: u64,
        trades: &[exchange::Trade],
        step: exchange::util::PriceStep,
    ) where
        D: Default,
    {
        let bucket = self.datapoints.entry(rounded_t).or_default();

        for trade in trades {
            bucket.add_trade(trade, step);
        }
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
            let initial_cap = std::cmp::min(64, ((latest - earliest) / interval) as usize);
            let mut missing_keys = Vec::with_capacity(initial_cap);
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
            exchange: None,
        };

        timeseries.insert_klines(klines);
        timeseries
    }

    pub fn with_trades(&self, trades: &[Trade]) -> TimeSeries<KlineDataPoint> {
        let mut new_series = Self {
            datapoints: self.datapoints.clone(),
            interval: self.interval,
            tick_size: self.tick_size,
            exchange: self.exchange,
        };

        new_series.insert_trades_or_create_bucket(trades);
        new_series
    }

    pub fn insert_klines(&mut self, klines: &[Kline]) {
        let now_ms = exchange::clock::server_time_ms(self.exchange);
        let latest_kline_time = klines.iter().map(|k| k.time).max().unwrap_or(0);
        let effective_now_ms = now_ms.max(latest_kline_time);

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

            let candle_end = self.interval.next_bucket_start(kline.time);
            let candle_dur = self.interval.candle_duration_ms(kline.time);
            if candle_end <= effective_now_ms
                && !entry.footprint.is_empty()
                && entry.trades_fetched
                && !entry.is_trades_coverage_complete(candle_dur, self.tick_size, effective_now_ms)
            {
                entry.trades_fetched = false;
            }
        }

        self.update_poc_status();
    }

    pub fn insert_trades_or_create_bucket(&mut self, buffer: &[Trade]) {
        if buffer.is_empty() {
            return;
        }
        let aggr_time = self.interval.to_milliseconds();
        let mut updated_times = FxHashSet::default();
        let min_trade_time = buffer.first().map(|t| t.time).unwrap_or(0);
        let max_trade_time = buffer.last().map(|t| t.time).unwrap_or(0);

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
                    trades_fetched: false,
                });

            entry.add_trade(trade, self.tick_size);
        });

        if aggr_time > 0 && max_trade_time >= min_trade_time && min_trade_time > 0 {
            let start_bucket = (min_trade_time / aggr_time) * aggr_time;
            let end_bucket = (max_trade_time / aggr_time) * aggr_time;
            let latest_ts = self.latest_timestamp();
            let tolerance = (aggr_time / 10).clamp(500, 120_000).min(aggr_time / 2);
            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            let effective_now_ms = now_ms.max(max_trade_time);

            for (&time, dp) in self.datapoints.range_mut(start_bucket..=end_bucket) {
                let is_latest = latest_ts.is_some_and(|latest| time >= latest);
                let has_trades = !dp.footprint.is_empty();
                let no_volume = (dp.kline.volume.0 + dp.kline.volume.1) == 0.0;

                if is_latest {
                    let first_t = dp.first_trade_time().unwrap_or(0);
                    let is_near_start = has_trades && first_t <= time.saturating_add(tolerance);
                    let allow_active = is_active_candle_fetch_supported(self.exchange);
                    if (!allow_active && has_trades) || is_near_start || no_volume {
                        dp.trades_fetched = true;
                    }
                } else if dp.is_trades_coverage_complete(
                    aggr_time,
                    self.tick_size,
                    effective_now_ms,
                ) || no_volume
                {
                    dp.trades_fetched = true;
                }
            }
        }

        for time in updated_times {
            if let Some(data_point) = self.datapoints.get_mut(&time) {
                data_point.calculate_poc();
            }
        }

        self.enforce_retention_limit();
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
            let latest_ts = self.latest_timestamp();
            let tolerance = (aggr_time / 10).clamp(500, 120_000).min(aggr_time / 2);
            let now_ms = exchange::clock::server_time_ms(self.exchange);
            let effective_now_ms = now_ms.max(max_trade_time);

            for (&time, dp) in self.datapoints.range_mut(start_bucket..=end_bucket) {
                let is_latest = latest_ts.is_some_and(|latest| time >= latest);
                let has_trades = !dp.footprint.is_empty();
                let no_volume = (dp.kline.volume.0 + dp.kline.volume.1) == 0.0;

                if is_latest {
                    let first_t = dp.first_trade_time().unwrap_or(0);
                    let is_near_start = has_trades && first_t <= time.saturating_add(tolerance);
                    let allow_active = is_active_candle_fetch_supported(self.exchange);
                    if (!allow_active && has_trades) || is_near_start || no_volume {
                        dp.trades_fetched = true;
                    }
                } else if dp.is_trades_coverage_complete(
                    aggr_time,
                    self.tick_size,
                    effective_now_ms,
                ) || no_volume
                {
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

    pub fn rebuild_buckets_from_raw_trades(
        &mut self,
        raw_trades: &[Trade],
        affected_buckets: &rustc_hash::FxHashSet<u64>,
    ) {
        if affected_buckets.is_empty() {
            return;
        }
        let now_ms = exchange::clock::server_time_ms(self.exchange);
        let latest_trade_time = raw_trades.last().map(|t| t.time).unwrap_or(0);
        let effective_now_ms = now_ms.max(latest_trade_time);

        for &bucket_time in affected_buckets {
            let candle_end = self.interval.next_bucket_start(bucket_time);
            let candle_dur = self.interval.candle_duration_ms(bucket_time);
            let start_idx = raw_trades.partition_point(|t| t.time < bucket_time);
            let end_idx = raw_trades.partition_point(|t| t.time < candle_end);
            let trades = &raw_trades[start_idx..end_idx];

            if !trades.is_empty() {
                let entry = self
                    .datapoints
                    .entry(bucket_time)
                    .or_insert_with(|| KlineDataPoint {
                        kline: Kline {
                            time: bucket_time,
                            open: trades[0].price,
                            high: trades[0].price,
                            low: trades[0].price,
                            close: trades[trades.len() - 1].price,
                            volume: (0.0, 0.0),
                        },
                        footprint: KlineTrades::new(),
                        trades_fetched: false,
                    });

                entry.clear_trades();
                for trade in trades {
                    entry
                        .footprint
                        .add_trade_to_nearest_bin(trade, self.tick_size);
                }
                entry.calculate_poc();

                if candle_end <= effective_now_ms {
                    entry.trades_fetched = entry.is_trades_coverage_complete(
                        candle_dur,
                        self.tick_size,
                        effective_now_ms,
                    );
                }
            }
        }

        self.update_poc_status();
    }

    pub fn insert_preaggregated_footprint(&mut self, dps: Vec<(u64, KlineDataPoint)>) {
        if dps.is_empty() {
            return;
        }
        for (time, mut new_dp) in dps {
            if let Some(existing) = self.datapoints.get_mut(&time) {
                if !new_dp.footprint.is_empty() {
                    existing.footprint = new_dp.footprint;
                    existing.trades_fetched = new_dp.trades_fetched;
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
        if from_time >= to_time {
            return;
        }
        let rounded_from = self.interval.bucket_start(from_time);
        if rounded_from >= to_time {
            return;
        }

        let aggr_time = self.interval.to_milliseconds().max(1);
        let now_ms = exchange::clock::server_time_ms(self.exchange);
        let tolerance = (aggr_time / 10).clamp(500, 120_000).min(aggr_time / 2);

        for (&time, dp) in self.datapoints.range_mut(rounded_from..to_time) {
            let candle_end = self.interval.next_bucket_start(time);
            if candle_end <= to_time.saturating_add(tolerance)
                || (candle_end > now_ms && to_time >= now_ms.saturating_sub(120_000))
            {
                dp.trades_fetched = true;
            }
        }
    }

    pub fn invalidate_unfetched_empty_footprints(&mut self) {
        let lookback_days = exchange::fetcher::trade_history_lookback_days() as u64;
        let lookback_ms = lookback_days * 24 * 3600 * 1000;
        let today_midnight = exchange::clock::today_midnight_ms(self.exchange);
        let cutoff_ms = today_midnight.saturating_sub(lookback_ms);
        for (_t, dp) in self.datapoints.range_mut(cutoff_ms..) {
            if dp.trades_fetched
                && dp.footprint.is_empty()
                && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0
            {
                dp.trades_fetched = false;
            }
        }
    }

    pub fn insert_realtime_trades(&mut self, buffer: &[Trade]) {
        if buffer.is_empty() {
            return;
        }
        let mut updated_times: FxHashSet<u64> = FxHashSet::default();
        let latest_ts = self.latest_timestamp();

        for trade in buffer {
            let rounded_time = self.interval.bucket_start(trade.time);

            if let Some(entry) = self.datapoints.get_mut(&rounded_time) {
                updated_times.insert(rounded_time);
                entry.add_trade(trade, self.tick_size);
            } else if latest_ts.is_none_or(|latest| rounded_time >= latest) {
                if let Some(prev_latest) = latest_ts
                    && rounded_time > prev_latest
                    && let Some(prev_entry) = self.datapoints.get_mut(&prev_latest)
                {
                    let now_ms = exchange::clock::server_time_ms(self.exchange);
                    let effective_now_ms = now_ms.max(trade.time);
                    let prev_candle_dur = self.interval.candle_duration_ms(prev_latest);
                    prev_entry.trades_fetched = prev_entry.is_trades_coverage_complete(
                        prev_candle_dur,
                        self.tick_size,
                        effective_now_ms,
                    );
                }

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

        self.enforce_retention_limit();
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
        self.enforce_retention_limit();

        if self.datapoints.is_empty() {
            return;
        }

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
        if total_points == 0 {
            return;
        }
        let last_candle_time = entries[total_points - 1].0;

        let mut active_unfilled: Vec<(u64, Price)> = Vec::with_capacity(64);
        let mut status_updates: Vec<(u64, NPoc)> = Vec::with_capacity(total_points);

        for (time, low, high, poc_price) in entries {
            // Check active unfilled POCs against current candle's range
            let mut j = 0;
            while j < active_unfilled.len() {
                let (candle_t, poc) = active_unfilled[j];
                if low <= poc && poc <= high {
                    let mut n = NPoc::default();
                    n.filled(time);
                    status_updates.push((candle_t, n));
                    active_unfilled.swap_remove(j);
                } else {
                    j += 1;
                }
            }

            // Register this candle's POC as active unfilled
            if let Some(poc) = poc_price {
                active_unfilled.push((time, poc));
            }
        }

        // Apply statuses for remaining unfilled POCs
        for (candle_t, _) in active_unfilled {
            if candle_t == last_candle_time {
                status_updates.push((candle_t, NPoc::default()));
            } else {
                status_updates.push((candle_t, NPoc::Naked));
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
        self.suggest_trade_fetch_range_with_exchange(
            visible_earliest,
            visible_latest,
            self.exchange,
        )
    }

    pub fn suggest_trade_fetch_range_with_exchange(
        &self,
        visible_earliest: u64,
        visible_latest: u64,
        exchange: Option<Exchange>,
    ) -> Option<(u64, u64)> {
        self.suggest_trade_fetch_range_with_exchange_and_lookback(
            visible_earliest,
            visible_latest,
            exchange,
            None,
        )
    }

    pub fn suggest_trade_fetch_range_with_exchange_and_lookback(
        &self,
        visible_earliest: u64,
        visible_latest: u64,
        exchange: Option<Exchange>,
        custom_lookback_days: Option<u32>,
    ) -> Option<(u64, u64)> {
        if self.datapoints.is_empty() || visible_earliest >= visible_latest {
            return None;
        }

        let interval_ms = self.interval.to_milliseconds();
        if interval_ms == 0 {
            return None;
        }

        let now_ms = exchange::clock::server_time_ms(exchange);
        let today_midnight = exchange::clock::today_midnight_ms(exchange);
        let current_candle_start = (now_ms / interval_ms) * interval_ms;
        let allow_active = is_active_candle_fetch_supported(exchange);

        let aligned_earliest = (visible_earliest / interval_ms) * interval_ms;
        let aligned_latest = (visible_latest / interval_ms) * interval_ms;

        let lookback_days = custom_lookback_days
            .unwrap_or_else(exchange::fetcher::trade_history_lookback_days)
            as u64;
        let lookback_ms = lookback_days * 24 * 3600 * 1000;
        let is_real_timestamp = aligned_latest >= 1_000_000_000_000;
        let cutoff_time = if is_real_timestamp {
            today_midnight.saturating_sub(lookback_ms)
        } else {
            0
        };

        // 1. Priority 1: Unfetched past-day candles in visible range [aligned_earliest, aligned_latest]
        let search_earliest = aligned_earliest.max(cutoff_time);
        if search_earliest <= aligned_latest {
            let mut visible_past_unfetched = self
                .datapoints
                .range(search_earliest..=aligned_latest)
                .filter(|(t, dp)| {
                    **t < today_midnight
                        && (**t < current_candle_start
                            || (allow_active && **t == current_candle_start))
                        && !dp.trades_fetched
                        && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0
                })
                .map(|(&t, _)| t);

            if let Some(first_unfetched) = visible_past_unfetched.next() {
                let last_unfetched = visible_past_unfetched
                    .next_back()
                    .unwrap_or(first_unfetched);
                let fetch_from = first_unfetched;
                let fetch_to = last_unfetched.saturating_add(interval_ms);
                return Some((fetch_from, fetch_to));
            }
        }

        // 2. Priority 2: Unfetched past-day candles preceding visible range
        let earliest_datapoint = self
            .datapoints
            .keys()
            .next()
            .copied()
            .unwrap_or(aligned_earliest);
        let prefetch_earliest = aligned_earliest
            .saturating_sub(lookback_ms)
            .max(cutoff_time)
            .max(earliest_datapoint)
            .min(aligned_earliest);
        let aligned_prefetch_earliest = (prefetch_earliest / interval_ms) * interval_ms;
        if aligned_prefetch_earliest < aligned_earliest {
            let mut past_unfetched = self
                .datapoints
                .range(aligned_prefetch_earliest..aligned_earliest)
                .filter(|(t, dp)| {
                    **t < today_midnight
                        && **t < current_candle_start
                        && !dp.trades_fetched
                        && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0
                })
                .map(|(&t, _)| t);

            if let Some(first_unfetched) = past_unfetched.next() {
                let last_unfetched = past_unfetched.next_back().unwrap_or(first_unfetched);
                let fetch_from = first_unfetched;
                let fetch_to = last_unfetched.saturating_add(interval_ms);
                return Some((fetch_from, fetch_to));
            }
        }

        // 3. Priority 3: Today's unfetched candles in visible range
        let mut visible_today_unfetched = self
            .datapoints
            .range(aligned_earliest..=aligned_latest)
            .filter(|(t, dp)| {
                **t >= today_midnight
                    && (**t < current_candle_start || (allow_active && **t == current_candle_start))
                    && !dp.trades_fetched
                    && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0
            })
            .map(|(&t, _)| t);

        if let Some(first_unfetched) = visible_today_unfetched.next() {
            let last_unfetched = visible_today_unfetched
                .next_back()
                .unwrap_or(first_unfetched);
            let fetch_from = first_unfetched;
            let fetch_to = last_unfetched.saturating_add(interval_ms);
            return Some((fetch_from, fetch_to));
        }

        // 4. Priority 4: Today's unfetched candles succeeding visible range
        let prefetch_latest = aligned_latest.saturating_add(7 * 24 * 3600 * 1000);
        let aligned_prefetch_latest = (prefetch_latest / interval_ms) * interval_ms;
        let future_start = aligned_latest.saturating_add(interval_ms);
        if future_start <= aligned_prefetch_latest {
            let mut future_unfetched = self
                .datapoints
                .range(future_start..=aligned_prefetch_latest)
                .filter(|(t, dp)| {
                    (**t < current_candle_start || (allow_active && **t == current_candle_start))
                        && !dp.trades_fetched
                        && (dp.kline.volume.0 + dp.kline.volume.1) > 0.0
                })
                .map(|(&t, _)| t);

            if let Some(first_unfetched) = future_unfetched.next() {
                let last_unfetched = future_unfetched.next_back().unwrap_or(first_unfetched);
                let fetch_from = first_unfetched;
                let fetch_to = last_unfetched.saturating_add(interval_ms);
                return Some((fetch_from, fetch_to));
            }
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

pub fn is_active_candle_fetch_supported(exchange: Option<Exchange>) -> bool {
    matches!(
        exchange,
        Some(Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse)
    )
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

    let now_ms = exchange::clock::server_time_ms(None);

    let mut map: BTreeMap<u64, KlineDataPoint> = BTreeMap::new();
    let max_gap_ms = exchange::trades::cache::INTRADAY_MAX_GAP_MS;
    let mut last_trade_per_bucket: FxHashMap<u64, u64> = FxHashMap::default();
    let mut bucket_has_gap: FxHashMap<u64, bool> = FxHashMap::default();

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

        if let Some(prev_t) = last_trade_per_bucket.get_mut(&rounded_time) {
            if trade.time.saturating_sub(*prev_t) > max_gap_ms {
                bucket_has_gap.insert(rounded_time, true);
            }
            *prev_t = trade.time;
        } else {
            last_trade_per_bucket.insert(rounded_time, trade.time);
        }

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
        let has_internal_gap = bucket_has_gap.get(&time).copied().unwrap_or(false);

        if is_historical_day
            || (!has_internal_gap && dp.is_trades_coverage_complete(interval_ms, step, now_ms))
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
            Basis::Tick(_) => {
                log::warn!(
                    "TimeSeries<HeatmapDataPoint> does not support tick basis; falling back to 1m"
                );
                exchange::Timeframe::M1
            }
        };

        Self {
            datapoints: BTreeMap::new(),
            interval: timeframe,
            tick_size,
            exchange: None,
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

    pub fn max_trade_qty_in_range(
        &self,
        earliest: u64,
        latest: u64,
        highest: Price,
        lowest: Price,
    ) -> f32 {
        let mut max_trade_qty = 0.0f32;

        self.datapoints
            .range(earliest..=latest)
            .for_each(|(_, dp)| {
                dp.grouped_trades.iter().for_each(|trade| {
                    if trade.price >= lowest && trade.price <= highest {
                        max_trade_qty = max_trade_qty.max(trade.qty);
                    }
                });
            });

        max_trade_qty
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
    fn test_suggest_trade_fetch_range_prioritizes_past_days_over_today() {
        let step = PriceStep::from_f32(1.0);
        let today_midnight = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let past_candle_time = today_midnight - 300_000;
        let today_candle_time = today_midnight + 300_000;

        let klines = vec![
            Kline {
                time: past_candle_time,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(99.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: today_candle_time,
                open: Price::from_f32(102.0),
                high: Price::from_f32(107.0),
                low: Price::from_f32(101.0),
                close: Price::from_f32(106.0),
                volume: (15.0, 15.0),
            },
        ];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);

        // Visible range covers both past and today candles
        let range = ts.suggest_trade_fetch_range(past_candle_time, today_candle_time + 300_000);
        // It must prioritize the past candle first!
        assert_eq!(range, Some((past_candle_time, past_candle_time + 300_000)));

        // Once the past candle is marked as fetched:
        ts.mark_trades_fetched(past_candle_time, past_candle_time + 300_000);

        // Now it can suggest fetching today's candle:
        let next_range =
            ts.suggest_trade_fetch_range(past_candle_time, today_candle_time + 300_000);
        assert_eq!(
            next_range,
            Some((today_candle_time, today_candle_time + 300_000))
        );
    }

    #[test]
    fn test_suggest_trade_fetch_range_never_suggests_active_forming_candle() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::M5;
        let interval_ms = interval.to_milliseconds();
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let current_candle_start = (now_ms / interval_ms) * interval_ms;

        let klines = vec![Kline {
            time: current_candle_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];
        let ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines);

        // Active/forming candle must NEVER be requested by background trade fetcher
        assert_eq!(
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms,),
            None
        );
    }

    #[test]
    fn test_suggest_trade_fetch_range_active_candle_supported_exchange() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::M5;
        let interval_ms = interval.to_milliseconds();
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let current_candle_start = (now_ms / interval_ms) * interval_ms;

        let klines = vec![Kline {
            time: current_candle_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];

        // 1. Unset exchange -> active candle is not suggested
        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines);
        assert_eq!(
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms,),
            None
        );

        // 2. Unsupported exchanges (Bybit, OKX, Hyperliquid) -> active candle is not suggested
        for unsupported in [
            Exchange::BybitLinear,
            Exchange::BybitSpot,
            Exchange::OkexLinear,
            Exchange::HyperliquidLinear,
        ] {
            ts.set_exchange(unsupported);
            assert_eq!(
                ts.suggest_trade_fetch_range(
                    current_candle_start,
                    current_candle_start + interval_ms,
                ),
                None,
                "Exchange {:?} must not suggest active candle",
                unsupported
            );
        }

        // 3. Supported exchange (Binance) -> active forming candle is suggested when unfetched
        for supported in [
            Exchange::BinanceSpot,
            Exchange::BinanceLinear,
            Exchange::BinanceInverse,
        ] {
            ts.datapoints
                .get_mut(&current_candle_start)
                .unwrap()
                .trades_fetched = false;
            ts.set_exchange(supported);
            assert_eq!(
                ts.suggest_trade_fetch_range(
                    current_candle_start,
                    current_candle_start + interval_ms,
                ),
                Some((current_candle_start, current_candle_start + interval_ms)),
                "Exchange {:?} must suggest active forming candle",
                supported
            );

            // Once marked fetched, it must stop being suggested
            ts.mark_trades_fetched(current_candle_start, current_candle_start + interval_ms);
            assert_eq!(
                ts.suggest_trade_fetch_range(
                    current_candle_start,
                    current_candle_start + interval_ms,
                ),
                None,
                "Exchange {:?} must stop suggesting after mark_trades_fetched",
                supported
            );
        }
    }

    #[test]
    fn test_rebuild_buckets_from_raw_trades_idempotency() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::M5;
        let klines = vec![Kline {
            time: 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];
        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines);

        let trades = vec![
            Trade {
                time: 301_000,
                price: Price::from_f32(100.0),
                qty: 2.0,
                is_sell: false,
            },
            Trade {
                time: 302_000,
                price: Price::from_f32(101.0),
                qty: 3.0,
                is_sell: true,
            },
        ];

        let mut affected = rustc_hash::FxHashSet::default();
        affected.insert(300_000);

        // First rebuild
        ts.rebuild_buckets_from_raw_trades(&trades, &affected);
        let dp_before = ts.datapoints.get(&300_000).unwrap();
        let total_vol_before: f32 = dp_before
            .footprint
            .trades
            .values()
            .map(|g| g.total_qty())
            .sum();
        let delta_before: f32 = dp_before
            .footprint
            .trades
            .values()
            .map(|g| g.delta_qty())
            .sum();
        assert_eq!(total_vol_before, 5.0);
        assert_eq!(delta_before, -1.0);

        // Second rebuild with same raw trades must be completely idempotent
        ts.rebuild_buckets_from_raw_trades(&trades, &affected);
        let dp_after = ts.datapoints.get(&300_000).unwrap();
        let total_vol_after: f32 = dp_after
            .footprint
            .trades
            .values()
            .map(|g| g.total_qty())
            .sum();
        let delta_after: f32 = dp_after
            .footprint
            .trades
            .values()
            .map(|g| g.delta_qty())
            .sum();
        assert_eq!(total_vol_after, total_vol_before);
        assert_eq!(delta_after, delta_before);
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

    #[test]
    fn test_aggregate_trades_for_day_today_partial_candle_remains_unfetched() {
        let step = PriceStep::from_f32(1.0);
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let today_midnight = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        // Select a closed candle from earlier today (e.g. today_midnight + 1 hour)
        let candle_time = today_midnight + 3_600_000;
        if now_ms < candle_time + 3_600_000 {
            return;
        }

        // Candle only has trades for the first 10 seconds (gap of 59.8 minutes)
        let trades = vec![
            Trade {
                time: candle_time + 100,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: candle_time + 10_000,
                price: Price::from_f32(101.0),
                qty: 1.0,
                is_sell: true,
            },
        ];

        let dps = aggregate_trades_for_day(&trades, Timeframe::H1, step);
        assert_eq!(dps.len(), 1);
        assert_eq!(dps[0].0, candle_time);
        assert!(
            !dps[0].1.trades_fetched,
            "A candle with a 59-minute trailing gap must remain unfetched"
        );
    }

    #[test]
    fn test_insert_preaggregated_footprint_preserves_unfetched_status() {
        let step = PriceStep::from_f32(1.0);
        let klines = vec![Kline {
            time: 100_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);
        assert!(!ts.datapoints.get(&100_000).unwrap().trades_fetched);

        let mut footprint = KlineTrades::new();
        footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 100_005,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            },
            step,
        );
        let new_dp = KlineDataPoint {
            kline: klines[0],
            footprint,
            trades_fetched: false,
        };

        ts.insert_preaggregated_footprint(vec![(100_000, new_dp)]);

        assert!(
            !ts.datapoints.get(&100_000).unwrap().trades_fetched,
            "insert_preaggregated_footprint must preserve new_dp.trades_fetched = false"
        );
    }

    #[test]
    fn test_insert_trades_existing_buckets_partial_batch_does_not_mark_earlier_candle_fetched() {
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
        ];
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M5, step, &klines);

        let trade = Trade {
            time: 10_000,
            price: Price::from_f32(101.0),
            qty: 1.0,
            is_sell: false,
        };
        ts.insert_trades_existing_buckets(&[trade]);

        assert!(
            !ts.datapoints.get(&0).unwrap().trades_fetched,
            "Partial trade batch must not mark earlier closed candle as fetched"
        );
    }

    #[test]
    fn test_insert_trades_existing_buckets_batch_across_gap_does_not_mark_gap_candles_fetched() {
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

        let trades = vec![
            Trade {
                time: 10_000,
                price: Price::from_f32(101.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: 605_000,
                price: Price::from_f32(107.0),
                qty: 2.0,
                is_sell: true,
            },
        ];
        ts.insert_trades_existing_buckets(&trades);

        assert!(
            !ts.datapoints.get(&0).unwrap().trades_fetched,
            "Candle 0 with 290s trailing gap must NOT be marked fetched even if batch extends past it"
        );
        assert!(
            !ts.datapoints.get(&300_000).unwrap().trades_fetched,
            "Candle 300_000 with 0 trades must NOT be marked fetched"
        );
        assert!(
            ts.datapoints.get(&600_000).unwrap().trades_fetched,
            "Latest candle 600_000 with trades should be marked fetched"
        );
    }

    #[test]
    fn test_aggregate_trades_for_day_with_subsequent_trades_does_not_mark_broken_candle_fetched() {
        let step = PriceStep::from_f32(1.0);
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let today_midnight = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let candle_time = today_midnight + 3_600_000;
        if now_ms < candle_time + 3_600_000 * 3 {
            return;
        }

        let trades = vec![
            Trade {
                time: candle_time + 100,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: candle_time + 10_000,
                price: Price::from_f32(101.0),
                qty: 1.0,
                is_sell: true,
            },
            Trade {
                time: candle_time + 7_200_000 + 500,
                price: Price::from_f32(105.0),
                qty: 2.0,
                is_sell: false,
            },
        ];

        let dps = aggregate_trades_for_day(&trades, Timeframe::H1, step);
        assert!(dps.len() >= 2);
        assert_eq!(dps[0].0, candle_time);
        assert!(
            !dps[0].1.trades_fetched,
            "Candle with a 59-minute gap must remain unfetched even if later trades exist in the day"
        );
    }

    #[test]
    fn test_aggregate_trades_for_day_internal_gap_remains_unfetched() {
        let step = PriceStep::from_f32(1.0);
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let today_midnight = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let candle_time = today_midnight + 3_600_000;
        if now_ms < candle_time + 3_600_000 {
            return;
        }

        let trades = vec![
            Trade {
                time: candle_time + 100,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: candle_time + 3_590_000,
                price: Price::from_f32(101.0),
                qty: 1.0,
                is_sell: true,
            },
        ];

        let dps = aggregate_trades_for_day(&trades, Timeframe::H1, step);
        assert_eq!(dps.len(), 1);
        assert_eq!(dps[0].0, candle_time);
        assert!(
            !dps[0].1.trades_fetched,
            "Candle with internal gap > 60s must remain unfetched even if start and end are covered"
        );
    }

    #[test]
    fn test_heatmap_timeseries_tick_basis_fallback() {
        let step = PriceStep { units: 100 };
        let basis = Basis::Tick(crate::aggr::TickCount(50));
        let ts = TimeSeries::<HeatmapDataPoint>::new(basis, step);
        assert_eq!(ts.interval, Timeframe::M1);
    }

    #[test]
    fn test_mark_trades_fetched_does_not_mark_next_active_candle() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::H1;
        let interval_ms = interval.to_milliseconds();
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let current_candle_start = (now_ms / interval_ms) * interval_ms;
        let past_candle_start = current_candle_start - interval_ms;

        let klines = vec![
            Kline {
                time: past_candle_start,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(99.0),
                close: Price::from_f32(102.0),
                volume: (10.0, 10.0),
            },
            Kline {
                time: current_candle_start,
                open: Price::from_f32(102.0),
                high: Price::from_f32(106.0),
                low: Price::from_f32(101.0),
                close: Price::from_f32(105.0),
                volume: (10.0, 10.0),
            },
        ];

        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines);
        // Historical fetch for the past closed candle ends at current_candle_start
        ts.mark_trades_fetched(past_candle_start, current_candle_start);

        // Past candle must be marked fetched
        assert!(
            ts.datapoints
                .get(&past_candle_start)
                .unwrap()
                .trades_fetched
        );

        // Active candle must NOT be marked fetched by this fetch
        assert!(
            !ts.datapoints
                .get(&current_candle_start)
                .unwrap()
                .trades_fetched,
            "Active candle starting at current_candle_start must not be marked fetched by a fetch ending at current_candle_start"
        );
    }

    #[test]
    fn test_with_trades_mid_candle_does_not_suppress_active_candle_fetch() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::H1;
        let interval_ms = interval.to_milliseconds();
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let current_candle_start = (now_ms / interval_ms) * interval_ms;

        let klines = vec![Kline {
            time: current_candle_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];

        // Live trade arrives mid-candle (e.g. 45 minutes into the 1h candle)
        let mid_candle_trade = Trade {
            time: current_candle_start + 45 * 60 * 1000,
            price: Price::from_f32(101.0),
            qty: 1.0,
            is_sell: false,
        };

        let ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines)
            .with_exchange(Exchange::BinanceLinear)
            .with_trades(&[mid_candle_trade]);

        // Active candle must NOT be marked trades_fetched because trades started mid-candle!
        assert!(
            !ts.datapoints
                .get(&current_candle_start)
                .unwrap()
                .trades_fetched,
            "Active candle must remain unfetched when trades start mid-candle"
        );

        // Active candle must be suggested for fetch!
        assert_eq!(
            ts.suggest_trade_fetch_range(current_candle_start, current_candle_start + interval_ms),
            Some((current_candle_start, current_candle_start + interval_ms)),
            "Mid-candle active candle must be suggested for historical backfill"
        );
    }

    #[test]
    fn test_rollover_clock_skew_preserves_trades_fetched() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::M5;
        let interval_ms = interval.to_milliseconds();
        let candle0_start = 300_000u64;
        let candle1_start = 600_000u64;

        let klines = vec![Kline {
            time: candle0_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];

        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines)
            .with_exchange(Exchange::BinanceLinear);

        // Candle 0 has trades covering from start to end
        let trades_c0 = vec![
            Trade {
                time: candle0_start + 1000,
                price: Price::from_f32(99.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: candle0_start + 150_000,
                price: Price::from_f32(105.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: candle0_start + interval_ms - 1000,
                price: Price::from_f32(102.0),
                qty: 1.0,
                is_sell: true,
            },
        ];
        ts.insert_realtime_trades(&trades_c0);
        ts.datapoints
            .get_mut(&candle0_start)
            .unwrap()
            .trades_fetched = true;

        // Trade arrives in candle 1 (rollover)
        let trade_c1 = Trade {
            time: candle1_start + 1000,
            price: Price::from_f32(102.0),
            qty: 1.0,
            is_sell: false,
        };
        ts.insert_realtime_trades(&[trade_c1]);

        // Candle 0 must STILL be trades_fetched = true
        assert!(
            ts.datapoints.get(&candle0_start).unwrap().trades_fetched,
            "Candle 0 must preserve trades_fetched = true across rollover even if now_ms has clock skew"
        );
    }

    #[test]
    fn test_clock_skew_over_120s_sync_unblocks_active_candle() {
        exchange::clock::reset_clock_skews();
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::D1;
        let interval_ms = interval.to_milliseconds();

        let local_now = chrono::Utc::now().timestamp_millis() as u64;

        // Simulate client clock ahead of exchange server by 150 seconds (> 120 seconds)
        let delta = -150_000i64;
        let server_now = (local_now as i64 + delta) as u64;

        let active_candle_start = (server_now / interval_ms) * interval_ms;

        let klines = vec![Kline {
            time: active_candle_start,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(99.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        }];

        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &klines)
            .with_exchange(Exchange::BinanceLinear);

        // Fetched trades up to server_now (e.g. server_now - 5_000)
        let trades_to_time = server_now - 5_000;

        // 1. Without clock skew correction (delta = 0):
        // local_now is 150s ahead of server_now.
        // trades_to_time is ~server_now, which is < (local_now - 120_000).
        // The active candle cutoff mechanism blocks marking trades_fetched = true!
        ts.mark_trades_fetched(active_candle_start, trades_to_time);
        assert!(
            !ts.datapoints
                .get(&active_candle_start)
                .unwrap()
                .trades_fetched,
            "Without clock skew tracking, client clock ahead by >120s must fail to mark active candle as fetched"
        );

        // 2. WITH clock skew tracking (delta = -150_000 ms via exchange ping):
        exchange::clock::set_clock_skew(Exchange::BinanceLinear, delta);

        // Now mark_trades_fetched uses server_time_ms(exchange) which matches server_now!
        ts.mark_trades_fetched(active_candle_start, trades_to_time);
        assert!(
            ts.datapoints
                .get(&active_candle_start)
                .unwrap()
                .trades_fetched,
            "With clock skew delta tracking, active candle must be successfully marked trades_fetched = true"
        );

        exchange::clock::reset_clock_skews();
    }

    #[test]
    fn test_dat02_naked_poc_chronological_first_touch() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M1, step, &[]);

        let k1 = Kline {
            time: 60_000,
            open: Price::from_f32(95.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (10.0, 10.0),
        };
        let k2 = Kline {
            time: 120_000,
            open: Price::from_f32(99.0),
            high: Price::from_f32(102.0),
            low: Price::from_f32(98.0),
            close: Price::from_f32(101.0),
            volume: (5.0, 5.0),
        };
        let k3 = Kline {
            time: 180_000,
            open: Price::from_f32(50.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(10.0),
            close: Price::from_f32(40.0),
            volume: (20.0, 20.0),
        };

        ts.insert_klines(&[k1, k2, k3]);

        let dp1 = ts.datapoints.get_mut(&60_000).unwrap();
        dp1.footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 60_001,
                price: Price::from_f32(100.0),
                qty: 100.0,
                is_sell: false,
            },
            step,
        );
        dp1.calculate_poc();

        ts.update_poc_status();

        let npoc = ts
            .datapoints
            .get(&60_000)
            .unwrap()
            .footprint
            .poc
            .as_ref()
            .unwrap()
            .status;
        assert_eq!(npoc, NPoc::Filled { at: 120_000 });
    }

    #[test]
    fn test_dat03_timeseries_retention_limit() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M1, step, &[]);

        let total_candles = TimeSeries::<KlineDataPoint>::MAX_RETAINED_DATAPOINTS + 50;
        let mut klines = Vec::with_capacity(total_candles);
        for i in 0..total_candles {
            klines.push(Kline {
                time: (i as u64 + 1) * 60_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(95.0),
                close: Price::from_f32(100.0),
                volume: (1.0, 1.0),
            });
        }

        ts.insert_klines(&klines);

        assert_eq!(
            ts.datapoints.len(),
            TimeSeries::<KlineDataPoint>::MAX_RETAINED_DATAPOINTS
        );
        let earliest_key = ts.datapoints.keys().next().copied().unwrap();
        assert_eq!(earliest_key, 51 * 60_000);
    }

    #[test]
    fn test_ter10_timeseries_realtime_trades_retention_limit() {
        let step = PriceStep::from_f32(1.0);
        let interval = Timeframe::M1;
        let mut ts = TimeSeries::<KlineDataPoint>::new(interval, step, &[]);

        let count = TimeSeries::<KlineDataPoint>::MAX_RETAINED_DATAPOINTS + 5;
        let trades: Vec<Trade> = (0..count)
            .map(|i| Trade {
                time: (i as u64 + 1) * 60_000,
                price: Price::from_f32(100.0),
                qty: 1.0,
                is_sell: false,
            })
            .collect();

        ts.insert_realtime_trades(&trades);

        assert_eq!(
            ts.datapoints.len(),
            TimeSeries::<KlineDataPoint>::MAX_RETAINED_DATAPOINTS
        );
    }

    #[test]
    fn test_dat05_historical_closed_day_marks_trades_fetched() {
        let step = PriceStep::from_f32(1.0);
        let t1 = 1704110400000u64;
        let t2 = t1 + 300_000;
        let trades = vec![
            Trade {
                time: t1,
                price: Price::from_f32(40000.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: t2,
                price: Price::from_f32(40010.0),
                qty: 1.0,
                is_sell: true,
            },
        ];

        let result = aggregate_trades_for_day(&trades, Timeframe::H1, step);
        assert_eq!(result.len(), 1);
        let (_, dp) = &result[0];
        assert!(
            dp.trades_fetched,
            "Historical closed day candle must be marked trades_fetched=true even with natural pauses > 60s"
        );
    }

    #[test]
    fn test_suggest_trade_fetch_range_respects_custom_lookback() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::D1, step, &[]);
        let now = chrono::Utc::now();
        let today_midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        // Create candle 100 days ago
        let candle_100d_ago = today_midnight - (100 * 24 * 3600 * 1000);
        let dp = KlineDataPoint {
            kline: Kline {
                time: candle_100d_ago,
                open: Price::from_f32(100.0),
                high: Price::from_f32(110.0),
                low: Price::from_f32(90.0),
                close: Price::from_f32(105.0),
                volume: (10.0, 10.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: false,
        };
        ts.datapoints.insert(candle_100d_ago, dp);

        // With 30 days lookback, candle 100 days ago is beyond cutoff and must NOT be suggested
        exchange::fetcher::set_trade_history_lookback_days(30);
        let suggested_30d =
            ts.suggest_trade_fetch_range(candle_100d_ago, candle_100d_ago + 86_400_000);
        assert_eq!(suggested_30d, None);

        // With 180 days lookback, candle 100 days ago is within cutoff and MUST be suggested
        exchange::fetcher::set_trade_history_lookback_days(180);
        let suggested_180d =
            ts.suggest_trade_fetch_range(candle_100d_ago, candle_100d_ago + 86_400_000);
        assert_eq!(
            suggested_180d,
            Some((candle_100d_ago, candle_100d_ago + 86_400_000))
        );

        // Per-chart override: when global is 90 days, but chart custom lookback is Some(180), it MUST be suggested
        exchange::fetcher::set_trade_history_lookback_days(90);
        let suggested_per_chart_180d = ts.suggest_trade_fetch_range_with_exchange_and_lookback(
            candle_100d_ago,
            candle_100d_ago + 86_400_000,
            None,
            Some(180),
        );
        assert_eq!(
            suggested_per_chart_180d,
            Some((candle_100d_ago, candle_100d_ago + 86_400_000))
        );

        // Per-chart override: when global is 180 days, but chart custom lookback is Some(30), it must NOT be suggested
        let suggested_per_chart_30d = ts.suggest_trade_fetch_range_with_exchange_and_lookback(
            candle_100d_ago,
            candle_100d_ago + 86_400_000,
            None,
            Some(30),
        );
        assert_eq!(suggested_per_chart_30d, None);

        // Reset default
        exchange::fetcher::set_trade_history_lookback_days(
            exchange::fetcher::DEFAULT_TRADE_HISTORY_LOOKBACK_DAYS,
        );
    }

    #[test]
    fn test_invalidate_unfetched_empty_footprints() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::D1, step, &[]);
        let now = chrono::Utc::now();
        let today_midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let candle_time = today_midnight - (50 * 24 * 3600 * 1000);
        let dp = KlineDataPoint {
            kline: Kline {
                time: candle_time,
                open: Price::from_f32(100.0),
                high: Price::from_f32(110.0),
                low: Price::from_f32(90.0),
                close: Price::from_f32(105.0),
                volume: (10.0, 10.0),
            },
            footprint: KlineTrades::new(),
            trades_fetched: true, // Marked fetched, but footprint is empty!
        };
        ts.datapoints.insert(candle_time, dp);

        exchange::fetcher::set_trade_history_lookback_days(90);
        ts.invalidate_unfetched_empty_footprints();

        assert!(!ts.datapoints.get(&candle_time).unwrap().trades_fetched);

        exchange::fetcher::set_trade_history_lookback_days(
            exchange::fetcher::DEFAULT_TRADE_HISTORY_LOOKBACK_DAYS,
        );
    }

    #[test]
    fn test_dat02_60k_candles_performance_benchmark() {
        let step = PriceStep::from_f32(1.0);
        let mut ts = TimeSeries::<KlineDataPoint>::new(Timeframe::M1, step, &[]);

        // Populate 60,000 candles with a wandering price
        let mut price = 100.0f32;
        for i in 0..60_000u64 {
            let t = (i + 1) * 60_000;
            let offset = ((i % 50) as f32 - 25.0) * 0.1;
            price = (price + offset).clamp(50.0, 500.0);
            let mut dp = KlineDataPoint {
                kline: Kline {
                    time: t,
                    open: Price::from_f32(price - 1.0),
                    high: Price::from_f32(price + 2.0),
                    low: Price::from_f32(price - 2.0),
                    close: Price::from_f32(price + 0.5),
                    volume: (5.0, 5.0),
                },
                footprint: KlineTrades::new(),
                trades_fetched: false,
            };
            let poc_price = Price::from_f32(price);
            dp.footprint.poc = Some(crate::chart::kline::PointOfControl {
                price: poc_price,
                volume: 10.0,
                status: NPoc::default(),
            });
            ts.datapoints.insert(t, dp);
        }

        let start = std::time::Instant::now();
        ts.update_poc_status();
        let elapsed = start.elapsed();

        // Must finish well under 50ms (previously took > 1000ms)
        assert!(
            elapsed.as_millis() < 100,
            "60k candles update_poc_status must complete in <100ms, took {:?}",
            elapsed
        );
    }
}
