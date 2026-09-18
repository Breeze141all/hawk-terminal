use super::journal::{self, Journal};
use super::tickers_table::{self, TickersTable};
use crate::{
    TooltipPosition,
    layout::SavedState,
    style::{Icon, icon_text},
    widget::button_with_tooltip,
};
use data::sidebar;

use iced::{
    Alignment, Element, Subscription, Task,
    widget::responsive,
    widget::{column, row, space},
};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub enum Message {
    ToggleSidebarMenu(Option<sidebar::Menu>),
    SetSidebarPosition(sidebar::Position),
    TickersTable(super::tickers_table::Message),
    Journal(super::journal::Message),
}

pub struct Sidebar {
    pub state: data::Sidebar,
    pub tickers_table: TickersTable,
    pub journal: Journal,
    pub journal_mode: data::JournalMode,
    pub is_journal_window_open: bool,
}

pub enum Action {
    TickerSelected(
        exchange::TickerInfo,
        Option<data::layout::pane::ContentKind>,
    ),
    ErrorOccurred(data::InternalError),
    ToggleJournalWindow,
}

impl Sidebar {
    pub fn new(state: &SavedState) -> (Self, Task<Message>) {
        let (tickers_table, initial_fetch) =
            if let Some(settings) = state.sidebar.tickers_table.as_ref() {
                TickersTable::new_with_settings(settings)
            } else {
                TickersTable::new()
            };

        (
            Self {
                state: state.sidebar.clone(),
                tickers_table,
                journal: Journal::new(),
                journal_mode: state.journal_mode,
                is_journal_window_open: false,
            },
            initial_fetch.map(Message::TickersTable),
        )
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Action>) {
        match message {
            Message::ToggleSidebarMenu(menu) => {
                self.set_menu(menu.filter(|&m| !self.is_menu_active(m)));
            }
            Message::SetSidebarPosition(position) => {
                self.state.position = position;
            }
            Message::TickersTable(msg) => {
                let action = self.tickers_table.update(msg);
                if self.tickers_table.is_shown {
                    self.journal.is_shown = false;
                }

                match action {
                    Some(tickers_table::Action::TickerSelected(ticker_info, content)) => {
                        return (
                            Task::none(),
                            Some(Action::TickerSelected(ticker_info, content)),
                        );
                    }
                    Some(tickers_table::Action::Fetch(task)) => {
                        return (task.map(Message::TickersTable), None);
                    }
                    Some(tickers_table::Action::ErrorOccurred(error)) => {
                        return (Task::none(), Some(Action::ErrorOccurred(error)));
                    }
                    Some(tickers_table::Action::FocusWidget(id)) => {
                        return (iced::widget::operation::focus(id), None);
                    }
                    None => {}
                }
            }
            Message::Journal(msg) => {
                if let super::journal::Message::ToggleJournal = &msg
                    && self.journal_mode == data::JournalMode::Extended
                {
                    return (Task::none(), Some(Action::ToggleJournalWindow));
                }

                let action = self.journal.update(msg);
                if self.journal.is_shown {
                    self.tickers_table.is_shown = false;
                }

                match action {
                    Some(journal::Action::TickerSelected(ticker_str)) => {
                        let found_ticker =
                            self.tickers_table
                                .tickers_info
                                .iter()
                                .find_map(|(t, info)| {
                                    if t.to_string().eq_ignore_ascii_case(&ticker_str) {
                                        *info
                                    } else {
                                        None
                                    }
                                });
                        if let Some(ticker_info) = found_ticker {
                            return (
                                Task::none(),
                                Some(Action::TickerSelected(ticker_info, None)),
                            );
                        }
                    }
                    None => {}
                }
            }
        }

        (Task::none(), None)
    }

    pub fn view(&self, audio_volume: Option<f32>) -> Element<'_, Message> {
        let state = &self.state;

        let tooltip_position = if state.position == sidebar::Position::Left {
            TooltipPosition::Right
        } else {
            TooltipPosition::Left
        };

        let is_table_open = self.tickers_table.is_shown;
        let is_journal_open = self.journal.is_shown;

        let nav_buttons = self.nav_buttons(
            is_table_open,
            is_journal_open,
            audio_volume,
            tooltip_position,
        );

        let side_content = if is_table_open {
            column![responsive(move |size| self
                .tickers_table
                .view(size)
                .map(Message::TickersTable))]
            .width(200)
        } else if is_journal_open {
            column![responsive(move |size| self
                .journal
                .view(size)
                .map(Message::Journal))]
            .width(330)
        } else {
            column![]
        };

        let has_panel_open = is_table_open || is_journal_open;

