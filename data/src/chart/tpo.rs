use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::kline::KlineDataPoint;
use exchange::{Kline, Trade};

/// Trait abstracting candle data for TPO Profile calculation.
pub trait TpoCandle {
    fn open_time_ms(&self) -> i64;
    fn high_price(&self) -> f64;
    fn low_price(&self) -> f64;
}

impl TpoCandle for Kline {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.high.to_f64()
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low.to_f64()
    }
}

impl TpoCandle for &Kline {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.high.to_f64()
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low.to_f64()
    }
}

impl TpoCandle for KlineDataPoint {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.kline.high.to_f64()
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.kline.low.to_f64()
    }
}

impl TpoCandle for &KlineDataPoint {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.kline.high.to_f64()
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.kline.low.to_f64()
    }
}

impl TpoCandle for (i64, f64, f64) {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.0
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.1
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.2
    }
}

/// Initial Balance (IB) representing the first hour trading range and standard extensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitialBalance {
    pub high: f64,
    pub low: f64,
    pub extension_high_1_5: f64,
    pub extension_high_2_0: f64,
    pub extension_low_1_5: f64,
    pub extension_low_2_0: f64,
    #[serde(default)]
    pub extension_1_5: f64,
    #[serde(default)]
    pub extension_2_0: f64,
}

/// Discrete price level with tick bin index and TPO count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: f64,
    pub tick_bin: i64,
    pub count: usize,
}

/// 70% Value Area containing Value Area High (VAH), Value Area Low (VAL), and POC price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueArea {
    pub vah: f64,
    pub val: f64,
    pub poc_price: f64,
    pub total_tpos: usize,
    pub va_tpos: usize,
}

/// Single print range representing buying/selling tails or interior single prints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SinglePrintRange {
    pub start_price: f64,
    pub end_price: f64,
    pub bracket: char,
    pub is_tail: bool,
}

/// Market Profile (TPO) structure for a single session or merged composite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TpoProfile {
    pub session_date: String,
    pub session_start: i64,
    pub session_end: i64,
    pub tick_size: f64,
    pub matrix: BTreeMap<i64, Vec<char>>,
    #[serde(default)]
    pub bracket_indices: BTreeMap<i64, Vec<u32>>,
    pub ib: Option<InitialBalance>,
    pub poc: Option<PriceLevel>,
    pub value_area: Option<ValueArea>,
    pub single_prints: Vec<SinglePrintRange>,
    pub is_poor_high: bool,
    pub is_poor_low: bool,
    pub is_split: bool,
}

/// Maps timestamp to 30-minute bracket character across a 24h+ session.
/// Brackets 0..25 map to 'A'..'Z', 26..51 map to 'a'..'z'.
/// Returns None if candle_time < session_start or bracket_idx >= 52.
pub fn get_tpo_bracket(candle_time: i64, session_start: i64) -> Option<char> {
    let delta_ms = candle_time - session_start;
    if delta_ms < 0 {
        return None;
    }
    let bracket_idx = delta_ms / (30 * 60 * 1_000);
    if bracket_idx < 26 {
        Some((b'A' + bracket_idx as u8) as char)
    } else if bracket_idx < 52 {
        Some((b'a' + (bracket_idx - 26) as u8) as char)
    } else {
        None
    }
}

/// Continuous bracket mapping for extended multi-period sessions (Weekly, Monthly, Custom N-Days).
/// Does not terminate at 52; continuously cycles through 'A'..'Z' and 'a'..'z'.
pub fn get_tpo_bracket_continuous(candle_time: i64, session_start: i64) -> Option<char> {
    let delta_ms = candle_time - session_start;
    if delta_ms < 0 {
        return None;
    }
    let bracket_idx = delta_ms / (30 * 60 * 1_000);
    let cycle_idx = (bracket_idx % 52) as u8;
    if cycle_idx < 26 {
        Some((b'A' + cycle_idx) as char)
    } else {
        Some((b'a' + (cycle_idx - 26)) as char)
    }
}

/// Calendar anchor period determining session boundary resets for TPO calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TpoAnchor {
    #[default]
    Daily,
    Weekly,
    Monthly,
}

impl TpoAnchor {
    pub const ALL: [TpoAnchor; 3] = [TpoAnchor::Daily, TpoAnchor::Weekly, TpoAnchor::Monthly];

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }

    #[inline]
    pub fn period_bounds(self, ts: i64) -> (i64, i64) {
        match self {
            Self::Daily => period_bounds_utc(ts, SessionPeriod::Daily),
            Self::Weekly => period_bounds_utc(ts, SessionPeriod::Weekly),
            Self::Monthly => period_bounds_utc(ts, SessionPeriod::Monthly),
        }
    }
}

impl std::fmt::Display for TpoAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

impl From<TpoAnchor> for SessionPeriod {
    fn from(anchor: TpoAnchor) -> Self {
        match anchor {
            TpoAnchor::Daily => SessionPeriod::Daily,
            TpoAnchor::Weekly => SessionPeriod::Weekly,
            TpoAnchor::Monthly => SessionPeriod::Monthly,
        }
    }
}

impl TryFrom<SessionPeriod> for TpoAnchor {
    type Error = ();
    fn try_from(period: SessionPeriod) -> Result<Self, Self::Error> {
        match period {
            SessionPeriod::Daily => Ok(TpoAnchor::Daily),
            SessionPeriod::Weekly => Ok(TpoAnchor::Weekly),
            SessionPeriod::Monthly => Ok(TpoAnchor::Monthly),
            _ => Err(()),
        }
    }
}

/// 1-minute candle aggregation synthesized from raw trades.
/// Provides a consistent sub-period basis for TPO brackets even under low liquidity or candle gaps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OneMinBar {
    pub open_time_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub trade_count: usize,
}

impl OneMinBar {
    pub fn new(ts: i64, price: f64, qty: f64) -> Self {
        let open_time_ms = ts.div_euclid(60_000) * 60_000;
        Self {
            open_time_ms,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: qty.max(0.0),
            trade_count: 1,
        }
    }

    pub fn update(&mut self, price: f64, qty: f64) {
        if price > self.high {
            self.high = price;
        }
        if price < self.low {
            self.low = price;
        }
        self.close = price;
        self.volume += qty.max(0.0);
        self.trade_count += 1;
    }
}

impl TpoCandle for OneMinBar {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.open_time_ms
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.high
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low
    }
}

impl TpoCandle for &OneMinBar {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.open_time_ms
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.high
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low
    }
}

/// Supported aggregation periods for grouping historical candles into TPO profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SessionPeriod {
    #[default]
    Daily,
    TradingSessions,
    FourHours,
    Weekly,
    Monthly,
    CustomDays(u32),
}

impl SessionPeriod {
    pub const ALL: [SessionPeriod; 9] = [
        SessionPeriod::Daily,
        SessionPeriod::TradingSessions,
        SessionPeriod::FourHours,
        SessionPeriod::Weekly,
        SessionPeriod::Monthly,
        SessionPeriod::CustomDays(2),
        SessionPeriod::CustomDays(3),
        SessionPeriod::CustomDays(4),
        SessionPeriod::CustomDays(5),
    ];

