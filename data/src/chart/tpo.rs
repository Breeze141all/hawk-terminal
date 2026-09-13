use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::kline::KlineDataPoint;
use exchange::Kline;

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
        self.high.to_f32() as f64
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low.to_f32() as f64
    }
}

impl TpoCandle for &Kline {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.high.to_f32() as f64
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.low.to_f32() as f64
    }
}

impl TpoCandle for KlineDataPoint {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.kline.high.to_f32() as f64
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.kline.low.to_f32() as f64
    }
}

impl TpoCandle for &KlineDataPoint {
    #[inline]
    fn open_time_ms(&self) -> i64 {
        self.kline.time as i64
    }

    #[inline]
    fn high_price(&self) -> f64 {
        self.kline.high.to_f32() as f64
    }

    #[inline]
    fn low_price(&self) -> f64 {
        self.kline.low.to_f32() as f64
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

/// Supported aggregation periods for grouping historical candles into TPO profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SessionPeriod {
    #[default]
    Daily,
    Weekly,
    Monthly,
    CustomDays(u32),
}

impl SessionPeriod {
    pub const ALL: [SessionPeriod; 7] = [
        SessionPeriod::Daily,
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
                        last_bracket_per_tick.insert(t, bracket_idx);
                    }
                }
            } else {
                for t in low_tick..=high_tick {
                    let row = matrix.entry(t).or_default();
                    if !row.contains(&bracket) {
                        row.push(bracket);
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
    let mut ib_candidates = Vec::new();

    for p in &sorted {
        if let Some(ref ib) = p.ib {
            ib_candidates.push(ib.clone());
        }
        for (&t, row) in &p.matrix {
            let entry = composite_matrix.entry(t).or_default();
            for &ch in row {
                if !entry.contains(&ch) {
                    entry.push(ch);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
