use super::Message;
use crate::style;
use data::config::timezone::TimeLabelKind;
pub use data::panel::timeandsales::Config;
use data::panel::timeandsales::{HistAgg, StackedBar, StackedBarRatio, TapeTrade};
use exchange::liquidation::{Liquidation, LiquidationEvent};
use exchange::{TickerInfo, Trade, volume_size_unit};

use iced::widget::canvas::{self, Text};
use iced::{Alignment, Event, Point, Rectangle, Renderer, Size, Theme, mouse};
use std::collections::VecDeque;
use std::time::Instant;

const TEXT_SIZE: iced::Pixels = iced::Pixels(11.0);
const METRICS_HEIGHT_COMPACT: f32 = 8.0;
const METRICS_HEIGHT_FULL: f32 = 18.0;
const TRADE_ROW_HEIGHT: f32 = 14.0;

fn format_filter_value(value: f32) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        data::util::format_with_commas(value)
    }
}

/// Parse filter input string to f32
pub fn parse_filter_input(input: &str) -> Option<f32> {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    if cleaned.is_empty() {
        return Some(0.0);
    }

    let value: f32 = cleaned.parse().ok()?;
    if value.is_finite() && value >= 0.0 {
        Some(value)
    } else {
        None
    }
}

impl super::Panel for TimeAndSales {
    fn scroll(&mut self, delta: f32) {
        self.scroll_offset -= delta;

        let stacked_bar_h = self.stacked_bar_height();
        let total_content_height =
            (self.recent_trades.len() as f32 * TRADE_ROW_HEIGHT) + stacked_bar_h;
        let max_scroll_offset = (total_content_height - TRADE_ROW_HEIGHT).max(0.0);

        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll_offset);

        if self.scroll_offset > stacked_bar_h + TRADE_ROW_HEIGHT {
            self.is_paused = true;
        } else if self.is_paused {
            self.is_paused = false;

            for trade in self.paused_trades_buffer.iter() {
                self.hist_agg.add(&trade.trade);
            }

            self.recent_trades
                .extend(self.paused_trades_buffer.drain(..));

            self.prune_by_time(None);
        }

        self.invalidate(Some(Instant::now()));
    }

    fn reset_scroll(&mut self) {
        self.scroll_offset = 0.0;
        self.is_paused = false;

        for trade in self.paused_trades_buffer.iter() {
            self.hist_agg.add(&trade.trade);
        }

        self.recent_trades
            .extend(self.paused_trades_buffer.drain(..));

        self.prune_by_time(None);

        self.invalidate(Some(Instant::now()));
    }

    fn invalidate(&mut self, now: Option<Instant>) -> Option<super::Action> {
        self.invalidate(now)
    }

    fn is_empty(&self) -> bool {
        self.recent_trades.is_empty() && self.paused_trades_buffer.is_empty()
    }

    fn view<'a>(&'a self, timezone: data::UserTimezone) -> iced::Element<'a, Message> {
        let tape = iced::widget::canvas(TimeAndSalesView {
            panel: self,
            timezone,
        })
        .height(iced::Length::Fill)
        .width(iced::Length::Fill);

        if self.config.show_liquidations {
            iced::widget::column![self.liquidations_view(timezone), tape].into()
        } else {
            tape.into()
        }
    }
}

/// Liquidations kept per panel; the strip shows the newest few.
const LIQUIDATION_HISTORY: usize = 200;
const LIQUIDATION_ROWS: usize = 6;

pub struct TimeAndSales {
    recent_trades: VecDeque<TapeTrade>,
    paused_trades_buffer: VecDeque<TapeTrade>,
    hist_agg: HistAgg,
    is_paused: bool,
    max_filtered_qty: f32,
    pub ticker_info: TickerInfo,
    pub tickers: Vec<TickerInfo>,
    pub config: Config,
    cache: canvas::Cache,
    last_tick: Instant,
    scroll_offset: f32,
    pub trade_size_input: String,
    /// Newest first: real liquidations from the exchange stream (see `exchange::liquidation`).
    liquidations: VecDeque<(TickerInfo, Liquidation)>,
    /// Last connection notice of the liquidation stream, shown in the strip header.
    liquidation_status: Option<String>,
}

