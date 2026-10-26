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
    #[allow(dead_code)]
    pub fn is_date_range_valid(&self) -> bool {
        self.is_start_date_valid && self.is_end_date_valid && self.start_date <= self.end_date
    }

    /// Computes the contiguous sequence of calendar dates spanned by [start_date, end_date].
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn inherit_active_pane_ticker(&mut self, ticker: Option<TickerInfo>) {
        if self.selected_ticker.is_none() {
            self.selected_ticker = ticker;
        }
    }

    /// Returns the currently selected instrument.
    #[allow(dead_code)]
    pub fn selected_ticker(&self) -> Option<TickerInfo> {
        self.selected_ticker
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

    #[allow(dead_code)]
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
                    (dp.kline.volume.0 + dp.kline.volume.1) == 0.0 || !dp.footprint.is_empty()
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
                (dp.kline.volume.0 + dp.kline.volume.1) == 0.0 || !dp.footprint.is_empty()
            });
        if day < today && is_day_complete {
            let _ = data::chart::kline::save_daily_footprint(&cache_path, &dps);
        }
    }

    Ok((day, dps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::adapter::ExchangeInclusive;
    use exchange::{Exchange, Ticker, TickerInfo};

    fn mock_ticker(symbol: &str, exchange: Exchange) -> TickerInfo {
        TickerInfo::new(Ticker::new(symbol, exchange), 0.01, 0.001, None)
    }

    fn sample_catalog() -> Vec<TickerInfo> {
        vec![
            // Binance: Linear, Spot, Inverse
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceSpot),
            mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse),
            // Bybit: Linear, Spot, Inverse
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
            mock_ticker("XRPUSDT", Exchange::BybitSpot),
            mock_ticker("BTCUSD", Exchange::BybitInverse),
            // OKX: Linear, Spot, Inverse
            mock_ticker("DOGE-USDT-SWAP", Exchange::OkexLinear),
            mock_ticker("LTC-USDT", Exchange::OkexSpot),
            mock_ticker("ETH-USD-SWAP", Exchange::OkexInverse),
            // Hyperliquid: Linear, Spot
            mock_ticker("HYPE", Exchange::HyperliquidLinear),
            mock_ticker("PURR", Exchange::HyperliquidSpot),
        ]
    }

    #[test]
    fn test_m1_global_ticker_filtering_all_4_exchanges_with_zero_panes() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog.clone());

        assert_eq!(modal.available_tickers.len(), 11);
        assert_eq!(modal.selected_ticker, Some(catalog[0]));

        // 1. All exchanges (None filter)
        modal.selected_exchange_filter = None;
        modal.ticker_search_query.clear();
        let all = modal.filtered_and_ranked_candidates();
        assert_eq!(all.len(), 11);

        // 2. Filter Binance (matches Linear, Spot, Inverse)
        modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
        let binance = modal.filtered_and_ranked_candidates();
        assert_eq!(binance.len(), 3);
        assert!(
            binance
                .iter()
                .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Binance)
        );

        // 3. Filter Bybit
        modal.selected_exchange_filter = Some(ExchangeInclusive::Bybit);
        let bybit = modal.filtered_and_ranked_candidates();
        assert_eq!(bybit.len(), 3);
        assert!(
            bybit
                .iter()
                .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Bybit)
        );

        // 4. Filter OKX
        modal.selected_exchange_filter = Some(ExchangeInclusive::Okex);
        let okx = modal.filtered_and_ranked_candidates();
        assert_eq!(okx.len(), 3);
        assert!(
            okx.iter()
                .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Okex)
        );

        // 5. Filter Hyperliquid
        modal.selected_exchange_filter = Some(ExchangeInclusive::Hyperliquid);
        let hl = modal.filtered_and_ranked_candidates();
        assert_eq!(hl.len(), 2);
        assert!(
            hl.iter()
                .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Hyperliquid)
        );

        // 6. Select a Hyperliquid ticker with 0 panes open in dashboard
        let target_ticker = hl[0];
        modal.update(Message::TickerSelected(target_ticker), None);
        assert_eq!(modal.selected_ticker, Some(target_ticker));
        assert!(!modal.is_ticker_selector_open);
    }

    #[test]
    fn test_m1_exchange_variant_normalization_matches_all_submarkets() {
        // Exhaustive verification of all 11 Exchange variants
        for exchange in Exchange::ALL {
            let inclusive = ExchangeInclusive::of(exchange);
            match exchange {
                Exchange::BinanceLinear | Exchange::BinanceInverse | Exchange::BinanceSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Binance);
                }
                Exchange::BybitLinear | Exchange::BybitInverse | Exchange::BybitSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Bybit);
                }
                Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Okex);
                }
                Exchange::HyperliquidLinear | Exchange::HyperliquidSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Hyperliquid);
                }
                Exchange::MexcLinear | Exchange::MexcInverse | Exchange::MexcSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Mexc);
                }
                Exchange::CoinbaseSpot => {
                    assert_eq!(inclusive, ExchangeInclusive::Coinbase);
                }
            }
        }
    }

    #[test]
    fn test_m1_search_matching_case_prefix_substring_queries() {
        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
            mock_ticker("1000PEPEUSDT", Exchange::BybitLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDC", Exchange::BybitLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.selected_exchange_filter = None;

        // 1. Lowercase prefix query
        modal.ticker_search_query = "btc".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].ticker.as_str(), "BTCUSDT"); // Exact prefix ranks first
        assert_eq!(res[1].ticker.as_str(), "WBTCUSDT"); // Substring ranks second

        // 2. Uppercase query
        modal.ticker_search_query = "BTC".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].ticker.as_str(), "BTCUSDT");

        // 3. Mixed case exact symbol
        modal.ticker_search_query = "bTcUsDt".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].ticker.as_str(), "BTCUSDT");

        // 4. Substring in the middle of a token
        modal.ticker_search_query = "pepe".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].ticker.as_str(), "1000PEPEUSDT");

        // 5. Leading and trailing whitespace handling
        modal.ticker_search_query = "  eth  ".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].ticker.as_str(), "ETHUSDT");

        // 6. Non-matching query
        modal.ticker_search_query = "DOGE".to_string();
        let res = modal.filtered_and_ranked_candidates();
        assert!(res.is_empty());
    }

    #[test]
    fn test_m1_catalog_update_preserves_selection_and_query() {
        let initial_catalog = vec![mock_ticker("BTCUSDT", Exchange::BinanceLinear)];
        let mut modal = VolumeSearchModal::new(None, initial_catalog);

        modal.ticker_search_query = "ETH".to_string();
        modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
        let active_sel = mock_ticker("BTCUSDT", Exchange::BinanceLinear);
        modal.selected_ticker = Some(active_sel);

        // Simulate incoming async batch with newly arrived pairs
        let updated_catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
        ];
        modal.update_available_tickers(updated_catalog);

        // Invariants: available_tickers updated; search query, exchange filter, and selection preserved
        assert_eq!(modal.available_tickers.len(), 3);
        assert_eq!(modal.selected_ticker, Some(active_sel));
        assert_eq!(modal.ticker_search_query, "ETH");
        assert_eq!(
            modal.selected_exchange_filter,
            Some(ExchangeInclusive::Binance)
        );

        let res = modal.filtered_and_ranked_candidates();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].ticker.as_str(), "ETHUSDT");
    }

    #[test]
    fn test_ticker_ranked_matching() {
        let btc = mock_ticker("BTCUSDT", Exchange::BinanceLinear);
        let wbtc = mock_ticker("WBTCUSDT", Exchange::BinanceSpot);
        let eth = mock_ticker("ETHUSDT", Exchange::BinanceLinear);

        let rank_btc = calc_ticker_rank(&btc, "BTC").unwrap();
        let rank_wbtc = calc_ticker_rank(&wbtc, "BTC").unwrap();
        let rank_eth = calc_ticker_rank(&eth, "BTC");

        assert!(rank_eth.is_none());
        assert_eq!(rank_btc.bucket, 1); // prefix
        assert_eq!(rank_wbtc.bucket, 3); // substring
        assert!(rank_btc.bucket < rank_wbtc.bucket);
    }

    #[test]
    fn test_adversarial_zero_panes_event_flow_and_view_rendering() {
        let mock_coin = mock_ticker("NONEXISTENT_COIN", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        // View before search in isolated scope
        {
            let _view_initial = modal.view();
        }

        // State transitions with dashboard = None
        assert!(
            modal
                .update(Message::TimeframeSelected(Timeframe::M15), None)
                .is_none()
        );
        assert_eq!(modal.selected_timeframe, Timeframe::M15);

        assert!(
            modal
                .update(Message::PresetSelected(VolumeSearchPreset::Last7d), None)
                .is_none()
        );
        assert_eq!(modal.preset, VolumeSearchPreset::Last7d);

        assert!(
            modal
                .update(Message::TimeFilterStartChanged("08:00".into()), None)
                .is_none()
        );
        assert_eq!(modal.time_filter_start, "08:00");

        assert!(
            modal
                .update(Message::TimeFilterEndChanged("20:00".into()), None)
                .is_none()
        );
        assert_eq!(modal.time_filter_end, "20:00");

        assert!(
            modal
                .update(Message::MultiplierSelected(TickMultiplier(50)), None)
                .is_none()
        );
        assert_eq!(modal.selected_multiplier, TickMultiplier(50));

        assert!(
            modal
                .update(Message::ToggleCustomMultiplier(true), None)
                .is_none()
        );
        assert!(modal.is_custom_multiplier);

        assert!(
            modal
                .update(Message::CustomMultiplierChanged("150".into()), None)
                .is_none()
        );
        assert_eq!(modal.selected_multiplier, TickMultiplier(150));

        assert!(
            modal
                .update(Message::MinVolumeChanged("5000".into()), None)
                .is_none()
        );
        assert_eq!(modal.min_volume_input, "5000");

        assert!(
            modal
                .update(Message::SortColumnClicked(VolumeSearchSortCol::Delta), None)
                .is_none()
        );
        assert_eq!(modal.sort_col, VolumeSearchSortCol::Delta);
        assert_eq!(modal.sort_order, SortOrder::Descending);

        assert!(
            modal
                .update(Message::SortColumnClicked(VolumeSearchSortCol::Delta), None)
                .is_none()
        );
        assert_eq!(modal.sort_order, SortOrder::Ascending);

        // Apply search with 0 panes and no disk cache initiates download task
        let action = modal.update(Message::ApplySearch, None);
        assert!(matches!(action, Some(Action::Task(_))));
        assert!(modal.is_downloading);
        assert!(modal.download_progress.is_some());
        assert_eq!(modal.download_missing_dates.len(), 7);

        // Cancel search resets downloading state
        modal.update(Message::CancelSearch, None);
        assert!(!modal.is_downloading);
        assert!(modal.download_progress.is_none());

        // View after search
        {
            let _view_searched = modal.view();
        }

        // Close message returns Action::Close
        assert!(matches!(
            modal.update(Message::Close, None),
            Some(Action::Close)
        ));
    }

    #[test]
    fn test_adversarial_dynamic_catalog_progressive_updates() {
        let mut modal = VolumeSearchModal::new(None, Vec::new());
        assert_eq!(modal.available_tickers.len(), 0);
        assert_eq!(modal.selected_ticker, None);
        assert_eq!(modal.filtered_tickers.len(), 0);

        modal.update(Message::TickerSearchChanged("ETH".into()), None);
        assert_eq!(modal.ticker_search_query, "ETH");
        assert_eq!(modal.filtered_tickers.len(), 0);

        modal.update(Message::TickerSearchSubmitted, None);
        assert_eq!(modal.selected_ticker, None);

        // Batch 1: BTC and SOL only
        let batch_1 = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BinanceLinear),
        ];
        modal.update_available_tickers(batch_1);
        assert_eq!(modal.available_tickers.len(), 2);
        assert_eq!(modal.filtered_tickers.len(), 0);
        assert_eq!(modal.selected_ticker, None);

        // Batch 2: ETH pairs added
        let batch_2 = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceSpot),
        ];
        modal.update_available_tickers(batch_2);
        assert_eq!(modal.available_tickers.len(), 4);
        assert_eq!(modal.filtered_tickers.len(), 2);
        assert_eq!(
            modal.selected_ticker,
            Some(mock_ticker("ETHUSDT", Exchange::BinanceLinear))
        );

        // User explicitly selects Spot ETH
        let spot_eth = mock_ticker("ETHUSDT", Exchange::BinanceSpot);
        modal.update(Message::TickerSelected(spot_eth), None);
        assert_eq!(modal.selected_ticker, Some(spot_eth));

        // Batch 3: Bybit and OKX added
        let batch_3 = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceSpot),
            mock_ticker("ETHUSDT", Exchange::BybitLinear),
            mock_ticker("ETH-USDT-SWAP", Exchange::OkexLinear),
        ];
        modal.update_available_tickers(batch_3.clone());
        assert_eq!(modal.available_tickers.len(), 6);
        assert_eq!(modal.selected_ticker, Some(spot_eth));
        assert_eq!(modal.filtered_tickers.len(), 4);

        modal.update_available_tickers(batch_3);
        assert_eq!(modal.selected_ticker, Some(spot_eth));
    }

    #[test]
    fn test_adversarial_exchange_filter_oscillation_and_empty_transitions() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        modal.update(Message::TickerSearchChanged("HYPE".into()), None);
        assert_eq!(modal.filtered_tickers.len(), 1);
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "HYPE");

        modal.update(Message::TickerSearchSubmitted, None);
        let hype_ticker = modal.filtered_tickers[0];
        assert_eq!(modal.selected_ticker, Some(hype_ticker));

        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Binance)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 0);
        assert_eq!(modal.selected_ticker, Some(hype_ticker));

        modal.update(Message::TickerSearchSubmitted, None);
        assert_eq!(modal.selected_ticker, Some(hype_ticker));

        {
            let _view_empty_filter = modal.view();
        }

        for _ in 0..20 {
            modal.update(
                Message::ExchangeFilterSelected(Some(ExchangeInclusive::Bybit)),
                None,
            );
            assert_eq!(modal.filtered_tickers.len(), 0);

            modal.update(
                Message::ExchangeFilterSelected(Some(ExchangeInclusive::Okex)),
                None,
            );
            assert_eq!(modal.filtered_tickers.len(), 0);

            modal.update(
                Message::ExchangeFilterSelected(Some(ExchangeInclusive::Hyperliquid)),
                None,
            );
            assert_eq!(modal.filtered_tickers.len(), 1);

            modal.update(Message::ExchangeFilterSelected(None), None);
            assert_eq!(modal.filtered_tickers.len(), 1);
        }

        assert_eq!(modal.selected_ticker, Some(hype_ticker));
    }

    #[test]
    fn test_adversarial_enter_vs_picklist_selection_and_ranking() {
        let catalog = vec![
            mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTCUSD", Exchange::BybitInverse),
        ];
        let mut modal = VolumeSearchModal::new(None, catalog);

        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        // Expected: LinearPerps (BTCUSDT) ranks before InversePerps (BTCUSD).
        // Actual defect: String length comparison rank.len (6 < 7) evaluates before market_type,
        // causing InversePerps BTCUSD to rank before LinearPerps BTCUSDT.
        assert_eq!(
            modal.filtered_tickers[0].ticker.as_str(),
            "BTCUSDT",
            "ranking bug: InversePerps BTCUSD ranked before LinearPerps BTCUSDT due to string length"
        );
        assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSD");
        assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "WBTCUSDT");
        assert_eq!(modal.filtered_tickers[3].ticker.as_str(), "1000BTCUSDT");

        modal.update(Message::TickerSearchSubmitted, None);
        assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "BTCUSDT");

        let picklist_choice = modal.filtered_tickers[2];
        modal.update(Message::TickerSelected(picklist_choice), None);
        assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "WBTCUSDT");

        modal.update(Message::TickerSearchChanged("NONEXISTENT".into()), None);
        assert!(modal.filtered_tickers.is_empty());

        modal.update(Message::TickerSearchSubmitted, None);
        assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "WBTCUSDT");
    }

    #[test]
    fn test_adversarial_exact_match_pruning_hides_cross_exchange_pairs() {
        let catalog = vec![
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.selected_exchange_filter = None;

        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        let binance_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::BinanceLinear);
        let bybit_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::BybitLinear);

        assert!(
            binance_present && bybit_present,
            "BUG DETECTED: Searching 'BTC' across all exchanges purged Binance and Bybit pairs because Hyperliquid had an exact match 'BTC'!"
        );
    }

    #[test]
    fn test_challenger1_empty_and_whitespace_queries() {
        let catalog = sample_catalog();
        let modal = VolumeSearchModal::new(None, catalog.clone());

        // Empty query
        let res_empty = modal.compute_filtered_tickers("");
        assert_eq!(res_empty.len(), catalog.len());

        // Whitespace-only queries
        let res_spaces = modal.compute_filtered_tickers("   ");
        assert_eq!(res_spaces.len(), catalog.len());
        let res_tabs = modal.compute_filtered_tickers(" \t \n ");
        assert_eq!(res_tabs.len(), catalog.len());

        // Invariants: MarketKind ordering (Linear < Spot < Inverse) when rank is identical
        for window in res_empty.windows(2) {
            let kind_rank = |k: MarketKind| match k {
                MarketKind::LinearPerps => 0,
                MarketKind::Spot => 1,
                MarketKind::InversePerps => 2,
            };
            let k0 = kind_rank(window[0].market_type());
            let k1 = kind_rank(window[1].market_type());
            assert!(
                k0 <= k1,
                "Order violation: market kinds not sorted properly on empty query"
            );
        }
    }

    #[test]
    fn test_challenger1_unicode_and_adversarial_queries() {
        let catalog = sample_catalog();
        let modal = VolumeSearchModal::new(None, catalog);

        // 1. Cyrillic input (e.g. Ukrainian/Russian keyboard layout slips)
        let res_cyr = modal.compute_filtered_tickers("БТК");
        assert!(
            res_cyr.is_empty(),
            "Cyrillic query should safely yield 0 results without panic"
        );

        let res_cyr_lower = modal.compute_filtered_tickers("бтк");
        assert!(res_cyr_lower.is_empty());

        // 2. Unicode Emojis
        let res_emoji = modal.compute_filtered_tickers("BTC🚀");
        assert!(res_emoji.is_empty());

        let res_fire = modal.compute_filtered_tickers("🔥");
        assert!(res_fire.is_empty());

        // 3. Currency symbols
        let res_currency = modal.compute_filtered_tickers("€$¥₴");
        assert!(res_currency.is_empty());

        // 4. German Eszett (uppercases to "SS")
        let res_eszett = modal.compute_filtered_tickers("ß");
        assert!(res_eszett.is_empty());

        // 5. Extremely long query (100,000 characters) - stress test memory & bounds
        let huge_query = "A".repeat(100_000);
        let res_huge = modal.compute_filtered_tickers(&huge_query);
        assert!(res_huge.is_empty());

        // 6. Null byte and control characters
        let res_control = modal.compute_filtered_tickers("BTC\0USDT");
        assert!(res_control.is_empty());
    }

    #[test]
    fn test_challenger1_all_11_exchange_variants_and_unusual_tickers() {
        let exhaustive_catalog = vec![
            // 1. Binance Linear
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("1000PEPEUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear),
            // 2. Binance Inverse
            mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse),
            mock_ticker("BTCUSD_241227", Exchange::BinanceInverse),
            // 3. Binance Spot
            mock_ticker("BTCUSDT", Exchange::BinanceSpot),
            mock_ticker("WBTCBTC", Exchange::BinanceSpot),
            // 4. Bybit Linear
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("1000PEPEUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-PERP", Exchange::BybitLinear),
            // 5. Bybit Inverse
            mock_ticker("BTCUSD", Exchange::BybitInverse),
            mock_ticker("BTCUSDM24", Exchange::BybitInverse),
            // 6. Bybit Spot
            mock_ticker("BTCUSDT", Exchange::BybitSpot),
            mock_ticker("ETHUSDT", Exchange::BybitSpot),
            // 7. OKX Linear
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
            mock_ticker("BTC-USDT-241227", Exchange::OkexLinear),
            // 8. OKX Inverse
            mock_ticker("BTC-USD-SWAP", Exchange::OkexInverse),
            mock_ticker("BTC-USD-241227", Exchange::OkexInverse),
            // 9. OKX Spot
            mock_ticker("BTC-USDT", Exchange::OkexSpot),
            mock_ticker("LTC-BTC", Exchange::OkexSpot),
            // 10. Hyperliquid Linear
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("kPEPE", Exchange::HyperliquidLinear),
            mock_ticker("PURR", Exchange::HyperliquidLinear),
            // 11. Hyperliquid Spot
            mock_ticker("PURR/USDC", Exchange::HyperliquidSpot),
            mock_ticker("PURR", Exchange::HyperliquidSpot),
            mock_ticker("HYPE", Exchange::HyperliquidSpot),
        ];

        let mut modal = VolumeSearchModal::new(None, exhaustive_catalog.clone());

        // Verify TickerDisplayWrapper formatting on all 26 variants
        for ti in &exhaustive_catalog {
            let wrapper = TickerDisplayWrapper(*ti);
            let s = format!("{wrapper}");
            assert!(!s.is_empty());
            assert!(s.contains(':'), "Wrapper must format as 'Exchange: Symbol'");
        }

        // Test filtering by each exchange
        modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
        let binance = modal.compute_filtered_tickers("");
        assert_eq!(binance.len(), 7);

        modal.selected_exchange_filter = Some(ExchangeInclusive::Bybit);
        let bybit = modal.compute_filtered_tickers("");
        assert_eq!(bybit.len(), 7);

        modal.selected_exchange_filter = Some(ExchangeInclusive::Okex);
        let okx = modal.compute_filtered_tickers("");
        assert_eq!(okx.len(), 6);

        modal.selected_exchange_filter = Some(ExchangeInclusive::Hyperliquid);
        let hl = modal.compute_filtered_tickers("");
        assert_eq!(hl.len(), 6);

        // Test unusual token formats
        modal.selected_exchange_filter = None;
        let swap_query = modal.compute_filtered_tickers("SWAP");
        assert_eq!(swap_query.len(), 2); // BTC-USDT-SWAP, BTC-USD-SWAP
        assert!(
            swap_query
                .iter()
                .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Okex)
        );

        let pepe_query = modal.compute_filtered_tickers("PEPE");
        assert_eq!(pepe_query.len(), 3); // 1000PEPEUSDT (Binance), 1000PEPEUSDT (Bybit), kPEPE (Hyperliquid)

        let dated_query = modal.compute_filtered_tickers("241227");
        assert_eq!(dated_query.len(), 4); // Binance Linear, Binance Inverse, OKX Linear, OKX Inverse
    }

    #[test]
    fn test_challenger1_extreme_catalog_sizes_performance() {
        use std::time::Instant;

        // Construct 10,000 synthetic tickers
        let mut large_catalog = Vec::with_capacity(10_000);
        let exchanges = [
            Exchange::BinanceLinear,
            Exchange::BinanceSpot,
            Exchange::BybitLinear,
            Exchange::OkexLinear,
            Exchange::HyperliquidLinear,
        ];

        for i in 0..10_000 {
            let ex = exchanges[i % exchanges.len()];
            let sym = format!("SYM{i:05}USDT");
            large_catalog.push(mock_ticker(&sym, ex));
        }
        // Insert recognizable targets
        large_catalog.push(mock_ticker("BTCUSDT", Exchange::BinanceLinear));
        large_catalog.push(mock_ticker("ETHUSDT", Exchange::BybitLinear));

        let modal = VolumeSearchModal::new(None, large_catalog);

        // 1. Empty query benchmark
        let t0 = Instant::now();
        let res_empty = modal.compute_filtered_tickers("");
        let d_empty = t0.elapsed();
        assert_eq!(res_empty.len(), 10_002);

        // 2. Prefix query benchmark
        let t1 = Instant::now();
        let res_prefix = modal.compute_filtered_tickers("SYM00");
        let d_prefix = t1.elapsed();
        assert_eq!(res_prefix.len(), 1000);

        // 3. Single hit query
        let t2 = Instant::now();
        let res_single = modal.compute_filtered_tickers("BTCUSDT");
        let d_single = t2.elapsed();
        assert_eq!(res_single.len(), 1);

        // 4. Non-matching query
        let t3 = Instant::now();
        let res_miss = modal.compute_filtered_tickers("NONEXISTENT");
        let d_miss = t3.elapsed();
        assert!(res_miss.is_empty());

        println!(
            "PERFORMANCE 10k Catalog: Empty: {:?}, Prefix: {:?}, Single: {:?}, Miss: {:?}",
            d_empty, d_prefix, d_single, d_miss
        );
    }

    #[test]
    fn test_challenger1_empirical_proof_exact_match_pruning_bug() {
        let catalog = vec![
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        ];
        let modal = VolumeSearchModal::new(None, catalog);
        let res = modal.compute_filtered_tickers("BTC");

        // Verifies that matched.retain(|(_, r)| r.bucket <= 1) preserves
        // Binance, Bybit, and OKX when Hyperliquid has an exact asset name match.
        assert_eq!(res.len(), 4);
        assert!(
            res.iter()
                .any(|t| t.exchange() == Exchange::HyperliquidLinear)
        );
        assert!(res.iter().any(|t| t.exchange() == Exchange::BinanceLinear));
        assert!(res.iter().any(|t| t.exchange() == Exchange::BybitLinear));
        assert!(res.iter().any(|t| t.exchange() == Exchange::OkexLinear));
    }

    #[test]
    fn test_challenger1_empirical_proof_inverse_perp_length_bug() {
        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSD", Exchange::BybitInverse),
        ];
        let modal = VolumeSearchModal::new(None, catalog);
        let res = modal.compute_filtered_tickers("BTC");

        // Verifies that Linear Perps ("BTCUSDT", len 7) ranks ahead of
        // Inverse Perps ("BTCUSD", len 6) because market kind priority precedes symbol length.
        assert_eq!(res[0].ticker.as_str(), "BTCUSDT");
        assert_eq!(res[0].exchange(), Exchange::BinanceLinear);
        assert_eq!(res[1].ticker.as_str(), "BTCUSD");
        assert_eq!(res[1].exchange(), Exchange::BybitInverse);
    }

    #[test]
    fn test_regression_btc_cross_exchange_catalog_preserves_all_pairs() {
        let catalog = vec![
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.selected_exchange_filter = None;

        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        // Verification 1: All 4 exchanges must be preserved in search results
        assert_eq!(
            modal.filtered_tickers.len(),
            4,
            "Searching 'BTC' must preserve all 4 cross-exchange candidates (Hyperliquid, Binance, Bybit, OKX)"
        );

        let hl_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::HyperliquidLinear);
        let binance_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::BinanceLinear);
        let bybit_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::BybitLinear);
        let okx_present = modal
            .filtered_tickers
            .iter()
            .any(|t| t.exchange() == Exchange::OkexLinear);

        assert!(hl_present, "Hyperliquid Linear BTC must be preserved");
        assert!(binance_present, "Binance Linear BTCUSDT must be preserved");
        assert!(bybit_present, "Bybit Linear BTCUSDT must be preserved");
        assert!(okx_present, "OKX Linear BTC-USDT-SWAP must be preserved");

        // Verification 2: Exact match (Hyperliquid BTC) must rank first
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::HyperliquidLinear,
            "Exact match Hyperliquid BTC must rank at index 0"
        );
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
    }

    #[test]
    fn test_adversarial_ranking_exact_and_prefix_ahead_of_substrings() {
        let catalog = vec![
            // Substring candidates (bucket 3)
            mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
            mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
            mock_ticker("CBTCUSDT", Exchange::BinanceSpot),
            // Suffix candidate (bucket 2)
            mock_ticker("LTC-BTC", Exchange::OkexSpot),
            // Prefix candidates (bucket 1)
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceSpot),
            // Exact candidate (bucket 0)
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            // Non-matches
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.selected_exchange_filter = None;

        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        // Verification 1: Non-matches excluded
        assert!(
            !modal
                .filtered_tickers
                .iter()
                .any(|t| t.ticker.as_str() == "ETHUSDT")
        );
        assert!(
            !modal
                .filtered_tickers
                .iter()
                .any(|t| t.ticker.as_str() == "SOLUSDT")
        );

        // Verification 2: Exact match is at index 0
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::HyperliquidLinear
        );

        // Verification 3: All exact and prefix matches rank ahead of any substring/suffix matches
        let is_exact_or_prefix = |sym: &str| sym == "BTC" || sym.starts_with("BTC");
        let last_prefix_idx = modal
            .filtered_tickers
            .iter()
            .rposition(|t| is_exact_or_prefix(t.ticker.as_str()))
            .expect("Must have prefix matches");

        let first_substring_idx = modal
            .filtered_tickers
            .iter()
            .position(|t| !is_exact_or_prefix(t.ticker.as_str()));

        if let Some(sub_idx) = first_substring_idx {
            assert!(
                last_prefix_idx < sub_idx,
                "All exact and prefix matches (last at {last_prefix_idx}) must rank ahead of substrings (first at {sub_idx})"
            );
        }
    }

    #[test]
    fn test_adversarial_multi_token_base_symbols_cross_exchange() {
        let catalog = vec![
            // ETH
            mock_ticker("ETH", Exchange::HyperliquidLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BybitLinear),
            mock_ticker("ETH-USDT-SWAP", Exchange::OkexLinear),
            // SOL
            mock_ticker("SOL", Exchange::HyperliquidLinear),
            mock_ticker("SOLUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
            mock_ticker("SOL-USDT-SWAP", Exchange::OkexLinear),
            // HYPE
            mock_ticker("HYPE", Exchange::HyperliquidLinear),
            mock_ticker("HYPEUSDT", Exchange::BybitLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);

        for base_symbol in ["ETH", "SOL", "HYPE"] {
            modal.update(Message::TickerSearchChanged(base_symbol.into()), None);

            let hl_match = modal.filtered_tickers.iter().any(|t| {
                ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Hyperliquid
                    && t.ticker.as_str() == base_symbol
            });
            let cross_exchange_matches = modal
                .filtered_tickers
                .iter()
                .filter(|t| {
                    ExchangeInclusive::of(t.exchange()) != ExchangeInclusive::Hyperliquid
                        && t.ticker.as_str().starts_with(base_symbol)
                })
                .count();

            assert!(hl_match, "Hyperliquid {base_symbol} must be present");
            assert!(
                cross_exchange_matches >= 1,
                "Cross-exchange suffixed pairs for {base_symbol} must not be purged by Hyperliquid exact match"
            );
        }
    }

    #[test]
    fn test_adversarial_whitespace_and_case_preservation() {
        let catalog = vec![
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);

        for query in ["  btc  ", "bTc", " \tBTC\r\n ", "btc   "] {
            modal.update(Message::TickerSearchChanged(query.into()), None);
            assert_eq!(
                modal.filtered_tickers.len(),
                4,
                "Query '{query}' must preserve all 4 candidates regardless of whitespace or casing"
            );
            assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
            assert_eq!(
                modal.filtered_tickers[0].exchange(),
                Exchange::HyperliquidLinear
            );
        }
    }

    #[test]
    fn test_adversarial_exchange_filter_cross_exchange_isolation() {
        let catalog = vec![
            mock_ticker("BTC", Exchange::HyperliquidLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BybitLinear),
            mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        // Filter Binance
        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Binance)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 1);
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::BinanceLinear
        );

        // Filter Hyperliquid
        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Hyperliquid)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 1);
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::HyperliquidLinear
        );

        // Reset to All
        modal.update(Message::ExchangeFilterSelected(None), None);
        assert_eq!(modal.filtered_tickers.len(), 4);
    }

    #[test]
    fn test_regression_linear_perps_rank_ahead_of_shorter_inverse_perps() {
        let catalog = vec![
            mock_ticker("BTCUSD", Exchange::BybitInverse), // len 6, InversePerps
            mock_ticker("BTCUSDT", Exchange::BinanceLinear), // len 7, LinearPerps
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.selected_exchange_filter = None;

        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        assert_eq!(modal.filtered_tickers.len(), 2);
        assert_eq!(
            modal.filtered_tickers[0].ticker.as_str(),
            "BTCUSDT",
            "LinearPerps BTCUSDT (len 7) must rank strictly ahead of InversePerps BTCUSD (len 6)"
        );
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::BinanceLinear
        );
        assert_eq!(
            modal.filtered_tickers[0].market_type(),
            MarketKind::LinearPerps
        );

        assert_eq!(
            modal.filtered_tickers[1].ticker.as_str(),
            "BTCUSD",
            "InversePerps BTCUSD must rank after LinearPerps BTCUSDT"
        );
        assert_eq!(modal.filtered_tickers[1].exchange(), Exchange::BybitInverse);
        assert_eq!(
            modal.filtered_tickers[1].market_type(),
            MarketKind::InversePerps
        );
    }

    #[test]
    fn test_adversarial_enter_submission_selects_linear_over_inverse() {
        let catalog = vec![
            mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
            mock_ticker("BTCUSD", Exchange::BybitInverse),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::TickerSearchChanged("BTC".into()), None);
        modal.update(Message::TickerSearchSubmitted, None);

        let selected = modal.selected_ticker.expect("Must have selected a ticker");
        assert_eq!(
            selected.ticker.as_str(),
            "BTCUSDT",
            "Enter submission must select LinearPerps BTCUSDT, not InversePerps BTCUSD"
        );
        assert_eq!(selected.exchange(), Exchange::BinanceLinear);
        assert_eq!(selected.market_type(), MarketKind::LinearPerps);
    }

    #[test]
    fn test_adversarial_market_kind_three_tier_hierarchy() {
        let catalog = vec![
            mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse), // Inverse, len 11
            mock_ticker("BTCUSD", Exchange::BybitInverse),        // Inverse, len 6
            mock_ticker("BTCUSDT", Exchange::BinanceSpot),        // Spot, len 7
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),      // Linear, len 7
            mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear), // Linear, len 14
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        assert_eq!(modal.filtered_tickers.len(), 5);

        // Tier 1: Linear Perps (sorted by length: 7 < 14)
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTCUSDT");
        assert_eq!(
            modal.filtered_tickers[0].market_type(),
            MarketKind::LinearPerps
        );

        assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSDT_241227");
        assert_eq!(
            modal.filtered_tickers[1].market_type(),
            MarketKind::LinearPerps
        );

        // Tier 2: Spot
        assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "BTCUSDT");
        assert_eq!(modal.filtered_tickers[2].market_type(), MarketKind::Spot);

        // Tier 3: Inverse Perps (sorted by length: 6 < 11)
        assert_eq!(modal.filtered_tickers[3].ticker.as_str(), "BTCUSD");
        assert_eq!(
            modal.filtered_tickers[3].market_type(),
            MarketKind::InversePerps
        );

        assert_eq!(modal.filtered_tickers[4].ticker.as_str(), "BTCUSD_PERP");
        assert_eq!(
            modal.filtered_tickers[4].market_type(),
            MarketKind::InversePerps
        );
    }

    #[test]
    fn test_adversarial_linear_tie_breaking_by_length_and_symbol() {
        let catalog = vec![
            mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear), // len 14
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),        // len 7
            mock_ticker("BTCUSDC", Exchange::BinanceLinear),        // len 7 (alphabetical tiebreak)
        ];

        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::TickerSearchChanged("BTC".into()), None);

        assert_eq!(modal.filtered_tickers.len(), 3);
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTCUSDC");
        assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSDT");
        assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "BTCUSDT_241227");
    }

    #[test]
    fn test_regression_active_pane_inherited_on_modal_open_with_populated_catalog() {
        let active_pane_ticker = mock_ticker("ETHUSDT", Exchange::BinanceLinear);
        let catalog = vec![
            mock_ticker("1000BONKUSDT", Exchange::BinanceLinear), // Catalog head
            mock_ticker("AAVEUSDT", Exchange::BinanceLinear),
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
        ];

        let mut modal = VolumeSearchModal::new(None, Vec::new());

        // Correct order: inherit active pane before populating catalog
        modal.inherit_active_pane_ticker(Some(active_pane_ticker));
        modal.update_available_tickers(catalog);

        assert_eq!(
            modal.selected_ticker,
            Some(active_pane_ticker),
            "Active chart pane ticker (ETHUSDT) must be inherited and NOT overwritten by catalog head (1000BONKUSDT)"
        );
    }

    #[test]
    fn test_adversarial_active_pane_fallback_to_catalog_first_when_zero_panes() {
        let mut modal = VolumeSearchModal::new(None, Vec::new());
        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        ];

        // Zero open panes: active_pane_ticker is None
        modal.inherit_active_pane_ticker(None);
        modal.update_available_tickers(catalog.clone());

        assert_eq!(
            modal.selected_ticker,
            Some(catalog[0]),
            "When zero panes are open, selected_ticker must fall back to the first catalog candidate"
        );
    }

    #[test]
    fn test_adversarial_catalog_arrives_after_modal_opened() {
        let active_pane_ticker = mock_ticker("SOLUSDT", Exchange::BybitLinear);
        let mut modal = VolumeSearchModal::new(None, Vec::new());

        // Modal opened while catalog is still downloading over network
        modal.inherit_active_pane_ticker(Some(active_pane_ticker));
        assert_eq!(modal.selected_ticker, Some(active_pane_ticker));

        // Network response arrives later with 500 items
        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("SOLUSDT", Exchange::BybitLinear),
        ];
        modal.update_available_tickers(catalog);

        assert_eq!(
            modal.selected_ticker,
            Some(active_pane_ticker),
            "Subsequent catalog arrival must not overwrite already inherited active pane selection"
        );
    }

    #[test]
    fn test_adversarial_explicit_user_selection_immune_to_subsequent_pane_or_catalog_events() {
        let active_pane_ticker = mock_ticker("ETHUSDT", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(None, Vec::new());
        modal.inherit_active_pane_ticker(Some(active_pane_ticker));

        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
            mock_ticker("DOGEUSDT", Exchange::BinanceLinear),
        ];
        modal.update_available_tickers(catalog.clone());

        // User explicitly selects DOGEUSDT
        let doge = mock_ticker("DOGEUSDT", Exchange::BinanceLinear);
        modal.update(Message::TickerSelected(doge), None);
        assert_eq!(modal.selected_ticker, Some(doge));

        // Re-triggering active pane inheritance or catalog refresh must NOT override user choice
        modal.inherit_active_pane_ticker(Some(active_pane_ticker));
        modal.update_available_tickers(catalog);

        assert_eq!(
            modal.selected_ticker,
            Some(doge),
            "Explicit user selection must remain immune to subsequent background sync events"
        );
    }

    #[test]
    fn test_adversarial_active_pane_unlisted_ticker_safe() {
        let unlisted_ticker = mock_ticker("EXOTIC-PAIR", Exchange::OkexLinear);
        let mut modal = VolumeSearchModal::new(None, Vec::new());

        modal.inherit_active_pane_ticker(Some(unlisted_ticker));

        let catalog = vec![
            mock_ticker("BTCUSDT", Exchange::BinanceLinear),
            mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        ];
        modal.update_available_tickers(catalog);

        assert_eq!(
            modal.selected_ticker,
            Some(unlisted_ticker),
            "Modal must safely preserve unlisted active pane ticker without panicking"
        );
    }

    // ========================================================================
    // Milestone 2 (R2): Calendar Dates & Custom Range Unit Test Suite
    // ========================================================================

    #[test]
    fn test_m2_preset_selection_auto_populates_date_inputs() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        let today = chrono::Utc::now().date_naive();

        // 1. Initial state (Last3d default)
        assert_eq!(modal.preset, VolumeSearchPreset::Last3d);
        let expected_3d_start = today
            .checked_sub_signed(chrono::Duration::days(2))
            .unwrap_or(today);
        assert_eq!(modal.start_date, expected_3d_start);
        assert_eq!(modal.end_date, today);
        assert_eq!(
            modal.start_date_input,
            expected_3d_start.format("%Y-%m-%d").to_string()
        );
        assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
        assert!(modal.is_start_date_valid);
        assert!(modal.is_end_date_valid);

        // 2. Select Last24h
        modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Last24h);
        let expected_24h_start = today.pred_opt().unwrap_or(today);
        assert_eq!(modal.start_date, expected_24h_start);
        assert_eq!(modal.end_date, today);
        assert_eq!(
            modal.start_date_input,
            expected_24h_start.format("%Y-%m-%d").to_string()
        );
        assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
        assert!(modal.is_start_date_valid);
        assert!(modal.is_end_date_valid);

        // 3. Select Last7d
        modal.update(Message::PresetSelected(VolumeSearchPreset::Last7d), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Last7d);
        let expected_7d_start = today
            .checked_sub_signed(chrono::Duration::days(6))
            .unwrap_or(today);
        assert_eq!(modal.start_date, expected_7d_start);
        assert_eq!(modal.end_date, today);
        assert_eq!(
            modal.start_date_input,
            expected_7d_start.format("%Y-%m-%d").to_string()
        );
        assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
        assert!(modal.is_start_date_valid);
        assert!(modal.is_end_date_valid);

        // 4. Select Custom (preserves existing dates)
        modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);
        assert_eq!(modal.start_date, expected_7d_start);
        assert_eq!(modal.end_date, today);
    }

    #[test]
    fn test_m2_preset_selection_preserves_intraday_time_filter() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // User customizes intraday time filters
        modal.update(Message::TimeFilterStartChanged("08:30".into()), None);
        modal.update(Message::TimeFilterEndChanged("17:45".into()), None);
        assert_eq!(modal.time_filter_start, "08:30");
        assert_eq!(modal.time_filter_end, "17:45");

        // Switching presets must preserve time filters intact
        modal.update(Message::PresetSelected(VolumeSearchPreset::Last7d), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Last7d);
        assert_eq!(modal.time_filter_start, "08:30");
        assert_eq!(modal.time_filter_end, "17:45");

        modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Last24h);
        assert_eq!(modal.time_filter_start, "08:30");
        assert_eq!(modal.time_filter_end, "17:45");

        modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);
        assert_eq!(modal.time_filter_start, "08:30");
        assert_eq!(modal.time_filter_end, "17:45");
    }

    #[test]
    fn test_m2_manual_input_switches_preset_to_custom() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        assert_eq!(modal.preset, VolumeSearchPreset::Last3d);

        // Editing start date input automatically switches preset to Custom
        modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        assert_eq!(modal.start_date_input, "2026-10-01");
        assert!(modal.is_start_date_valid);

        // Switch back to Last24h
        modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Last24h);

        // Editing end date input automatically switches preset to Custom
        modal.update(Message::EndDateInputChanged("2026-10-08".into()), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()
        );
        assert_eq!(modal.end_date_input, "2026-10-08");
        assert!(modal.is_end_date_valid);
    }

    #[test]
    fn test_m2_manual_input_validation_and_boundary_clamping() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // Invalid date string
        modal.update(Message::StartDateInputChanged("2026-13-45".into()), None);
        assert_eq!(modal.start_date_input, "2026-13-45");
        assert!(!modal.is_start_date_valid);
        assert!(!modal.is_date_range_valid());

        // Valid start date
        modal.update(Message::StartDateInputChanged("2026-10-05".into()), None);
        assert!(modal.is_start_date_valid);
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
        );

        // Set valid end date
        modal.update(Message::EndDateInputChanged("2026-10-10".into()), None);
        assert!(modal.is_end_date_valid);
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
        );
        assert!(modal.is_date_range_valid());

        // Boundary clamping: if user sets start_date > end_date, end_date clamps to start_date
        modal.update(Message::StartDateInputChanged("2026-10-15".into()), None);
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
        );
        assert_eq!(modal.end_date_input, "2026-10-15");
        assert!(modal.is_date_range_valid());

        // Boundary clamping: if user sets end_date < start_date, start_date clamps to end_date
        modal.update(Message::EndDateInputChanged("2026-10-02".into()), None);
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()
        );
        assert_eq!(modal.start_date_input, "2026-10-02");
        assert!(modal.is_date_range_valid());
    }

    #[test]
    fn test_m2_calendar_date_selection_auto_clamps_inverted_range() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // Open start calendar
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

        // Select start date 2026-10-05
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()),
            None,
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
        );
        assert_eq!(modal.start_date_input, "2026-10-05");
        assert_eq!(modal.active_calendar, None); // Popover dismissed
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);

        // Open end calendar and pick 2026-10-12
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()),
            None,
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        );
        assert_eq!(modal.end_date_input, "2026-10-12");

        // Pick start date later than end date (2026-10-20 > 2026-10-12)
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
            None,
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        );
        // Auto-clamped end_date to start_date
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        );
        assert_eq!(modal.end_date_input, "2026-10-20");

        // Pick end date earlier than start date (2026-10-10 < 2026-10-20)
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()),
            None,
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
        );
        // Auto-clamped start_date to end_date
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
        );
        assert_eq!(modal.start_date_input, "2026-10-10");
    }

    #[test]
    fn test_m2_calendar_navigation_and_jump_to_today() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        let today = chrono::Utc::now().date_naive();

        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.cal_view_year, modal.start_date.year());
        assert_eq!(modal.cal_view_month, modal.start_date.month());

        // Next month
        let cur_m = modal.cal_view_month;
        modal.update(Message::CalNextMonth, None);
        let expected_next_m = if cur_m == 12 { 1 } else { cur_m + 1 };
        assert_eq!(modal.cal_view_month, expected_next_m);

        // Prev month
        modal.update(Message::CalPrevMonth, None);
        assert_eq!(modal.cal_view_month, cur_m);

        // Next year
        let cur_y = modal.cal_view_year;
        modal.update(Message::CalNextYear, None);
        assert_eq!(modal.cal_view_year, cur_y + 1);

        // Prev year
        modal.update(Message::CalPrevYear, None);
        assert_eq!(modal.cal_view_year, cur_y);

        // Jump to today
        modal.cal_view_year = 2030;
        modal.cal_view_month = 6;
        modal.update(Message::CalJumpToToday, None);
        assert_eq!(modal.cal_view_year, today.year());
        assert_eq!(modal.cal_view_month, today.month());

        // Close calendar
        modal.update(Message::CloseCalendar, None);
        assert_eq!(modal.active_calendar, None);
    }

    #[test]
    fn test_m2_calendar_popover_toggling_and_view_sync() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        let d1 = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();
        modal.update(
            Message::StartDateInputChanged(d1.format("%Y-%m-%d").to_string()),
            None,
        );
        modal.update(
            Message::EndDateInputChanged(d2.format("%Y-%m-%d").to_string()),
            None,
        );

        // Toggle Start calendar opens with start_date coordinates
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
        assert_eq!(modal.cal_view_year, 2025);
        assert_eq!(modal.cal_view_month, 3);

        // Toggling Start again closes it
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, None);

        // Toggle End calendar opens with end_date coordinates
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::End));
        assert_eq!(modal.cal_view_year, 2025);
        assert_eq!(modal.cal_view_month, 7);
    }

    #[test]
    fn test_m2_parse_date_valid_and_invalid_formats() {
        // Valid
        assert_eq!(
            parse_date_ymd("2026-10-11"),
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );
        assert_eq!(
            parse_date_ymd("2024-01-01"),
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
        );
        assert_eq!(
            parse_date_ymd("   2026-10-11   "),
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );

        // Invalid formats & lengths
        assert_eq!(parse_date_ymd(""), None);
        assert_eq!(parse_date_ymd("2026/10/11"), None);
        assert_eq!(parse_date_ymd("2026.10.11"), None);
        assert_eq!(parse_date_ymd("2026-1-1"), None);
        assert_eq!(parse_date_ymd("invalid"), None);

        // Out of bounds months & days
        assert_eq!(parse_date_ymd("2026-00-10"), None);
        assert_eq!(parse_date_ymd("2026-13-10"), None);
        assert_eq!(parse_date_ymd("2026-05-00"), None);
        assert_eq!(parse_date_ymd("2026-04-31"), None); // April has 30 days
        assert_eq!(parse_date_ymd("2026-02-30"), None);
    }

    #[test]
    fn test_m2_leap_years_and_days_in_month() {
        // Standard leap years
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2020));
        assert!(is_leap_year(2016));
        // Non-leap years
        assert!(!is_leap_year(2023));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(2026));
        // Century boundaries
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2100));

        // Days in February
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);

        // All 12 months in 2026
        let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (idx, &expected) in month_days.iter().enumerate() {
            assert_eq!(days_in_month(2026, (idx + 1) as u32), expected);
        }

        // Parsing Feb 29
        assert_eq!(
            parse_date_ymd("2024-02-29"),
            Some(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(parse_date_ymd("2025-02-29"), None);
    }

    #[test]
    fn test_m2_month_and_year_boundary_navigation_rollovers() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

        // Wrap from January to December of previous year
        modal.cal_view_year = 2026;
        modal.cal_view_month = 1;
        modal.update(Message::CalPrevMonth, None);
        assert_eq!(modal.cal_view_year, 2025);
        assert_eq!(modal.cal_view_month, 12);

        // Wrap from December to January of next year
        modal.update(Message::CalNextMonth, None);
        assert_eq!(modal.cal_view_year, 2026);
        assert_eq!(modal.cal_view_month, 1);

        // Year lower clamp (2000)
        modal.cal_view_year = 2000;
        modal.update(Message::CalPrevYear, None);
        assert_eq!(modal.cal_view_year, 2000);

        // Year upper clamp (2099)
        modal.cal_view_year = 2099;
        modal.update(Message::CalNextYear, None);
        assert_eq!(modal.cal_view_year, 2099);
    }

    #[test]
    fn test_m2_calculate_query_range_standard_intraday() {
        use chrono::Timelike;
        let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let (s_ms, e_ms, s_min, e_min) =
            calculate_custom_query_range(start, end, "09:30", "16:00", now_ms);

        assert_eq!(s_min, 570); // 9 * 60 + 30
        assert_eq!(e_min, 960); // 16 * 60
        assert!(s_ms < e_ms);

        let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
        let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

        assert_eq!(s_dt.date_naive(), start);
        assert_eq!(s_dt.hour(), 9);
        assert_eq!(s_dt.minute(), 30);
        assert_eq!(s_dt.second(), 0);

        assert_eq!(e_dt.date_naive(), end);
        assert_eq!(e_dt.hour(), 16);
        assert_eq!(e_dt.minute(), 0);
        assert_eq!(e_dt.second(), 59);
    }

    #[test]
    fn test_m2_calculate_query_range_same_day_overnight_zero_panic() {
        use chrono::Timelike;
        // Critical edge case: Single-day range with cross-midnight filter (22:00 -> 04:00)
        let day = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let (s_ms, e_ms, s_min, e_min) =
            calculate_custom_query_range(day, day, "22:00", "04:00", now_ms);

        assert_eq!(s_min, 1320); // 22:00
        assert_eq!(e_min, 240); // 04:00
        // Monotonicity invariant: s_ms <= e_ms MUST hold, preventing BTreeMap::range panic!
        assert!(
            s_ms <= e_ms,
            "start_ms must never exceed end_ms on single-day cross-midnight ranges"
        );

        let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
        let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

        assert_eq!(s_dt.date_naive(), day);
        assert_eq!(s_dt.hour(), 0);
        assert_eq!(e_dt.date_naive(), day);
        assert_eq!(e_dt.hour(), 23);
    }

    #[test]
    fn test_m2_calculate_query_range_multi_day_overnight() {
        use chrono::Timelike;
        let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let (s_ms, e_ms, s_min, e_min) =
            calculate_custom_query_range(start, end, "22:00", "04:00", now_ms);

        assert_eq!(s_min, 1320);
        assert_eq!(e_min, 240);
        assert!(s_ms < e_ms);

        let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
        let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

        assert_eq!(s_dt.date_naive(), start);
        assert_eq!(s_dt.hour(), 22);
        assert_eq!(e_dt.date_naive(), end);
        assert_eq!(e_dt.hour(), 4);
    }

    #[test]
    fn test_m2_calculate_query_range_inversion_and_future_clamping() {
        let future_end = NaiveDate::from_ymd_opt(2029, 1, 1).unwrap();
        let future_start = NaiveDate::from_ymd_opt(2029, 1, 10).unwrap(); // start > end
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let now_ms = today
            .and_hms_opt(15, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        let (s_ms, e_ms, _, _) =
            calculate_custom_query_range(future_start, future_end, "00:00", "23:59", now_ms);

        assert!(s_ms <= e_ms);
        assert!(e_ms <= now_ms, "end_ms must be clamped to now_ms");

        let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();
        assert_eq!(e_dt.date_naive(), today, "Future date must clamp to today");
    }

    #[test]
    fn test_m2_target_dates_contiguous_sequence() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
        modal.update(Message::EndDateInputChanged("2026-10-05".into()), None);

        let dates = modal.target_dates();
        assert_eq!(dates.len(), 5);
        assert_eq!(dates[0], NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
        assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 10, 2).unwrap());
        assert_eq!(dates[2], NaiveDate::from_ymd_opt(2026, 10, 3).unwrap());
        assert_eq!(dates[3], NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
        assert_eq!(dates[4], NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());

        // Single day
        modal.update(Message::EndDateInputChanged("2026-10-01".into()), None);
        let single = modal.target_dates();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0], NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());

        // Invalid date returns empty vector
        modal.update(Message::StartDateInputChanged("invalid".into()), None);
        assert!(modal.target_dates().is_empty());
    }

    #[test]
    fn test_m2_perform_search_custom_preset_monotonic_timestamps() {
        let mock_coin = mock_ticker("NONEXISTENT_COIN", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
        modal.update(Message::EndDateInputChanged("2026-10-03".into()), None);
        modal.update(Message::TimeFilterStartChanged("10:00".into()), None);
        modal.update(Message::TimeFilterEndChanged("15:00".into()), None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);

        // Perform search without panes (verifies zero panics and has_searched flag)
        let action = modal.update(Message::ApplySearch, None);
        if action.is_some() {
            modal.update(Message::CancelSearch, None);
        }
        assert!(modal.has_searched);
        assert!(modal.rows.is_empty());
    }

    // ========================================================================
    // Milestone 2 (R2) Challenger 2 Empirical Adversarial Stress Test Suite
    // ========================================================================

    #[test]
    fn test_c2_adversarial_preset_switching_state_machine_matrix() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        let today = chrono::Utc::now().date_naive();

        let presets = [
            VolumeSearchPreset::Last24h,
            VolumeSearchPreset::Last3d,
            VolumeSearchPreset::Last7d,
            VolumeSearchPreset::Custom,
            VolumeSearchPreset::AllLoaded,
        ];

        // Custom user state to verify preservation
        modal.update(Message::TimeFilterStartChanged("07:15".into()), None);
        modal.update(Message::TimeFilterEndChanged("21:45".into()), None);
        modal.update(Message::MultiplierSelected(TickMultiplier(50)), None);
        modal.update(Message::MinVolumeChanged("1250.5".into()), None);

        for &p1 in &presets {
            for &p2 in &presets {
                // Open calendar popover before transition
                modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
                assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

                modal.update(Message::PresetSelected(p1), None);
                assert_eq!(modal.preset, p1);
                // All presets must dismiss open calendar popover
                assert_eq!(modal.active_calendar, None);

                // Verify user state preserved
                assert_eq!(modal.time_filter_start, "07:15");
                assert_eq!(modal.time_filter_end, "21:45");
                assert_eq!(modal.selected_multiplier, TickMultiplier(50));
                assert_eq!(modal.min_volume_input, "1250.5");

                // Switch to p2
                modal.update(Message::PresetSelected(p2), None);
                assert_eq!(modal.preset, p2);
                assert_eq!(modal.active_calendar, None);

                match p2 {
                    VolumeSearchPreset::Last24h => {
                        let expected = today.pred_opt().unwrap_or(today);
                        assert_eq!(modal.start_date, expected);
                        assert_eq!(modal.end_date, today);
                        assert!(modal.is_start_date_valid);
                        assert!(modal.is_end_date_valid);
                    }
                    VolumeSearchPreset::Last3d => {
                        let expected = today
                            .checked_sub_signed(chrono::Duration::days(2))
                            .unwrap_or(today);
                        assert_eq!(modal.start_date, expected);
                        assert_eq!(modal.end_date, today);
                    }
                    VolumeSearchPreset::Last7d => {
                        let expected = today
                            .checked_sub_signed(chrono::Duration::days(6))
                            .unwrap_or(today);
                        assert_eq!(modal.start_date, expected);
                        assert_eq!(modal.end_date, today);
                    }
                    VolumeSearchPreset::Custom | VolumeSearchPreset::AllLoaded => {}
                }
            }
        }

        // PresetSelected(Custom) dismisses active_calendar if one was open
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
        modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
        assert_eq!(
            modal.active_calendar, None,
            "PresetSelected(Custom) dismisses active_calendar"
        );
    }

    #[test]
    fn test_c2_adversarial_popover_backdrop_click_absorption_and_stack_view() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // 1. Initially no popover
        assert_eq!(modal.active_calendar, None);
        let _ = modal.view(); // base view must render without panic

        // 2. Open Start calendar popover
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
        let _ = modal.view(); // stack view with popover must render without panic

        // 3. Toggle Start calendar again -> closes
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, None);

        // 4. Open End calendar popover
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::End));
        let _ = modal.view();

        // 5. Switching directly to Start calendar popover
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

        // 6. Backdrop click dispatches CloseCalendar
        modal.update(Message::CloseCalendar, None);
        assert_eq!(modal.active_calendar, None);

        // 7. CalSelectDate closes popover and switches to Custom
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
            None,
        );
        assert_eq!(modal.active_calendar, None);
        assert_eq!(modal.preset, VolumeSearchPreset::Custom);
    }

    #[test]
    fn test_c2_adversarial_zero_panic_invariants_on_malformed_inputs() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // Inject extreme/malformed inputs
        modal.update(Message::StartDateInputChanged("not-a-date".into()), None);
        modal.update(Message::EndDateInputChanged("9999-99-99".into()), None);
        modal.update(Message::TimeFilterStartChanged("99:99".into()), None);
        modal.update(Message::TimeFilterEndChanged("abc:def".into()), None);
        modal.update(Message::MinVolumeChanged("NaN".into()), None);
        modal.update(Message::CustomMultiplierChanged("999999".into()), None);

        assert!(!modal.is_start_date_valid);
        assert!(!modal.is_end_date_valid);

        // ApplySearch must sanitize invalid date strings back to last valid dates and execute without panic
        let action = modal.update(Message::ApplySearch, None);
        if action.is_some() {
            modal.update(Message::CancelSearch, None);
        }
        assert!(modal.has_searched);
        assert!(modal.is_start_date_valid);
        assert!(modal.is_end_date_valid);

        // Repeat with empty strings
        modal.update(Message::StartDateInputChanged("".into()), None);
        modal.update(Message::EndDateInputChanged("".into()), None);
        modal.update(Message::TimeFilterStartChanged("".into()), None);
        modal.update(Message::TimeFilterEndChanged("".into()), None);
        modal.update(Message::MinVolumeChanged("-500.0".into()), None);
        let action2 = modal.update(Message::ApplySearch, None);
        if action2.is_some() {
            modal.update(Message::CancelSearch, None);
        }
        assert!(modal.has_searched);

        // Repeat with selected_ticker = None
        modal.selected_ticker = None;
        modal.update(Message::ApplySearch, None);

        // Repeat with 100 random message dispatches in sequence
        for i in 0..100 {
            match i % 6 {
                0 => {
                    modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
                }
                1 => {
                    modal.update(
                        Message::StartDateInputChanged(format!("2026-02-{:02}", (i % 28) + 1)),
                        None,
                    );
                }
                2 => {
                    modal.update(
                        Message::TimeFilterStartChanged(format!("{:02}:00", i % 24)),
                        None,
                    );
                }
                3 => {
                    modal.update(Message::ApplySearch, None);
                }
                4 => {
                    modal.update(Message::CalPrevMonth, None);
                }
                _ => {
                    modal.update(Message::CalNextYear, None);
                }
            }
        }
        assert!(modal.has_searched);
    }

    #[test]
    fn test_c2_adversarial_monotonic_range_future_intraday_exceeds_now_ms() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let now_ms = today
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        // User sets today with intraday filter 18:00 - 20:00 (in the future relative to now_ms = 12:00)
        let (s_ms, e_ms, s_min, e_min) =
            calculate_custom_query_range(today, today, "18:00", "20:00", now_ms);

        assert_eq!(s_min, 1080);
        assert_eq!(e_min, 1200);

        // 1. Monotonicity: s_ms <= e_ms holds (preventing BTreeMap range panic)
        assert!(s_ms <= e_ms, "start_ms must never exceed end_ms");

        // 2. Bound invariant: e_ms <= now_ms
        // Worker claimed: "guaranteeing monotonic timestamps (start_time <= end_time <= now_ms)"
        // Docstring claimed: "Guarantees start_ms <= end_ms <= now_ms"
        // In reality, e_ms exceeds now_ms by 6 hours because start_dt (18:00) was never clamped to now_ms,
        // and when start_ms > end_ms, swapping sets final_end_ms = 18:00 (future)!
        assert!(
            e_ms <= now_ms,
            "BUG DETECTED: calculate_custom_query_range returned end_time ({e_ms}) > now_ms ({now_ms}). Difference: {} ms in future!",
            e_ms.saturating_sub(now_ms)
        );
    }

    // ========================================================================
    // Milestone 2 (R2) Challenger 1 Empirical Adversarial Test Suite

    // ========================================================================

    #[test]
    fn test_c1_adversarial_leap_years_and_century_rules() {
        // 1. Gregorian leap year rules across multiple centuries
        assert!(is_leap_year(1600), "1600 divisible by 400 must be leap");
        assert!(
            !is_leap_year(1700),
            "1700 divisible by 100 but not 400 must NOT be leap"
        );
        assert!(
            !is_leap_year(1800),
            "1800 divisible by 100 but not 400 must NOT be leap"
        );
        assert!(
            !is_leap_year(1900),
            "1900 divisible by 100 but not 400 must NOT be leap"
        );
        assert!(is_leap_year(2000), "2000 divisible by 400 must be leap");
        assert!(is_leap_year(2004), "2004 divisible by 4 must be leap");
        assert!(is_leap_year(2020), "2020 divisible by 4 must be leap");
        assert!(is_leap_year(2024), "2024 divisible by 4 must be leap");
        assert!(
            !is_leap_year(2025),
            "2025 not divisible by 4 must NOT be leap"
        );
        assert!(
            !is_leap_year(2026),
            "2026 not divisible by 4 must NOT be leap"
        );
        assert!(
            !is_leap_year(2100),
            "2100 divisible by 100 but not 400 must NOT be leap"
        );
        assert!(is_leap_year(2400), "2400 divisible by 400 must be leap");

        // 2. February day counts matching leap year rules
        assert_eq!(days_in_month(1600, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2400, 2), 29);

        // 3. Parsing Feb 29 across leap vs non-leap years
        assert!(parse_date_ymd("2000-02-29").is_some());
        assert!(parse_date_ymd("2024-02-29").is_some());
        assert!(parse_date_ymd("1900-02-29").is_none());
        assert!(parse_date_ymd("2025-02-29").is_none());
        assert!(parse_date_ymd("2026-02-29").is_none());
        assert!(parse_date_ymd("2100-02-29").is_none());

        // 4. Parity with chrono NaiveDate across all years in 1990..=2110
        for y in 1990..=2110 {
            let chrono_leap = NaiveDate::from_ymd_opt(y, 2, 29).is_some();
            assert_eq!(
                is_leap_year(y),
                chrono_leap,
                "Mismatch with chrono for year {y}"
            );
            assert_eq!(
                days_in_month(y, 2),
                if chrono_leap { 29 } else { 28 },
                "Mismatch in Feb days for year {y}"
            );
        }
    }

    #[test]
    fn test_c1_adversarial_month_transition_calendar_navigation() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

        // 1. Full 24-month forward cycle: verify year/month transitions
        modal.cal_view_year = 2024;
        modal.cal_view_month = 1;
        for i in 1..=24 {
            modal.update(Message::CalNextMonth, None);
            let expected_month = (i % 12) + 1;
            let expected_year = 2024 + (i / 12);
            assert_eq!(modal.cal_view_month, expected_month as u32);
            assert_eq!(modal.cal_view_year, expected_year);
        }

        // 2. Full 24-month backward cycle
        for i in (1..=24).rev() {
            modal.update(Message::CalPrevMonth, None);
            let expected_month = ((i - 1) % 12) + 1;
            let expected_year = 2024 + ((i - 1) / 12);
            assert_eq!(modal.cal_view_month, expected_month as u32);
            assert_eq!(modal.cal_view_year, expected_year);
        }

        // 3. Boundary navigation behavior at minimum year (2000-01)
        modal.cal_view_year = 2000;
        modal.cal_view_month = 1;
        modal.update(Message::CalPrevMonth, None);
        // cal_view_year and cal_view_month remain clamped at minimum boundary (2000-01)
        assert_eq!(modal.cal_view_year, 2000);
        assert_eq!(modal.cal_view_month, 1);

        // 4. Boundary navigation behavior at maximum year (2099-12)
        modal.cal_view_year = 2099;
        modal.cal_view_month = 12;
        modal.update(Message::CalNextMonth, None);
        // cal_view_year and cal_view_month remain clamped at maximum boundary (2099-12)
        assert_eq!(modal.cal_view_year, 2099);
        assert_eq!(modal.cal_view_month, 12);
    }

    #[test]
    fn test_c1_adversarial_inverted_date_ranges_and_boundary_clamping() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // 1. CalSelectDate: picking start date > end date auto-clamps end date to start date
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()),
            None,
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
        );

        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
            None,
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        );
        assert!(modal.is_date_range_valid());

        // 2. CalSelectDate: picking end date < start date auto-clamps start date to end date
        modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
        modal.update(
            Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()),
            None,
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
        );
        assert!(modal.is_date_range_valid());

        // 3. Text inputs: changing start > end clamps end
        modal.update(Message::StartDateInputChanged("2026-10-25".into()), None);
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
        );
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
        );
        assert_eq!(modal.end_date_input, "2026-10-25");
        assert!(modal.is_date_range_valid());

        // 4. Text inputs: changing end < start clamps start
        modal.update(Message::EndDateInputChanged("2026-10-05".into()), None);
        assert_eq!(
            modal.end_date,
            NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
        );
        assert_eq!(
            modal.start_date,
            NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
        );
        assert_eq!(modal.start_date_input, "2026-10-05");
        assert!(modal.is_date_range_valid());

        // 5. Inverted input in calculate_custom_query_range: start_date > end_date normalized
        let now_ms = 1800000000000_u64;
        let (s_ms, e_ms, _, _) = calculate_custom_query_range(
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
            "00:00",
            "23:59",
            now_ms,
        );
        assert!(
            s_ms <= e_ms,
            "calculate_custom_query_range must normalize start > end"
        );
    }

    #[test]
    fn test_c1_adversarial_malformed_and_boundary_date_strings() {
        let malformed = [
            "",
            "   ",
            "\t\n",
            "2026",
            "2026-1",
            "2026-10",
            "2026-10-1",
            "2026/10/11",
            "2026.10.11",
            "2026_10_11",
            "2026:10:11",
            "11-10-2026",
            "10-11-2026",
            "2026-00-15",
            "2026-13-15",
            "2026-05-00",
            "2026-05-32",
            "2026-04-31", // April has 30 days
            "2026-06-31", // June has 30 days
            "2026-09-31", // Sept has 30 days
            "2026-11-31", // Nov has 30 days
            "2026-02-30", // Feb never has 30 days
            "2025-02-29", // Non-leap year Feb 29
            "2100-02-29", // Century non-leap Feb 29
            "2026-10-11a",
            "a2026-10-11",
            "2026-10-11\0",
            "99999-10-11",
            "двадцять",
            "２０２６-１０-１１", // Full-width unicode
            "2026—10—11",         // Em-dash
        ];

        for &s in &malformed {
            assert!(
                parse_date_ymd(s).is_none(),
                "parse_date_ymd should return None for malformed input: {s:?}"
            );
        }

        // Empirical discovery: chrono's %Y allows signed negative years formatted with minus sign
        // "-001-10-11" has 10 characters and parses as proleptic Gregorian year -1 (1 BC)
        let parsed_neg = parse_date_ymd("-001-10-11");
        assert_eq!(
            parsed_neg,
            Some(NaiveDate::from_ymd_opt(-1, 10, 11).unwrap())
        );

        // Valid formats with whitespace trimming
        assert_eq!(
            parse_date_ymd("2026-10-11"),
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );
        assert_eq!(
            parse_date_ymd("  2026-10-11  "),
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );
        assert_eq!(
            parse_date_ymd("\t2026-10-11\n"),
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );
    }

    #[test]
    fn test_c1_adversarial_asymmetric_invalid_input_inverted_range_bug() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        assert!(modal.is_date_range_valid());

        // Step 1: User types an invalid string into End Date ("invalid")
        modal.update(Message::EndDateInputChanged("invalid".into()), None);
        assert!(!modal.is_end_date_valid);
        assert!(!modal.is_date_range_valid());

        // Step 2: User sets Start Date to a date after the existing end_date (the default end
        // date is today, so use tomorrow to keep the test independent of the current date)
        let after_end = (modal.end_date + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        modal.update(Message::StartDateInputChanged(after_end.clone()), None);
        assert!(modal.is_start_date_valid);

        // Step 3: User hits ApplySearch (or presses Enter in search)
        modal.update(Message::ApplySearch, None);

        // ApplySearch restored end_date_input to the previous end_date, marked it valid,
        // and normalized end_date to start_date since start_date > end_date.
        assert!(modal.is_start_date_valid);
        assert!(modal.is_end_date_valid);

        // is_date_range_valid() is TRUE after normalization:
        assert!(
            modal.is_date_range_valid(),
            "Date range is valid after ApplySearch normalization"
        );
        assert_eq!(modal.start_date, modal.end_date);
        assert_eq!(modal.end_date_input, after_end);
        // And target_dates() returns non-empty slice:
        assert_eq!(
            modal.target_dates(),
            vec![modal.start_date],
            "target_dates() must not be empty"
        );
    }

    #[test]
    fn test_c1_adversarial_year_boundary_month_rollover_anomaly() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);
        modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

        // 1. Boundary clamping at year 2000, month 1 (January 2000)
        modal.cal_view_year = 2000;
        modal.cal_view_month = 1;
        modal.update(Message::CalPrevMonth, None);
        assert_eq!(modal.cal_view_year, 2000);
        assert_eq!(
            modal.cal_view_month, 1,
            "Jan 2000 CalPrevMonth clamps to Jan 2000"
        );

        // 2. Boundary clamping at year 2099, month 12 (December 2099)
        modal.cal_view_year = 2099;
        modal.cal_view_month = 12;
        modal.update(Message::CalNextMonth, None);
        assert_eq!(modal.cal_view_year, 2099);
        assert_eq!(
            modal.cal_view_month, 12,
            "Dec 2099 CalNextMonth clamps to Dec 2099"
        );
    }

    #[test]
    fn test_c1_adversarial_cross_midnight_and_temporal_fuzzing() {
        use chrono::Timelike;

        let base_date = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let now_ms = NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        // 1. Single-day cross-midnight: 23:30 to 04:30
        let (s_ms, e_ms, s_min, e_min) =
            calculate_custom_query_range(base_date, base_date, "23:30", "04:30", now_ms);
        assert_eq!(s_min, 23 * 60 + 30);
        assert_eq!(e_min, 4 * 60 + 30);
        assert!(
            s_ms <= e_ms,
            "Single-day cross-midnight must satisfy s_ms <= e_ms"
        );

        let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
        let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();
        assert_eq!(s_dt.date_naive(), base_date);
        assert_eq!(s_dt.hour(), 0);
        assert_eq!(e_dt.date_naive(), base_date);
        assert_eq!(e_dt.hour(), 23);

        // 2. Multi-day cross-midnight: 23:00 to 05:00 over 3 days
        let end_date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let (s2_ms, e2_ms, s2_min, e2_min) =
            calculate_custom_query_range(base_date, end_date, "23:00", "05:00", now_ms);
        assert_eq!(s2_min, 23 * 60);
        assert_eq!(e2_min, 5 * 60);
        assert!(s2_ms < e2_ms);

        let s2_dt = chrono::DateTime::from_timestamp_millis(s2_ms as i64).unwrap();
        let e2_dt = chrono::DateTime::from_timestamp_millis(e2_ms as i64).unwrap();
        assert_eq!(s2_dt.date_naive(), base_date);
        assert_eq!(s2_dt.hour(), 23);
        assert_eq!(e2_dt.date_naive(), end_date);
        assert_eq!(e2_dt.hour(), 5);

        // 3. Property oracle fuzzing: 2,000 randomized permutations
        let test_dates = [
            NaiveDate::from_ymd_opt(2024, 2, 28).unwrap(),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(),
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 11).unwrap(),
            NaiveDate::from_ymd_opt(2029, 6, 15).unwrap(),
        ];
        let test_times = [
            "00:00", "04:30", "09:15", "12:00", "16:45", "22:00", "23:59", "invalid", "99:99", "",
        ];
        let test_nows = [
            0_u64,
            1000000_u64,
            1728648000000_u64, // 2024-10-11
            1791720000000_u64, // 2026-10-11
            2000000000000_u64,
        ];

        let mut btree_map = std::collections::BTreeMap::new();
        btree_map.insert(1791720000000_u64, "candle");

        for d1 in &test_dates {
            for d2 in &test_dates {
                for t1 in &test_times {
                    for t2 in &test_times {
                        for now in &test_nows {
                            let (start, end, _, _) =
                                calculate_custom_query_range(*d1, *d2, t1, t2, *now);
                            assert!(
                                start <= end,
                                "Monotonic violation: start {start} > end {end} for d1={d1:?}, d2={d2:?}, t1={t1:?}, t2={t2:?}"
                            );
                            // Verify zero-panic in BTreeMap::range
                            let _ = btree_map.range(start..=end).count();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_c1_adversarial_target_dates_integrity() {
        let catalog = sample_catalog();
        let mut modal = VolumeSearchModal::new(None, catalog);

        // 1. Month boundary transition: 2026-01-30 to 2026-02-03 (5 days)
        modal.update(Message::StartDateInputChanged("2026-01-30".into()), None);
        modal.update(Message::EndDateInputChanged("2026-02-03".into()), None);
        let dates = modal.target_dates();
        assert_eq!(dates.len(), 5);
        assert_eq!(dates[0], NaiveDate::from_ymd_opt(2026, 1, 30).unwrap());
        assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
        assert_eq!(dates[2], NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(dates[3], NaiveDate::from_ymd_opt(2026, 2, 2).unwrap());
        assert_eq!(dates[4], NaiveDate::from_ymd_opt(2026, 2, 3).unwrap());

        // 2. Leap year transition: 2024-02-27 to 2024-03-02 (5 days, includes Feb 29)
        modal.update(Message::StartDateInputChanged("2024-02-27".into()), None);
        modal.update(Message::EndDateInputChanged("2024-03-02".into()), None);
        let leap_dates = modal.target_dates();
        assert_eq!(leap_dates.len(), 5);
        assert_eq!(leap_dates[1], NaiveDate::from_ymd_opt(2024, 2, 28).unwrap());
        assert_eq!(leap_dates[2], NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
        assert_eq!(leap_dates[3], NaiveDate::from_ymd_opt(2024, 3, 1).unwrap());

        // 3. Non-leap year transition: 2025-02-27 to 2025-03-02 (4 days, skips Feb 29)
        modal.update(Message::StartDateInputChanged("2025-02-27".into()), None);
        modal.update(Message::EndDateInputChanged("2025-03-02".into()), None);
        let non_leap_dates = modal.target_dates();
        assert_eq!(non_leap_dates.len(), 4);
        assert_eq!(
            non_leap_dates[1],
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
        assert_eq!(
            non_leap_dates[2],
            NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()
        );

        // 4. Year boundary transition: 2025-12-30 to 2026-01-02 (4 days)
        modal.update(Message::StartDateInputChanged("2025-12-30".into()), None);
        modal.update(Message::EndDateInputChanged("2026-01-02".into()), None);
        let year_dates = modal.target_dates();
        assert_eq!(year_dates.len(), 4);
        assert_eq!(
            year_dates[0],
            NaiveDate::from_ymd_opt(2025, 12, 30).unwrap()
        );
        assert_eq!(
            year_dates[1],
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()
        );
        assert_eq!(year_dates[2], NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(year_dates[3], NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
    }

    // ========================================================================
    // Milestone 3 (R3) Tiered Caching, Download Chaining & Progress Tests
    // ========================================================================

    #[test]
    fn test_m3_exchange_data_folder_mapping_integrity() {
        assert_eq!(exchange_data_folder(Exchange::BinanceSpot), "binance");
        assert_eq!(exchange_data_folder(Exchange::BinanceLinear), "binance");
        assert_eq!(exchange_data_folder(Exchange::BinanceInverse), "binance");
        assert_eq!(exchange_data_folder(Exchange::BybitSpot), "bybit");
        assert_eq!(exchange_data_folder(Exchange::BybitLinear), "bybit");
        assert_eq!(exchange_data_folder(Exchange::BybitInverse), "bybit");
        assert_eq!(exchange_data_folder(Exchange::OkexSpot), "okex");
        assert_eq!(exchange_data_folder(Exchange::OkexLinear), "okex");
        assert_eq!(exchange_data_folder(Exchange::OkexInverse), "okex");
        assert_eq!(
            exchange_data_folder(Exchange::HyperliquidSpot),
            "hyperliquid"
        );
        assert_eq!(
            exchange_data_folder(Exchange::HyperliquidLinear),
            "hyperliquid"
        );
    }

    #[test]
    fn test_m3_missing_cache_initiates_download_task_with_banner() {
        let mock_coin = mock_ticker("M3_MOCK_COIN", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
        modal.update(Message::EndDateInputChanged("2026-03-03".into()), None);
        assert_eq!(modal.target_dates().len(), 3);

        let action = modal.update(Message::ApplySearch, None);
        assert!(matches!(action, Some(Action::Task(_))));
        assert!(modal.is_downloading);
        assert!(modal.download_progress.is_some());
        let banner = modal.download_progress.as_ref().unwrap();
        assert!(banner.contains("Завантаження з біржі: день 1 з 3"));
        assert!(banner.contains("2026-03-01"));
        assert_eq!(modal.download_missing_dates.len(), 3);
        assert_eq!(modal.current_download_index, 0);
    }

    #[test]
    fn test_m3_day_download_chaining_and_final_search() {
        let mock_coin = mock_ticker("M3_CHAIN_COIN", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
        modal.update(Message::EndDateInputChanged("2026-03-02".into()), None);

        // Step 1: Initiate search
        let action1 = modal.update(Message::ApplySearch, None);
        assert!(matches!(action1, Some(Action::Task(_))));
        assert_eq!(modal.current_download_index, 0);

        // Construct mock candle datapoint for Day 1
        let day1 = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let day1_ts = day1
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;
        let mut mock_dp1 = KlineDataPoint {
            kline: exchange::Kline {
                time: day1_ts,
                open: exchange::util::Price::from_f32(100.0),
                high: exchange::util::Price::from_f32(105.0),
                low: exchange::util::Price::from_f32(95.0),
                close: exchange::util::Price::from_f32(102.0),
                volume: (5000.0, 5000.0),
            },
            footprint: data::chart::kline::KlineTrades::new(),
            trades_fetched: true,
        };
        mock_dp1.footprint.add_trade_to_nearest_bin(
            &exchange::Trade {
                time: day1_ts,
                price: exchange::util::Price::from_f32(100.0),
                qty: 10_000.0,
                is_sell: false,
            },
            PriceStep::from_f32(0.01),
        );

        // Step 2: Deliver Day 1 download completion -> dispatches Day 2 task
        let action2 = modal.update(
            Message::DayDownloadCompleted(Ok((day1, vec![(day1_ts, mock_dp1)]))),
            None,
        );
        assert!(matches!(action2, Some(Action::Task(_))));
        assert!(modal.is_downloading);
        assert_eq!(modal.current_download_index, 1);
        let banner = modal.download_progress.as_ref().unwrap();
        assert!(banner.contains("Завантаження з біржі: день 2 з 2"));
        assert!(banner.contains("2026-03-02"));

        // Step 3: Deliver Day 2 download completion (empty) -> terminates download & finalizes search
        let day2 = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let action3 = modal.update(Message::DayDownloadCompleted(Ok((day2, Vec::new()))), None);
        assert!(action3.is_none());
        assert!(!modal.is_downloading);
        assert!(modal.download_progress.is_none());
        assert!(modal.has_searched);
        assert_eq!(modal.rows.len(), 1);
        assert_eq!(modal.rows[0].max_volume, 10_000.0);
        assert_eq!(modal.summary.count, 1);
    }

    #[test]
    fn test_m3_cancel_search_lifecycle_and_partial_results() {
        let mock_coin = mock_ticker("M3_CANCEL_COIN", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
        modal.update(Message::EndDateInputChanged("2026-03-03".into()), None);

        // Initiate search (3 days)
        let _ = modal.update(Message::ApplySearch, None);
        assert!(modal.is_downloading);

        // Deliver Day 1
        let day1 = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let day1_ts = day1
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;
        let mut mock_dp1 = KlineDataPoint {
            kline: exchange::Kline {
                time: day1_ts,
                open: exchange::util::Price::from_f32(50.0),
                high: exchange::util::Price::from_f32(55.0),
                low: exchange::util::Price::from_f32(48.0),
                close: exchange::util::Price::from_f32(52.0),
                volume: (2500.0, 2500.0),
            },
            footprint: data::chart::kline::KlineTrades::new(),
            trades_fetched: true,
        };
        mock_dp1.footprint.add_trade_to_nearest_bin(
            &exchange::Trade {
                time: day1_ts,
                price: exchange::util::Price::from_f32(50.0),
                qty: 5000.0,
                is_sell: true,
            },
            PriceStep::from_f32(0.01),
        );
        let _ = modal.update(
            Message::DayDownloadCompleted(Ok((day1, vec![(day1_ts, mock_dp1)]))),
            None,
        );

        // User clicks Cancel while waiting for Day 2
        let cancel_action = modal.update(Message::CancelSearch, None);
        assert!(cancel_action.is_none());
        assert!(!modal.is_downloading);
        assert!(modal.download_progress.is_none());
        assert!(modal.has_searched);
        // Partial results from Day 1 are preserved and computed into search rows
        assert_eq!(modal.rows.len(), 1);
        assert_eq!(modal.rows[0].max_volume, 5000.0);
    }

    #[test]
    fn test_m3_all_loaded_preset_without_panes_completes_cleanly() {
        let mock_coin = mock_ticker("M3_ALL_LOADED", Exchange::BinanceLinear);
        let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

        modal.update(Message::PresetSelected(VolumeSearchPreset::AllLoaded), None);
        let action = modal.update(Message::ApplySearch, None);
        assert!(action.is_none());
        assert!(modal.has_searched);
        assert!(!modal.is_downloading);
        assert!(modal.download_progress.is_none());
    }
}
