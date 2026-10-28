#[cfg(feature = "volume-search")]
use crate::screen::dashboard::Dashboard;
use crate::style::{self, Icon, icon_text};
use chrono::{Datelike, NaiveDate};
use data::chart::kline::KlineDataPoint;
use data::chart::volume_search::{
    SortOrder, VolumeSearchQuery, VolumeSearchRow, VolumeSearchSortCol, VolumeSearchSummary,
    search_volume_candles, sort_rows,
};
use data::util::format_with_commas;
use exchange::adapter::{Exchange, ExchangeInclusive, MarketKind};
use exchange::util::PriceStep;
use exchange::{TickMultiplier, TickerInfo, Timeframe};
use iced::padding;
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, rule, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Theme};
use std::collections::BTreeMap;

pub enum Action {
    Close,
    Task(iced::Task<Message>),
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Close => write!(f, "Action::Close"),
            Self::Task(_) => write!(f, "Action::Task(...)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VolumeSearchPreset {
    Last24h,
    #[default]
    Last3d,
    Last7d,
    Custom,
    AllLoaded,
}

impl std::fmt::Display for VolumeSearchPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Last24h => write!(f, "24 години"),
            Self::Last3d => write!(f, "3 дні"),
            Self::Last7d => write!(f, "7 днів"),
            Self::Custom => write!(f, "Власний діапазон"),
            Self::AllLoaded => write!(f, "Вся історія"),
        }
    }
}

impl VolumeSearchPreset {
    pub const ALL: [VolumeSearchPreset; 5] = [
        VolumeSearchPreset::Last24h,
        VolumeSearchPreset::Last3d,
        VolumeSearchPreset::Last7d,
        VolumeSearchPreset::Custom,
        VolumeSearchPreset::AllLoaded,
    ];
}

/// Identifies which date input field is currently linked to the calendar popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarTarget {
    Start,
    End,
}

/// Ranked search score for ticker symbol matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickerSearchRank {
    pub bucket: u8, // 0 = exact, 1 = prefix, 2 = suffix, 3 = substring
    pub pos: u16,   // byte offset of match
    pub len: u16,   // candidate string length
}

/// Newtype wrapper formatting TickerInfo with exchange prefix in pick_list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickerDisplayWrapper(pub TickerInfo);

impl std::fmt::Display for TickerDisplayWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (sym, _) = self.0.ticker.display_symbol_and_type();
        let ex = match ExchangeInclusive::of(self.0.ticker.exchange) {
            ExchangeInclusive::Binance => "Binance",
            ExchangeInclusive::Bybit => "Bybit",
            ExchangeInclusive::Hyperliquid => "Hyperliquid",
            ExchangeInclusive::Okex => "OKX",
            ExchangeInclusive::Mexc => "MEXC",
            ExchangeInclusive::Coinbase => "Coinbase",
        };
        write!(f, "{ex}: {sym}")
    }
}

#[derive(Debug, Clone)]
pub struct VolumeSearchModal {
    pub selected_ticker: Option<TickerInfo>,
    pub available_tickers: Vec<TickerInfo>,
    pub ticker_search_query: String,
    pub selected_exchange_filter: Option<ExchangeInclusive>,
    pub filtered_tickers: Vec<TickerInfo>,
    pub filtered_display_tickers: Vec<TickerDisplayWrapper>,
    pub is_ticker_selector_open: bool,
    pub selected_timeframe: Timeframe,
    pub selected_multiplier: TickMultiplier,
    pub is_custom_multiplier: bool,
    pub custom_multiplier_input: String,
    pub min_volume_input: String,
    pub preset: VolumeSearchPreset,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub start_date_input: String,
    pub end_date_input: String,
    pub is_start_date_valid: bool,
    pub is_end_date_valid: bool,
    pub active_calendar: Option<CalendarTarget>,
    pub cal_view_year: i32,
    pub cal_view_month: u32,
    pub time_filter_start: String,
    pub time_filter_end: String,
    pub rows: Vec<VolumeSearchRow>,
    pub summary: VolumeSearchSummary,
    pub sort_col: VolumeSearchSortCol,
    pub sort_order: SortOrder,
    pub has_searched: bool,
    pub is_downloading: bool,
    pub download_progress: Option<String>,
    pub download_missing_dates: Vec<NaiveDate>,
    pub current_download_index: usize,
    pub cached_search_datapoints: BTreeMap<u64, KlineDataPoint>,
    pub active_search_query: Option<VolumeSearchQuery>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TickerSelected(TickerInfo),
    TickerSearchChanged(String),
    TickerSearchSubmitted,
    ExchangeFilterSelected(Option<ExchangeInclusive>),
    TimeframeSelected(Timeframe),
    PresetSelected(VolumeSearchPreset),
    StartDateInputChanged(String),
    EndDateInputChanged(String),
    ToggleCalendar(CalendarTarget),
    CloseCalendar,
    CalPrevMonth,
    CalNextMonth,
    CalPrevYear,
    CalNextYear,
    CalJumpToToday,
    CalSelectDate(NaiveDate),
    TimeFilterStartChanged(String),
    TimeFilterEndChanged(String),
    MultiplierSelected(TickMultiplier),
    ToggleCustomMultiplier(bool),
    CustomMultiplierChanged(String),
    MinVolumeChanged(String),
    ApplySearch,
    SortColumnClicked(VolumeSearchSortCol),
    DayDownloadCompleted(Result<(NaiveDate, Vec<(u64, KlineDataPoint)>), String>),
    CancelSearch,
    Close,
}

impl VolumeSearchModal {
    pub fn new(current_ticker: Option<TickerInfo>, available_tickers: Vec<TickerInfo>) -> Self {
        let selected_ticker = current_ticker.or_else(|| available_tickers.first().copied());
        let today = chrono::Utc::now().date_naive();
        let default_start = today
            .checked_sub_signed(chrono::Duration::days(2))
            .unwrap_or(today);
        let default_end = today;
        let start_date_input = default_start.format("%Y-%m-%d").to_string();
        let end_date_input = default_end.format("%Y-%m-%d").to_string();

        let mut modal = Self {
            selected_ticker,
            available_tickers,
            ticker_search_query: String::new(),
            selected_exchange_filter: None,
            filtered_tickers: Vec::new(),
            filtered_display_tickers: Vec::new(),
            is_ticker_selector_open: false,
            selected_timeframe: Timeframe::H1,
            selected_multiplier: TickMultiplier(200),
            is_custom_multiplier: false,
            custom_multiplier_input: "200".to_string(),
            min_volume_input: "0".to_string(),
            preset: VolumeSearchPreset::Last3d,
            start_date: default_start,
            end_date: default_end,
            start_date_input,
            end_date_input,
            is_start_date_valid: true,
            is_end_date_valid: true,
            active_calendar: None,
            cal_view_year: today.year(),
            cal_view_month: today.month(),
            time_filter_start: "00:00".to_string(),
            time_filter_end: "23:59".to_string(),
            rows: Vec::new(),
            summary: VolumeSearchSummary::default(),
            sort_col: VolumeSearchSortCol::MaxVolume,
            sort_order: SortOrder::Descending,
            has_searched: false,
            is_downloading: false,
            download_progress: None,
            download_missing_dates: Vec::new(),
            current_download_index: 0,
            cached_search_datapoints: BTreeMap::new(),
            active_search_query: None,
        };
        modal.rebuild_filtered_tickers();
        modal
    }