impl TimeAndSales {
    pub fn new(config: Option<Config>, ticker_info: TickerInfo) -> Self {
        let cfg = config.unwrap_or_default();
        let trade_size_input = format_filter_value(cfg.trade_size_filter);
        let mut tickers = vec![ticker_info];
        for add_ti in &cfg.additional_tickers {
            if !tickers.iter().any(|t| t.ticker == add_ti.ticker) {
                tickers.push(*add_ti);
            }
        }

        Self {
            recent_trades: VecDeque::new(),
            paused_trades_buffer: VecDeque::new(),
            hist_agg: HistAgg::default(),
            is_paused: false,
            config: cfg,
            max_filtered_qty: 0.0,
            ticker_info,
            tickers,
            cache: canvas::Cache::default(),
            last_tick: Instant::now(),
            scroll_offset: 0.0,
            trade_size_input,
            liquidations: VecDeque::new(),
            liquidation_status: None,
        }
    }

    /// Whether this panel wants the liquidation stream of `ti`.
    pub fn wants_liquidations_for(&self, ti: &TickerInfo) -> bool {
        self.config.show_liquidations && self.matches_ticker(ti)
    }

    pub fn on_liquidation_event(&mut self, ticker_info: TickerInfo, event: LiquidationEvent) {
        match event {
            LiquidationEvent::Connected => self.liquidation_status = None,
            LiquidationEvent::Disconnected(reason) => self.liquidation_status = Some(reason),
            LiquidationEvent::Received(liqs) => {
                for liq in liqs {
                    self.liquidations.push_front((ticker_info, liq));
                }
                self.liquidations.truncate(LIQUIDATION_HISTORY);
            }
        }
    }