        match state.position {
            sidebar::Position::Left => row![nav_buttons, side_content],
            sidebar::Position::Right => row![side_content, nav_buttons],
        }
        .spacing(if has_panel_open { 8 } else { 4 })
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        self.tickers_table.subscription().map(Message::TickersTable)
    }

    fn nav_buttons(
        &self,
        is_table_open: bool,
        is_journal_open: bool,
        audio_volume: Option<f32>,
        tooltip_position: TooltipPosition,
    ) -> iced::widget::Column<'_, Message> {
        let settings_modal_button = {
            let is_active = self.is_menu_active(sidebar::Menu::Settings)
                || self.is_menu_active(sidebar::Menu::ThemeEditor);

            button_with_tooltip(
                icon_text(Icon::Cog, 14)
                    .width(24)
                    .align_x(Alignment::Center),
                Message::ToggleSidebarMenu(Some(sidebar::Menu::Settings)),
                None,
                tooltip_position,
                move |theme, status| crate::style::button::transparent(theme, status, is_active),
            )
        };

        let layout_modal_button = {
            let is_active = self.is_menu_active(sidebar::Menu::Layout);

            button_with_tooltip(
                icon_text(Icon::Layout, 14)
                    .width(24)
                    .align_x(Alignment::Center),
                Message::ToggleSidebarMenu(Some(sidebar::Menu::Layout)),
                None,
                tooltip_position,
                move |theme, status| crate::style::button::transparent(theme, status, is_active),
            )
        };

        let ticker_search_button = {
            button_with_tooltip(
                icon_text(Icon::Search, 14)
                    .width(24)
                    .align_x(Alignment::Center),
                Message::TickersTable(super::tickers_table::Message::ToggleTable),
                None,
                tooltip_position,
                move |theme, status| {
                    crate::style::button::transparent(theme, status, is_table_open)
                },
            )
        };

        let is_journal_active = match self.journal_mode {
            data::JournalMode::Disabled => false,
            data::JournalMode::Basic => is_journal_open,
            data::JournalMode::Extended => self.is_journal_window_open,
        };

        let journal_button = {
            button_with_tooltip(
                icon_text(Icon::Journal, 14)
                    .width(24)
                    .align_x(Alignment::Center),
                Message::Journal(super::journal::Message::ToggleJournal),
                Some("Trade Journal"),
                tooltip_position,
                move |theme, status| {
                    crate::style::button::transparent(theme, status, is_journal_active)
                },
            )
        };

        let audio_btn = {
            let is_active = self.is_menu_active(sidebar::Menu::Audio);

            let icon = match audio_volume.unwrap_or(0.0) {
                v if v >= 40.0 => Icon::SpeakerHigh,
                v if v > 0.0 => Icon::SpeakerLow,
                _ => Icon::SpeakerOff,
            };

            button_with_tooltip(
                icon_text(icon, 14).width(24).align_x(Alignment::Center),
                Message::ToggleSidebarMenu(Some(sidebar::Menu::Audio)),
                None,
                tooltip_position,
                move |theme, status| crate::style::button::transparent(theme, status, is_active),
            )
        };

        let mut buttons = column![ticker_search_button, layout_modal_button, audio_btn,];

        if self.journal_mode != data::JournalMode::Disabled {
            buttons = buttons.push(journal_button);
        }

        buttons
            .push(space::vertical())
            .push(settings_modal_button)
            .width(32)
            .spacing(8)
    }

    pub fn set_journal_mode(&mut self, mode: data::JournalMode) {
        self.journal_mode = mode;
        if mode == data::JournalMode::Disabled {
            self.journal.is_shown = false;
            self.is_journal_window_open = false;
        } else if mode == data::JournalMode::Extended {
            self.journal.is_shown = false;
        } else {
            self.is_journal_window_open = false;
        }
    }

    pub fn set_journal_window_open(&mut self, is_open: bool) {
        self.is_journal_window_open = is_open;
    }

    pub fn hide_tickers_table(&mut self) -> bool {
        let table = &mut self.tickers_table;

        if table.expand_ticker_card.is_some() {
            table.expand_ticker_card = None;
            return true;
        } else if table.is_shown {
            table.is_shown = false;
            return true;
        } else if self.journal.is_shown {
            self.journal.is_shown = false;
            return true;
        }

        false
    }

    pub fn is_menu_active(&self, menu: sidebar::Menu) -> bool {
        self.state.active_menu == Some(menu)
    }

    pub fn active_menu(&self) -> Option<sidebar::Menu> {
        self.state.active_menu
    }

    pub fn position(&self) -> sidebar::Position {
        self.state.position
    }

    pub fn set_menu(&mut self, menu: Option<sidebar::Menu>) {
        self.state.active_menu = menu;
    }

    pub fn sync_tickers_table_settings(&mut self) {
        let settings = &self.tickers_table.settings();
        self.state.tickers_table = Some(settings.clone());
    }

    pub fn tickers_info(&self) -> &FxHashMap<exchange::Ticker, Option<exchange::TickerInfo>> {
        &self.tickers_table.tickers_info
    }
}