    pub fn label(self) -> String {
        match self {
            Self::Daily => "Daily".to_string(),
            Self::TradingSessions => "Sessions (Asia/LDN/NY)".to_string(),
            Self::FourHours => "4H".to_string(),
            Self::Weekly => "Weekly".to_string(),
            Self::Monthly => "Monthly".to_string(),
            Self::CustomDays(days) => format!("{}D", days.max(1)),
        }
    }
}

impl std::fmt::Display for SessionPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Calculates UTC session start and end boundaries for a given candle timestamp and aggregation period.
pub fn period_bounds_utc(ts: i64, period: SessionPeriod) -> (i64, i64) {
    match period {
        SessionPeriod::Daily => {
            let day_ms = 86_400_000i64;
            let start = ts.div_euclid(day_ms) * day_ms;
            let end = start + day_ms;
            (start, end)
        }
        SessionPeriod::TradingSessions => {
            let day_ms = 86_400_000i64;
            let day_start = ts.div_euclid(day_ms) * day_ms;
            let hour_ms = 3_600_000i64;
            let delta = ts - day_start;
            let session_idx = (delta / (8 * hour_ms)).clamp(0, 2);
            let start = day_start + session_idx * 8 * hour_ms;
            let end = start + 8 * hour_ms;
            (start, end)
        }
        SessionPeriod::FourHours => {
            let span_ms = 4 * 3_600_000i64;
            let start = ts.div_euclid(span_ms) * span_ms;
            let end = start + span_ms;
            (start, end)
        }
        SessionPeriod::Weekly => {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                use chrono::Datelike;
                let days_from_monday = dt.weekday().num_days_from_monday() as i64;
                let monday_date = dt.date_naive() - chrono::Duration::days(days_from_monday);
                let start = monday_date
                    .and_hms_opt(0, 0, 0)
                    .map(|d| d.and_utc().timestamp_millis())
                    .unwrap_or_else(|| {
                        let day_ms = 86_400_000i64;
                        let days_since_epoch = ts.div_euclid(day_ms);
                        let monday_idx = (days_since_epoch - 4).div_euclid(7) * 7 + 4;
                        monday_idx * day_ms
                    });
                let end = (monday_date + chrono::Duration::days(7))
                    .and_hms_opt(0, 0, 0)
                    .map(|d| d.and_utc().timestamp_millis())
                    .unwrap_or(start + 7 * 86_400_000);
                (start, end)
            } else {
                let day_ms = 86_400_000i64;
                let days_since_epoch = ts.div_euclid(day_ms);
                let monday_idx = (days_since_epoch - 4).div_euclid(7) * 7 + 4;
                let start = monday_idx * day_ms;
                (start, start + 7 * day_ms)
            }
        }
        SessionPeriod::Monthly => {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                use chrono::Datelike;
                let year = dt.year();
                let month = dt.month();
                let start_date = chrono::NaiveDate::from_ymd_opt(year, month, 1)
                    .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
                let (next_year, next_month) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };
                let next_date = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year, month, 28).unwrap());
                let start = start_date
                    .and_hms_opt(0, 0, 0)
                    .map(|d| d.and_utc().timestamp_millis())
                    .unwrap_or(ts);
                let end = next_date
                    .and_hms_opt(0, 0, 0)
                    .map(|d| d.and_utc().timestamp_millis())
                    .unwrap_or(start + 30 * 86_400_000);
                (start, end)
            } else {
                let day_ms = 86_400_000i64;
                let start = ts.div_euclid(day_ms) * day_ms;
                (start, start + 30 * day_ms)
            }
        }
        SessionPeriod::CustomDays(n) => {
            let days = (n as i64).max(1);
            let span_ms = days * 86_400_000i64;
            let start = ts.div_euclid(span_ms) * span_ms;
            let end = start + span_ms;
            (start, end)
        }
    }
}

/// Groups historical candles into session buckets based on the specified aggregation period.
pub fn group_candles_by_period<C: TpoCandle + Clone>(
    candles: &[C],
    period: SessionPeriod,
) -> Vec<(i64, i64, Vec<C>)> {
    if candles.is_empty() {
        return Vec::new();
    }

    let mut sessions: Vec<(i64, i64, Vec<C>)> = Vec::new();

    for c in candles {
        let open_time = c.open_time_ms();
        if let Some(last) = sessions.last_mut()
            && open_time >= last.0
            && open_time < last.1
        {
            last.2.push(c.clone());
            continue;
        }

        let (start, end) = period_bounds_utc(open_time, period);
        if let Some(pos) = sessions.iter().position(|s| s.0 == start) {
            sessions[pos].2.push(c.clone());
        } else {
            sessions.push((start, end, vec![c.clone()]));
        }
    }

    sessions.sort_unstable_by_key(|s| s.0);
    sessions
}

/// Cluster of sessions to be merged into a single composite TPO profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionCluster {
    /// Sorted, unique session_start timestamps belonging to this cluster.
    pub session_starts: Vec<i64>,
}

impl SessionCluster {
    pub fn new(mut session_starts: Vec<i64>) -> Self {
        session_starts.sort_unstable();
        session_starts.dedup();
        Self { session_starts }
    }

    #[inline(always)]
    pub fn contains(&self, start: i64) -> bool {
        self.session_starts.binary_search(&start).is_ok()
    }
}

/// Merges two sessions or existing clusters into a unified session cluster.
pub fn merge_adjacent_clusters(clusters: &mut Vec<SessionCluster>, s1: i64, s2: i64) {
    if s1 == s2 {
        return;
    }
    let mut starts_to_merge = vec![s1, s2];

    clusters.retain(|c| {
        if c.contains(s1) || c.contains(s2) {
            starts_to_merge.extend_from_slice(&c.session_starts);
            false
        } else {
            true
        }
    });

    starts_to_merge.sort_unstable();
    starts_to_merge.dedup();
    if starts_to_merge.len() >= 2 {
        clusters.push(SessionCluster {
            session_starts: starts_to_merge,
        });
    }
}

/// Splits (unmerges) any cluster containing the specified session start timestamp.
pub fn split_cluster(clusters: &mut Vec<SessionCluster>, session_start: i64) {
    clusters.retain(|c| !c.contains(session_start));
}

/// Resolves raw unmerged session profiles according to active session clusters.
pub fn apply_session_clusters(
    raw_profiles: &[TpoProfile],
    clusters: &[SessionCluster],
) -> Vec<TpoProfile> {
    if raw_profiles.is_empty() {
        return Vec::new();
    }

    let mut out_tpos = Vec::new();
    let mut idx = 0;
    while idx < raw_profiles.len() {
        let cur_start = raw_profiles[idx].session_start;
        let matching_cluster = clusters.iter().find(|c| c.contains(cur_start));

        if let Some(cluster) = matching_cluster {
            let mut cluster_profiles = Vec::new();
            while idx < raw_profiles.len() && cluster.contains(raw_profiles[idx].session_start) {
                cluster_profiles.push(&raw_profiles[idx]);
                idx += 1;
            }

            if cluster_profiles.len() > 1 {
                let to_merge: Vec<TpoProfile> =
                    cluster_profiles.iter().map(|&p| p.clone()).collect();
                if let Some(merged) = merge_tpo_profiles(&to_merge) {
                    out_tpos.push(merged);
                } else {
                    for p in cluster_profiles {
                        out_tpos.push(p.clone());
                    }
                }
            } else if let Some(&single) = cluster_profiles.first() {
                out_tpos.push(single.clone());
            }
        } else {
            let p = &raw_profiles[idx];
            out_tpos.push(p.clone());
            idx += 1;
        }
    }

    out_tpos
}