    fn liquidations_view<'a>(&'a self, timezone: data::UserTimezone) -> iced::Element<'a, Message> {
        use iced::widget::{column, container, text};

        let min_usd = f64::from(self.config.liquidation_min_usd);
        let supported = self
            .tickers
            .iter()
            .any(|t| exchange::liquidation::is_supported(t.exchange()));
        let header = match (&self.liquidation_status, supported) {
            (_, false) => "Liquidations: only Binance and Bybit perps".to_string(),
            (Some(reason), true) => format!("Liquidations: {reason}"),
            (None, true) if min_usd > 0.0 => format!(
                "Liquidations ≥ ${}",
                data::util::format_with_commas(self.config.liquidation_min_usd)
            ),
            (None, true) => "Liquidations".to_string(),
        };

        let mut rows = column![text(header).size(11)].spacing(2);
        let shown = self
            .liquidations
            .iter()
            .filter(|(_, l)| l.notional_usd >= min_usd)
            .take(LIQUIDATION_ROWS);
        for (ti, liq) in shown {
            let when = timezone
                .format_with_kind(liq.time as i64, TimeLabelKind::Custom("%H:%M:%S"))
                .unwrap_or_default();
            let (side, is_long) = if liq.long_liquidated {
                ("LONG", true)
            } else {
                ("SHORT", false)
            };
            let label = format!(
                "{when}  {side} liq  {}  {}  ${}",
                ti.ticker,
                data::util::format_with_commas(liq.price),
                data::util::format_with_commas(liq.notional_usd as f32),
            );
            rows = rows.push(text(label).size(11).style(move |theme: &Theme| {
                let palette = theme.extended_palette();
                iced::widget::text::Style {
                    // A long liquidation is a forced sell; color it like a sell.
                    color: Some(if is_long {
                        palette.danger.base.color
                    } else {
                        palette.success.base.color
                    }),
                }
            }));
        }

        container(rows)
            .padding(iced::padding::left(6).right(6).top(4).bottom(4))
            .width(iced::Length::Fill)
            .into()
    }

    pub fn tickers(&self) -> &[TickerInfo] {
        &self.tickers
    }

    pub fn matches_ticker(&self, ti: &TickerInfo) -> bool {
        self.tickers.iter().any(|t| t.ticker == ti.ticker)
    }

    pub fn insert_buffer(&mut self, trades_buffer: &[Trade]) {
        let ti = self.ticker_info;
        self.insert_buffer_tagged(&ti, trades_buffer);
    }

    pub fn insert_buffer_tagged(&mut self, ti: &TickerInfo, trades_buffer: &[Trade]) {
        let ticker_idx = match self.tickers.iter().position(|t| t.ticker == ti.ticker) {
            Some(idx) => idx as u8,
            None => {
                let idx = self.tickers.len() as u8;
                self.tickers.push(*ti);
                idx
            }
        };

        let size_filter = self.config.trade_size_filter;
        let market_type = ti.market_type();
        let size_in_quote_ccy = volume_size_unit() == exchange::SizeUnit::Quote;

        for &trade in trades_buffer {
            let trade_size_value =
                market_type.qty_in_quote_value(trade.qty, trade.price, size_in_quote_ccy);

            if trade_size_value >= size_filter {
                self.max_filtered_qty = self.max_filtered_qty.max(trade.qty);
            }

            let tape_trade = TapeTrade {
                trade,
                ticker_index: ticker_idx,
            };

            let target_trades = if self.is_paused {
                &mut self.paused_trades_buffer
            } else {
                &mut self.recent_trades
            };

            // Chronological merge: if empty or newer than last element, append O(1),
            // else binary search O(log N) insertion point to preserve temporal monotonicity.
            if target_trades.is_empty()
                || target_trades
                    .back()
                    .is_none_or(|last| last.trade.time <= trade.time)
            {
                target_trades.push_back(tape_trade);
            } else {
                let insert_idx = target_trades.partition_point(|t| t.trade.time <= trade.time);
                target_trades.insert(insert_idx, tape_trade);
            }

            if !self.is_paused {
                self.hist_agg.add(&trade);
            }
        }

        if !self.is_paused {
            self.prune_by_time(None);
        }
        self.prune_paused_by_time(None);
    }

    pub fn last_update(&self) -> Instant {
        self.last_tick
    }

    pub fn invalidate(&mut self, now: Option<Instant>) -> Option<super::Action> {
        if !self.is_paused {
            self.prune_by_time(None);
        }
        self.prune_paused_by_time(None);

        self.cache.clear();
        if let Some(now) = now {
            self.last_tick = now;
        }
        None
    }

    /// Get reference to size filter input state for settings modal
    pub fn size_filter_input(&self) -> &str {
        &self.trade_size_input
    }

    /// Update trade size filter input and apply if valid
    pub fn set_trade_size_input(&mut self, input: String) {
        self.trade_size_input = input;
        if let Some(value) = parse_filter_input(&self.trade_size_input) {
            self.config.trade_size_filter = value;
            self.invalidate(Some(Instant::now()));
        }
    }

    fn stacked_bar_height(&self) -> f32 {
        match &self.config.stacked_bar {
            Some(StackedBar::Compact(_)) => METRICS_HEIGHT_COMPACT,
            Some(StackedBar::Full(_)) => METRICS_HEIGHT_FULL,
            None => 0.0,
        }
    }

    fn pause_overlay_height(&self) -> f32 {
        self.stacked_bar_height().max(METRICS_HEIGHT_COMPACT) + TRADE_ROW_HEIGHT
    }

    fn prune_by_time(&mut self, now_epoch_ms: Option<u64>) {
        if self.recent_trades.is_empty() {
            return;
        }

        let now_ms = now_epoch_ms.unwrap_or_else(|| {
            let ts = chrono::Utc::now().timestamp_millis();
            if ts < 0 { 0 } else { ts as u64 }
        });

        let trade_retention_ms = self.config.trade_retention.as_millis() as u64;
        let prune_slack_ms = trade_retention_ms / 10;

        let low_cutoff = now_ms.saturating_sub(trade_retention_ms);
        let high_cutoff = now_ms.saturating_sub(trade_retention_ms.saturating_add(prune_slack_ms));

        if let Some(oldest) = self.recent_trades.front() {
            if oldest.trade.time >= high_cutoff {
                return;
            }
        } else {
            return;
        }

        let size_filter = self.config.trade_size_filter;

        let mut popped_any = false;
        while let Some(front) = self.recent_trades.front() {
            if front.trade.time >= low_cutoff {
                break;
            }
            let old = self.recent_trades.pop_front().unwrap();
            self.hist_agg.remove(&old.trade);
            popped_any = true;
        }

        if popped_any {
            let size_in_quote_ccy = volume_size_unit() == exchange::SizeUnit::Quote;

            self.max_filtered_qty = self
                .recent_trades
                .iter()
                .filter(|t| {
                    let ti = self
                        .tickers
                        .get(t.ticker_index as usize)
                        .unwrap_or(&self.ticker_info);
                    let trade_size = ti.market_type().qty_in_quote_value(
                        t.trade.qty,
                        t.trade.price,
                        size_in_quote_ccy,
                    );
                    trade_size >= size_filter
                })
                .map(|e| e.trade.qty)
                .fold(0.0, f32::max);

            let filtered_count = self
                .recent_trades
                .iter()
                .filter(|t| {
                    let ti = self
                        .tickers
                        .get(t.ticker_index as usize)
                        .unwrap_or(&self.ticker_info);
                    let trade_size = ti.market_type().qty_in_quote_value(
                        t.trade.qty,
                        t.trade.price,
                        size_in_quote_ccy,
                    );
                    trade_size >= size_filter
                })
                .count();

            let stacked_bar_h = self.stacked_bar_height();
            let total_content_height = (filtered_count as f32 * TRADE_ROW_HEIGHT) + stacked_bar_h;
            let max_scroll_offset = (total_content_height - TRADE_ROW_HEIGHT).max(0.0);
            self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll_offset);
        }
    }

    fn prune_paused_by_time(&mut self, now_epoch_ms: Option<u64>) {
        if self.paused_trades_buffer.is_empty() {
            return;
        }

        let trade_retention_ms = self.config.trade_retention.as_millis() as u64;
        let prune_slack_ms = trade_retention_ms / 10;

        let now_ms = now_epoch_ms.unwrap_or_else(|| {
            let ts = chrono::Utc::now().timestamp_millis();
            if ts < 0 { 0 } else { ts as u64 }
        });

        let low_cutoff = now_ms.saturating_sub(trade_retention_ms);
        let high_cutoff = now_ms.saturating_sub(trade_retention_ms.saturating_add(prune_slack_ms));

        if let Some(oldest) = self.paused_trades_buffer.front() {
            if oldest.trade.time >= high_cutoff {
                return;
            }
        } else {
            return;
        }

        while let Some(front) = self.paused_trades_buffer.front() {
            if front.trade.time >= low_cutoff {
                break;
            }
            self.paused_trades_buffer.pop_front();
        }
    }
}