    /// Validates that both start and end inputs are valid YYYY-MM-DD dates and start <= end.
    pub fn is_date_range_valid(&self) -> bool {
        self.is_start_date_valid && self.is_end_date_valid && self.start_date <= self.end_date
    }

    /// Computes the contiguous sequence of calendar dates spanned by [start_date, end_date].
    pub fn target_dates(&self) -> Vec<NaiveDate> {
        if !self.is_date_range_valid() {
            return Vec::new();
        }
        let mut dates = Vec::new();
        let mut cur = self.start_date;
        while cur <= self.end_date {
            dates.push(cur);
            match cur.succ_opt() {
                Some(next) => cur = next,
                None => break,
            }
        }
        dates
    }

    /// Supplies the global catalog from sidebar.tickers_table.tickers_info.
    pub fn update_available_tickers(&mut self, tickers: Vec<TickerInfo>) {
        let same_len =
            self.available_tickers.len() == tickers.len() && !self.available_tickers.is_empty();
        if !same_len {
            self.available_tickers = tickers;
            self.rebuild_filtered_tickers();
        }
        if self.selected_ticker.is_none() {
            self.selected_ticker = self.filtered_tickers.first().copied();
        }
    }

    /// Inherits the active chart pane ticker if no ticker has been selected yet.
    #[cfg(test)]
    pub fn inherit_active_pane_ticker(&mut self, ticker: Option<TickerInfo>) {
        if self.selected_ticker.is_none() {
            self.selected_ticker = ticker;
        }
    }

    pub fn current_query(&self) -> &str {
        &self.ticker_search_query
    }

    /// Pure filtering and ranking over available tickers.
    pub fn compute_filtered_tickers(&self, raw_query: &str) -> Vec<TickerInfo> {
        let query_upper = raw_query.trim().to_uppercase();

        let mut matched: Vec<(TickerInfo, TickerSearchRank)> = self
            .available_tickers
            .iter()
            .copied()
            .filter(|ti| {
                if let Some(ex_filter) = self.selected_exchange_filter {
                    ExchangeInclusive::of(ti.exchange()) == ex_filter
                } else {
                    true
                }
            })
            .filter_map(|ti| calc_ticker_rank(&ti, &query_upper).map(|rank| (ti, rank)))
            .collect();

        matched.sort_by(|(a, rank_a), (b, rank_b)| {
            (rank_a.bucket, rank_a.pos)
                .cmp(&(rank_b.bucket, rank_b.pos))
                .then_with(|| {
                    let kind_rank = |k: MarketKind| match k {
                        MarketKind::LinearPerps => 0,
                        MarketKind::Spot => 1,
                        MarketKind::InversePerps => 2,
                    };
                    kind_rank(a.market_type()).cmp(&kind_rank(b.market_type()))
                })
                .then_with(|| rank_a.len.cmp(&rank_b.len))
                .then_with(|| a.ticker.as_str().cmp(b.ticker.as_str()))
        });

        if !query_upper.is_empty() && matched.iter().any(|(_, r)| r.bucket == 0) {
            matched.retain(|(_, r)| r.bucket <= 1);
        }

        matched.into_iter().map(|(ti, _)| ti).collect()
    }

    #[cfg(test)]
    pub fn filtered_and_ranked_candidates(&self) -> Vec<TickerInfo> {
        let query = self.current_query();
        self.compute_filtered_tickers(query)
    }

    /// Re-evaluates exchange filters and ranked text query across available tickers.
    pub fn rebuild_filtered_tickers(&mut self) {
        let query = self.current_query().to_string();
        self.filtered_tickers = self.compute_filtered_tickers(&query);
        self.filtered_display_tickers = self
            .filtered_tickers
            .iter()
            .copied()
            .map(TickerDisplayWrapper)
            .collect();
    }

