use crate::style::{self, Icon, icon_text};
use data::journal::{JournalEntry, JournalStats, TradeSide, TradeStatus};
use iced::{
    Alignment, Element, Length, Size, Theme,
    widget::{button, column, container, row, scrollable, space, text, text_input},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum Message {
    ToggleJournal,
    SetStatusFilter(Option<TradeStatus>),
    ToggleAddForm,
    InputTicker(String),
    InputExchange(String),
    InputSide(TradeSide),
    InputEntryPrice(String),
    InputExitPrice(String),
    InputSize(String),
    InputFee(String),
    InputTag(String),
    InputNotes(String),
    SubmitTrade,
    CancelForm,
    DeleteTrade(Uuid),
    OpenClosePrompt(Uuid),
    InputClosingExitPrice(String),
    InputClosingFee(String),
    ConfirmCloseTrade,
    CancelClosePrompt,
    SelectTicker(String),
}

pub enum Action {
    TickerSelected(String),
}

pub struct Journal {
    pub entries: Vec<JournalEntry>,
    pub stats: JournalStats,
    pub is_shown: bool,
    pub filter_status: Option<TradeStatus>,
    pub show_add_form: bool,

    // Form inputs
    pub input_ticker: String,
    pub input_exchange: String,
    pub input_side: TradeSide,
    pub input_entry_price: String,
    pub input_exit_price: String,
    pub input_size: String,
    pub input_fee: String,
    pub input_tag: String,
    pub input_notes: String,

    // Quick close modal/prompt
    pub closing_id: Option<Uuid>,
    pub closing_exit_price: String,
    pub closing_fee: String,

    pub error_message: Option<String>,
}

impl Journal {
    pub fn new() -> Self {
        let entries = data::load_journal();
        let stats = JournalStats::compute(&entries);

        Self {
            entries,
            stats,
            is_shown: false,
            filter_status: None,
            show_add_form: false,

            input_ticker: String::new(),
            input_exchange: "Binance".to_string(),
            input_side: TradeSide::Long,
            input_entry_price: String::new(),
            input_exit_price: String::new(),
            input_size: String::new(),
            input_fee: "0".to_string(),
            input_tag: String::new(),
            input_notes: String::new(),

            closing_id: None,
            closing_exit_price: String::new(),
            closing_fee: "0".to_string(),

            error_message: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Option<Action> {
        match message {
            Message::ToggleJournal => {
                self.is_shown = !self.is_shown;
                self.error_message = None;
            }
            Message::SetStatusFilter(status) => {
                self.filter_status = status;
            }
            Message::ToggleAddForm => {
                self.show_add_form = !self.show_add_form;
                self.error_message = None;
            }
            Message::InputTicker(ticker) => {
                self.input_ticker = ticker.to_uppercase();
            }
            Message::InputExchange(exchange) => {
                self.input_exchange = exchange;
            }
            Message::InputSide(side) => {
                self.input_side = side;
            }
            Message::InputEntryPrice(price) => {
                self.input_entry_price = price;
            }
            Message::InputExitPrice(price) => {
                self.input_exit_price = price;
            }
            Message::InputSize(size) => {
                self.input_size = size;
            }
            Message::InputFee(fee) => {
                self.input_fee = fee;
            }
            Message::InputTag(tag) => {
                self.input_tag = tag;
            }
            Message::InputNotes(notes) => {
                self.input_notes = notes;
            }
            Message::SubmitTrade => {
                let entry_price = match self.input_entry_price.trim().parse::<f64>() {
                    Ok(p) if p > 0.0 => p,
                    _ => {
                        self.error_message = Some("Invalid entry price".to_string());
                        return None;
                    }
                };

                let size = match self.input_size.trim().parse::<f64>() {
                    Ok(s) if s > 0.0 => s,
                    _ => {
                        self.error_message = Some("Invalid size".to_string());
                        return None;
                    }
                };

                let fee = self.input_fee.trim().parse::<f64>().unwrap_or(0.0);
                let ticker = self.input_ticker.trim().to_uppercase();
                if ticker.is_empty() {
                    self.error_message = Some("Ticker cannot be empty".to_string());
                    return None;
                }

                let tag = if self.input_tag.trim().is_empty() {
                    None
                } else {
                    Some(self.input_tag.trim().to_string())
                };

                let mut entry = JournalEntry::new(
                    self.input_exchange.trim().to_string(),
                    ticker,
                    self.input_side,
                    entry_price,
                    size,
                    fee,
                    tag,
                    self.input_notes.trim().to_string(),
                );

                if !self.input_exit_price.trim().is_empty()
                    && let Ok(exit_price) = self.input_exit_price.trim().parse::<f64>()
                    && exit_price > 0.0
                {
                    entry.close(exit_price, 0.0);
                }

                // Insert newest at top
                self.entries.insert(0, entry);
                self.stats = JournalStats::compute(&self.entries);
                let _ = data::save_journal(&self.entries);

                // Reset form
                self.show_add_form = false;
                self.input_ticker.clear();
                self.input_entry_price.clear();
                self.input_exit_price.clear();
                self.input_size.clear();
                self.input_fee = "0".to_string();
                self.input_tag.clear();
                self.input_notes.clear();
                self.error_message = None;
            }
            Message::CancelForm => {
                self.show_add_form = false;
                self.error_message = None;
            }
            Message::DeleteTrade(id) => {
                self.entries.retain(|e| e.id != id);
                self.stats = JournalStats::compute(&self.entries);
                let _ = data::save_journal(&self.entries);
            }
            Message::OpenClosePrompt(id) => {
                self.closing_id = Some(id);
                self.closing_exit_price.clear();
                self.closing_fee = "0".to_string();
                self.error_message = None;
            }
            Message::InputClosingExitPrice(price) => {
                self.closing_exit_price = price;
            }
            Message::InputClosingFee(fee) => {
                self.closing_fee = fee;
            }
            Message::ConfirmCloseTrade => {
                if let Some(id) = self.closing_id {
                    let exit_price = match self.closing_exit_price.trim().parse::<f64>() {
                        Ok(p) if p > 0.0 => p,
                        _ => {
                            self.error_message = Some("Invalid exit price".to_string());
                            return None;
                        }
                    };
                    let fee = self.closing_fee.trim().parse::<f64>().unwrap_or(0.0);

                    if let Some(trade) = self.entries.iter_mut().find(|e| e.id == id) {
                        trade.close(exit_price, fee);
                    }

                    self.stats = JournalStats::compute(&self.entries);
                    let _ = data::save_journal(&self.entries);
                    self.closing_id = None;
                    self.error_message = None;
                }
            }
            Message::CancelClosePrompt => {
                self.closing_id = None;
                self.error_message = None;
            }
            Message::SelectTicker(ticker) => {
                return Some(Action::TickerSelected(ticker));
            }
        }
        None
    }

    pub fn view(&self, _size: Size) -> Element<'_, Message> {
        let title_bar = row![
            text("TRADE JOURNAL").size(13).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            }),
            space::horizontal(),
            button(
                text(if self.show_add_form {
                    "Cancel"
                } else {
                    "+ New"
                })
                .size(12)
                .align_x(Alignment::Center)
            )
            .padding([3.0, 6.0])
            .on_press(Message::ToggleAddForm)
            .style(|theme, status| style::button::bordered_toggle(
                theme,
                status,
                self.show_add_form
            )),
            button(icon_text(Icon::Close, 12).align_x(Alignment::Center))
                .padding([3.0, 6.0])
                .on_press(Message::ToggleJournal)
                .style(|theme, status| style::button::transparent(theme, status, false)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let stats_bar = self.view_stats_bar();
        let filter_bar = self.view_filter_bar();

        let mut content = column![title_bar, stats_bar, filter_bar].spacing(8);

        if let Some(err) = &self.error_message {
            let err_container = container(
                text(err)
                    .size(12)
                    .color(iced::Color::from_rgb8(239, 68, 68)),
            )
            .padding([4.0, 8.0])
            .style(|theme: &Theme| style::journal_card(theme, Some(false)));
            content = content.push(err_container);
        }

        if self.show_add_form {
            content = content.push(self.view_add_form());
        }

        if let Some(closing_id) = self.closing_id {
            content = content.push(self.view_close_prompt(closing_id));
        }

        let trade_cards = self.view_trade_list();
        content = content.push(trade_cards);

        container(content)
            .width(330)
            .padding(8)
            .style(style::journal_panel)
            .into()
    }

    fn view_stats_bar(&self) -> Element<'_, Message> {
        let pnl_sign = if self.stats.net_pnl > 0.0 { "+" } else { "" };
        let pnl_str = format!("{}${:.2}", pnl_sign, self.stats.net_pnl);
        let winrate_str = format!("{:.1}%", self.stats.win_rate);
        let count_str = format!("{}/{}", self.stats.closed_trades, self.stats.total_trades);
        let net_pnl = self.stats.net_pnl;

        let pnl_box = container(
            column![
                text("Net PnL").size(10),
                text(pnl_str)
                    .size(13)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    })
                    .style(move |theme: &Theme| style::pnl_text(theme, net_pnl)),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding(6)
        .style(style::journal_stat_box);

        let winrate_box = container(
            column![
                text("Winrate").size(10),
                text(winrate_str).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding(6)
        .style(style::journal_stat_box);

        let count_box = container(
            column![
                text("Closed/Tot").size(10),
                text(count_str).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding(6)
        .style(style::journal_stat_box);

        row![pnl_box, winrate_box, count_box]
            .spacing(4)
            .width(Length::Fill)
            .into()
    }

    fn view_filter_bar(&self) -> Element<'_, Message> {
        let is_all = self.filter_status.is_none();
        let is_open = self.filter_status == Some(TradeStatus::Open);
        let is_closed = self.filter_status == Some(TradeStatus::Closed);

        let all_btn = button(text("All").size(11).align_x(Alignment::Center))
            .width(Length::FillPortion(1))
            .padding([2.0, 4.0])
            .on_press(Message::SetStatusFilter(None))
            .style(move |theme, status| style::button::bordered_toggle(theme, status, is_all));

        let open_btn = button(text("Open").size(11).align_x(Alignment::Center))
            .width(Length::FillPortion(1))
            .padding([2.0, 4.0])
            .on_press(Message::SetStatusFilter(Some(TradeStatus::Open)))
            .style(move |theme, status| style::button::bordered_toggle(theme, status, is_open));

        let closed_btn = button(text("Closed").size(11).align_x(Alignment::Center))
            .width(Length::FillPortion(1))
            .padding([2.0, 4.0])
            .on_press(Message::SetStatusFilter(Some(TradeStatus::Closed)))
            .style(move |theme, status| style::button::bordered_toggle(theme, status, is_closed));

        row![all_btn, open_btn, closed_btn]
            .spacing(4)
            .width(Length::Fill)
            .into()
    }

    fn view_add_form(&self) -> Element<'_, Message> {
        let is_long = self.input_side == TradeSide::Long;

        let side_row = row![
            button(text("LONG").size(11).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([4.0, 6.0])
                .on_press(Message::InputSide(TradeSide::Long))
                .style(move |theme, status| {
                    if is_long {
                        style::button::confirm(theme, status, true)
                    } else {
                        style::button::bordered_toggle(theme, status, false)
                    }
                }),
            button(text("SHORT").size(11).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([4.0, 6.0])
                .on_press(Message::InputSide(TradeSide::Short))
                .style(move |theme, status| {
                    if !is_long {
                        style::button::cancel(theme, status, true)
                    } else {
                        style::button::bordered_toggle(theme, status, false)
                    }
                }),
        ]
        .spacing(4);

        let ticker_input = text_input("Ticker (e.g. BTCUSDT)", &self.input_ticker)
            .on_input(Message::InputTicker)
            .padding([4.0, 6.0])
            .size(12);

        let exchange_input = text_input("Exchange (e.g. Binance)", &self.input_exchange)
            .on_input(Message::InputExchange)
            .padding([4.0, 6.0])
            .size(12);

        let entry_input = text_input("Entry Price", &self.input_entry_price)
            .on_input(Message::InputEntryPrice)
            .padding([4.0, 6.0])
            .size(12);

        let exit_input = text_input("Exit Price (optional)", &self.input_exit_price)
            .on_input(Message::InputExitPrice)
            .padding([4.0, 6.0])
            .size(12);

        let size_input = text_input("Size / Qty", &self.input_size)
            .on_input(Message::InputSize)
            .padding([4.0, 6.0])
            .size(12);

        let fee_input = text_input("Fee ($)", &self.input_fee)
            .on_input(Message::InputFee)
            .padding([4.0, 6.0])
            .size(12);

        let tag_input = text_input("Strategy Tag (e.g. Breakout)", &self.input_tag)
            .on_input(Message::InputTag)
            .padding([4.0, 6.0])
            .size(12);

        let notes_input = text_input("Notes...", &self.input_notes)
            .on_input(Message::InputNotes)
            .padding([4.0, 6.0])
            .size(12);

        let action_row = row![
            button(text("Save Trade").size(12).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([4.0, 8.0])
                .on_press(Message::SubmitTrade)
                .style(|theme, status| style::button::confirm(theme, status, true)),
            button(text("Cancel").size(12).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([4.0, 8.0])
                .on_press(Message::CancelForm)
                .style(|theme, status| style::button::bordered_toggle(theme, status, false)),
        ]
        .spacing(4);

        container(
            column![
                text("New Trade Entry").size(12).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                side_row,
                row![ticker_input, exchange_input].spacing(4),
                row![entry_input, size_input].spacing(4),
                row![exit_input, fee_input].spacing(4),
                tag_input,
                notes_input,
                action_row,
            ]
            .spacing(6),
        )
        .padding(8)
        .style(|theme: &Theme| style::journal_card(theme, None))
        .into()
    }

    fn view_close_prompt(&self, _id: Uuid) -> Element<'_, Message> {
        let exit_input = text_input("Exit Price", &self.closing_exit_price)
            .on_input(Message::InputClosingExitPrice)
            .padding([4.0, 6.0])
            .size(12);

        let fee_input = text_input("Closing Fee ($)", &self.closing_fee)
            .on_input(Message::InputClosingFee)
            .padding([4.0, 6.0])
            .size(12);

        let action_row = row![
            button(text("Confirm Close").size(11).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([3.0, 6.0])
                .on_press(Message::ConfirmCloseTrade)
                .style(|theme, status| style::button::confirm(theme, status, true)),
            button(text("Cancel").size(11).align_x(Alignment::Center))
                .width(Length::FillPortion(1))
                .padding([3.0, 6.0])
                .on_press(Message::CancelClosePrompt)
                .style(|theme, status| style::button::bordered_toggle(theme, status, false)),
        ]
        .spacing(4);

        container(
            column![
                text("Close Position").size(12).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                row![exit_input, fee_input].spacing(4),
                action_row,
            ]
            .spacing(6),
        )
        .padding(8)
        .style(|theme: &Theme| style::journal_card(theme, Some(true)))
        .into()
    }

    fn view_trade_list<'a>(&'a self) -> Element<'a, Message> {
        let filtered: Vec<&'a JournalEntry> = self
            .entries
            .iter()
            .filter(|e| match self.filter_status {
                Some(status) => e.status == status,
                None => true,
            })
            .collect();

        if filtered.is_empty() {
            return container(
                text("No trades recorded yet")
                    .size(12)
                    .color(iced::Color::from_rgb8(150, 150, 150))
                    .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(16)
            .into();
        }

        let mut trade_col = column![].spacing(6);

        for entry in filtered {
            trade_col = trade_col.push(self.view_trade_card(entry));
        }

        scrollable::Scrollable::with_direction(
            trade_col,
            scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(4),
            ),
        )
        .height(Length::Fill)
        .style(style::scroll_bar)
        .into()
    }

    fn view_trade_card<'a>(&'a self, entry: &'a JournalEntry) -> Element<'a, Message> {
        let is_long = entry.side == TradeSide::Long;
        let is_profit = entry.pnl.map(|p| p > 0.0001);

        let side_badge = container(text(if is_long { "LONG" } else { "SHORT" }).size(10).font(
            iced::Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            },
        ))
        .padding([2.0, 4.0])
        .style(move |theme: &Theme| style::journal_badge(theme, is_long));

        let ticker_btn = button(text(&entry.ticker).size(12).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        }))
        .padding(0)
        .on_press(Message::SelectTicker(entry.ticker.clone()))
        .style(|theme, status| style::button::transparent(theme, status, false));

        let status_text = match entry.status {
            TradeStatus::Open => text("OPEN")
                .size(10)
                .color(iced::Color::from_rgb8(234, 179, 8)),
            TradeStatus::Closed => text("CLOSED")
                .size(10)
                .color(iced::Color::from_rgb8(156, 163, 175)),
            TradeStatus::Cancelled => text("CANCELLED")
                .size(10)
                .color(iced::Color::from_rgb8(156, 163, 175)),
        };

        let delete_btn = button(icon_text(Icon::TrashBin, 10).align_x(Alignment::Center))
            .padding([2.0, 4.0])
            .on_press(Message::DeleteTrade(entry.id))
            .style(|theme, status| style::button::transparent(theme, status, false));

        let header_row = row![
            side_badge,
            ticker_btn,
            space::horizontal(),
            status_text,
            delete_btn,
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let exit_str = match entry.exit_price {
            Some(exit) => format!("{:.2}", exit),
            None => "-".to_string(),
        };

        let price_row = row![
            text(format!("Entry: {:.2}", entry.entry_price)).size(11),
            text(format!("Exit: {}", exit_str)).size(11),
            space::horizontal(),
            text(format!("Qty: {:.4}", entry.size)).size(11),
        ]
        .spacing(6);

        let pnl_row = if let Some(pnl) = entry.pnl {
            let pct = entry.pnl_percent.unwrap_or(0.0);
            let sign = if pnl > 0.0 { "+" } else { "" };
            let pnl_str = format!("{}${:.2} ({}{:.2}%)", sign, pnl, sign, pct);

            row![
                text("PnL:").size(11),
                text(pnl_str)
                    .size(11)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    })
                    .style(move |theme: &Theme| style::pnl_text(theme, pnl)),
                space::horizontal(),
                text(format!("Fee: ${:.2}", entry.fee)).size(10),
            ]
            .spacing(4)
        } else {
            let close_action = button(text("Close Position").size(10).align_x(Alignment::Center))
                .padding([2.0, 6.0])
                .on_press(Message::OpenClosePrompt(entry.id))
                .style(|theme, status| style::button::modifier(theme, status, false));

            row![
                text("Active position").size(11),
                space::horizontal(),
                close_action,
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        };

        let mut card_col = column![header_row, price_row, pnl_row].spacing(4);

        if let Some(tag) = &entry.setup_tag {
            let tag_badge = container(text(tag).size(9))
                .padding([1.0, 3.0])
                .style(style::journal_stat_box);
            card_col = card_col.push(tag_badge);
        }

        if !entry.notes.is_empty() {
            card_col = card_col.push(
                text(&entry.notes)
                    .size(10)
                    .color(iced::Color::from_rgb8(160, 160, 160)),
            );
        }

        // Timestamp
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(entry.timestamp_open as i64) {
            let date_str = dt.format("%Y-%m-%d %H:%M").to_string();
            card_col = card_col.push(
                text(date_str)
                    .size(9)
                    .color(iced::Color::from_rgb8(120, 120, 120)),
            );
        }

        container(card_col)
            .width(Length::Fill)
            .padding(6)
            .style(move |theme: &Theme| style::journal_card(theme, is_profit))
            .into()
    }

    pub fn view_dashboard(&self) -> Element<'_, Message> {
        let title_bar = row![
            icon_text(Icon::Journal, 18),
            text("TRADE JOURNAL DASHBOARD").size(16).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            }),
            space::horizontal(),
            container(self.view_filter_bar()).width(300),
            space::horizontal(),
            button(
                text(if self.show_add_form {
                    "Cancel Form"
                } else {
                    "+ Add Trade"
                })
                .size(12)
                .align_x(Alignment::Center)
            )
            .padding([6.0, 14.0])
            .on_press(Message::ToggleAddForm)
            .style(move |theme, status| {
                if self.show_add_form {
                    style::button::bordered_toggle(theme, status, true)
                } else {
                    style::button::confirm(theme, status, true)
                }
            }),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        // 6 KPI Metric Cards
        let kpi_row = self.view_dashboard_kpis();

        // 2 Analytics Breakdowns (Exchange & Strategy)
        let analytics_row = self.view_dashboard_analytics();

        let mut main_col = column![title_bar, kpi_row, analytics_row].spacing(16);

        if let Some(err) = &self.error_message {
            let err_container = container(
                text(err)
                    .size(12)
                    .color(iced::Color::from_rgb8(239, 68, 68)),
            )
            .padding([6.0, 12.0])
            .style(|theme: &Theme| style::journal_card(theme, Some(false)));
            main_col = main_col.push(err_container);
        }

        if self.show_add_form {
            main_col = main_col.push(self.view_dashboard_add_form());
        }

        if let Some(closing_id) = self.closing_id {
            main_col = main_col.push(self.view_close_prompt(closing_id));
        }

        // Trades Table
        let table = self.view_dashboard_table();
        main_col = main_col.push(table);

        let content = scrollable::Scrollable::with_direction(
            main_col,
            scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(10).scroller_width(8),
            ),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.base.color.into()),
                    ..Default::default()
                }
            })
            .into()
    }

    fn view_dashboard_kpis(&self) -> Element<'_, Message> {
        let pnl_sign = if self.stats.net_pnl > 0.0 { "+" } else { "" };
        let pnl_str = format!("{}${:.2}", pnl_sign, self.stats.net_pnl);
        let profit_str = format!("Gross Profit: +${:.2}", self.stats.total_profit);
        let loss_str = format!("Gross Loss: -${:.2}", self.stats.total_loss);
        let net_pnl = self.stats.net_pnl;

        let card_pnl = container(
            column![
                text("NET P&L")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(pnl_str)
                    .size(20)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    })
                    .style(move |theme: &Theme| style::pnl_text(theme, net_pnl)),
                text(format!("{} | {}", profit_str, loss_str))
                    .size(10)
                    .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        let winrate_str = format!("{:.1}%", self.stats.win_rate);
        let wl_sub = format!(
            "{}W / {}L / {}BE",
            self.stats.winning_trades, self.stats.losing_trades, self.stats.breakeven_trades
        );
        let card_winrate = container(
            column![
                text("WIN RATE")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(winrate_str).size(20).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                text(wl_sub)
                    .size(10)
                    .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        let pf_str = if self.stats.profit_factor.is_infinite() {
            "∞".to_string()
        } else {
            format!("{:.2}", self.stats.profit_factor)
        };
        let card_pf = container(
            column![
                text("PROFIT FACTOR")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(pf_str).size(20).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                text(format!("Closed: {} trades", self.stats.closed_trades))
                    .size(10)
                    .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        let card_trades = container(
            column![
                text("TOTAL TRADES")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(format!("{}", self.stats.total_trades))
                    .size(20)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    }),
                text(format!(
                    "Open: {} | Closed: {}",
                    self.stats.open_trades, self.stats.closed_trades
                ))
                .size(10)
                .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        let card_fees = container(
            column![
                text("FEES PAID")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(format!("${:.2}", self.stats.total_fees))
                    .size(20)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    }),
                text(format!(
                    "Avg Fee: ${:.2}",
                    if self.stats.total_trades > 0 {
                        self.stats.total_fees / self.stats.total_trades as f64
                    } else {
                        0.0
                    }
                ))
                .size(10)
                .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        let avg_win_loss = format!("+${:.1} / -${:.1}", self.stats.avg_win, self.stats.avg_loss);
        let ratio = if self.stats.avg_loss > 0.0 {
            format!("Payoff: {:.2}x", self.stats.avg_win / self.stats.avg_loss)
        } else {
            "Payoff: N/A".to_string()
        };
        let card_payoff = container(
            column![
                text("AVG WIN / LOSS")
                    .size(10)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
                text(avg_win_loss).size(20).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                text(ratio)
                    .size(10)
                    .color(iced::Color::from_rgb8(140, 140, 140)),
            ]
            .spacing(4)
            .align_x(Alignment::Start),
        )
        .width(Length::FillPortion(1))
        .padding(12)
        .style(style::journal_stat_box);

        row![
            card_pnl,
            card_winrate,
            card_pf,
            card_trades,
            card_fees,
            card_payoff
        ]
        .spacing(10)
        .width(Length::Fill)
        .into()
    }

    fn compute_exchange_breakdown(&self) -> Vec<(String, usize, f64, f64)> {
        let mut map: std::collections::BTreeMap<String, (usize, usize, f64)> =
            std::collections::BTreeMap::new();
        for entry in &self.entries {
            let item = map.entry(entry.exchange.clone()).or_insert((0, 0, 0.0));
            item.0 += 1;
            if let Some(pnl) = entry.pnl {
                item.2 += pnl;
                if pnl > 0.0001 {
                    item.1 += 1;
                }
            }
        }
        map.into_iter()
            .map(|(name, (total, wins, pnl))| {
                let wr = if total > 0 {
                    (wins as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                (name, total, wr, pnl)
            })
            .collect()
    }

    fn compute_tag_breakdown(&self) -> Vec<(String, usize, f64, f64)> {
        let mut map: std::collections::BTreeMap<String, (usize, usize, f64)> =
            std::collections::BTreeMap::new();
        for entry in &self.entries {
            let tag = entry
                .setup_tag
                .clone()
                .unwrap_or_else(|| "Untagged".to_string());
            let item = map.entry(tag).or_insert((0, 0, 0.0));
            item.0 += 1;
            if let Some(pnl) = entry.pnl {
                item.2 += pnl;
                if pnl > 0.0001 {
                    item.1 += 1;
                }
            }
        }
        map.into_iter()
            .map(|(name, (total, wins, pnl))| {
                let wr = if total > 0 {
                    (wins as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                (name, total, wr, pnl)
            })
            .collect()
    }

    fn view_dashboard_analytics(&self) -> Element<'_, Message> {
        let exchange_data = self.compute_exchange_breakdown();
        let tag_data = self.compute_tag_breakdown();

        let mut exchange_col = column![
            text("EXCHANGE PERFORMANCE").size(11).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            }),
            row![
                text("Exchange")
                    .size(10)
                    .width(120)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Trades")
                    .size(10)
                    .width(60)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Win Rate")
                    .size(10)
                    .width(80)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Net P&L")
                    .size(10)
                    .width(Length::Fill)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
            ]
            .spacing(8)
        ]
        .spacing(6);

        if exchange_data.is_empty() {
            exchange_col = exchange_col.push(
                text("No trades recorded yet")
                    .size(11)
                    .color(iced::Color::from_rgb8(120, 120, 120)),
            );
        } else {
            for (ex, total, wr, pnl) in exchange_data {
                let sign = if pnl > 0.0 { "+" } else { "" };
                let pnl_str = format!("{}${:.2}", sign, pnl);
                let row_view = row![
                    text(ex).size(11).width(120),
                    text(format!("{}", total)).size(11).width(60),
                    text(format!("{:.1}%", wr)).size(11).width(80),
                    text(pnl_str)
                        .size(11)
                        .width(Length::Fill)
                        .style(move |theme: &Theme| style::pnl_text(theme, pnl)),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                exchange_col = exchange_col.push(row_view);
            }
        }

        let card_exchanges = container(exchange_col)
            .width(Length::FillPortion(1))
            .padding(12)
            .style(style::journal_stat_box);

        let mut tag_col = column![
            text("STRATEGY / TAG PERFORMANCE")
                .size(11)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
            row![
                text("Strategy / Setup")
                    .size(10)
                    .width(140)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Trades")
                    .size(10)
                    .width(60)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Win Rate")
                    .size(10)
                    .width(80)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
                text("Net P&L")
                    .size(10)
                    .width(Length::Fill)
                    .color(iced::Color::from_rgb8(150, 150, 150)),
            ]
            .spacing(8)
        ]
        .spacing(6);

        if tag_data.is_empty() {
            tag_col = tag_col.push(
                text("No trades recorded yet")
                    .size(11)
                    .color(iced::Color::from_rgb8(120, 120, 120)),
            );
        } else {
            for (tag, total, wr, pnl) in tag_data {
                let sign = if pnl > 0.0 { "+" } else { "" };
                let pnl_str = format!("{}${:.2}", sign, pnl);
                let row_view = row![
                    text(tag).size(11).width(140),
                    text(format!("{}", total)).size(11).width(60),
                    text(format!("{:.1}%", wr)).size(11).width(80),
                    text(pnl_str)
                        .size(11)
                        .width(Length::Fill)
                        .style(move |theme: &Theme| style::pnl_text(theme, pnl)),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                tag_col = tag_col.push(row_view);
            }
        }

        let card_tags = container(tag_col)
            .width(Length::FillPortion(1))
            .padding(12)
            .style(style::journal_stat_box);

        row![card_exchanges, card_tags]
            .spacing(10)
            .width(Length::Fill)
            .into()
    }

    fn view_dashboard_add_form(&self) -> Element<'_, Message> {
        let is_long = self.input_side == TradeSide::Long;

        let side_selector = row![
            button(text("LONG").size(11).align_x(Alignment::Center))
                .width(100)
                .padding([4.0, 8.0])
                .on_press(Message::InputSide(TradeSide::Long))
                .style(move |theme, status| {
                    if is_long {
                        style::button::confirm(theme, status, true)
                    } else {
                        style::button::bordered_toggle(theme, status, false)
                    }
                }),
            button(text("SHORT").size(11).align_x(Alignment::Center))
                .width(100)
                .padding([4.0, 8.0])
                .on_press(Message::InputSide(TradeSide::Short))
                .style(move |theme, status| {
                    if !is_long {
                        style::button::cancel(theme, status, true)
                    } else {
                        style::button::bordered_toggle(theme, status, false)
                    }
                }),
        ]
        .spacing(6);

        let ticker_input = text_input("Ticker (e.g. BTCUSDT)", &self.input_ticker)
            .on_input(Message::InputTicker)
            .padding([5.0, 8.0])
            .size(12);

        let exchange_input = text_input("Exchange (e.g. Binance)", &self.input_exchange)
            .on_input(Message::InputExchange)
            .padding([5.0, 8.0])
            .size(12);

        let entry_input = text_input("Entry Price", &self.input_entry_price)
            .on_input(Message::InputEntryPrice)
            .padding([5.0, 8.0])
            .size(12);

        let exit_input = text_input("Exit Price (optional)", &self.input_exit_price)
            .on_input(Message::InputExitPrice)
            .padding([5.0, 8.0])
            .size(12);

        let size_input = text_input("Size / Qty", &self.input_size)
            .on_input(Message::InputSize)
            .padding([5.0, 8.0])
            .size(12);

        let fee_input = text_input("Fee ($)", &self.input_fee)
            .on_input(Message::InputFee)
            .padding([5.0, 8.0])
            .size(12);

        let tag_input = text_input("Strategy Tag (e.g. Breakout, Footprint)", &self.input_tag)
            .on_input(Message::InputTag)
            .padding([5.0, 8.0])
            .size(12);

        let notes_input = text_input("Trade Notes & Observations...", &self.input_notes)
            .on_input(Message::InputNotes)
            .padding([5.0, 8.0])
            .size(12);

        let buttons = row![
            button(text("Save Trade").size(12).align_x(Alignment::Center))
                .padding([6.0, 16.0])
                .on_press(Message::SubmitTrade)
                .style(|theme, status| style::button::confirm(theme, status, true)),
            button(text("Cancel").size(12).align_x(Alignment::Center))
                .padding([6.0, 16.0])
                .on_press(Message::CancelForm)
                .style(|theme, status| style::button::bordered_toggle(theme, status, false)),
        ]
        .spacing(8);

        let form_content = column![
            row![
                text("NEW TRADE ENTRY").size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
                space::horizontal(),
                side_selector,
            ]
            .align_y(Alignment::Center),
            row![
                column![text("Ticker").size(11), ticker_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Exchange").size(11), exchange_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Entry Price").size(11), entry_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Exit Price").size(11), exit_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Size / Qty").size(11), size_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Fee ($)").size(11), fee_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            ]
            .spacing(10),
            row![
                column![text("Strategy Tag").size(11), tag_input]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
                column![text("Notes").size(11), notes_input]
                    .spacing(4)
                    .width(Length::FillPortion(2)),
            ]
            .spacing(10),
            buttons,
        ]
        .spacing(10);

        container(form_content)
            .width(Length::Fill)
            .padding(14)
            .style(|theme: &Theme| style::journal_card(theme, None))
            .into()
    }

    fn view_dashboard_table(&self) -> Element<'_, Message> {
        let header = row![
            text("STATUS")
                .size(10)
                .width(70)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("SIDE")
                .size(10)
                .width(60)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("TICKER")
                .size(10)
                .width(90)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("EXCHANGE")
                .size(10)
                .width(85)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("ENTRY")
                .size(10)
                .width(90)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("EXIT")
                .size(10)
                .width(90)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("SIZE")
                .size(10)
                .width(80)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("FEE")
                .size(10)
                .width(65)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("NET PNL")
                .size(10)
                .width(95)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("PNL %")
                .size(10)
                .width(80)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("STRATEGY")
                .size(10)
                .width(110)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("DATE OPENED")
                .size(10)
                .width(130)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("NOTES")
                .size(10)
                .width(Length::Fill)
                .color(iced::Color::from_rgb8(150, 150, 150)),
            text("ACTIONS")
                .size(10)
                .width(110)
                .align_x(Alignment::End)
                .color(iced::Color::from_rgb8(150, 150, 150)),
        ]
        .spacing(8)
        .padding([8.0, 12.0]);

        let header_container = container(header)
            .width(Length::Fill)
            .style(style::journal_stat_box);

        let filtered_entries: Vec<&JournalEntry> = self
            .entries
            .iter()
            .filter(|e| match self.filter_status {
                Some(status) => e.status == status,
                None => true,
            })
            .collect();

        let mut rows_col = column![].spacing(4);

        if filtered_entries.is_empty() {
            rows_col = rows_col.push(
                container(
                    text("No trades match the current filter")
                        .size(12)
                        .color(iced::Color::from_rgb8(140, 140, 140)),
                )
                .width(Length::Fill)
                .padding(20)
                .align_x(Alignment::Center),
            );
        } else {
            for entry in filtered_entries {
                rows_col = rows_col.push(self.view_dashboard_table_row(entry));
            }
        }

        column![header_container, rows_col].spacing(6).into()
    }

    fn view_dashboard_table_row<'a>(&'a self, entry: &'a JournalEntry) -> Element<'a, Message> {
        let is_long = entry.side == TradeSide::Long;
        let is_profit = entry.pnl.map(|p| p > 0.0);

        let status_badge = match entry.status {
            TradeStatus::Open => container(
                text("OPEN")
                    .size(9)
                    .color(iced::Color::from_rgb8(234, 179, 8)),
            )
            .padding([2.0, 6.0])
            .style(style::journal_stat_box),
            TradeStatus::Closed => container(
                text("CLOSED")
                    .size(9)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
            )
            .padding([2.0, 6.0])
            .style(style::journal_stat_box),
            TradeStatus::Cancelled => container(
                text("CANCEL")
                    .size(9)
                    .color(iced::Color::from_rgb8(156, 163, 175)),
            )
            .padding([2.0, 6.0])
            .style(style::journal_stat_box),
        };

        let side_badge = container(text(entry.side.to_string()).size(9).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        }))
        .padding([2.0, 6.0])
        .style(move |theme: &Theme| style::journal_badge(theme, is_long));

        let ticker_btn = button(text(&entry.ticker).size(11).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        }))
        .padding(0)
        .on_press(Message::SelectTicker(entry.ticker.clone()))
        .style(|theme, status| style::button::transparent(theme, status, false));

        let exit_str = match entry.exit_price {
            Some(p) => format!("{:.2}", p),
            None => "-".to_string(),
        };

        let (pnl_elem, pct_elem): (Element<'_, Message>, Element<'_, Message>) = match entry.pnl {
            Some(pnl) => {
                let pct = entry.pnl_percent.unwrap_or(0.0);
                let sign = if pnl > 0.0 { "+" } else { "" };
                let pnl_str = format!("{}${:.2}", sign, pnl);
                let pct_str = format!("{}{:.2}%", sign, pct);
                (
                    text(pnl_str)
                        .size(11)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        })
                        .style(move |theme: &Theme| style::pnl_text(theme, pnl))
                        .into(),
                    text(pct_str)
                        .size(11)
                        .style(move |theme: &Theme| style::pnl_text(theme, pnl))
                        .into(),
                )
            }
            None => (text("-").size(11).into(), text("-").size(11).into()),
        };

        let strategy_elem: Element<'_, Message> = if let Some(tag) = &entry.setup_tag {
            container(text(tag).size(9))
                .padding([1.0, 4.0])
                .style(style::journal_stat_box)
                .into()
        } else {
            text("-")
                .size(11)
                .color(iced::Color::from_rgb8(120, 120, 120))
                .into()
        };

        let date_str = if let Some(dt) =
            chrono::DateTime::from_timestamp_millis(entry.timestamp_open as i64)
        {
            dt.format("%Y-%m-%d %H:%M").to_string()
        } else {
            "-".to_string()
        };

        let delete_btn = button(icon_text(Icon::TrashBin, 11).align_x(Alignment::Center))
            .padding([2.0, 6.0])
            .on_press(Message::DeleteTrade(entry.id))
            .style(|theme, status| style::button::transparent(theme, status, false));

        let actions_elem = if entry.status == TradeStatus::Open {
            row![
                button(text("Close").size(10).align_x(Alignment::Center))
                    .padding([2.0, 6.0])
                    .on_press(Message::OpenClosePrompt(entry.id))
                    .style(|theme, status| style::button::modifier(theme, status, false)),
                delete_btn,
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        } else {
            row![delete_btn].align_y(Alignment::Center)
        };

        let row_content = row![
            container(status_badge).width(70),
            container(side_badge).width(60),
            container(ticker_btn).width(90),
            container(text(&entry.exchange).size(11)).width(85),
            container(text(format!("{:.2}", entry.entry_price)).size(11)).width(90),
            container(text(exit_str).size(11)).width(90),
            container(text(format!("{:.4}", entry.size)).size(11)).width(80),
            container(text(format!("${:.2}", entry.fee)).size(11)).width(65),
            container(pnl_elem).width(95),
            container(pct_elem).width(80),
            container(strategy_elem).width(110),
            container(
                text(date_str)
                    .size(11)
                    .color(iced::Color::from_rgb8(130, 130, 130))
            )
            .width(130),
            container(
                text(&entry.notes)
                    .size(11)
                    .color(iced::Color::from_rgb8(160, 160, 160))
            )
            .width(Length::Fill),
            container(actions_elem).width(110).align_x(Alignment::End),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([6.0, 12.0]);

        container(row_content)
            .width(Length::Fill)
            .style(move |theme: &Theme| style::journal_card(theme, is_profit))
            .into()
    }
}