/// Calculates the optimal TPO tick step size based on price magnitude or manual multiplier.
/// Ensures standard 40-100 TPO price bins per daily session for maximum clarity and high FPS.
pub fn calculate_tpo_tick_size(base_price: f64, exchange_tick: f64, multiplier: u32) -> f64 {
    let tick = if exchange_tick <= 0.0 || !exchange_tick.is_finite() {
        0.01
    } else {
        exchange_tick
    };

    if multiplier > 0 {
        return tick * (multiplier as f64);
    }

    let price = if base_price.is_finite() && base_price > 0.0 {
        base_price
    } else {
        100.0
    };

    // Target ~60 to 80 TPO bins across a typical ~3% daily session range
    let target_step = price * 0.0004;
    if target_step <= tick {
        return tick;
    }

    // Find the nearest clean step in {1, 2, 5} * 10^k
    let exponent = 10.0_f64.powf(target_step.log10().floor());
    let fraction = target_step / exponent;
    let nice_mult = if fraction < 1.5 {
        1.0
    } else if fraction < 3.5 {
        2.0
    } else if fraction < 7.5 {
        5.0
    } else {
        10.0
    };

    let step = nice_mult * exponent;
    let rounded = (step / tick).round() * tick;
    rounded.max(tick)
}

/// Recalculates POC, 70% Value Area (CBOT dual-row expansion), Poor High/Low, and Single Prints.
pub fn recalculate_metrics(
    matrix: &BTreeMap<i64, Vec<char>>,
    tick_size: f64,
) -> (
    Option<PriceLevel>,
    Option<ValueArea>,
    Vec<SinglePrintRange>,
    bool,
    bool,
) {
    if matrix.is_empty() {
        return (None, None, Vec::new(), false, false);
    }

    let min_tick = *matrix.keys().next().unwrap();
    let max_tick = *matrix.keys().next_back().unwrap();

    // 1. Point of Control (POC) with range midpoint tie-breaker
    let mut max_count = 0;
    let mut total_tpos = 0;
    for row in matrix.values() {
        total_tpos += row.len();
        if row.len() > max_count {
            max_count = row.len();
        }
    }

    let poc = if max_count > 0 {
        let mid_tick = (min_tick as f64 + max_tick as f64) / 2.0;

        let mut candidate_ticks: Vec<i64> = matrix
            .iter()
            .filter(|(_, row)| row.len() == max_count)
            .map(|(t, _)| *t)
            .collect();

        // Stable sort by distance to range midpoint; ascending tick order breaks equidistant ties
        candidate_ticks.sort_by(|a, b| {
            let dist_a = (*a as f64 - mid_tick).abs();
            let dist_b = (*b as f64 - mid_tick).abs();
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let chosen_tick = candidate_ticks[0];
        Some(PriceLevel {
            price: chosen_tick as f64 * tick_size,
            tick_bin: chosen_tick,
            count: max_count,
        })
    } else {
        None
    };

    // 2. 70% Value Area (CBOT dual-row outward expansion)
    let value_area = if let Some(ref poc_level) = poc {
        let target_tpos = (total_tpos as f64 * 0.70).ceil() as usize;
        let mut va_tpos = poc_level.count;
        let mut upper_tick = poc_level.tick_bin.saturating_add(1);
        let mut lower_tick = poc_level.tick_bin.saturating_sub(1);

        while va_tpos < target_tpos
            && (matrix.contains_key(&upper_tick) || matrix.contains_key(&lower_tick))
        {
            let upper_count = matrix.get(&upper_tick).map(|r| r.len()).unwrap_or(0);
            let lower_count = matrix.get(&lower_tick).map(|r| r.len()).unwrap_or(0);

            if upper_count >= lower_count && upper_count > 0 {
                va_tpos += upper_count;
                upper_tick = upper_tick.saturating_add(1);
            } else if lower_count > 0 {
                va_tpos += lower_count;
                lower_tick = lower_tick.saturating_sub(1);
            } else {
                break;
            }
        }

        Some(ValueArea {
            vah: (upper_tick.saturating_sub(1)) as f64 * tick_size,
            val: (lower_tick.saturating_add(1)) as f64 * tick_size,
            poc_price: poc_level.price,
            total_tpos,
            va_tpos,
        })
    } else {
        None
    };

    // 3. Poor High & Poor Low detection
    // A poor extreme occurs when the extreme tick has >= 2 TPOs (absence of single-print excess)
    let is_poor_high = matrix[&max_tick].len() >= 2;
    let is_poor_low = matrix[&min_tick].len() >= 2;

    // 4. Single Prints detection
    let mut single_prints = Vec::new();
    let ticks: Vec<i64> = matrix.keys().copied().collect();

    // Bottom tail: contiguous single prints starting from min_tick
    let mut bottom_tail_max = min_tick.saturating_sub(1);
    if !is_poor_low {
        for &t in &ticks {
            if matrix[&t].len() == 1 && t == bottom_tail_max.saturating_add(1) {
                bottom_tail_max = t;
            } else if t > min_tick {
                break;
            }
        }
    }

    // Top tail: contiguous single prints ending at max_tick
    let mut top_tail_min = max_tick.saturating_add(1);
    if !is_poor_high {
        for &t in ticks.iter().rev() {
            if matrix[&t].len() == 1 && t == top_tail_min.saturating_sub(1) {
                top_tail_min = t;
            } else if t < max_tick {
                break;
            }
        }
    }

    for &t in &ticks {
        let row = &matrix[&t];
        if row.len() == 1 {
            let is_tail = t <= bottom_tail_max || t >= top_tail_min;
            single_prints.push(SinglePrintRange {
                start_price: t as f64 * tick_size,
                end_price: (t as f64 + 1.0) * tick_size,
                bracket: row[0],
                is_tail,
            });
        }
    }

    (poc, value_area, single_prints, is_poor_high, is_poor_low)
}

/// Builds a complete TPO Profile from historical candles and session bounds.
pub fn build_tpo_profile<C: TpoCandle>(
    candles: &[C],
    session_start: i64,
    session_end: i64,
    session_date: &str,
    tick_size: f64,
) -> TpoProfile {
    let tick = if tick_size <= 0.0 || !tick_size.is_finite() {
        0.01
    } else {
        tick_size
    };
    let mut matrix: BTreeMap<i64, Vec<char>> = BTreeMap::new();
    let mut bracket_indices: BTreeMap<i64, Vec<u32>> = BTreeMap::new();
    let is_extended_period = (session_end - session_start) > 52 * 30 * 60_000;

    let mut ib_high = f64::NEG_INFINITY;
    let mut ib_low = f64::INFINITY;
    let mut has_ib = false;

    let mut last_bracket_per_tick: std::collections::HashMap<i64, i64> = if is_extended_period {
        std::collections::HashMap::new()
    } else {
        std::collections::HashMap::with_capacity(0)
    };

    for c in candles {
        let open_time = c.open_time_ms();
        if open_time < session_start || open_time >= session_end {
            continue;
        }
        let high = c.high_price();
        let low = c.low_price();
        if !low.is_finite() || !high.is_finite() {
            continue;
        }

        let delta_ms = open_time - session_start;
        if delta_ms < 0 {
            continue;
        }
        let bracket_idx = delta_ms / (30 * 60 * 1_000);

        let bracket_opt = if is_extended_period {
            get_tpo_bracket_continuous(open_time, session_start)
        } else {
            get_tpo_bracket(open_time, session_start)
        };

        if let Some(bracket) = bracket_opt {
            let low_tick = (low / tick).round() as i64;
            let high_tick = (high / tick).round() as i64;

            if low_tick > high_tick {
                continue;
            }

            const MAX_TICK_BINS_PER_CANDLE: i64 = 5_000;
            let high_tick = if high_tick.saturating_sub(low_tick) > MAX_TICK_BINS_PER_CANDLE {
                low_tick.saturating_add(MAX_TICK_BINS_PER_CANDLE)
            } else {
                high_tick
            };

            if is_extended_period {
                for t in low_tick..=high_tick {
                    if last_bracket_per_tick.get(&t).copied() != Some(bracket_idx) {
                        matrix.entry(t).or_default().push(bracket);
                        bracket_indices
                            .entry(t)
                            .or_default()
                            .push(bracket_idx as u32);
                        last_bracket_per_tick.insert(t, bracket_idx);
                    }
                }
            } else {
                for t in low_tick..=high_tick {
                    let row = matrix.entry(t).or_default();
                    if !row.contains(&bracket) {
                        row.push(bracket);
                        bracket_indices
                            .entry(t)
                            .or_default()
                            .push(bracket_idx as u32);
                    }
                }
            }

            // Initial Balance: first hour (brackets 0 and 1: 'A' and 'B')
            if bracket_idx == 0 || bracket_idx == 1 {
                has_ib = true;
                if high > ib_high {
                    ib_high = high;
                }
                if low < ib_low {
                    ib_low = low;
                }
            }
        }
    }

    let ib = if has_ib && ib_high >= ib_low {
        let span = ib_high - ib_low;
        Some(InitialBalance {
            high: ib_high,
            low: ib_low,
            extension_high_1_5: ib_high + span * 0.5,
            extension_high_2_0: ib_high + span * 1.0,
            extension_low_1_5: ib_low - span * 0.5,
            extension_low_2_0: ib_low - span * 1.0,
            extension_1_5: ib_high + span * 0.5,
            extension_2_0: ib_high + span * 1.0,
        })
    } else {
        None
    };

    let (poc, value_area, single_prints, is_poor_high, is_poor_low) =
        recalculate_metrics(&matrix, tick);

    TpoProfile {
        session_date: session_date.to_string(),
        session_start,
        session_end,
        tick_size: tick,
        matrix,
        bracket_indices,
        ib,
        poc,
        value_area,
        single_prints,
        is_poor_high,
        is_poor_low,
        is_split: false,
    }
}

/// Convenience builder omitting session_date for callers without date tags.
pub fn build_tpo_profile_simple<C: TpoCandle>(
    candles: &[C],
    session_start: i64,
    session_end: i64,
    tick_size: f64,
) -> TpoProfile {
    build_tpo_profile(
        candles,
        session_start,
        session_end,
        &session_start.to_string(),
        tick_size,
    )
}

/// Merges multiple TPO profiles into a single composite profile.
pub fn merge_tpo_profiles(profiles: &[TpoProfile]) -> Option<TpoProfile> {
    if profiles.is_empty() {
        return None;
    }

    let mut sorted = profiles.to_vec();
    sorted.sort_by_key(|p| p.session_start);
    let session_start = sorted[0].session_start;
    let session_end = sorted.iter().map(|p| p.session_end).max().unwrap();
    let tick_size = sorted[0].tick_size;
    let session_date = format!("{}+", sorted[0].session_date);

    let mut composite_matrix: BTreeMap<i64, Vec<char>> = BTreeMap::new();
    let mut composite_indices: BTreeMap<i64, Vec<u32>> = BTreeMap::new();
    let mut ib_candidates = Vec::new();

    for p in &sorted {
        if let Some(ref ib) = p.ib {
            ib_candidates.push(ib.clone());
        }
        let p_offset_brackets =
            ((p.session_start - session_start).max(0) / (30 * 60 * 1_000)) as u32;

        for (&t, row) in &p.matrix {
            let row_indices = p.bracket_indices.get(&t);
            let entry = composite_matrix.entry(t).or_default();
            let entry_indices = composite_indices.entry(t).or_default();

            for (idx_in_row, &ch) in row.iter().enumerate() {
                let actual_idx = if let Some(indices) = row_indices
                    && let Some(&stored_idx) = indices.get(idx_in_row)
                {
                    p_offset_brackets + stored_idx
                } else {
                    let b_idx = if ch.is_ascii_uppercase() {
                        (ch as u8 - b'A') as u32
                    } else if ch.is_ascii_lowercase() {
                        26 + (ch as u8 - b'a') as u32
                    } else {
                        0
                    };
                    p_offset_brackets + b_idx
                };

                if !entry_indices.contains(&actual_idx) {
                    entry.push(ch);
                    entry_indices.push(actual_idx);
                }
            }
        }
    }

    // Recalculate Initial Balance from all ticks containing 'A' or 'B' across the composite matrix
    let mut ib_ticks: Vec<i64> = composite_matrix
        .iter()
        .filter(|(_, row)| row.contains(&'A') || row.contains(&'B'))
        .map(|(t, _)| *t)
        .collect();

    let ib = if !ib_ticks.is_empty() {
        ib_ticks.sort_unstable();
        let min_t = ib_ticks[0];
        let max_t = ib_ticks[ib_ticks.len() - 1];
        let low = min_t as f64 * tick_size;
        let high = max_t as f64 * tick_size;
        let span = high - low;
        Some(InitialBalance {
            high,
            low,
            extension_high_1_5: high + span * 0.5,
            extension_high_2_0: high + span * 1.0,
            extension_low_1_5: low - span * 0.5,
            extension_low_2_0: low - span * 1.0,
            extension_1_5: high + span * 0.5,
            extension_2_0: high + span * 1.0,
        })
    } else if !ib_candidates.is_empty() {
        let ib_high = ib_candidates
            .iter()
            .map(|ib| ib.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let ib_low = ib_candidates
            .iter()
            .map(|ib| ib.low)
            .fold(f64::INFINITY, f64::min);
        let span = ib_high - ib_low;
        Some(InitialBalance {
            high: ib_high,
            low: ib_low,
            extension_high_1_5: ib_high + span * 0.5,
            extension_high_2_0: ib_high + span * 1.0,
            extension_low_1_5: ib_low - span * 0.5,
            extension_low_2_0: ib_low - span * 1.0,
            extension_1_5: ib_high + span * 0.5,
            extension_2_0: ib_high + span * 1.0,
        })
    } else {
        None
    };

    let (poc, value_area, single_prints, is_poor_high, is_poor_low) =
        recalculate_metrics(&composite_matrix, tick_size);

    Some(TpoProfile {
        session_date,
        session_start,
        session_end,
        tick_size,
        matrix: composite_matrix,
        bracket_indices: composite_indices,
        ib,
        poc,
        value_area,
        single_prints,
        is_poor_high,
        is_poor_low,
        is_split: false,
    })
}

/// Splits a composite TPO profile back into individual session profiles.
pub fn split_tpo_profile(
    _composite: &TpoProfile,
    original_sessions: &[TpoProfile],
) -> Vec<TpoProfile> {
    original_sessions
        .iter()
        .map(|p| {
            let mut cloned = p.clone();
            cloned.is_split = true;
            cloned
        })
        .collect()
}

/// Expands a collapsed TPO matrix into bracket-specific tick columns.
pub fn expand_bracket_columns(profile: &TpoProfile) -> BTreeMap<char, Vec<i64>> {
    let mut columns: BTreeMap<char, Vec<i64>> = BTreeMap::new();
    for (&tick, row) in &profile.matrix {
        for &ch in row {
            columns.entry(ch).or_default().push(tick);
        }
    }
    for list in columns.values_mut() {
        list.sort_unstable();
    }
    columns
}

/// Groups candles by session duration (e.g. 86_400_000 for 24h).
pub fn group_candles_by_session<C: TpoCandle + Clone>(
    candles: &[C],
    session_duration_ms: i64,
) -> Vec<(i64, i64, Vec<C>)> {
    let duration = if session_duration_ms <= 0 {
        86_400_000
    } else {
        session_duration_ms
    };
    let mut sessions: BTreeMap<i64, Vec<C>> = BTreeMap::new();

    for c in candles {
        let bucket = (c.open_time_ms().div_euclid(duration)) * duration;
        sessions.entry(bucket).or_default().push(c.clone());
    }

    sessions
        .into_iter()
        .map(|(start, list)| {
            let end = start + duration;
            (start, end, list)
        })
        .collect()
}

/// Sub-second TPO calculation engine operating directly on raw trade streams (`Trade`).
/// Synthesizes 1-minute bars to guarantee accurate bracket placement across low-liquidity gaps,
/// and maintains incremental TPO profile state per session according to calendar `TpoAnchor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeTpoCache {
    anchor: TpoAnchor,
    tick_size: f64,
    sessions: BTreeMap<i64, TpoProfile>,
    one_min_bars: BTreeMap<i64, OneMinBar>,
    active_session_start: Option<i64>,
    metrics_dirty: bool,
    max_bars: usize,
}

impl Default for TradeTpoCache {
    fn default() -> Self {
        Self::new(TpoAnchor::Daily, 1.0)
    }
}

impl TradeTpoCache {
    /// Maximum 1-minute bars retained in RAM (approx. 70 days).
    pub const DEFAULT_MAX_BARS: usize = 100_000;

    /// Creates a new `TradeTpoCache` with the specified calendar anchor and tick step size.
    pub fn new(anchor: TpoAnchor, tick_size: f64) -> Self {
        let tick = if tick_size.is_finite() && tick_size > 0.0 {
            tick_size
        } else {
            1.0
        };
        Self {
            anchor,
            tick_size: tick,
            sessions: BTreeMap::new(),
            one_min_bars: BTreeMap::new(),
            active_session_start: None,
            metrics_dirty: false,
            max_bars: Self::DEFAULT_MAX_BARS,
        }
    }

    #[inline]
    pub fn anchor(&self) -> TpoAnchor {
        self.anchor
    }

    /// Sets a new calendar anchor (Daily, Weekly, Monthly) and rebuilds session profiles from 1-minute bars.
    pub fn set_anchor(&mut self, anchor: TpoAnchor) {
        if self.anchor != anchor {
            self.anchor = anchor;
            self.rebuild();
        }
    }

    #[inline]
    pub fn tick_size(&self) -> f64 {
        self.tick_size
    }

    /// Sets a new tick step size and rebuilds session profiles from 1-minute bars.
    pub fn set_tick_size(&mut self, tick_size: f64) {
        let valid_tick = if tick_size.is_finite() && tick_size > 0.0 {
            tick_size
        } else {
            self.tick_size
        };
        if (self.tick_size - valid_tick).abs() > 1e-9 {
            self.tick_size = valid_tick;
            self.rebuild();
        }
    }

    #[inline]
    pub fn one_min_bars(&self) -> &BTreeMap<i64, OneMinBar> {
        &self.one_min_bars
    }

    #[inline]
    pub fn sessions(&self) -> &BTreeMap<i64, TpoProfile> {
        &self.sessions
    }

    #[inline]
    pub fn has_session(&self, session_start: i64) -> bool {
        self.sessions.contains_key(&session_start)
    }

    #[inline]
    pub fn active_session_start(&self) -> Option<i64> {
        self.active_session_start
    }

    #[inline]
    pub fn max_bars(&self) -> usize {
        self.max_bars
    }

    pub fn set_max_bars(&mut self, max_bars: usize) {
        self.max_bars = max_bars.max(100);
        if self.one_min_bars.len() > self.max_bars {
            let excess = self.one_min_bars.len() - self.max_bars;
            let keys_to_remove: Vec<i64> = self.one_min_bars.keys().take(excess).copied().collect();
            for k in keys_to_remove {
                self.one_min_bars.remove(&k);
            }
        }
    }

    /// Returns a mutable reference to the active session profile, ensuring metrics are up-to-date.
    pub fn active_profile(&mut self) -> Option<&TpoProfile> {
        self.recompute_active_metrics();
        self.active_session_start
            .and_then(|s| self.sessions.get(&s))
    }

    /// Returns a reference to the profile of a specific session, recomputing metrics if active.
    pub fn session_profile(&mut self, session_start: i64) -> Option<&TpoProfile> {
        if self.active_session_start == Some(session_start) && self.metrics_dirty {
            self.recompute_active_metrics();
        }
        self.sessions.get(&session_start)
    }

    /// Ingests a single raw trade, updating the 1-minute bar and the active session TPO matrix.
    pub fn on_trade(&mut self, trade: &Trade) {
        let ts = trade.time as i64;
        let price = trade.price.to_f64();
        if !price.is_finite() || price <= 0.0 {
            return;
        }
        let qty = trade.qty as f64;
        let bar_start = ts.div_euclid(60_000) * 60_000;

        let (is_new_bar, prev_low, prev_high) = match self.one_min_bars.get_mut(&bar_start) {
            Some(existing) => {
                let pl = existing.low;
                let ph = existing.high;
                existing.update(price, qty);
                (false, pl, ph)
            }
            None => {
                self.one_min_bars
                    .insert(bar_start, OneMinBar::new(ts, price, qty));
                (true, price, price)
            }
        };

        if self.one_min_bars.len() > self.max_bars {
            let excess = self.one_min_bars.len() - self.max_bars;
            let keys_to_remove: Vec<i64> = self.one_min_bars.keys().take(excess).copied().collect();
            for k in keys_to_remove {
                self.one_min_bars.remove(&k);
            }
        }

        let (session_start, session_end) = self.anchor.period_bounds(ts);
        if let Some(active_start) = self.active_session_start
            && active_start != session_start
            && self.metrics_dirty
            && let Some(prev_prof) = self.sessions.get_mut(&active_start)
        {
            let (poc, va, sp, poor_h, poor_l) =
                recalculate_metrics(&prev_prof.matrix, self.tick_size);
            prev_prof.poc = poc;
            prev_prof.value_area = va;
            prev_prof.single_prints = sp;
            prev_prof.is_poor_high = poor_h;
            prev_prof.is_poor_low = poor_l;
        }
        self.active_session_start = Some(session_start);

        let session = self.sessions.entry(session_start).or_insert_with(|| {
            let date_str = chrono::DateTime::from_timestamp_millis(session_start)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| session_start.to_string());
            TpoProfile {
                session_date: date_str,
                session_start,
                session_end,
                tick_size: self.tick_size,
                matrix: BTreeMap::new(),
                bracket_indices: BTreeMap::new(),
                ib: None,
                poc: None,
                value_area: None,
                single_prints: Vec::new(),
                is_poor_high: false,
                is_poor_low: false,
                is_split: false,
            }
        });

        let delta_ms = ts.saturating_sub(session_start);
        let bracket_idx = (delta_ms / (30 * 60_000)).max(0);
        let is_extended = (session_end - session_start) > 52 * 30 * 60_000;
        let bracket_opt = if is_extended {
            get_tpo_bracket_continuous(ts, session_start)
        } else {
            get_tpo_bracket(ts, session_start)
        };

        if let Some(bracket) = bracket_opt {
            let tick = self.tick_size;
            let (low_t, high_t) = if is_new_bar {
                let t = (price / tick).round() as i64;
                (t, t)
            } else if price > prev_high {
                let low = (prev_high / tick).round() as i64;
                let high = (price / tick).round() as i64;
                (low, high)
            } else if price < prev_low {
                let low = (price / tick).round() as i64;
                let high = (prev_low / tick).round() as i64;
                (low, high)
            } else {
                let t = (price / tick).round() as i64;
                (t, t)
            };

            const MAX_TICK_BINS_PER_TRADE: i64 = 5_000;
            let bounded_high_t = if high_t.saturating_sub(low_t) > MAX_TICK_BINS_PER_TRADE {
                low_t.saturating_add(MAX_TICK_BINS_PER_TRADE)
            } else {
                high_t
            };

            for t in low_t..=bounded_high_t {
                let row = session.matrix.entry(t).or_default();
                if !row.contains(&bracket) {
                    row.push(bracket);
                    session
                        .bracket_indices
                        .entry(t)
                        .or_default()
                        .push(bracket_idx as u32);
                }
            }

            if bracket_idx == 0 || bracket_idx == 1 {
                let (new_high, new_low) = if let Some(ref ib) = session.ib {
                    (ib.high.max(price), ib.low.min(price))
                } else {
                    (price, price)
                };
                let span = new_high - new_low;
                session.ib = Some(InitialBalance {
                    high: new_high,
                    low: new_low,
                    extension_high_1_5: new_high + span * 0.5,
                    extension_high_2_0: new_high + span * 1.0,
                    extension_low_1_5: new_low - span * 0.5,
                    extension_low_2_0: new_low - span * 1.0,
                    extension_1_5: new_high + span * 0.5,
                    extension_2_0: new_high + span * 1.0,
                });
            }

            self.metrics_dirty = true;
        }
    }

    /// Batch ingests raw trades with sub-second execution, updating the active profile metrics.
    pub fn on_trades(&mut self, trades: &[Trade], tick_size: f64) {
        if trades.is_empty() {
            return;
        }
        let valid_tick = if tick_size.is_finite() && tick_size > 0.0 {
            tick_size
        } else {
            self.tick_size
        };
        if (self.tick_size - valid_tick).abs() > 1e-9 {
            self.set_tick_size(valid_tick);
        }
        for trade in trades {
            self.on_trade(trade);
        }
        self.recompute_active_metrics();
    }

    /// Recalculates metrics (POC, Value Area, IB, Single Prints) for the active session.
    pub fn recompute_active_metrics(&mut self) {
        if !self.metrics_dirty {
            return;
        }
        if let Some(active_start) = self.active_session_start
            && let Some(profile) = self.sessions.get_mut(&active_start)
        {
            let (poc, va, sp, poor_h, poor_l) =
                recalculate_metrics(&profile.matrix, self.tick_size);
            profile.poc = poc;
            profile.value_area = va;
            profile.single_prints = sp;
            profile.is_poor_high = poor_h;
            profile.is_poor_low = poor_l;
        }
        self.metrics_dirty = false;
    }

    /// Rebuilds all session profiles from stored 1-minute bars.
    pub fn rebuild(&mut self) {
        self.sessions.clear();
        self.active_session_start = None;
        self.metrics_dirty = false;

        if self.one_min_bars.is_empty() {
            return;
        }

        let mut session_groups: BTreeMap<i64, (i64, Vec<OneMinBar>)> = BTreeMap::new();
        for bar in self.one_min_bars.values() {
            let (start, end) = self.anchor.period_bounds(bar.open_time_ms);
            let entry = session_groups
                .entry(start)
                .or_insert_with(|| (end, Vec::new()));
            entry.1.push(*bar);
        }

        for (session_start, (session_end, bars)) in session_groups {
            let date_str = chrono::DateTime::from_timestamp_millis(session_start)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| session_start.to_string());
            let profile =
                build_tpo_profile(&bars, session_start, session_end, &date_str, self.tick_size);
            self.sessions.insert(session_start, profile);
            self.active_session_start = Some(session_start);
        }
    }

    /// Resolves profiles with optional session clusters.
    pub fn effective_profiles(&mut self, clusters: &[SessionCluster]) -> Vec<TpoProfile> {
        self.recompute_active_metrics();
        let raw: Vec<TpoProfile> = self.sessions.values().cloned().collect();
        if clusters.is_empty() {
            raw
        } else {
            apply_session_clusters(&raw, clusters)
        }
    }

    /// Clears all stored bars, sessions, and active session state.
    pub fn clear(&mut self) {
        self.sessions.clear();
        self.one_min_bars.clear();
        self.active_session_start = None;
        self.metrics_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::Price;

    #[test]
    fn test_bracket_mapping_boundaries() {
        let start = 1_700_000_000_000;
        assert_eq!(get_tpo_bracket(start, start), Some('A'));
        assert_eq!(get_tpo_bracket(start + 29 * 60_000, start), Some('A'));
        assert_eq!(get_tpo_bracket(start + 30 * 60_000, start), Some('B'));
        assert_eq!(get_tpo_bracket(start + 25 * 30 * 60_000, start), Some('Z'));
        assert_eq!(get_tpo_bracket(start + 26 * 30 * 60_000, start), Some('a'));
        assert_eq!(get_tpo_bracket(start + 51 * 30 * 60_000, start), Some('z'));
        assert_eq!(get_tpo_bracket(start + 52 * 30 * 60_000, start), None);
        assert_eq!(get_tpo_bracket(start - 1, start), None);
    }

    #[test]
    fn test_initial_balance_and_extensions() {
        let start = 1_000_000;
        let candles = vec![
            (start, 110.0, 90.0),               // Bracket A: High 110, Low 90
            (start + 30 * 60_000, 115.0, 95.0), // Bracket B: High 115, Low 95
        ];

        let profile = build_tpo_profile(&candles, start, start + 3_600_000, "2024-01-01", 1.0);
        let ib = profile.ib.expect("IB should be calculated");
        assert_eq!(ib.high, 115.0);
        assert_eq!(ib.low, 90.0);
        // Span = 25
        assert_eq!(ib.extension_high_1_5, 115.0 + 12.5);
        assert_eq!(ib.extension_high_2_0, 115.0 + 25.0);
        assert_eq!(ib.extension_low_1_5, 90.0 - 12.5);
        assert_eq!(ib.extension_low_2_0, 90.0 - 25.0);
    }

    #[test]
    fn test_poc_midpoint_tie_breaker() {
        let mut matrix = BTreeMap::new();
        matrix.insert(10, vec!['A']);
        matrix.insert(20, vec!['A', 'B', 'C']); // count = 3, dist to 30 = 10
        matrix.insert(30, vec!['A', 'B']); // midpoint = 30
        matrix.insert(40, vec!['A', 'B', 'C']); // count = 3, dist to 30 = 10
        matrix.insert(50, vec!['A']);

        let (poc, _, _, _, _) = recalculate_metrics(&matrix, 1.0);
        let p = poc.expect("POC present");
        // Equidistant tie: 20 vs 40 => lower tick bin 20 wins
        assert_eq!(p.tick_bin, 20);
        assert_eq!(p.count, 3);
    }

    #[test]
    fn test_poor_high_and_poor_low() {
        // Test poor high (max tick count >= 2) and single print low (min tick count == 1)
        let mut matrix = BTreeMap::new();
        matrix.insert(10, vec!['A']); // count 1 -> excess low (not poor)
        matrix.insert(11, vec!['A', 'B']);
        matrix.insert(12, vec!['A', 'B', 'C']);
        matrix.insert(13, vec!['B', 'C']); // max tick count 2 -> poor high

        let (_, _, single_prints, is_poor_high, is_poor_low) = recalculate_metrics(&matrix, 1.0);
        assert!(is_poor_high, "High should be poor (count >= 2)");
        assert!(!is_poor_low, "Low should not be poor (count == 1)");

        // Min tick 10 is a single print tail
        let tail_low = single_prints
            .iter()
            .find(|sp| sp.start_price == 10.0)
            .unwrap();
        assert!(tail_low.is_tail);

        // Test poor low
        let mut matrix2 = BTreeMap::new();
        matrix2.insert(10, vec!['A', 'B']); // min tick count 2 -> poor low
        matrix2.insert(11, vec!['A', 'B', 'C']);
        matrix2.insert(12, vec!['C']); // max tick count 1 -> excess high (not poor)

        let (_, _, _, poor_high2, poor_low2) = recalculate_metrics(&matrix2, 1.0);
        assert!(!poor_high2, "High should not be poor");
        assert!(poor_low2, "Low should be poor (count >= 2)");
    }

    #[test]
    fn test_merge_and_split_profiles() {
        let start1 = 1_000_000;
        let c1 = vec![(start1, 105.0, 95.0)];
        let p1 = build_tpo_profile(&c1, start1, start1 + 86_400_000, "Day1", 1.0);

        let start2 = start1 + 86_400_000;
        let c2 = vec![(start2, 110.0, 100.0)];
        let p2 = build_tpo_profile(&c2, start2, start2 + 86_400_000, "Day2", 1.0);

        let merged = merge_tpo_profiles(&[p1.clone(), p2.clone()]).expect("Merged profile");
        assert_eq!(merged.session_start, start1);
        assert_eq!(merged.session_end, start2 + 86_400_000);
        assert!(merged.matrix.contains_key(&95));
        assert!(merged.matrix.contains_key(&110));

        let split = split_tpo_profile(&merged, &[p1, p2]);
        assert_eq!(split.len(), 2);
        assert!(split[0].is_split);
        assert!(split[1].is_split);
    }

    #[test]
    fn test_calculate_tpo_tick_size() {
        // BTCUSDT (~60,000, 0.1 exchange tick) -> auto step should be 20.0
        let btc_step = calculate_tpo_tick_size(60_000.0, 0.1, 0);
        assert_eq!(btc_step, 20.0);

        // ETHUSDT (~2,500, 0.01 exchange tick) -> auto step should be 1.0
        let eth_step = calculate_tpo_tick_size(2_500.0, 0.01, 0);
        assert_eq!(eth_step, 1.0);

        // SOLUSDT (~150, 0.01 exchange tick) -> auto step should be 0.05
        let sol_step = calculate_tpo_tick_size(150.0, 0.01, 0);
        assert_eq!(sol_step, 0.05);

        // Manual multiplier 10x
        let manual_btc = calculate_tpo_tick_size(60_000.0, 0.1, 10);
        assert_eq!(manual_btc, 1.0);
    }

    #[test]
    fn test_tpo_anchor_bounds_and_conversions() {
        let ts_wednesday = 1_700_050_000_000i64; // Nov 15, 2023 12:06:40 UTC
        let (d_start, d_end) = TpoAnchor::Daily.period_bounds(ts_wednesday);
        assert_eq!(d_end - d_start, 86_400_000);
        assert!(ts_wednesday >= d_start && ts_wednesday < d_end);

        let (w_start, w_end) = TpoAnchor::Weekly.period_bounds(ts_wednesday);
        assert_eq!(w_end - w_start, 7 * 86_400_000);
        assert!(ts_wednesday >= w_start && ts_wednesday < w_end);

        let (m_start, m_end) = TpoAnchor::Monthly.period_bounds(ts_wednesday);
        assert!(ts_wednesday >= m_start && ts_wednesday < m_end);

        assert_eq!(SessionPeriod::from(TpoAnchor::Daily), SessionPeriod::Daily);
        assert_eq!(
            SessionPeriod::from(TpoAnchor::Weekly),
            SessionPeriod::Weekly
        );
        assert_eq!(
            SessionPeriod::from(TpoAnchor::Monthly),
            SessionPeriod::Monthly
        );

        assert_eq!(
            TpoAnchor::try_from(SessionPeriod::Daily),
            Ok(TpoAnchor::Daily)
        );
        assert_eq!(
            TpoAnchor::try_from(SessionPeriod::Weekly),
            Ok(TpoAnchor::Weekly)
        );
        assert_eq!(
            TpoAnchor::try_from(SessionPeriod::Monthly),
            Ok(TpoAnchor::Monthly)
        );
        assert!(TpoAnchor::try_from(SessionPeriod::FourHours).is_err());
    }

    #[test]
    fn test_one_min_bar_aggregation() {
        let ts_base = 1_700_000_000_000i64; // Exactly aligned to 60s
        let mut bar = OneMinBar::new(ts_base + 5_000, 100.0, 1.5);
        assert_eq!(bar.open_time_ms, ts_base.div_euclid(60_000) * 60_000);
        assert_eq!(bar.open, 100.0);
        assert_eq!(bar.high, 100.0);
        assert_eq!(bar.low, 100.0);
        assert_eq!(bar.close, 100.0);
        assert_eq!(bar.volume, 1.5);
        assert_eq!(bar.trade_count, 1);

        bar.update(105.0, 2.0);
        assert_eq!(bar.high, 105.0);
        assert_eq!(bar.low, 100.0);
        assert_eq!(bar.close, 105.0);
        assert_eq!(bar.volume, 3.5);
        assert_eq!(bar.trade_count, 2);

        bar.update(98.0, 0.5);
        assert_eq!(bar.high, 105.0);
        assert_eq!(bar.low, 98.0);
        assert_eq!(bar.close, 98.0);
        assert_eq!(bar.volume, 4.0);
        assert_eq!(bar.trade_count, 3);
    }

    #[test]
    fn test_trade_tpo_cache_incremental_matches_rebuild() {
        let start = 1_700_000_000_000i64; // Assume session start aligned
        let (s_start, _) = TpoAnchor::Daily.period_bounds(start);
        let mut cache = TradeTpoCache::new(TpoAnchor::Daily, 1.0);

        let trades = vec![
            Trade {
                time: (s_start + 5 * 60_000) as u64, // Bracket A
                is_sell: false,
                price: Price::from_f64(100.0),
                qty: 1.0,
            },
            Trade {
                time: (s_start + 10 * 60_000) as u64, // Bracket A
                is_sell: false,
                price: Price::from_f64(102.0),
                qty: 2.0,
            },
            Trade {
                time: (s_start + 15 * 60_000) as u64, // Bracket A
                is_sell: true,
                price: Price::from_f64(98.0),
                qty: 1.5,
            },
            Trade {
                time: (s_start + 35 * 60_000) as u64, // Bracket B
                is_sell: false,
                price: Price::from_f64(100.0),
                qty: 2.5,
            },
            Trade {
                time: (s_start + 35 * 60_000 + 10_000) as u64, // Bracket B (sweeps to 103)
                is_sell: false,
                price: Price::from_f64(103.0),
                qty: 1.0,
            },
        ];

        cache.on_trades(&trades, 1.0);
        let active = cache
            .active_profile()
            .expect("Active profile should exist")
            .clone();
        assert!(active.matrix.contains_key(&100));
        assert!(active.matrix.contains_key(&102));
        assert!(active.matrix.contains_key(&98));
        assert!(active.matrix.contains_key(&103));
        assert_eq!(active.matrix[&100], vec!['A', 'B']); // Both brackets touched 100

        // IB should span 98.0 to 103.0 (brackets A and B)
        let ib = active.ib.expect("IB should be present");
        assert_eq!(ib.high, 103.0);
        assert_eq!(ib.low, 98.0);

        // Rebuild from 1-min bars must match incremental matrix exactly
        let pre_rebuild_matrix = active.matrix.clone();
        cache.rebuild();
        let post_rebuild = cache.active_profile().expect("Profile after rebuild");
        assert_eq!(post_rebuild.matrix, pre_rebuild_matrix);
        assert_eq!(post_rebuild.poc, active.poc);
        assert_eq!(post_rebuild.value_area, active.value_area);
    }

    #[test]
    fn test_trade_tpo_cache_low_liquidity_gaps() {
        let (s_start, _) = TpoAnchor::Daily.period_bounds(1_700_000_000_000);
        let mut cache = TradeTpoCache::new(TpoAnchor::Daily, 1.0);

        let t1 = Trade {
            time: (s_start + 10 * 60_000) as u64, // Bracket A (0..30m)
            is_sell: false,
            price: Price::from_f64(50.0),
            qty: 1.0,
        };
        // 2-hour gap: B (30..60), C (60..90), D (90..120), E (120..150) are empty
        let t2 = Trade {
            time: (s_start + 155 * 60_000) as u64, // Bracket F (150..180m)
            is_sell: true,
            price: Price::from_f64(55.0),
            qty: 2.0,
        };

        cache.on_trades(&[t1, t2], 1.0);
        let prof = cache.active_profile().expect("Profile exists");

        // 50.0 has 'A'
        assert_eq!(prof.matrix[&50], vec!['A']);
        // 55.0 has 'F'
        assert_eq!(prof.matrix[&55], vec!['F']);
        // Brackets B, C, D, E must not appear anywhere
        for row in prof.matrix.values() {
            assert!(!row.contains(&'B'));
            assert!(!row.contains(&'C'));
            assert!(!row.contains(&'D'));
            assert!(!row.contains(&'E'));
        }
    }

    #[test]
    fn test_trade_tpo_cache_anchor_switching() {
        // Wednesday, Thursday, Friday within the same week
        let t_wed = 1_700_050_000_000u64; // Wednesday
        let t_thu = t_wed + 86_400_000;
        let t_fri = t_thu + 86_400_000;

        let mut cache = TradeTpoCache::new(TpoAnchor::Daily, 1.0);
        cache.on_trade(&Trade {
            time: t_wed,
            is_sell: false,
            price: Price::from_f64(100.0),
            qty: 1.0,
        });
        cache.on_trade(&Trade {
            time: t_thu,
            is_sell: false,
            price: Price::from_f64(110.0),
            qty: 1.0,
        });
        cache.on_trade(&Trade {
            time: t_fri,
            is_sell: false,
            price: Price::from_f64(120.0),
            qty: 1.0,
        });

        // Under Daily anchor, 3 distinct session profiles exist
        assert_eq!(cache.sessions().len(), 3);

        // Switch to Weekly anchor: all 3 days merge into 1 unified weekly profile
        cache.set_anchor(TpoAnchor::Weekly);
        assert_eq!(cache.sessions().len(), 1);
        let weekly = cache.active_profile().expect("Weekly profile");
        assert!(weekly.matrix.contains_key(&100));
        assert!(weekly.matrix.contains_key(&110));
        assert!(weekly.matrix.contains_key(&120));
    }

    #[test]
    fn test_trade_tpo_cache_tick_size_scaling() {
        let (s_start, _) = TpoAnchor::Daily.period_bounds(1_700_000_000_000);
        let mut cache = TradeTpoCache::new(TpoAnchor::Daily, 1.0);

        cache.on_trade(&Trade {
            time: (s_start + 60_000) as u64,
            is_sell: false,
            price: Price::from_f64(101.0),
            qty: 1.0,
        });
        cache.on_trade(&Trade {
            time: (s_start + 120_000) as u64,
            is_sell: false,
            price: Price::from_f64(104.0),
            qty: 1.0,
        });

        assert_eq!(cache.tick_size(), 1.0);
        assert!(cache.active_profile().unwrap().matrix.contains_key(&101));
        assert!(cache.active_profile().unwrap().matrix.contains_key(&104));

        // Re-scale tick size to 5.0
        cache.set_tick_size(5.0);
        assert_eq!(cache.tick_size(), 5.0);
        let prof = cache.active_profile().unwrap();
        // 101 / 5 = 20.2 => bin 20; 104 / 5 = 20.8 => bin 21
        assert!(prof.matrix.contains_key(&20));
        assert!(prof.matrix.contains_key(&21));
    }

    #[test]
    fn test_trade_tpo_cache_sub_second_benchmark() {
        let (s_start, _) = TpoAnchor::Daily.period_bounds(1_700_000_000_000);
        let mut cache = TradeTpoCache::new(TpoAnchor::Daily, 0.5);

        // Generate 25,000 synthetic trades across a 12-hour intraday window
        let mut trades = Vec::with_capacity(25_000);
        let mut current_price = 30_000.0f64;
        for i in 0..25_000 {
            let offset_ms = (i as i64 * 12 * 3600 * 1000) / 25_000;
            let step = if i % 2 == 0 { 0.5 } else { -0.5 };
            current_price += step;
            trades.push(Trade {
                time: (s_start + offset_ms) as u64,
                is_sell: i % 2 == 1,
                price: Price::from_f64(current_price),
                qty: 0.1,
            });
        }

        let start_time = std::time::Instant::now();
        cache.on_trades(&trades, 0.5);
        let elapsed = start_time.elapsed();

        assert!(
            elapsed.as_millis() < crate::perf_budget_ms(500),
            "TradeTpoCache must ingest 25,000 trades in sub-second time (took {}ms)",
            elapsed.as_millis()
        );

        let active = cache.active_profile().expect("Active profile must exist");
        assert!(active.poc.is_some(), "POC must be calculated");
        assert!(active.value_area.is_some(), "Value Area must be calculated");
        assert!(active.ib.is_some(), "IB must be calculated");
    }
}