    pub fn update(&mut self, message: Message, dashboard: Option<&Dashboard>) -> Option<Action> {
        match message {
            Message::TickerSelected(t) => {
                self.selected_ticker = Some(t);
                self.is_ticker_selector_open = false;
            }
            Message::TickerSearchChanged(q) => {
                self.ticker_search_query = q;
                self.rebuild_filtered_tickers();
            }
            Message::TickerSearchSubmitted => {
                if let Some(first) = self.filtered_tickers.first().copied() {
                    self.selected_ticker = Some(first);
                    self.is_ticker_selector_open = false;
                }
            }
            Message::ExchangeFilterSelected(f) => {
                self.selected_exchange_filter = f;
                self.rebuild_filtered_tickers();
            }
            Message::TimeframeSelected(tf) => {
                self.selected_timeframe = tf;
            }
            Message::PresetSelected(p) => {
                self.preset = p;
                let today = chrono::Utc::now().date_naive();
                match p {
                    VolumeSearchPreset::Last24h => {
                        let start = today.pred_opt().unwrap_or(today);
                        self.start_date = start;
                        self.end_date = today;
                        self.start_date_input = start.format("%Y-%m-%d").to_string();
                        self.end_date_input = today.format("%Y-%m-%d").to_string();
                        self.is_start_date_valid = true;
                        self.is_end_date_valid = true;
                        self.active_calendar = None;
                    }
                    VolumeSearchPreset::Last3d => {
                        let start = today
                            .checked_sub_signed(chrono::Duration::days(2))
                            .unwrap_or(today);
                        self.start_date = start;
                        self.end_date = today;
                        self.start_date_input = start.format("%Y-%m-%d").to_string();
                        self.end_date_input = today.format("%Y-%m-%d").to_string();
                        self.is_start_date_valid = true;
                        self.is_end_date_valid = true;
                        self.active_calendar = None;
                    }
                    VolumeSearchPreset::Last7d => {
                        let start = today
                            .checked_sub_signed(chrono::Duration::days(6))
                            .unwrap_or(today);
                        self.start_date = start;
                        self.end_date = today;
                        self.start_date_input = start.format("%Y-%m-%d").to_string();
                        self.end_date_input = today.format("%Y-%m-%d").to_string();
                        self.is_start_date_valid = true;
                        self.is_end_date_valid = true;
                        self.active_calendar = None;
                    }
                    VolumeSearchPreset::Custom => {
                        self.active_calendar = None;
                    }
                    VolumeSearchPreset::AllLoaded => {
                        self.active_calendar = None;
                    }
                }
            }
            Message::StartDateInputChanged(s) => {
                self.start_date_input = s.clone();
                self.preset = VolumeSearchPreset::Custom;
                if let Some(parsed) = parse_date_ymd(&s) {
                    self.start_date = parsed;
                    self.is_start_date_valid = true;
                    if self.is_end_date_valid && self.start_date > self.end_date {
                        self.end_date = self.start_date;
                        self.end_date_input = self.end_date.format("%Y-%m-%d").to_string();
                    }
                } else {
                    self.is_start_date_valid = false;
                }
            }
            Message::EndDateInputChanged(s) => {
                self.end_date_input = s.clone();
                self.preset = VolumeSearchPreset::Custom;
                if let Some(parsed) = parse_date_ymd(&s) {
                    self.end_date = parsed;
                    self.is_end_date_valid = true;
                    if self.is_start_date_valid && self.end_date < self.start_date {
                        self.start_date = self.end_date;
                        self.start_date_input = self.start_date.format("%Y-%m-%d").to_string();
                    }
                } else {
                    self.is_end_date_valid = false;
                }
            }
            Message::ToggleCalendar(target) => {
                if self.active_calendar == Some(target) {
                    self.active_calendar = None;
                } else {
                    self.active_calendar = Some(target);
                    let target_date = match target {
                        CalendarTarget::Start => self.start_date,
                        CalendarTarget::End => self.end_date,
                    };
                    self.cal_view_year = target_date.year();
                    self.cal_view_month = target_date.month();
                }
            }
            Message::CloseCalendar => {
                self.active_calendar = None;
            }
            Message::CalPrevMonth => {
                if self.cal_view_month == 1 {
                    if self.cal_view_year > 2000 {
                        self.cal_view_month = 12;
                        self.cal_view_year -= 1;
                    }
                } else {
                    self.cal_view_month -= 1;
                }
            }
            Message::CalNextMonth => {
                if self.cal_view_month == 12 {
                    if self.cal_view_year < 2099 {
                        self.cal_view_month = 1;
                        self.cal_view_year += 1;
                    }
                } else {
                    self.cal_view_month += 1;
                }
            }
            Message::CalPrevYear => {
                self.cal_view_year = self.cal_view_year.saturating_sub(1).max(2000);
            }
            Message::CalNextYear => {
                self.cal_view_year = self.cal_view_year.saturating_add(1).min(2099);
            }
            Message::CalJumpToToday => {
                let today = chrono::Utc::now().date_naive();
                self.cal_view_year = today.year();
                self.cal_view_month = today.month();
            }
            Message::CalSelectDate(date) => {
                if let Some(target) = self.active_calendar {
                    match target {
                        CalendarTarget::Start => {
                            self.start_date = date;
                            self.start_date_input = date.format("%Y-%m-%d").to_string();
                            self.is_start_date_valid = true;
                            if self.start_date > self.end_date {
                                self.end_date = self.start_date;
                                self.end_date_input = self.start_date_input.clone();
                                self.is_end_date_valid = true;
                            }
                        }
                        CalendarTarget::End => {
                            self.end_date = date;
                            self.end_date_input = date.format("%Y-%m-%d").to_string();
                            self.is_end_date_valid = true;
                            if self.end_date < self.start_date {
                                self.start_date = self.end_date;
                                self.start_date_input = self.end_date_input.clone();
                                self.is_start_date_valid = true;
                            }
                        }
                    }
                    self.preset = VolumeSearchPreset::Custom;
                    self.active_calendar = None;
                }
            }
            Message::TimeFilterStartChanged(s) => {
                self.time_filter_start = s;
            }
            Message::TimeFilterEndChanged(s) => {
                self.time_filter_end = s;
            }
            Message::MultiplierSelected(tm) => {
                self.selected_multiplier = tm;
                self.is_custom_multiplier = false;
                self.custom_multiplier_input = tm.0.to_string();
            }
            Message::ToggleCustomMultiplier(custom) => {
                self.is_custom_multiplier = custom;
            }
            Message::CustomMultiplierChanged(s) => {
                let numeric: String = s.chars().filter(char::is_ascii_digit).take(5).collect();
                if let Ok(val) = numeric.parse::<u16>()
                    && (1..=2000).contains(&val)
                {
                    self.selected_multiplier = TickMultiplier(val);
                }
                self.custom_multiplier_input = numeric;
            }
            Message::MinVolumeChanged(s) => {
                self.min_volume_input = s;
            }
            Message::SortColumnClicked(col) => {
                if self.sort_col == col {
                    self.sort_order = match self.sort_order {
                        SortOrder::Ascending => SortOrder::Descending,
                        SortOrder::Descending => SortOrder::Ascending,
                    };
                } else {
                    self.sort_col = col;
                    self.sort_order = SortOrder::Descending;
                }
                sort_rows(&mut self.rows, self.sort_col, self.sort_order);
            }
            Message::ApplySearch => {
                if !self.is_start_date_valid {
                    self.start_date_input = self.start_date.format("%Y-%m-%d").to_string();
                    self.is_start_date_valid = true;
                }
                if !self.is_end_date_valid {
                    self.end_date_input = self.end_date.format("%Y-%m-%d").to_string();
                    self.is_end_date_valid = true;
                }
                if self.start_date > self.end_date {
                    self.end_date = self.start_date;
                    self.end_date_input = self.end_date.format("%Y-%m-%d").to_string();
                }
                return self.perform_search(dashboard);
            }
            Message::DayDownloadCompleted(res) => {
                if !self.is_downloading {
                    return None;
                }
                match res {
                    Ok((day, dps)) => {
                        log::info!(
                            "Volume search: downloaded and cached {} datapoints for {}",
                            dps.len(),
                            day
                        );
                        for (t, dp) in dps {
                            self.cached_search_datapoints.insert(t, dp);
                        }
                    }
                    Err(err) => {
                        log::warn!("Volume search: failed to download day: {}", err);
                    }
                }
                self.current_download_index += 1;
                let total = self.download_missing_dates.len();
                if self.current_download_index < total {
                    let next_day = self.download_missing_dates[self.current_download_index];
                    self.download_progress = Some(format!(
                        "Завантаження з біржі: день {} з {} ({})...",
                        self.current_download_index + 1,
                        total,
                        next_day.format("%Y-%m-%d")
                    ));
                    let Some(ticker_info) = self.selected_ticker else {
                        self.is_downloading = false;
                        self.download_progress = None;
                        return None;
                    };
                    let timeframe = self.selected_timeframe;
                    let multiplier = self.selected_multiplier;
                    let task = iced::Task::perform(
                        download_and_cache_day(ticker_info, next_day, timeframe, multiplier),
                        Message::DayDownloadCompleted,
                    );
                    return Some(Action::Task(task));
                } else {
                    self.is_downloading = false;
                    self.download_progress = None;
                    self.has_searched = true;
                    if let Some(query) = &self.active_search_query {
                        let (mut rows, summary) =
                            search_volume_candles(&self.cached_search_datapoints, query);
                        sort_rows(&mut rows, self.sort_col, self.sort_order);
                        self.rows = rows;
                        self.summary = summary;
                    }
                    return None;
                }
            }
            Message::CancelSearch => {
                self.is_downloading = false;
                self.download_progress = None;
                if !self.cached_search_datapoints.is_empty()
                    && let Some(query) = &self.active_search_query
                {
                    let (mut rows, summary) =
                        search_volume_candles(&self.cached_search_datapoints, query);
                    sort_rows(&mut rows, self.sort_col, self.sort_order);
                    self.rows = rows;
                    self.summary = summary;
                } else {
                    self.rows = Vec::new();
                    self.summary = VolumeSearchSummary::default();
                }
                self.has_searched = true;
                return None;
            }
            Message::Close => {
                self.is_downloading = false;
                self.download_progress = None;
                return Some(Action::Close);
            }
        }
        None
    }