/// Canvas program view for TimeAndSales supporting UserTimezone formatting and Multi-Ticker differentiation
pub struct TimeAndSalesView<'a> {
    pub panel: &'a TimeAndSales,
    pub timezone: data::UserTimezone,
}

impl<'a> canvas::Program<Message> for TimeAndSalesView<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        bounds: iced::Rectangle,
        cursor: iced_core::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let cursor_position = cursor.position_in(bounds)?;

        match event {
            Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(button) => match button {
                    mouse::Button::Middle => {
                        Some(canvas::Action::publish(Message::ResetScroll).and_capture())
                    }
                    mouse::Button::Left => {
                        let paused_box_height = self.panel.pause_overlay_height();
                        let paused_box = Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: bounds.width,
                            height: paused_box_height,
                        };

                        if self.panel.is_paused && paused_box.contains(cursor_position) {
                            Some(canvas::Action::publish(Message::ResetScroll).and_capture())
                        } else {
                            None
                        }
                    }
                    _ => None,
                },
                mouse::Event::WheelScrolled { delta } => {
                    let scroll_amount = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y * TRADE_ROW_HEIGHT * 3.0,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };

                    Some(canvas::Action::publish(Message::Scrolled(scroll_amount)).and_capture())
                }
                mouse::Event::CursorMoved { .. } => {
                    if self.panel.is_paused {
                        let now = Some(Instant::now());
                        Some(canvas::Action::publish(Message::Invalidate(now)).and_capture())
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let palette = theme.extended_palette();
        let is_scroll_paused = self.panel.is_paused;
        let stacked_bar_h = self.panel.stacked_bar_height();

        let content = self.panel.cache.draw(renderer, bounds.size(), |frame| {
            let content_top_y = -self.panel.scroll_offset;

            if let Some(hist) = &self.panel.config.stacked_bar {
                let ratio_kind = match hist {
                    StackedBar::Compact(r) | StackedBar::Full(r) => *r,
                };

                if let Some((buy_val, sell_val, buy_ratio)) =
                    self.panel.hist_agg.values_for(ratio_kind)
                {
                    let draw_stacked_bar =
                        |frame: &mut canvas::Frame, buy_bar_width: f32, sell_bar_width: f32| {
                            frame.fill_rectangle(
                                Point {
                                    x: 0.0,
                                    y: content_top_y,
                                },
                                Size {
                                    width: buy_bar_width,
                                    height: stacked_bar_h,
                                },
                                palette.success.weak.color,
                            );

                            frame.fill_rectangle(
                                Point {
                                    x: buy_bar_width,
                                    y: content_top_y,
                                },
                                Size {
                                    width: sell_bar_width,
                                    height: stacked_bar_h,
                                },
                                palette.danger.weak.color,
                            );
                        };

                    let buy_bar_width = (bounds.width * buy_ratio).round();
                    let sell_bar_width = bounds.width - buy_bar_width;

                    draw_stacked_bar(frame, buy_bar_width, sell_bar_width);

                    if matches!(hist, StackedBar::Full(_)) {
                        let center_y = content_top_y + (stacked_bar_h / 2.0);

                        let buy_text_content = match ratio_kind {
                            StackedBarRatio::Count => format!("{}", buy_val as i64),
                            StackedBarRatio::AverageSize | StackedBarRatio::Volume => {
                                data::util::abbr_large_numbers(buy_val as f32)
                            }
                        };
                        let buy_text = Text {
                            content: buy_text_content,
                            position: Point {
                                x: 8.0,
                                y: center_y,
                            },
                            size: TEXT_SIZE,
                            font: style::AZERET_MONO,
                            color: palette.success.weak.text,
                            align_x: Alignment::Start.into(),
                            align_y: Alignment::Center.into(),
                            ..Default::default()
                        };
                        frame.fill_text(buy_text);

                        let sell_text_content = match ratio_kind {
                            StackedBarRatio::Count => format!("{}", sell_val as i64),
                            StackedBarRatio::AverageSize | StackedBarRatio::Volume => {
                                data::util::abbr_large_numbers(sell_val as f32)
                            }
                        };
                        let sell_text = Text {
                            content: sell_text_content,
                            position: Point {
                                x: bounds.width - 8.0,
                                y: center_y,
                            },
                            size: TEXT_SIZE,
                            font: style::AZERET_MONO,
                            color: palette.danger.weak.text,
                            align_x: Alignment::End.into(),
                            align_y: Alignment::Center.into(),
                            ..Default::default()
                        };
                        frame.fill_text(sell_text);
                    }
                }
            }

            // Feed
            let row_height = TRADE_ROW_HEIGHT;
            let row_width = bounds.width;
            let is_multi_ticker = self.panel.tickers.len() > 1;

            let size_in_quote_ccy = volume_size_unit() == exchange::SizeUnit::Quote;

            let filtered_trades: Vec<&TapeTrade> = self
                .panel
                .recent_trades
                .iter()
                .filter(|t| {
                    let ti = self
                        .panel
                        .tickers
                        .get(t.ticker_index as usize)
                        .unwrap_or(&self.panel.ticker_info);

                    if self.panel.config.disabled_tickers.contains(&ti.ticker) {
                        return false;
                    }

                    let trade_size = ti.market_type().qty_in_quote_value(
                        t.trade.qty,
                        t.trade.price,
                        size_in_quote_ccy,
                    );
                    trade_size >= self.panel.config.trade_size_filter
                })
                .collect();

            let max_qty = filtered_trades
                .iter()
                .map(|e| e.trade.qty)
                .fold(0.0, f32::max);

            let row_scroll_offset = (self.panel.scroll_offset - stacked_bar_h).max(0.0);
            let start_index = (row_scroll_offset / row_height).floor() as usize;
            let visible_rows = (bounds.height / row_height).ceil() as usize;

            let trades_to_draw = filtered_trades
                .into_iter()
                .rev()
                .skip(start_index)
                .take(visible_rows + 2);

            let create_text =
                |content: String, position: Point, align_x: Alignment, color: iced::Color| Text {
                    content,
                    position,
                    size: TEXT_SIZE,
                    font: style::AZERET_MONO,
                    color,
                    align_x: align_x.into(),
                    ..Default::default()
                };

            for (i, entry) in trades_to_draw.enumerate() {
                let trade = &entry.trade;
                let ticker_ti = self
                    .panel
                    .tickers
                    .get(entry.ticker_index as usize)
                    .unwrap_or(&self.panel.ticker_info);

                let y_position =
                    content_top_y + stacked_bar_h + ((start_index + i) as f32 * row_height);

                if y_position + row_height < 0.0 || y_position > bounds.height {
                    continue;
                }

                let (trade_color, bg_color) = if trade.is_sell {
                    (palette.danger.base.color, palette.danger.base.color)
                } else {
                    (palette.success.base.color, palette.success.base.color)
                };

                let relative_ratio = if max_qty > 0.0 {
                    (trade.qty / max_qty).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let bg_color_alpha = (0.04 + 0.30 * relative_ratio).min(0.40);

                let mut text_color = trade_color;
                let mut time_color = palette.background.strong.text.scale_alpha(0.70);

                if is_scroll_paused
                    && y_position
                        < (stacked_bar_h.max(METRICS_HEIGHT_COMPACT)) + (TRADE_ROW_HEIGHT * 0.8)
                {
                    text_color = text_color.scale_alpha(0.15);
                    time_color = time_color.scale_alpha(0.15);
                }

                frame.fill_rectangle(
                    Point {
                        x: 0.0,
                        y: y_position,
                    },
                    Size {
                        width: row_width,
                        height: row_height,
                    },
                    bg_color.scale_alpha(bg_color_alpha),
                );

                // Multi-ticker badge
                let (time_x, price_x, qty_x) = if is_multi_ticker {
                    let badge_label = match ticker_ti.exchange().venue() {
                        exchange::Venue::Binance => "BIN",
                        exchange::Venue::Bybit => "BYB",
                        exchange::Venue::Okex => "OKX",
                        exchange::Venue::Hyperliquid => "HL",
                        exchange::Venue::Mexc => "MEXC",
                        exchange::Venue::Coinbase => "CB",
                    };

                    let badge_color = match entry.ticker_index % 6 {
                        0 => iced::Color::from_rgb(0.25, 0.82, 0.95), // Cyan
                        1 => iced::Color::from_rgb(1.0, 0.62, 0.15),  // Orange
                        2 => iced::Color::from_rgb(0.80, 0.45, 1.0),  // Purple
                        3 => iced::Color::from_rgb(1.0, 0.88, 0.25),  // Yellow
                        4 => iced::Color::from_rgb(0.35, 0.90, 0.45), // Green
                        _ => iced::Color::from_rgb(1.0, 0.45, 0.70),  // Pink
                    };

                    let badge_text = create_text(
                        badge_label.to_string(),
                        Point {
                            x: 4.0,
                            y: y_position,
                        },
                        Alignment::Start,
                        badge_color,
                    );
                    frame.fill_text(badge_text);

                    (row_width * 0.18, row_width * 0.67, row_width * 0.94)
                } else {
                    (row_width * 0.10, row_width * 0.67, row_width * 0.90)
                };

                let formatted_time = self
                    .timezone
                    .format_with_kind(trade.time as i64, TimeLabelKind::Custom("%M:%S.%3f"))
                    .unwrap_or_else(|| "00:00.000".to_string());

                let trade_time = create_text(
                    formatted_time,
                    Point {
                        x: time_x,
                        y: y_position,
                    },
                    Alignment::Start,
                    time_color,
                );
                frame.fill_text(trade_time);

                let trade_price = create_text(
                    trade.price.to_string(ticker_ti.min_ticksize),
                    Point {
                        x: price_x,
                        y: y_position,
                    },
                    Alignment::End,
                    text_color,
                );
                frame.fill_text(trade_price);

                let trade_qty = create_text(
                    data::util::abbr_large_numbers(trade.qty),
                    Point {
                        x: qty_x,
                        y: y_position,
                    },
                    Alignment::End,
                    text_color,
                );
                frame.fill_text(trade_qty);
            }

            if is_scroll_paused {
                let pause_overlay_height = self.panel.pause_overlay_height();
                let pause_overlay_y = 0.0;

                let cursor_position = cursor.position_in(bounds);

                let paused_box = Rectangle {
                    x: 0.0,
                    y: pause_overlay_y,
                    width: frame.width(),
                    height: pause_overlay_height,
                };

                let bg_color = if let Some(cursor) = cursor_position {
                    if paused_box.contains(cursor) {
                        palette.background.strong.color
                    } else {
                        palette.background.weak.color
                    }
                } else {
                    palette.background.weak.color
                };

                frame.fill_rectangle(
                    Point {
                        x: 0.0,
                        y: pause_overlay_y,
                    },
                    Size {
                        width: frame.width(),
                        height: pause_overlay_height,
                    },
                    bg_color,
                );

                frame.fill_text(Text {
                    content: "Paused".to_string(),
                    position: Point {
                        x: frame.width() * 0.5,
                        y: pause_overlay_y + (pause_overlay_height / 2.0),
                    },
                    size: 12.0.into(),
                    font: style::AZERET_MONO,
                    color: palette.background.strong.text,
                    align_x: Alignment::Center.into(),
                    align_y: Alignment::Center.into(),
                    ..Default::default()
                });
            }
        });

        vec![content]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: iced::Rectangle,
        cursor: iced_core::mouse::Cursor,
    ) -> iced_core::mouse::Interaction {
        if self.panel.is_paused {
            let stacked_bar_h = self.panel.stacked_bar_height();
            let paused_box = Rectangle {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: stacked_bar_h.max(METRICS_HEIGHT_COMPACT) + TRADE_ROW_HEIGHT,
            };

            if cursor.is_over(paused_box) {
                return mouse::Interaction::Pointer;
            }
        }

        mouse::Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::Exchange;
    use exchange::util::Price;

    fn dummy_ticker_info(ticker_str: &str, ex: Exchange) -> TickerInfo {
        TickerInfo::new(exchange::Ticker::new(ticker_str, ex), 0.1, 0.001, None)
    }

    fn liq(time: u64, notional_usd: f64) -> Liquidation {
        Liquidation {
            time,
            price: 60_000.0,
            qty: (notional_usd / 60_000.0) as f32,
            notional_usd,
            long_liquidated: true,
        }
    }

    #[test]
    fn test_liquidations_reach_only_panels_that_asked_for_them() {
        let btc = dummy_ticker_info("BTCUSDT", Exchange::BinanceLinear);
        let eth = dummy_ticker_info("ETHUSDT", Exchange::BinanceLinear);

        let mut off = TimeAndSales::new(None, btc);
        assert!(!off.wants_liquidations_for(&btc), "off by default");

        off.config.show_liquidations = true;
        assert!(off.wants_liquidations_for(&btc));
        assert!(!off.wants_liquidations_for(&eth));
    }

    #[test]
    fn test_liquidation_events_keep_newest_first_and_track_status() {
        let btc = dummy_ticker_info("BTCUSDT", Exchange::BinanceLinear);
        let mut tas = TimeAndSales::new(None, btc);
        tas.config.show_liquidations = true;

        tas.on_liquidation_event(btc, LiquidationEvent::Disconnected("Read error".into()));
        assert_eq!(tas.liquidation_status.as_deref(), Some("Read error"));
        tas.on_liquidation_event(btc, LiquidationEvent::Connected);
        assert!(tas.liquidation_status.is_none());

        tas.on_liquidation_event(btc, LiquidationEvent::Received(vec![liq(1, 5_000.0)]));
        tas.on_liquidation_event(btc, LiquidationEvent::Received(vec![liq(2, 80_000.0)]));
        assert_eq!(tas.liquidations.front().unwrap().1.time, 2);

        for i in 0..(LIQUIDATION_HISTORY as u64 + 10) {
            tas.on_liquidation_event(btc, LiquidationEvent::Received(vec![liq(10 + i, 1.0)]));
        }
        assert_eq!(tas.liquidations.len(), LIQUIDATION_HISTORY);
    }

    #[test]
    fn test_ter16_timeandsales_zero_alloc_buffer_and_tz() {
        let ti = dummy_ticker_info("BTCUSDT", Exchange::BinanceLinear);
        let mut tas = TimeAndSales::new(None, ti);

        let now = chrono::Utc::now().timestamp_millis() as u64;
        let trade = Trade {
            time: now - 1000,
            is_sell: false,
            price: Price::from_f64(35000.0),
            qty: 1.5,
        };

        tas.insert_buffer(&[trade]);
        assert_eq!(tas.recent_trades.len(), 1);

        let tz = data::UserTimezone::default();
        let formatted = tz
            .format_with_kind(trade.time as i64, TimeLabelKind::Custom("%M:%S.%3f"))
            .expect("formatted tz");
        assert!(!formatted.is_empty());
    }

    #[test]
    fn test_ter20_multi_ticker_chronological_ordering() {
        let ti_bin = dummy_ticker_info("BTCUSDT", Exchange::BinanceLinear);
        let ti_byb = dummy_ticker_info("BTCUSDT", Exchange::BybitLinear);

        let mut tas = TimeAndSales::new(None, ti_bin);

        let now = chrono::Utc::now().timestamp_millis() as u64;
        let t1 = Trade {
            time: now - 3000,
            is_sell: false,
            price: Price::from_f64(30000.0),
            qty: 1.0,
        };
        let t3 = Trade {
            time: now - 1000,
            is_sell: true,
            price: Price::from_f64(30005.0),
            qty: 2.0,
        };
        let t2 = Trade {
            time: now - 2000,
            is_sell: false,
            price: Price::from_f64(30002.0),
            qty: 0.5,
        };

        tas.insert_buffer_tagged(&ti_bin, &[t1, t3]);
        tas.insert_buffer_tagged(&ti_byb, &[t2]);

        // Buffer must maintain chronological sort order: t1, t2, t3
        assert_eq!(tas.recent_trades.len(), 3);
        assert_eq!(tas.recent_trades[0].trade.time, now - 3000);
        assert_eq!(tas.recent_trades[0].ticker_index, 0);

        assert_eq!(tas.recent_trades[1].trade.time, now - 2000);
        assert_eq!(tas.recent_trades[1].ticker_index, 1);

        assert_eq!(tas.recent_trades[2].trade.time, now - 1000);
        assert_eq!(tas.recent_trades[2].ticker_index, 0);
    }
}