    pub fn perform_search(&mut self, dashboard: Option<&Dashboard>) -> Option<Action> {
        let ticker_info = self.selected_ticker?;

        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let (start_time, end_time) = match self.preset {
            VolumeSearchPreset::Last24h => (now_ms.saturating_sub(86_400_000), now_ms),
            VolumeSearchPreset::Last3d => (now_ms.saturating_sub(3 * 86_400_000), now_ms),
            VolumeSearchPreset::Last7d => (now_ms.saturating_sub(7 * 86_400_000), now_ms),
            VolumeSearchPreset::AllLoaded => (0, u64::MAX),
            VolumeSearchPreset::Custom => {
                let (s_ms, e_ms, _, _) = calculate_custom_query_range(
                    self.start_date,
                    self.end_date,
                    &self.time_filter_start,
                    &self.time_filter_end,
                    now_ms,
                );
                (s_ms, e_ms)
            }
        };

        let min_vol = self
            .min_volume_input
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
            .max(0.0);
        let base_step = PriceStep::from_f32_lossy(ticker_info.min_ticksize.into());

        let start_min = parse_minute_of_day(&self.time_filter_start).unwrap_or(0);
        let end_min = parse_minute_of_day(&self.time_filter_end).unwrap_or(1439);

        let query = VolumeSearchQuery {
            start_time,
            end_time,
            min_volume: min_vol,
            tick_multiplier: self.selected_multiplier,
            base_tick_size: base_step,
            start_minute_of_day: start_min,
            end_minute_of_day: end_min,
        };

        self.cached_search_datapoints.clear();

        // 1. Preset AllLoaded handling
        if self.preset == VolumeSearchPreset::AllLoaded {
            if let Some(dashboard) = dashboard {
                for (_, pane_state) in dashboard.panes.iter() {
                    if let crate::screen::dashboard::pane::Content::Kline { chart: Some(c), .. } =
                        &pane_state.content
                        && c.ticker_info.ticker == ticker_info.ticker
                        && let data::chart::PlotData::TimeBased(ts) = c.data_source()
                        && ts.interval == self.selected_timeframe
                    {
                        let (mut rows, summary) = search_volume_candles(&ts.datapoints, &query);
                        sort_rows(&mut rows, self.sort_col, self.sort_order);
                        self.rows = rows;
                        self.summary = summary;
                        self.has_searched = true;
                        self.is_downloading = false;
                        self.download_progress = None;
                        return None;
                    }
                }
            }

            let all_dps = scan_all_local_cached_days(
                &ticker_info,
                self.selected_timeframe,
                self.selected_multiplier,
            );
            let (mut rows, summary) = search_volume_candles(&all_dps, &query);
            sort_rows(&mut rows, self.sort_col, self.sort_order);
            self.rows = rows;
            self.summary = summary;
            self.has_searched = true;
            self.is_downloading = false;
            self.download_progress = None;
            return None;
        }

        // 2. Presets / Custom: Target dates scanning across tiers
        let target_days = self.target_dates();
        let mut missing_dates = Vec::new();

        for d in target_days {
            let mut day_found = false;

            // Tier 0: Check open chart pane
            if let Some(dashboard) = dashboard {
                for (_, pane_state) in dashboard.panes.iter() {
                    if let crate::screen::dashboard::pane::Content::Kline { chart: Some(c), .. } =
                        &pane_state.content
                        && c.ticker_info.ticker == ticker_info.ticker
                        && let data::chart::PlotData::TimeBased(ts) = c.data_source()
                        && ts.interval == self.selected_timeframe
                        && let Some(day_start_dt) = d.and_hms_opt(0, 0, 0)
                    {
                        let day_start = day_start_dt.and_utc().timestamp_millis() as u64;
                        let day_end = day_start.saturating_add(86_400_000);
                        let mut count = 0;
                        for (t, dp) in ts.datapoints.range(day_start..day_end) {
                            self.cached_search_datapoints.insert(*t, dp.clone());
                            count += 1;
                        }
                        if count > 0 {
                            day_found = true;
                            break;
                        }
                    }
                }
            }

            // Tier 1 & 2: Local Footprint (.fst) and Raw Trades (.bin) caches
            if !day_found
                && let Some(dps) = scan_local_cache_for_day(
                    &ticker_info,
                    d,
                    self.selected_timeframe,
                    self.selected_multiplier,
                )
            {
                for (t, dp) in dps {
                    self.cached_search_datapoints.insert(t, dp);
                }
                day_found = true;
            }

            if !day_found {
                missing_dates.push(d);
            }
        }

        if missing_dates.is_empty() {
            let (mut rows, summary) = search_volume_candles(&self.cached_search_datapoints, &query);
            sort_rows(&mut rows, self.sort_col, self.sort_order);
            self.rows = rows;
            self.summary = summary;
            self.has_searched = true;
            self.is_downloading = false;
            self.download_progress = None;
            return None;
        }

        // Tier 3: Remote download required for missing dates
        self.is_downloading = true;
        self.download_missing_dates = missing_dates;
        self.current_download_index = 0;
        self.active_search_query = Some(query);

        let total = self.download_missing_dates.len();
        let first_day = self.download_missing_dates[0];
        self.download_progress = Some(format!(
            "Завантаження з біржі: день 1 з {} ({})...",
            total,
            first_day.format("%Y-%m-%d")
        ));

        let timeframe = self.selected_timeframe;
        let multiplier = self.selected_multiplier;
        let task = iced::Task::perform(
            download_and_cache_day(ticker_info, first_day, timeframe, multiplier),
            Message::DayDownloadCompleted,
        );

        Some(Action::Task(task))
    }

    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let title_bar = row![
            row![
                icon_text(Icon::Search, 15),
                text("Пошук об'єму (Volume Search)")
                    .size(15)
                    .style(|theme: &Theme| {
                        let palette = theme.extended_palette();
                        iced::widget::text::Style {
                            color: Some(palette.primary.base.color),
                        }
                    }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            space::horizontal(),
            button(icon_text(Icon::Close, 12))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::Close)
                .padding([4, 8]),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        // 1. Ticker Search & Exchange Filter Row
        let search_input = text_input("Пошук символу...", &self.ticker_search_query)
            .on_input(Message::TickerSearchChanged)
            .on_submit(Message::TickerSearchSubmitted)
            .width(Length::Fixed(140.0))
            .padding(4)
            .style(|theme, status| style::validated_text_input(theme, status, true));

        let mut exchange_pills = row![].spacing(3).align_y(Alignment::Center);
        let all_active = self.selected_exchange_filter.is_none();
        exchange_pills = exchange_pills.push(
            button(text("Всі").size(11))
                .padding([3, 6])
                .style(move |theme, status| style::button::menu_body(theme, status, all_active))
                .on_press(Message::ExchangeFilterSelected(None)),
        );

        for (ex_inc, label) in [
            (ExchangeInclusive::Binance, "Binance"),
            (ExchangeInclusive::Bybit, "Bybit"),
            (ExchangeInclusive::Okex, "OKX"),
            (ExchangeInclusive::Hyperliquid, "Hyperliquid"),
            (ExchangeInclusive::Coinbase, "Coinbase"),
        ] {
            let is_selected = self.selected_exchange_filter == Some(ex_inc);
            exchange_pills = exchange_pills.push(
                button(text(label).size(11))
                    .padding([3, 6])
                    .style(move |theme, status| {
                        style::button::menu_body(theme, status, is_selected)
                    })
                    .on_press(Message::ExchangeFilterSelected(Some(ex_inc))),
            );
        }

        let ticker_picklist = pick_list(
            &self.filtered_display_tickers[..],
            self.selected_ticker.map(TickerDisplayWrapper),
            |wrapper| Message::TickerSelected(wrapper.0),
        )
        .width(Length::Fixed(190.0));

        let ticker_row = row![
            text("Інструмент:").size(12),
            search_input,
            exchange_pills,
            ticker_picklist,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let timeframe_picklist = pick_list(
            &Timeframe::KLINE[..],
            Some(self.selected_timeframe),
            Message::TimeframeSelected,
        )
        .width(Length::Fixed(75.0));

        let preset_picklist = pick_list(
            &VolumeSearchPreset::ALL[..],
            Some(self.preset),
            Message::PresetSelected,
        )
        .width(Length::Fixed(140.0));

        let is_start_cal_open = self.active_calendar == Some(CalendarTarget::Start);
        let is_end_cal_open = self.active_calendar == Some(CalendarTarget::End);

        let date_controls = row![
            text("Дата:").size(12),
            text_input("YYYY-MM-DD", &self.start_date_input)
                .on_input(Message::StartDateInputChanged)
                .width(Length::Fixed(85.0))
                .padding(4)
                .style(|theme, status| {
                    style::validated_text_input(theme, status, self.is_start_date_valid)
                }),
            button(crate::modal::pane::replay_calendar::calendar_icon(
                None,
                is_start_cal_open,
                12.0,
            ))
            .style(move |theme, status| {
                style::button::menu_body(theme, status, is_start_cal_open)
            })
            .padding([4, 6])
            .on_press(Message::ToggleCalendar(CalendarTarget::Start)),
            text("-").size(12),
            text_input("YYYY-MM-DD", &self.end_date_input)
                .on_input(Message::EndDateInputChanged)
                .width(Length::Fixed(85.0))
                .padding(4)
                .style(|theme, status| {
                    style::validated_text_input(theme, status, self.is_end_date_valid)
                }),
            button(crate::modal::pane::replay_calendar::calendar_icon(
                None,
                is_end_cal_open,
                12.0,
            ))
            .style(move |theme, status| {
                style::button::menu_body(theme, status, is_end_cal_open)
            })
            .padding([4, 6])
            .on_press(Message::ToggleCalendar(CalendarTarget::End)),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let time_filter = row![
            text_input("00:00", &self.time_filter_start)
                .on_input(Message::TimeFilterStartChanged)
                .width(Length::Fixed(55.0))
                .padding(4)
                .style(|theme, status| style::validated_text_input(theme, status, true)),
            text("-").size(12),
            text_input("23:59", &self.time_filter_end)
                .on_input(Message::TimeFilterEndChanged)
                .width(Length::Fixed(55.0))
                .padding(4)
                .style(|theme, status| style::validated_text_input(theme, status, true)),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let apply_btn = if self.is_downloading {
            button(
                row![
                    icon_text(Icon::Search, 11),
                    text(" Завантаження...").size(12)
                ]
                .align_y(Alignment::Center),
            )
            .style(|theme, status| style::button::menu_body(theme, status, false))
            .padding([5, 14])
        } else {
            button(
                row![
                    icon_text(Icon::Checkmark, 11),
                    text(" Застосувати").size(12)
                ]
                .align_y(Alignment::Center),
            )
            .style(|theme, status| style::button::confirm(theme, status, false))
            .padding([5, 14])
            .on_press(Message::ApplySearch)
        };

        let controls_row = row![
            text("Період:").size(12),
            timeframe_picklist,
            text("Пресет:").size(12),
            preset_picklist,
            date_controls,
            text("Час:").size(12),
            time_filter,
            space::horizontal(),
            apply_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        // Price Scale Buttons: [1, 2, 5, 10, 25, 50, 100, 200, 500] + Custom
        let mut scale_buttons = row![].spacing(3).align_y(Alignment::Center);
        for tm in TickMultiplier::ALL {
            let is_selected = !self.is_custom_multiplier && self.selected_multiplier == tm;
            let val_str = tm.0.to_string();
            let btn = button(text(val_str).size(11))
                .padding([3, 6])
                .style(move |theme, status| style::button::menu_body(theme, status, is_selected))
                .on_press(Message::MultiplierSelected(tm));
            scale_buttons = scale_buttons.push(btn);
        }

        let custom_active = self.is_custom_multiplier;
        let custom_toggle_btn = button(text("Custom").size(11))
            .padding([3, 6])
            .style(move |theme, status| style::button::menu_body(theme, status, custom_active))
            .on_press(Message::ToggleCustomMultiplier(!custom_active));
        scale_buttons = scale_buttons.push(custom_toggle_btn);

        if self.is_custom_multiplier {
            let custom_input = text_input("200", &self.custom_multiplier_input)
                .on_input(Message::CustomMultiplierChanged)
                .width(Length::Fixed(50.0))
                .padding(3)
                .style(|theme, status| style::validated_text_input(theme, status, true));
            scale_buttons = scale_buttons.push(custom_input);
        }

        let min_vol_input = text_input("0", &self.min_volume_input)
            .on_input(Message::MinVolumeChanged)
            .width(Length::Fixed(85.0))
            .padding(4)
            .style(|theme, status| style::validated_text_input(theme, status, true));

        let scale_row = row![
            text("Масштаб ціни:").size(12),
            scale_buttons,
            text("Мін. об'єм:").size(12),
            min_vol_input,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        // 2. Table Headers
        let sort_icon = |col: VolumeSearchSortCol| {
            if self.sort_col == col {
                match self.sort_order {
                    SortOrder::Ascending => " ▲",
                    SortOrder::Descending => " ▼",
                }
            } else {
                ""
            }
        };

        let col_header = |title: &'static str, col: VolumeSearchSortCol, portion: u16| {
            let label = format!("{}{}", title, sort_icon(col));
            button(
                text(label)
                    .size(11)
                    .font(style::AZERET_MONO)
                    .width(Length::Fill)
                    .align_x(Alignment::End),
            )
            .width(Length::FillPortion(portion))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(Message::SortColumnClicked(col))
            .padding([3, 4])
        };

        let headers = row![
            text("#")
                .size(11)
                .font(style::AZERET_MONO)
                .width(Length::Fixed(40.0)),
            col_header("Час", VolumeSearchSortCol::Time, 15),
            col_header("Макс. об'єм", VolumeSearchSortCol::MaxVolume, 11),
            col_header("POC", VolumeSearchSortCol::PocPrice, 10),
            col_header("Розмір бару", VolumeSearchSortCol::BarRange, 9),
            col_header("Загальний об'єм", VolumeSearchSortCol::TotalVolume, 13),
            col_header("Дельта", VolumeSearchSortCol::Delta, 10),
            col_header("Покупки", VolumeSearchSortCol::BuyVol, 11),
            col_header("Продажі", VolumeSearchSortCol::SellVol, 11),
            col_header("Макс. Delta", VolumeSearchSortCol::MaxDelta, 10),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding([4, 8])
        .width(Length::Fill);

        // 3. Table Rows
        let mut rows_column = column![].spacing(1).width(Length::Fill);
        if self.rows.is_empty() {
            let empty_msg = if self.is_downloading {
                "Завантаження історичних даних із біржі..."
            } else if self.has_searched {
                "Не знайдено свічок із заданими критеріями об'єму."
            } else {
                "Натисніть 'Застосувати' для сканування та розрахунку кластерних об'ємів."
            };
            rows_column = rows_column.push(
                container(text(empty_msg).size(13))
                    .padding(24)
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
            );
        } else {
            for row_data in &self.rows {
                let dt_str = chrono::DateTime::from_timestamp_millis(row_data.timestamp as i64)
                    .map(|dt| dt.format("%d.%m.%Y %H:%M:%S").to_string())
                    .unwrap_or_else(|| "--".to_string());

                let delta_color = if row_data.total_delta > 0.0 {
                    iced::Color::from_rgb(0.2, 0.8, 0.4)
                } else if row_data.total_delta < 0.0 {
                    iced::Color::from_rgb(0.9, 0.3, 0.3)
                } else {
                    iced::Color::from_rgb(0.6, 0.6, 0.6)
                };

                let row_item = row![
                    text(format!("{}", row_data.index))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .width(Length::Fixed(40.0)),
                    text(dt_str)
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(15)),
                    text(format_with_commas(row_data.max_volume))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(11)),
                    text(format!("{:.2}", row_data.poc_price.to_f32()))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(10)),
                    text(format!("{}", row_data.bar_range_ticks))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(9)),
                    text(format_with_commas(row_data.total_volume))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(13)),
                    text(format!("{:+}", format_with_commas(row_data.total_delta)))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .color(delta_color)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(10)),
                    text(format_with_commas(row_data.buy_volume))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(11)),
                    text(format_with_commas(row_data.sell_volume))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(11)),
                    text(format!("{:+}", format_with_commas(row_data.max_delta)))
                        .size(11)
                        .font(style::AZERET_MONO)
                        .align_x(Alignment::End)
                        .width(Length::FillPortion(10)),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .padding([3, 8])
                .width(Length::Fill);

                rows_column = rows_column.push(row_item);
            }
        }

        let table_scroll = scrollable::Scrollable::with_direction(
            rows_column,
            scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        // 4. Summary Row (Footer)
        let summary_text = if self.summary.count > 0 {
            format!(
                "Середні значення:   Свічок: {}   |   Макс. об'єм: {}   |   Загальний об'єм: {}   |   Дельта: {:+}   |   Розмір бару: {:.1} тіків",
                self.summary.count,
                format_with_commas(self.summary.avg_max_volume),
                format_with_commas(self.summary.avg_total_volume),
                format_with_commas(self.summary.avg_delta),
                self.summary.avg_bar_range,
            )
        } else {
            "Середні значення:   Свічок: 0".to_string()
        };

        let footer = container(
            text(summary_text)
                .size(11)
                .font(style::AZERET_MONO)
                .align_x(Alignment::Start),
        )
        .padding([6, 12])
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(palette.background.weak.color.into()),
                ..Default::default()
            }
        })
        .width(Length::Fill);

        let mut content = column![
            title_bar,
            rule::horizontal(1),
            ticker_row,
            controls_row,
            scale_row,
        ]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

        if let Some(prog) = &self.download_progress {
            let banner = container(
                row![
                    icon_text(Icon::Search, 12),
                    text(prog).size(12).font(style::AZERET_MONO),
                    space::horizontal(),
                    button(text("Скасувати").size(11))
                        .padding([3, 8])
                        .style(|theme, status| style::button::cancel(theme, status, false))
                        .on_press(Message::CancelSearch),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            )
            .padding([6, 12])
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                iced::widget::container::Style {
                    background: Some(palette.background.weak.color.into()),
                    border: iced::Border {
                        color: palette.primary.base.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fill);

            content = content.push(banner);
        }

        content = content
            .push(rule::horizontal(1))
            .push(headers)
            .push(rule::horizontal(1))
            .push(table_scroll)
            .push(rule::horizontal(1))
            .push(footer);

        let modal_base = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::modal_container);

        if let Some(target) = self.active_calendar {
            let left_padding = match target {
                CalendarTarget::Start => 350.0,
                CalendarTarget::End => 480.0,
            };

            iced::widget::stack![
                modal_base,
                mouse_area(
                    container(opaque(self.view_calendar_popover(target)))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(padding::top(105.0).left(left_padding))
                        .align_x(Alignment::Start)
                        .align_y(Alignment::Start)
                )
                .on_press(Message::CloseCalendar)
            ]
            .into()
        } else {
            modal_base.into()
        }
    }

    pub fn view_calendar_popover<'a>(&'a self, target: CalendarTarget) -> Element<'a, Message> {
        let target_title = match target {
            CalendarTarget::Start => "Початкова дата",
            CalendarTarget::End => "Кінцева дата",
        };

        let header = row![
            button(text("«").size(11).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CalPrevYear)
                .padding([2, 5]),
            button(text("‹").size(12).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CalPrevMonth)
                .padding([2, 5]),
            container(
                text(format!(
                    "{} {}",
                    month_name(self.cal_view_month),
                    self.cal_view_year
                ))
                .size(12)
                .font(style::AZERET_MONO)
            )
            .width(Length::Fill)
            .align_x(Alignment::Center),
            button(text("›").size(12).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CalNextMonth)
                .padding([2, 5]),
            button(text("»").size(11).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CalNextYear)
                .padding([2, 5]),
            button(icon_text(Icon::Close, 11))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CloseCalendar)
                .padding([2, 4]),
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let weekdays = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Нд"];
        let weekday_header = row(weekdays.iter().map(|&d| {
            container(
                text(d)
                    .size(10)
                    .font(style::AZERET_MONO)
                    .style(|theme: &Theme| {
                        let p = theme.extended_palette();
                        iced::widget::text::Style {
                            color: Some(p.background.weak.text),
                        }
                    }),
            )
            .width(32)
            .align_x(Alignment::Center)
            .into()
        }))
        .spacing(2);

        let today = chrono::Utc::now().date_naive();
        let first_day =
            NaiveDate::from_ymd_opt(self.cal_view_year, self.cal_view_month, 1).unwrap_or(today);
        let offset = first_day.weekday().num_days_from_monday() as usize;
        let total_days = days_in_month(self.cal_view_year, self.cal_view_month);

        let mut day_cells: Vec<Element<'a, Message>> = Vec::with_capacity(42);

        for _ in 0..offset {
            day_cells.push(space().width(32).height(24).into());
        }

        for day in 1..=total_days {
            if let Some(date) =
                NaiveDate::from_ymd_opt(self.cal_view_year, self.cal_view_month, day)
            {
                let is_target_selected = match target {
                    CalendarTarget::Start => date == self.start_date,
                    CalendarTarget::End => date == self.end_date,
                };
                let is_in_range = date >= self.start_date && date <= self.end_date;
                let is_today = date == today;

                let btn = button(text(format!("{day}")).size(11).font(style::AZERET_MONO))
                    .width(32)
                    .height(24)
                    .padding([1, 0])
                    .style(move |theme: &Theme, status| {
                        let p = theme.extended_palette();
                        if is_target_selected {
                            button::Style {
                                background: Some(p.primary.weak.color.into()),
                                text_color: p.primary.weak.text,
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        } else if is_in_range {
                            button::Style {
                                background: Some(p.background.weak.color.into()),
                                text_color: p.background.base.text,
                                border: if is_today {
                                    iced::Border {
                                        radius: 2.0.into(),
                                        width: 1.0,
                                        color: p.primary.base.color,
                                    }
                                } else {
                                    iced::Border {
                                        radius: 2.0.into(),
                                        ..Default::default()
                                    }
                                },
                                ..Default::default()
                            }
                        } else {
                            let is_hovered = matches!(status, button::Status::Hovered);
                            let bg = if is_hovered {
                                Some(p.background.weakest.color.into())
                            } else {
                                None
                            };
                            let border = if is_today {
                                iced::Border {
                                    radius: 2.0.into(),
                                    width: 1.0,
                                    color: p.primary.base.color,
                                }
                            } else {
                                iced::Border::default()
                            };
                            button::Style {
                                background: bg,
                                text_color: p.background.base.text,
                                border,
                                ..Default::default()
                            }
                        }
                    })
                    .on_press(Message::CalSelectDate(date));

                day_cells.push(btn.into());
            }
        }

        while !day_cells.len().is_multiple_of(7) {
            day_cells.push(space().width(32).height(24).into());
        }

        let mut calendar_rows: Vec<Element<'a, Message>> = Vec::new();
        let mut current_row: Vec<Element<'a, Message>> = Vec::with_capacity(7);
        for cell in day_cells {
            current_row.push(cell);
            if current_row.len() == 7 {
                calendar_rows.push(row(std::mem::take(&mut current_row)).spacing(2).into());
            }
        }
        if !current_row.is_empty() {
            calendar_rows.push(row(current_row).spacing(2).into());
        }

        let calendar_grid = column(calendar_rows).spacing(2);

        let footer_actions = row![
            text(target_title).size(10).font(style::AZERET_MONO),
            space::horizontal(),
            button(text("Сьогодні").size(10).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::CalJumpToToday)
                .padding([2, 5]),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let content = column![
            header,
            rule::horizontal(1),
            weekday_header,
            calendar_grid,
            rule::horizontal(1),
            footer_actions,
        ]
        .spacing(6)
        .width(Length::Fixed(246.0));

        container(content)
            .padding([8, 10])
            .style(style::chart_modal)
            .into()
    }
}

/// Calculates ranked match score for a given ticker candidate and query string.
pub fn calc_ticker_rank(ticker_info: &TickerInfo, query: &str) -> Option<TickerSearchRank> {
    if query.is_empty() {
        return Some(TickerSearchRank {
            bucket: 0,
            pos: 0,
            len: 0,
        });
    }

    let (mut display_str, _) = ticker_info.ticker.display_symbol_and_type();
    let (mut raw_str, _) = ticker_info.ticker.to_full_symbol_and_type();

    display_str.make_ascii_uppercase();
    raw_str.make_ascii_uppercase();

    let suffix = match ticker_info.market_type() {
        MarketKind::LinearPerps | MarketKind::InversePerps => "P",
        _ => "",
    };

    let display_suffixed = format!("{display_str}{suffix}");
    let raw_suffixed = format!("{raw_str}{suffix}");

    let score_candidate = |cand: &str| -> Option<TickerSearchRank> {
        let (bucket, pos) = if cand == query {
            (0_u8, 0_usize)
        } else if cand.starts_with(query) {
            (1_u8, 0_usize)
        } else if cand.ends_with(query) {
            (2_u8, cand.len().saturating_sub(query.len()))
        } else {
            let p = cand.find(query)?;
            (3_u8, p)
        };

        Some(TickerSearchRank {
            bucket,
            pos: (pos.min(u16::MAX as usize)) as u16,
            len: (cand.len().min(u16::MAX as usize)) as u16,
        })
    };

    let mut best: Option<TickerSearchRank> = None;
    for cand in [
        display_str.as_str(),
        display_suffixed.as_str(),
        raw_str.as_str(),
        raw_suffixed.as_str(),
    ] {
        if let Some(rank) = score_candidate(cand) {
            best = Some(match best {
                None => rank,
                Some(cur) => {
                    if (rank.bucket, rank.pos, rank.len) < (cur.bucket, cur.pos, cur.len) {
                        rank
                    } else {
                        cur
                    }
                }
            });
        }
    }

    best
}

/// Parses a date string formatted as `YYYY-MM-DD`. Returns `None` on invalid strings or invalid calendar dates.
pub fn parse_date_ymd(s: &str) -> Option<NaiveDate> {
    let trimmed = s.trim();
    if trimmed.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok()
}

/// Computes whether a given Gregorian year is a leap year.
#[inline]
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Computes the number of days in the specified month of the given year.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Січень",
        2 => "Лютий",
        3 => "Березень",
        4 => "Квітень",
        5 => "Травень",
        6 => "Червень",
        7 => "Липень",
        8 => "Серпень",
        9 => "Вересень",
        10 => "Жовтень",
        11 => "Листопад",
        12 => "Грудень",
        _ => "",
    }
}

/// Pure calculation engine for VolumeSearchQuery temporal bounds from calendar dates and intraday time filters.
/// Guarantees `start_ms <= end_ms <= now_ms`, preventing BTreeMap::range panics.
pub fn calculate_custom_query_range(
    start_date: NaiveDate,
    end_date: NaiveDate,
    time_filter_start: &str,
    time_filter_end: &str,
    now_ms: u64,
) -> (u64, u64, u16, u16) {
    let today = chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    // Normalize date order and clamp to today
    let (norm_start, norm_end) = if start_date > end_date {
        (end_date, start_date)
    } else {
        (start_date, end_date)
    };
    let clamped_end = norm_end.min(today);
    let clamped_start = norm_start.min(clamped_end);

    let start_min = parse_minute_of_day(time_filter_start).unwrap_or(0);
    let end_min = parse_minute_of_day(time_filter_end).unwrap_or(1439);

    let (start_ms, end_ms) = if start_min <= end_min {
        // Standard intraday window
        let start_dt = clamped_start
            .and_hms_opt((start_min / 60) as u32, (start_min % 60) as u32, 0)
            .unwrap_or_else(|| clamped_start.and_hms_opt(0, 0, 0).unwrap());
        let end_dt = clamped_end
            .and_hms_milli_opt((end_min / 60) as u32, (end_min % 60) as u32, 59, 999)
            .unwrap_or_else(|| clamped_end.and_hms_milli_opt(23, 59, 59, 999).unwrap());

        let s = (start_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
        let e = (end_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
        (s, e)
    } else {
        // Cross-midnight window (e.g. 22:00 - 04:00)
        if clamped_start < clamped_end {
            let start_dt = clamped_start
                .and_hms_opt((start_min / 60) as u32, (start_min % 60) as u32, 0)
                .unwrap_or_else(|| clamped_start.and_hms_opt(0, 0, 0).unwrap());
            let end_dt = clamped_end
                .and_hms_milli_opt((end_min / 60) as u32, (end_min % 60) as u32, 59, 999)
                .unwrap_or_else(|| clamped_end.and_hms_milli_opt(23, 59, 59, 999).unwrap());

            let s = (start_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
            let e = (end_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
            (s, e)
        } else {
            // Single-day span with cross-midnight filter: span full 24h of that day in macro range
            let start_dt = clamped_start.and_hms_opt(0, 0, 0).unwrap();
            let end_dt = clamped_end.and_hms_milli_opt(23, 59, 59, 999).unwrap();

            let s = (start_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
            let e = (end_dt.and_utc().timestamp_millis().max(0) as u64).min(now_ms);
            (s, e)
        }
    };

    // Universal invariant: ensure start <= end to prevent BTreeMap::range panics
    let (final_start_ms, final_end_ms) = if start_ms > end_ms {
        (end_ms, start_ms)
    } else {
        (start_ms, end_ms)
    };
    let final_end_ms = final_end_ms.min(now_ms);
    let final_start_ms = final_start_ms.min(final_end_ms);

    (final_start_ms, final_end_ms, start_min, end_min)
}

fn parse_minute_of_day(s: &str) -> Option<u16> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u16 = parts[0].trim().parse().ok()?;
    let m: u16 = parts[1].trim().parse().ok()?;
    if h < 24 && m < 60 {
        Some(h * 60 + m)
    } else {
        None
    }
}

pub fn exchange_data_folder(exchange: exchange::adapter::Exchange) -> &'static str {
    match exchange {
        Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => "binance",
        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => "bybit",
        Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => "okex",
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => "hyperliquid",
        Exchange::MexcSpot | Exchange::MexcLinear | Exchange::MexcInverse => "mexc",
        Exchange::CoinbaseSpot => "coinbase",
    }
}

pub fn scan_local_cache_for_day(
    ticker_info: &TickerInfo,
    day: NaiveDate,
    timeframe: Timeframe,
    multiplier: TickMultiplier,
) -> Option<Vec<(u64, KlineDataPoint)>> {
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let base_data_path = data::data_path(None);
    let base_step = PriceStep::from_f32_lossy(ticker_info.min_ticksize.into());
    let effective_step = PriceStep {
        units: base_step.units.saturating_mul(multiplier.0 as i64).max(1),
    };
    let cache_path = data::chart::kline::footprint_cache_path(
        &base_data_path,
        &symbol,
        timeframe,
        effective_step,
        day,
    );
    let today = chrono::Utc::now().date_naive();

    // Tier 1: Footprint cache (.fst / .bin)
    if day < today
        && cache_path.exists()
        && let Some(dps) = data::chart::kline::load_daily_footprint(&cache_path)
    {
        return Some(dps);
    }

    // Tier 2: Raw trades cache
    let exchange_folder = exchange_data_folder(ticker_info.exchange());
    let trades_data_path = data::data_path(Some(&format!("market_data/{exchange_folder}/")));

    let raw_trades = if day < today {
        exchange::trades::cache::load_raw_trades_from_cache(&trades_data_path, ticker_info, day)
    } else {
        exchange::trades::cache::load_intraday_trades_from_cache(
            &trades_data_path,
            ticker_info,
            day,
        )
    };

    if let Some(trades) = raw_trades
        && !trades.is_empty()
    {
        let dps = data::aggr::time::aggregate_trades_for_day(&trades, timeframe, effective_step);
        if !dps.is_empty() {
            let interval_ms = timeframe.to_milliseconds();
            let expected_count =
                86_400_000u64.checked_div(interval_ms).unwrap_or(1).max(1) as usize;
            let is_day_complete = dps.len() >= expected_count
                && dps.iter().all(|(_, dp)| {
                    (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) == 0.0
                        || !dp.footprint.is_empty()
                });
            if day < today && is_day_complete {
                let _ = data::chart::kline::save_daily_footprint(&cache_path, &dps);
            }
            return Some(dps);
        }
    }

    None
}

pub fn scan_all_local_cached_days(
    ticker_info: &TickerInfo,
    timeframe: Timeframe,
    multiplier: TickMultiplier,
) -> BTreeMap<u64, KlineDataPoint> {
    let mut datapoints = BTreeMap::new();
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let base_data_path = data::data_path(None);
    let base_step = PriceStep::from_f32_lossy(ticker_info.min_ticksize.into());
    let effective_step = PriceStep {
        units: base_step.units.saturating_mul(multiplier.0 as i64).max(1),
    };
    let fp_dir = base_data_path
        .join("footprint")
        .join(symbol.to_uppercase())
        .join(timeframe.to_string())
        .join(effective_step.units.to_string());

    if fp_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&fp_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension()
                && (ext == "fst" || ext == "bin")
                && let Some(dps) = data::chart::kline::load_daily_footprint(&path)
            {
                for (t, dp) in dps {
                    datapoints.insert(t, dp);
                }
            }
        }
    }

    let exchange_folder = exchange_data_folder(ticker_info.exchange());
    let trades_dir = data::data_path(Some(&format!("market_data/{exchange_folder}/")))
        .join(symbol.to_uppercase());
    if trades_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&trades_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Some(date_part) = stem.split('_').next()
                && let Some(day) = parse_date_ymd(date_part)
            {
                let day_start = day
                    .and_hms_opt(0, 0, 0)
                    .unwrap_or_default()
                    .and_utc()
                    .timestamp_millis() as u64;
                if !datapoints.contains_key(&day_start)
                    && let Some(dps) =
                        scan_local_cache_for_day(ticker_info, day, timeframe, multiplier)
                {
                    for (t, dp) in dps {
                        datapoints.insert(t, dp);
                    }
                }
            }
        }
    }

    datapoints
}

pub async fn download_and_cache_day(
    ticker_info: TickerInfo,
    day: NaiveDate,
    timeframe: Timeframe,
    multiplier: TickMultiplier,
) -> Result<(NaiveDate, Vec<(u64, KlineDataPoint)>), String> {
    let exchange_folder = exchange_data_folder(ticker_info.exchange());
    let trades_data_path = data::data_path(Some(&format!("market_data/{exchange_folder}/")));
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let base_data_path = data::data_path(None);
    let base_step = PriceStep::from_f32_lossy(ticker_info.min_ticksize.into());
    let effective_step = PriceStep {
        units: base_step.units.saturating_mul(multiplier.0 as i64).max(1),
    };
    let cache_path = data::chart::kline::footprint_cache_path(
        &base_data_path,
        &symbol,
        timeframe,
        effective_step,
        day,
    );
    let today = chrono::Utc::now().date_naive();

    let day_start = day
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "Invalid date".to_string())?
        .and_utc()
        .timestamp_millis() as u64;
    let day_end = day_start.saturating_add(86_400_000);
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let to_time = day_end.min(now_ms);

    let mut latest_trade_t = day_start;
    let mut all_trades = Vec::new();

    if exchange::clock::is_sync_needed(ticker_info.exchange(), 60_000) {
        let _ = exchange::clock::ping_server_time(ticker_info.exchange()).await;
    }

    while latest_trade_t < to_time {
        match exchange::trades::fetch_trades(ticker_info, latest_trade_t, trades_data_path.clone())
            .await
        {
            Ok((mut batch, next_trade_t)) => {
                let batch_len = batch.len();
                let now_server = exchange::clock::server_time_ms(Some(ticker_info.exchange()));
                let is_at_market_tip = latest_trade_t >= now_server.saturating_sub(120_000)
                    || next_trade_t >= now_server.saturating_sub(120_000);

                if batch_len == 0 && is_at_market_tip {
                    break;
                }

                batch.retain(|t| t.time < to_time);
                all_trades.extend(batch);

                if next_trade_t >= to_time || next_trade_t <= latest_trade_t {
                    break;
                }

                latest_trade_t = next_trade_t;

                let sleep_ms = match ticker_info.exchange() {
                    Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => 200,
                    Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => 100,
                    _ => 50,
                };
                if (100..=2000).contains(&batch_len) {
                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                } else if batch_len == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
            Err(err) => return Err(format!("Download error: {err:?}")),
        }
    }

    let trades = if all_trades.is_empty() {
        if day < today {
            exchange::trades::cache::load_raw_trades_from_cache(
                &trades_data_path,
                &ticker_info,
                day,
            )
            .unwrap_or(all_trades)
        } else {
            exchange::trades::cache::load_intraday_trades_from_cache(
                &trades_data_path,
                &ticker_info,
                day,
            )
            .unwrap_or(all_trades)
        }
    } else {
        all_trades
    };

    let dps = data::aggr::time::aggregate_trades_for_day(&trades, timeframe, effective_step);
    if !dps.is_empty() {
        let interval_ms = timeframe.to_milliseconds();
        let expected_count = 86_400_000u64.checked_div(interval_ms).unwrap_or(1).max(1) as usize;
        let is_day_complete = dps.len() >= expected_count
            && dps.iter().all(|(_, dp)| {
                (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) == 0.0
                    || !dp.footprint.is_empty()
            });
        if day < today && is_day_complete {
            let _ = data::chart::kline::save_daily_footprint(&cache_path, &dps);
        }
    }

    Ok((day, dps))
}

#[cfg(test)]
mod tests;
