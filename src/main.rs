#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod chart;
pub mod journal_media;
mod layout;
mod logger;
mod modal;
pub mod profile;
mod screen;
mod style;
mod widget;
mod window;

use data::config::theme::default_theme;
use data::{layout::WindowSpec, sidebar};
use layout::{LayoutId, configuration};
use modal::{LayoutManager, ThemeEditor, audio::AudioStream};
use modal::{dashboard_modal, main_dialog_modal};
use screen::dashboard::{self, Dashboard};
use widget::{
    confirm_dialog_container,
    toast::{self, Toast},
    tooltip,
};

use iced::{
    Alignment, Element, Subscription, Task, keyboard, padding,
    widget::{
        button, column, container, pane_grid, pick_list, row, rule, scrollable, text,
        tooltip::Position as TooltipPosition,
    },
};
use std::{borrow::Cow, collections::HashMap, vec};

enum AppScreen {
    Splash { start: std::time::Instant },
    Running,
}

fn main() {
    logger::setup(cfg!(debug_assertions)).expect("Failed to initialize logger");

    std::thread::spawn(data::cleanup_old_market_data);

    let _ = iced::daemon(HawkTerminal::new, HawkTerminal::update, HawkTerminal::view)
        .settings(iced::Settings {
            antialiasing: true,
            fonts: vec![
                Cow::Borrowed(style::AZERET_MONO_BYTES),
                Cow::Borrowed(style::ICONS_BYTES),
            ],
            default_text_size: iced::Pixels(12.0),
            ..Default::default()
        })
        .title(HawkTerminal::title)
        .theme(HawkTerminal::theme)
        .scale_factor(HawkTerminal::scale_factor)
        .subscription(HawkTerminal::subscription)
        .run();
}

struct HawkTerminal {
    screen: AppScreen,
    main_window: window::Window,
    sidebar: dashboard::Sidebar,
    layout_manager: LayoutManager,
    theme_editor: ThemeEditor,
    audio_stream: AudioStream,
    confirm_dialog: Option<screen::ConfirmDialog<Message>>,
    volume_size_unit: exchange::SizeUnit,
    ui_scale_factor: data::ScaleFactor,
    timezone: data::UserTimezone,
    theme: data::Theme,
    notifications: Vec<Toast>,
    journal_window: Option<window::Id>,
    journal_mode: data::JournalMode,
}

#[derive(Debug, Clone)]
enum Message {
    Sidebar(dashboard::sidebar::Message),
    MarketWsEvent(exchange::Event),
    Dashboard {
        /// If `None`, the active layout is used for the event.
        layout_id: Option<uuid::Uuid>,
        event: dashboard::Message,
    },
    Tick(std::time::Instant),
    WindowEvent(window::Event),
    ExitRequested(HashMap<window::Id, WindowSpec>),
    RestartRequested(HashMap<window::Id, WindowSpec>),
    GoBack,
    DataFolderRequested,
    ThemeSelected(data::Theme),
    ScaleFactorChanged(data::ScaleFactor),
    SetTimezone(data::UserTimezone),
    SetJournalMode(data::JournalMode),
    ToggleTradeFetch(bool),
    ApplyVolumeSizeUnit(exchange::SizeUnit),
    RemoveNotification(usize),
    ToggleDialogModal(Option<screen::ConfirmDialog<Message>>),
    ThemeEditor(modal::theme_editor::Message),
    Layouts(modal::layout_manager::Message),
    AudioStream(modal::audio::Message),
    OfflineAlertsChecked(Vec<data::chart::alert::PriceAlert>),
    AutofillJournalFromActiveChart,
}

impl HawkTerminal {
    fn new() -> (Self, Task<Message>) {
        let saved_state = layout::load_saved_state();

        let (main_window_id, open_main_window) = {
            let (position, size) = saved_state.window();
            let config = window::Settings {
                size,
                position,
                exit_on_close_request: false,
                ..window::settings()
            };
            window::open(config)
        };

        let (mut sidebar, launch_sidebar) = dashboard::Sidebar::new(&saved_state);
        let open_layouts = std::env::var("HAWK_OPEN_LAYOUTS")
            .or_else(|_| std::env::var("FLOWSURFACE_OPEN_LAYOUTS"))
            .is_ok();
        if open_layouts {
            sidebar.state.set_menu(sidebar::Menu::Layout);
        }

        let (audio_stream, audio_init_err) = AudioStream::new(saved_state.audio_cfg);

        let mut state = Self {
            screen: if open_layouts {
                AppScreen::Running
            } else {
                AppScreen::Splash {
                    start: std::time::Instant::now(),
                }
            },
            main_window: window::Window::new(main_window_id),
            layout_manager: saved_state.layout_manager,
            theme_editor: ThemeEditor::new(saved_state.custom_theme),
            audio_stream,
            sidebar,
            confirm_dialog: None,
            timezone: saved_state.timezone,
            ui_scale_factor: saved_state.scale_factor,
            volume_size_unit: saved_state.volume_size_unit,
            theme: saved_state.theme,
            notifications: vec![],
            journal_window: None,
            journal_mode: saved_state.journal_mode,
        };

        let edit_layouts = std::env::var("HAWK_EDIT_LAYOUTS")
            .or_else(|_| std::env::var("FLOWSURFACE_EDIT_LAYOUTS"))
            .is_ok();
        if edit_layouts {
            state.layout_manager.edit_mode = modal::layout_manager::Editing::Preview;
        }

        if let Some(err) = audio_init_err {
            state
                .notifications
                .push(Toast::error(format!("Audio disabled: {err}")));
        }

        let active_layout_id = state.layout_manager.active_layout_id().unwrap_or(
            &state
                .layout_manager
                .layouts
                .first()
                .expect("No layouts available")
                .id,
        );
        let load_layout = state.load_layout(active_layout_id.unique, main_window_id);

        (
            state,
            open_main_window
                .discard()
                .chain(load_layout)
                .chain(launch_sidebar.map(Message::Sidebar))
                .chain(catch_up_offline_alerts_task()),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MarketWsEvent(event) => {
                let main_window_id = self.main_window.id;
                let dashboard = self.active_dashboard_mut();

                match event {
                    exchange::Event::Connected(exchange) => {
                        log::info!("a stream connected to {exchange} WS");
                    }
                    exchange::Event::Disconnected(exchange, reason) => {
                        log::info!("a stream disconnected from {exchange} WS: {reason:?}");
                    }
                    exchange::Event::DepthReceived(
                        stream,
                        depth_update_t,
                        depth,
                        trades_buffer,
                    ) => {
                        let task = dashboard
                            .update_depth_and_trades(
                                &stream,
                                depth_update_t,
                                &depth,
                                &trades_buffer,
                                main_window_id,
                            )
                            .map(move |msg| Message::Dashboard {
                                layout_id: None,
                                event: msg,
                            });

                        if let Some(msg) = self.audio_stream.try_play_sound(&stream, &trades_buffer)
                        {
                            self.notifications.push(Toast::error(msg));
                        }

                        return task;
                    }
                    exchange::Event::KlineReceived(stream, kline) => {
                        return dashboard
                            .update_latest_klines(&stream, &kline, main_window_id)
                            .map(move |msg| Message::Dashboard {
                                layout_id: None,
                                event: msg,
                            });
                    }
                }
            }
            Message::Tick(now) => {
                if let AppScreen::Splash { start } = self.screen {
                    if now.duration_since(start).as_secs() >= 3 {
                        self.screen = AppScreen::Running;
                    }
                    return Task::none();
                }

                let main_window_id = self.main_window.id;

                return self
                    .active_dashboard_mut()
                    .tick(now, main_window_id)
                    .map(move |msg| Message::Dashboard {
                        layout_id: None,
                        event: msg,
                    });
            }
            Message::WindowEvent(event) => match event {
                window::Event::CloseRequested(window) => {
                    if Some(window) == self.journal_window {
                        self.journal_window = None;
                        self.sidebar.set_journal_window_open(false);
                        self.sidebar.journal.is_shown = false;
                        return window::close(window);
                    }

                    let main_window = self.main_window.id;
                    let dashboard = self.active_dashboard_mut();

                    if window != main_window {
                        dashboard.popout.remove(&window);
                        return window::close(window);
                    }

                    let mut active_windows = dashboard
                        .popout
                        .keys()
                        .copied()
                        .collect::<Vec<window::Id>>();
                    active_windows.push(main_window);

                    return window::collect_window_specs(active_windows, Message::ExitRequested);
                }
            },
            Message::ExitRequested(windows) => {
                self.save_state_to_disk(&windows);
                return iced::exit();
            }
            Message::RestartRequested(windows) => {
                self.save_state_to_disk(&windows);
                return self.restart();
            }
            Message::GoBack => {
                let main_window = self.main_window.id;

                if self.confirm_dialog.is_some() {
                    self.confirm_dialog = None;
                } else if self.sidebar.journal.is_image_modal_open() {
                    self.sidebar
                        .journal
                        .update(screen::dashboard::journal::Message::CloseImageViewer);
                } else if self.sidebar.active_menu().is_some() {
                    self.sidebar.set_menu(None);
                } else {
                    let dashboard = self.active_dashboard_mut();

                    if dashboard.go_back(main_window) {
                        return Task::none();
                    } else if dashboard.focus.is_some() {
                        dashboard.focus = None;
                    } else {
                        self.sidebar.hide_tickers_table();
                    }
                }
            }
            Message::ThemeSelected(theme) => {
                self.theme = theme.clone();
            }
            Message::Dashboard {
                layout_id: id,
                event: msg,
            } => {
                let Some(active_layout) = self.layout_manager.active_layout_id() else {
                    log::error!("No active layout to handle dashboard message");
                    return Task::none();
                };

                let main_window = self.main_window;
                let layout_id = id.unwrap_or(active_layout.unique);

                if let Some(dashboard) = self.layout_manager.mut_dashboard(layout_id) {
                    let (main_task, event) = dashboard.update(msg, &main_window, &layout_id);

                    let additional_task = match event {
                        Some(dashboard::Event::DistributeFetchedData {
                            layout_id,
                            pane_id,
                            data,
                            stream,
                        }) => dashboard
                            .distribute_fetched_data(main_window.id, pane_id, data, stream)
                            .map(move |msg| Message::Dashboard {
                                layout_id: Some(layout_id),
                                event: msg,
                            }),
                        Some(dashboard::Event::Notification(toast)) => {
                            self.notifications.push(toast);
                            Task::none()
                        }
                        Some(dashboard::Event::ResolveStreams { pane_id, streams }) => {
                            let tickers_info = self.sidebar.tickers_info();

                            let resolved_streams =
                                streams.into_iter().try_fold(vec![], |mut acc, persist| {
                                    let resolver = |t: &exchange::Ticker| {
                                        tickers_info.get(t).and_then(|opt| *opt)
                                    };

                                    match persist.into_stream_kind(resolver) {
                                        Ok(stream) => {
                                            acc.push(stream);
                                            Ok(acc)
                                        }
                                        Err(err) => Err(format!(
                                            "Failed to resolve persisted stream: {}",
                                            err
                                        )),
                                    }
                                });

                            match resolved_streams {
                                Ok(resolved) => {
                                    if resolved.is_empty() {
                                        Task::none()
                                    } else {
                                        dashboard
                                            .resolve_streams(main_window.id, pane_id, resolved)
                                            .map(move |msg| Message::Dashboard {
                                                layout_id: None,
                                                event: msg,
                                            })
                                    }
                                }
                                Err(err) => {
                                    log::warn!("{err}",);
                                    Task::none()
                                }
                            }
                        }
                        Some(dashboard::Event::AutofillJournal(autofill)) => {
                            self.apply_journal_autofill(autofill)
                        }
                        None => Task::none(),
                    };

                    return main_task
                        .map(move |msg| Message::Dashboard {
                            layout_id: Some(layout_id),
                            event: msg,
                        })
                        .chain(additional_task);
                }
            }
            Message::RemoveNotification(index) => {
                if index < self.notifications.len() {
                    self.notifications.remove(index);
                }
            }
            Message::SetTimezone(tz) => {
                self.timezone = tz;
            }
            Message::SetJournalMode(mode) => {
                self.journal_mode = mode;
                self.sidebar.set_journal_mode(mode);
                if mode == data::JournalMode::Disabled {
                    self.sidebar.journal.is_shown = false;
                    if let Some(id) = self.journal_window.take() {
                        return window::close(id);
                    }
                } else if mode == data::JournalMode::Basic {
                    if let Some(id) = self.journal_window.take() {
                        return window::close(id);
                    }
                } else if mode == data::JournalMode::Extended {
                    self.sidebar.journal.is_shown = false;
                }
            }
            Message::ScaleFactorChanged(value) => {
                self.ui_scale_factor = value;
            }
            Message::ToggleTradeFetch(checked) => {
                self.layout_manager
                    .iter_dashboards_mut()
                    .for_each(|dashboard| {
                        dashboard.toggle_trade_fetch(checked, &self.main_window);
                    });

                if checked {
                    self.confirm_dialog = None;
                }
            }
            Message::ToggleDialogModal(dialog) => {
                self.confirm_dialog = dialog;
            }
            Message::Layouts(message) => {
                let action = self.layout_manager.update(message);

                match action {
                    Some(modal::layout_manager::Action::Select(layout)) => {
                        return self.switch_layout(layout);
                    }
                    Some(modal::layout_manager::Action::Clone(id)) => {
                        let manager = &mut self.layout_manager;

                        let source_data = manager.get(id).map(|layout| {
                            (
                                layout.id.name.clone(),
                                layout.id.unique,
                                data::Dashboard::from(&layout.dashboard),
                            )
                        });

                        if let Some((name, old_id, ser_dashboard)) = source_data {
                            let new_uid = uuid::Uuid::new_v4();
                            let new_layout = LayoutId {
                                unique: new_uid,
                                name: manager.ensure_unique_name(&name, new_uid),
                            };

                            let mut popout_windows = Vec::new();

                            for (pane, window_spec) in &ser_dashboard.popout {
                                let configuration = configuration(pane.clone());
                                popout_windows.push((configuration, *window_spec));
                            }

                            let dashboard = Dashboard::from_config(
                                configuration(ser_dashboard.pane.clone()),
                                popout_windows,
                                old_id,
                            );

                            manager.insert_layout(new_layout.clone(), dashboard);
                        }
                    }
                    Some(modal::layout_manager::Action::ExportLayout(id)) => {
                        if let Some(layout) = self.layout_manager.get(id) {
                            let ser_dashboard = data::Dashboard::from(&layout.dashboard);
                            let data_layout = data::Layout {
                                name: layout.id.name.clone(),
                                dashboard: ser_dashboard,
                            };
                            let bundle = data::ConfigBundle::new_layout(
                                data_layout,
                                Some(data::BundleMetadata {
                                    name: layout.id.name.clone(),
                                    description: None,
                                    author: None,
                                }),
                            );
                            match bundle.to_json_pretty() {
                                Ok(json) => {
                                    let sanitized_filename = layout
                                        .id
                                        .name
                                        .chars()
                                        .map(|c| {
                                            if c.is_alphanumeric() || c == '-' || c == '_' {
                                                c
                                            } else {
                                                '_'
                                            }
                                        })
                                        .collect::<String>();
                                    let filename = format!("layout_{sanitized_filename}.json");

                                    let picked_path = rfd::FileDialog::new()
                                        .set_title("Export Layout")
                                        .set_file_name(&filename)
                                        .add_filter("Hawk Layout (*.json)", &["json"])
                                        .save_file();

                                    if let Some(path) = picked_path {
                                        match std::fs::write(&path, &json) {
                                            Ok(()) => {
                                                let display_name = path
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .unwrap_or(&filename);
                                                self.notifications.push(Toast::info(format!(
                                                    "Layout '{}' exported to {display_name}",
                                                    layout.id.name
                                                )));
                                            }
                                            Err(e) => {
                                                self.notifications.push(Toast::error(format!(
                                                    "Failed to save to {}: {e}",
                                                    path.display()
                                                )));
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.notifications.push(Toast::error(format!(
                                        "Failed to serialize layout: {e}"
                                    )));
                                }
                            }
                        }
                    }
                    Some(modal::layout_manager::Action::ExportWorkspace) => {
                        let mut ser_layouts = vec![];
                        for layout in &self.layout_manager.layouts {
                            if let Some(l) = self.layout_manager.get(layout.id.unique) {
                                ser_layouts.push(data::Layout {
                                    name: l.id.name.clone(),
                                    dashboard: data::Dashboard::from(&l.dashboard),
                                });
                            }
                        }

                        let ws_bundle = data::WorkspaceBundle {
                            layouts: ser_layouts,
                            active_layout: self
                                .layout_manager
                                .active_layout_id()
                                .map(|l| l.name.clone()),
                            custom_theme: self.theme_editor.custom_theme.clone().map(data::Theme),
                            timezone: Some(self.timezone),
                            tickers_table: self.sidebar.state.tickers_table.clone(),
                            audio_cfg: Some(data::AudioStream::from(&self.audio_stream)),
                            size_in_quote_ccy: Some(self.volume_size_unit),
                            default_kline_config: data::chart::kline::user_default_kline_config(),
                            drawings: Some(data::DrawingStore::all()),
                        };

                        let bundle = data::ConfigBundle::new_workspace(
                            ws_bundle,
                            Some(data::BundleMetadata {
                                name: "Hawk Workspace".to_string(),
                                description: None,
                                author: None,
                            }),
                        );

                        match bundle.to_json_pretty() {
                            Ok(json) => {
                                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                                let filename = format!("workspace_{timestamp}.json");

                                let picked_path = rfd::FileDialog::new()
                                    .set_title("Export Workspace")
                                    .set_file_name(&filename)
                                    .add_filter("Hawk Workspace (*.json)", &["json"])
                                    .save_file();

                                if let Some(path) = picked_path {
                                    match std::fs::write(&path, &json) {
                                        Ok(()) => {
                                            let display_name = path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or(&filename);
                                            self.notifications.push(Toast::info(format!(
                                                "Workspace exported to {display_name}",
                                            )));
                                        }
                                        Err(e) => {
                                            self.notifications.push(Toast::error(format!(
                                                "Failed to save to {}: {e}",
                                                path.display()
                                            )));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                self.notifications.push(Toast::error(format!(
                                    "Failed to serialize workspace: {e}"
                                )));
                            }
                        }
                    }
                    Some(modal::layout_manager::Action::OpenExportsFolder) => {
                        if let Err(err) = data::open_exports_folder() {
                            self.notifications.push(Toast::error(format!(
                                "Failed to open exports folder: {err}"
                            )));
                        }
                    }
                    Some(modal::layout_manager::Action::ImportFromFile) => {
                        let picked_path = rfd::FileDialog::new()
                            .set_title("Import Hawk Layout or Workspace")
                            .add_filter("Hawk Config (*.json)", &["json"])
                            .pick_file();

                        if let Some(path) = picked_path {
                            match std::fs::read_to_string(&path) {
                                Err(e) => {
                                    self.notifications.push(Toast::error(format!(
                                        "Failed to read {}: {e}",
                                        path.display()
                                    )));
                                }
                                Ok(text) => match data::ConfigBundle::from_json(&text) {
                                    Err(e) => {
                                        self.notifications.push(Toast::error(format!(
                                            "Invalid layout or workspace JSON: {e}"
                                        )));
                                    }
                                    Ok(bundle) => {
                                        return self.apply_imported_bundle(bundle);
                                    }
                                },
                            }
                        }
                    }
                    Some(modal::layout_manager::Action::ImportFromClipboard) => {
                        match window::read_text_from_clipboard() {
                            Err(e) => {
                                self.notifications.push(Toast::error(format!(
                                    "Failed to read from clipboard: {e}"
                                )));
                            }
                            Ok(text) => match data::ConfigBundle::from_json(&text) {
                                Err(e) => {
                                    self.notifications.push(Toast::error(format!(
                                        "Invalid layout or workspace JSON: {e}"
                                    )));
                                }
                                Ok(bundle) => {
                                    return self.apply_imported_bundle(bundle);
                                }
                            },
                        }
                    }
                    None => {}
                }
            }
            Message::AudioStream(message) => {
                if let Some(event) = self.audio_stream.update(message) {
                    match event {
                        modal::audio::UpdateEvent::RetryFailed(err) => {
                            self.notifications
                                .push(Toast::error(format!("Audio still unavailable: {err}")));
                        }
                        modal::audio::UpdateEvent::RetrySucceeded => {
                            self.notifications
                                .push(Toast::new(toast::Notification::Info(
                                    "Audio output re-initialized successfully".to_string(),
                                )));
                        }
                    }
                }
            }
            Message::DataFolderRequested => {
                if let Err(err) = data::open_data_folder() {
                    self.notifications
                        .push(Toast::error(format!("Failed to open data folder: {err}")));
                }
            }
            Message::ThemeEditor(msg) => {
                let action = self.theme_editor.update(msg, &self.theme.clone().into());

                match action {
                    Some(modal::theme_editor::Action::Exit) => {
                        self.sidebar.set_menu(Some(sidebar::Menu::Settings));
                    }
                    Some(modal::theme_editor::Action::UpdateTheme(theme)) => {
                        self.theme = data::Theme(theme);

                        let main_window = self.main_window.id;

                        self.active_dashboard_mut()
                            .invalidate_all_panes(main_window);
                    }
                    None => {}
                }
            }
            Message::Sidebar(message) => {
                let (task, action) = self.sidebar.update(message);

                match action {
                    Some(dashboard::sidebar::Action::TickerSelected(ticker_info, content)) => {
                        let main_window_id = self.main_window.id;

                        let task = {
                            if let Some(kind) = content {
                                self.active_dashboard_mut().init_focused_pane(
                                    main_window_id,
                                    ticker_info,
                                    kind,
                                )
                            } else {
                                self.active_dashboard_mut()
                                    .switch_tickers_in_group(main_window_id, ticker_info)
                            }
                        };

                        return task.map(move |msg| Message::Dashboard {
                            layout_id: None,
                            event: msg,
                        });
                    }
                    Some(dashboard::sidebar::Action::ErrorOccurred(err)) => {
                        self.notifications.push(Toast::error(err.to_string()));
                    }
                    Some(dashboard::sidebar::Action::ToggleJournalWindow) => {
                        self.sidebar.journal.is_shown = false;
                        if let Some(id) = self.journal_window.take() {
                            self.sidebar.set_journal_window_open(false);
                            return window::close(id);
                        } else {
                            let (id, task) = window::open(window::Settings {
                                position: window::Position::Centered,
                                exit_on_close_request: false,
                                min_size: Some(iced::Size::new(960.0, 600.0)),
                                size: iced::Size::new(1440.0, 850.0),
                                ..window::settings()
                            });
                            self.journal_window = Some(id);
                            self.sidebar.set_journal_window_open(true);
                            return task.discard();
                        }
                    }
                    None => {}
                }

                return task.map(Message::Sidebar);
            }
            Message::ApplyVolumeSizeUnit(pref) => {
                self.volume_size_unit = pref;
                self.confirm_dialog = None;

                let mut active_windows: Vec<window::Id> =
                    self.active_dashboard().popout.keys().copied().collect();
                active_windows.push(self.main_window.id);

                return window::collect_window_specs(active_windows, Message::RestartRequested);
            }
            Message::OfflineAlertsChecked(triggered) => {
                for alert in triggered {
                    let time_str = if let Some(t) = alert.triggered_at {
                        chrono::DateTime::from_timestamp_millis(t as i64)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "--".to_string())
                    } else {
                        "--".to_string()
                    };
                    self.notifications.push(Toast::info(format!(
                        "🔔 Offline Price Alert: {} reached {:.2} ({}) at {}",
                        alert.ticker_symbol, alert.target_price, alert.condition, time_str
                    )));
                }
                return Task::none();
            }
            Message::AutofillJournalFromActiveChart => {
                let autofill = self.active_dashboard().find_active_position();
                if let Some(autofill) = autofill {
                    return self.apply_journal_autofill(autofill);
                } else {
                    self.notifications.push(Toast::info(
                        "No position drawing found on active chart (draw Long/Short first)",
                    ));
                    return Task::none();
                }
            }
        }
        Task::none()
    }

    fn apply_journal_autofill(
        &mut self,
        autofill: data::journal::PositionAutofill,
    ) -> Task<Message> {
        if self.journal_mode == data::JournalMode::Disabled {
            self.journal_mode = data::JournalMode::Basic;
            self.sidebar.set_journal_mode(data::JournalMode::Basic);
        }
        let ticker = autofill.ticker.clone();
        self.sidebar.journal.autofill_trade(autofill);

        if self.journal_mode == data::JournalMode::Extended {
            self.sidebar.journal.is_shown = false;
        } else {
            self.sidebar.tickers_table.is_shown = false;
            self.sidebar.journal.is_shown = true;
        }

        self.notifications.push(Toast::info(format!(
            "Loaded {} position into journal",
            ticker
        )));

        let window_task =
            if self.journal_mode == data::JournalMode::Extended && self.journal_window.is_none() {
                let (id, task) = window::open(window::Settings {
                    position: window::Position::Centered,
                    exit_on_close_request: false,
                    min_size: Some(iced::Size::new(960.0, 600.0)),
                    size: iced::Size::new(1440.0, 850.0),
                    ..window::settings()
                });
                self.journal_window = Some(id);
                self.sidebar.set_journal_window_open(true);
                task.discard()
            } else {
                Task::none()
            };

        iced::widget::operation::focus(iced::widget::Id::new("journal_size_input"))
            .chain(window_task)
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if matches!(self.screen, AppScreen::Splash { .. }) && id == self.main_window.id {
            let theme: iced::Theme = self.theme.clone().into();
            let bg_color = theme.palette().background;

            return container(text("hawk - terminal").size(48).font(iced::Font {
                family: iced::font::Family::Name("Azeret Mono"),
                weight: iced::font::Weight::Bold,
                ..Default::default()
            }))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: Some(bg_color.into()),
                ..Default::default()
            })
            .into();
        }

        let dashboard = self.active_dashboard();
        let sidebar_pos = self.sidebar.position();

        let tickers_table = &self.sidebar.tickers_table;

        let content = if id == self.main_window.id {
            let sidebar_view = self
                .sidebar
                .view(self.audio_stream.volume())
                .map(Message::Sidebar);

            let dashboard_view = dashboard
                .view(&self.main_window, tickers_table, self.timezone)
                .map(move |msg| Message::Dashboard {
                    layout_id: None,
                    event: msg,
                });

            let header_title = {
                #[cfg(target_os = "macos")]
                {
                    iced::widget::center(
                        text("HAWK TERMINAL")
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..Default::default()
                            })
                            .size(16)
                            .style(style::title_text),
                    )
                    .height(20)
                    .align_y(Alignment::Center)
                    .padding(padding::top(4))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    column![]
                }
            };

            let base = column![
                header_title,
                match sidebar_pos {
                    sidebar::Position::Left => row![sidebar_view, dashboard_view,],
                    sidebar::Position::Right => row![dashboard_view, sidebar_view],
                }
                .spacing(4)
                .padding(8),
            ];

            let content_with_menu = if let Some(menu) = self.sidebar.active_menu() {
                self.view_with_modal(base.into(), dashboard, menu)
            } else {
                base.into()
            };

            if self.journal_mode == data::JournalMode::Basic
                && self.sidebar.journal.is_shown
                && self.sidebar.journal.is_image_modal_open()
            {
                let modal = self
                    .sidebar
                    .journal
                    .view_image_modal()
                    .map(dashboard::sidebar::Message::Journal)
                    .map(Message::Sidebar);
                iced::widget::stack![content_with_menu, modal].into()
            } else {
                content_with_menu
            }
        } else if Some(id) == self.journal_window {
            container(
                self.sidebar
                    .journal
                    .view_dashboard()
                    .map(dashboard::sidebar::Message::Journal)
                    .map(Message::Sidebar),
            )
            .padding(padding::top(style::TITLE_PADDING_TOP))
            .into()
        } else {
            container(
                dashboard
                    .view_window(id, &self.main_window, tickers_table, self.timezone)
                    .map(move |msg| Message::Dashboard {
                        layout_id: None,
                        event: msg,
                    }),
            )
            .padding(padding::top(style::TITLE_PADDING_TOP))
            .into()
        };

        toast::Manager::new(
            content,
            &self.notifications,
            match sidebar_pos {
                sidebar::Position::Left => Alignment::Start,
                sidebar::Position::Right => Alignment::End,
            },
            Message::RemoveNotification,
        )
        .into()
    }

    fn theme(&self, _window: window::Id) -> iced_core::Theme {
        self.theme.clone().into()
    }

    fn title(&self, window: window::Id) -> String {
        if Some(window) == self.journal_window {
            "Hawk Terminal - Trade Journal Dashboard".to_string()
        } else if let Some(id) = self.layout_manager.active_layout_id() {
            format!("Hawk Terminal [{}]", id.name)
        } else {
            "Hawk Terminal".to_string()
        }
    }

    fn scale_factor(&self, _window: window::Id) -> f32 {
        self.ui_scale_factor.into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let window_events = window::events().map(Message::WindowEvent);
        let sidebar = self.sidebar.subscription().map(Message::Sidebar);

        let exchange_streams = self
            .active_dashboard()
            .market_subscriptions()
            .map(Message::MarketWsEvent);

        let tick = iced::time::every(std::time::Duration::from_millis(50)).map(Message::Tick);

        let hotkeys = keyboard::listen().filter_map(|event| {
            let keyboard::Event::KeyPressed {
                key,
                modifiers,
                physical_key,
                ..
            } = event
            else {
                return None;
            };
            let is_shift_g = modifiers.shift()
                && (matches!(&key, keyboard::Key::Character(c) if c.eq_ignore_ascii_case("g"))
                    || matches!(
                        physical_key,
                        keyboard::key::Physical::Code(keyboard::key::Code::KeyG)
                    ));
            if is_shift_g {
                return Some(Message::AutofillJournalFromActiveChart);
            }
            let is_ctrl_v = (modifiers.control() || modifiers.command())
                && (matches!(&key, keyboard::Key::Character(c) if c.eq_ignore_ascii_case("v"))
                    || matches!(
                        physical_key,
                        keyboard::key::Physical::Code(keyboard::key::Code::KeyV)
                    ));
            if is_ctrl_v {
                return Some(Message::Sidebar(dashboard::sidebar::Message::Journal(
                    screen::dashboard::journal::Message::PasteScreenshotFromHotkey,
                )));
            }
            match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::GoBack),
                _ => None,
            }
        });

        Subscription::batch(vec![
            exchange_streams,
            sidebar,
            window_events,
            tick,
            hotkeys,
        ])
    }

    fn active_dashboard(&self) -> &Dashboard {
        let active_layout = self
            .layout_manager
            .active_layout_id()
            .expect("No active layout");
        self.layout_manager
            .get(active_layout.unique)
            .map(|layout| &layout.dashboard)
            .expect("No active dashboard")
    }

    fn active_dashboard_mut(&mut self) -> &mut Dashboard {
        let active_layout = self
            .layout_manager
            .active_layout_id()
            .expect("No active layout");
        self.layout_manager
            .get_mut(active_layout.unique)
            .map(|layout| &mut layout.dashboard)
            .expect("No active dashboard")
    }

    fn load_layout(&mut self, layout_uid: uuid::Uuid, main_window: window::Id) -> Task<Message> {
        match self.layout_manager.set_active_layout(layout_uid) {
            Ok(layout) => {
                layout
                    .dashboard
                    .load_layout(main_window)
                    .map(move |msg| Message::Dashboard {
                        layout_id: Some(layout_uid),
                        event: msg,
                    })
            }
            Err(err) => {
                log::error!("Failed to set active layout: {}", err);
                Task::none()
            }
        }
    }

    fn switch_layout(&mut self, layout_uid: uuid::Uuid) -> Task<Message> {
        let active_popout_keys = self
            .active_dashboard()
            .popout
            .keys()
            .copied()
            .collect::<Vec<_>>();

        let window_tasks = Task::batch(
            active_popout_keys
                .iter()
                .map(|&popout_id| window::close::<window::Id>(popout_id))
                .collect::<Vec<_>>(),
        )
        .discard();

        let old_layout_id = self
            .layout_manager
            .active_layout_id()
            .as_ref()
            .map(|layout| layout.unique);

        window::collect_window_specs(active_popout_keys, dashboard::Message::SavePopoutSpecs)
            .map(move |msg| Message::Dashboard {
                layout_id: old_layout_id,
                event: msg,
            })
            .chain(window_tasks)
            .chain(self.load_layout(layout_uid, self.main_window.id))
    }

    fn apply_imported_bundle(&mut self, bundle: data::ConfigBundle) -> Task<Message> {
        match bundle.payload {
            data::BundlePayload::Layout(data_layout) => {
                let new_uid = uuid::Uuid::new_v4();
                let unique_name = self
                    .layout_manager
                    .ensure_unique_name(&data_layout.name, new_uid);
                let new_layout = LayoutId {
                    unique: new_uid,
                    name: unique_name.clone(),
                };

                let mut popout_windows = Vec::new();
                for (pane, window_spec) in &data_layout.dashboard.popout {
                    let configuration = configuration(pane.clone());
                    popout_windows.push((configuration, *window_spec));
                }

                fn extract_kline_config(pane: &data::Pane) -> Option<data::chart::kline::Config> {
                    match pane {
                        data::Pane::KlineChart { settings, .. } => {
                            settings.visual_config.as_ref().and_then(|vc| vc.kline())
                        }
                        data::Pane::Split { a, b, .. } => {
                            extract_kline_config(a).or_else(|| extract_kline_config(b))
                        }
                        _ => None,
                    }
                }

                if let Some(cfg) = extract_kline_config(&data_layout.dashboard.pane) {
                    data::chart::kline::set_user_default_kline_config(cfg);
                }

                let dashboard = Dashboard::from_config(
                    configuration(data_layout.dashboard.pane.clone()),
                    popout_windows,
                    new_uid,
                );

                self.layout_manager.insert_layout(new_layout, dashboard);
                self.notifications
                    .push(Toast::info(format!("Imported layout '{unique_name}'")));

                self.switch_layout(new_uid)
            }
            data::BundlePayload::Workspace(ws_bundle) => {
                let count = ws_bundle.layouts.len();
                let mut first_uid = None;
                let mut active_target_uid = None;

                for data_layout in ws_bundle.layouts {
                    let new_uid = uuid::Uuid::new_v4();
                    if first_uid.is_none() {
                        first_uid = Some(new_uid);
                    }
                    let unique_name = self
                        .layout_manager
                        .ensure_unique_name(&data_layout.name, new_uid);

                    if let Some(ref target_name) = ws_bundle.active_layout
                        && target_name == &data_layout.name
                    {
                        active_target_uid = Some(new_uid);
                    }

                    let new_layout = LayoutId {
                        unique: new_uid,
                        name: unique_name,
                    };

                    let mut popout_windows = Vec::new();
                    for (pane, window_spec) in &data_layout.dashboard.popout {
                        let configuration = configuration(pane.clone());
                        popout_windows.push((configuration, *window_spec));
                    }

                    let dashboard = Dashboard::from_config(
                        configuration(data_layout.dashboard.pane.clone()),
                        popout_windows,
                        new_uid,
                    );

                    self.layout_manager.insert_layout(new_layout, dashboard);
                }

                if let Some(custom_theme) = ws_bundle.custom_theme {
                    self.theme_editor.custom_theme = Some(custom_theme.0);
                }

                if let Some(kline_cfg) = ws_bundle.default_kline_config {
                    data::chart::kline::set_user_default_kline_config(kline_cfg);
                }

                if let Some(drawings) = ws_bundle.drawings {
                    for rec in drawings {
                        data::DrawingStore::add(rec.ticker_symbol, rec.drawing);
                    }
                }

                self.notifications.push(Toast::info(format!(
                    "Imported {count} layout(s) from workspace"
                )));

                if let Some(uid) = active_target_uid.or(first_uid) {
                    self.switch_layout(uid)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn view_with_modal<'a>(
        &'a self,
        base: Element<'a, Message>,
        dashboard: &'a Dashboard,
        menu: sidebar::Menu,
    ) -> Element<'a, Message> {
        let sidebar_pos = self.sidebar.position();

        match menu {
            sidebar::Menu::Settings => {
                let settings_modal = {
                    let theme_picklist = {
                        let default_theme = iced_core::Theme::Custom(default_theme().into());
                        let deeptrades = iced_core::Theme::Custom(
                            data::config::theme::deeptrades_theme().into(),
                        );
                        let flowsurface_classic = iced_core::Theme::Custom(
                            data::config::theme::flowsurface_legacy_theme().into(),
                        );

                        let mut themes: Vec<iced::Theme> =
                            vec![default_theme, deeptrades, flowsurface_classic];

                        if let Some(custom_theme) = &self.theme_editor.custom_theme {
                            themes.push(custom_theme.clone());
                        }

                        themes.extend(iced_core::Theme::ALL.iter().cloned());

                        pick_list(themes, Some(self.theme.0.clone()), |theme| {
                            Message::ThemeSelected(data::Theme(theme))
                        })
                    };

                    let toggle_theme_editor = button(text("Theme editor")).on_press(
                        Message::Sidebar(dashboard::sidebar::Message::ToggleSidebarMenu(Some(
                            sidebar::Menu::ThemeEditor,
                        ))),
                    );

                    let timezone_picklist = pick_list(
                        [data::UserTimezone::Utc, data::UserTimezone::Local],
                        Some(self.timezone),
                        Message::SetTimezone,
                    );

                    let journal_mode_picklist = pick_list(
                        data::JournalMode::ALL,
                        Some(self.journal_mode),
                        Message::SetJournalMode,
                    );

                    let size_in_quote_currency_checkbox = {
                        let is_active = match self.volume_size_unit {
                            exchange::SizeUnit::Quote => true,
                            exchange::SizeUnit::Base => false,
                        };

                        let checkbox = iced::widget::checkbox(is_active)
                            .label("Size in quote currency")
                            .on_toggle(|checked| {
                                let on_dialog_confirm = Message::ApplyVolumeSizeUnit(if checked {
                                    exchange::SizeUnit::Quote
                                } else {
                                    exchange::SizeUnit::Base
                                });

                                let confirm_dialog = screen::ConfirmDialog::new(
                                    "Changing size display currency requires application restart"
                                        .to_string(),
                                    Box::new(on_dialog_confirm.clone()),
                                )
                                .with_confirm_btn_text("Restart now".to_string());

                                Message::ToggleDialogModal(Some(confirm_dialog))
                            });

                        tooltip(
                            checkbox,
                            Some(
                                "Display sizes/volumes in quote currency (USD)\nHas no effect on inverse perps or open interest",
                            ),
                            TooltipPosition::Top,
                        )
                    };

                    let sidebar_pos = pick_list(
                        [sidebar::Position::Left, sidebar::Position::Right],
                        Some(sidebar_pos),
                        |pos| {
                            Message::Sidebar(dashboard::sidebar::Message::SetSidebarPosition(pos))
                        },
                    );

                    let scale_factor = {
                        let current_value: f32 = self.ui_scale_factor.into();

                        let decrease_btn = if current_value > data::config::MIN_SCALE {
                            button(text("-"))
                                .on_press(Message::ScaleFactorChanged((current_value - 0.1).into()))
                        } else {
                            button(text("-"))
                        };

                        let increase_btn = if current_value < data::config::MAX_SCALE {
                            button(text("+"))
                                .on_press(Message::ScaleFactorChanged((current_value + 0.1).into()))
                        } else {
                            button(text("+"))
                        };

                        container(
                            row![
                                decrease_btn,
                                text(format!("{:.0}%", current_value * 100.0)).size(14),
                                increase_btn,
                            ]
                            .align_y(Alignment::Center)
                            .spacing(8)
                            .padding(4),
                        )
                        .style(style::modal_container)
                    };

                    let trade_fetch_checkbox = {
                        let is_active = exchange::fetcher::is_trade_fetch_enabled();

                        let checkbox = iced::widget::checkbox(is_active)
                            .label("Fetch trades")
                            .on_toggle(|checked| {
                                if checked {
                                    let confirm_dialog = screen::ConfirmDialog::new(
                                        "Fetching historical trades may download large archives and take some time to complete. Proceed?"
                                            .to_string(),
                                        Box::new(Message::ToggleTradeFetch(true)),
                                    );
                                    Message::ToggleDialogModal(Some(confirm_dialog))
                                } else {
                                    Message::ToggleTradeFetch(false)
                                }
                            });

                        tooltip(
                            checkbox,
                            Some(
                                "Fetch historical and intraday trades for footprint charts (Binance, Bybit, OKX, Hyperliquid)",
                            ),
                            TooltipPosition::Top,
                        )
                    };

                    let open_data_folder = {
                        let button =
                            button(text("Open data folder")).on_press(Message::DataFolderRequested);

                        tooltip(
                            button,
                            Some("Open the folder where the data & config is stored"),
                            TooltipPosition::Top,
                        )
                    };

                    let column_content = split_column![
                        column![open_data_folder,].spacing(8),
                        column![text("Sidebar position").size(14), sidebar_pos,].spacing(12),
                        column![text("Time zone").size(14), timezone_picklist,].spacing(12),
                        column![text("Trade journal").size(14), journal_mode_picklist,].spacing(12),
                        column![text("Market data").size(14), size_in_quote_currency_checkbox,].spacing(12),
                        column![text("Theme").size(14), theme_picklist,].spacing(12),
                        column![text("Interface scale").size(14), scale_factor,].spacing(12),
                        column![
                            text("Experimental").size(14),
                            column![trade_fetch_checkbox, toggle_theme_editor,].spacing(8),
                        ]
                        .spacing(12),
                        ; spacing = 16, align_x = Alignment::Start
                    ];

                    let content = scrollable::Scrollable::with_direction(
                        column_content,
                        scrollable::Direction::Vertical(
                            scrollable::Scrollbar::new().width(8).scroller_width(6),
                        ),
                    );

                    container(content)
                        .align_x(Alignment::Start)
                        .max_width(240)
                        .padding(24)
                        .style(style::dashboard_modal)
                };

                let (align_x, padding) = match sidebar_pos {
                    sidebar::Position::Left => (Alignment::Start, padding::left(44).bottom(4)),
                    sidebar::Position::Right => (Alignment::End, padding::right(44).bottom(4)),
                };

                let base_content = dashboard_modal(
                    base,
                    settings_modal,
                    Message::Sidebar(dashboard::sidebar::Message::ToggleSidebarMenu(None)),
                    padding,
                    Alignment::End,
                    align_x,
                );

                if let Some(dialog) = &self.confirm_dialog {
                    let dialog_content =
                        confirm_dialog_container(dialog.clone(), Message::ToggleDialogModal(None));

                    main_dialog_modal(
                        base_content,
                        dialog_content,
                        Message::ToggleDialogModal(None),
                    )
                } else {
                    base_content
                }
            }
            sidebar::Menu::Layout => {
                let main_window = self.main_window.id;

                let manage_pane = if let Some((window_id, pane_id)) = dashboard.focus {
                    let selected_pane_str =
                        if let Some(state) = dashboard.get_pane(main_window, window_id, pane_id) {
                            let link_group_name: String =
                                state.link_group.as_ref().map_or_else(String::new, |g| {
                                    " - Group ".to_string() + &g.to_string()
                                });

                            state.content.to_string() + &link_group_name
                        } else {
                            "".to_string()
                        };

                    let is_main_window = window_id == main_window;

                    let reset_pane_button = {
                        let btn = button(text("Reset").align_x(Alignment::Center))
                            .width(iced::Length::Fill);
                        if is_main_window {
                            let dashboard_msg = Message::Dashboard {
                                layout_id: None,
                                event: dashboard::Message::Pane(
                                    main_window,
                                    dashboard::pane::Message::ReplacePane(pane_id),
                                ),
                            };

                            btn.on_press(dashboard_msg)
                        } else {
                            btn
                        }
                    };
                    let split_pane_button = {
                        let btn = button(text("Split").align_x(Alignment::Center))
                            .width(iced::Length::Fill);
                        if is_main_window {
                            let dashboard_msg = Message::Dashboard {
                                layout_id: None,
                                event: dashboard::Message::Pane(
                                    main_window,
                                    dashboard::pane::Message::SplitPane(
                                        pane_grid::Axis::Horizontal,
                                        pane_id,
                                    ),
                                ),
                            };
                            btn.on_press(dashboard_msg)
                        } else {
                            btn
                        }
                    };

                    column![
                        text(selected_pane_str),
                        row![
                            tooltip(
                                reset_pane_button,
                                if is_main_window {
                                    Some("Reset selected pane")
                                } else {
                                    None
                                },
                                TooltipPosition::Top,
                            ),
                            tooltip(
                                split_pane_button,
                                if is_main_window {
                                    Some("Split selected pane horizontally")
                                } else {
                                    None
                                },
                                TooltipPosition::Top,
                            ),
                        ]
                        .spacing(8)
                    ]
                    .spacing(8)
                } else {
                    column![text("No pane selected"),].spacing(8)
                };

                let manage_layout_modal = {
                    let col = column![
                        manage_pane,
                        rule::horizontal(1.0).style(style::split_ruler),
                        self.layout_manager.view().map(Message::Layouts)
                    ];

                    container(col.align_x(Alignment::Center).spacing(20))
                        .width(280)
                        .padding(24)
                        .style(style::dashboard_modal)
                };

                let (align_x, padding) = match sidebar_pos {
                    sidebar::Position::Left => (Alignment::Start, padding::left(44).top(40)),
                    sidebar::Position::Right => (Alignment::End, padding::right(44).top(40)),
                };

                dashboard_modal(
                    base,
                    manage_layout_modal,
                    Message::Sidebar(dashboard::sidebar::Message::ToggleSidebarMenu(None)),
                    padding,
                    Alignment::Start,
                    align_x,
                )
            }
            sidebar::Menu::Audio => {
                let (align_x, padding) = match sidebar_pos {
                    sidebar::Position::Left => (Alignment::Start, padding::left(44).top(76)),
                    sidebar::Position::Right => (Alignment::End, padding::right(44).top(76)),
                };

                let depth_streams_list = dashboard.streams.depth_streams(None);

                dashboard_modal(
                    base,
                    self.audio_stream
                        .view(depth_streams_list)
                        .map(Message::AudioStream),
                    Message::Sidebar(dashboard::sidebar::Message::ToggleSidebarMenu(None)),
                    padding,
                    Alignment::Start,
                    align_x,
                )
            }
            sidebar::Menu::ThemeEditor => {
                let (align_x, padding) = match sidebar_pos {
                    sidebar::Position::Left => (Alignment::Start, padding::left(44).bottom(4)),
                    sidebar::Position::Right => (Alignment::End, padding::right(44).bottom(4)),
                };

                dashboard_modal(
                    base,
                    self.theme_editor
                        .view(&self.theme.0)
                        .map(Message::ThemeEditor),
                    Message::Sidebar(dashboard::sidebar::Message::ToggleSidebarMenu(None)),
                    padding,
                    Alignment::End,
                    align_x,
                )
            }
        }
    }

    fn save_state_to_disk(&mut self, windows: &HashMap<window::Id, WindowSpec>) {
        let main_window_id = self.main_window.id;
        for dashboard in self.layout_manager.iter_dashboards_mut() {
            dashboard.flush_all_footprint_caches(main_window_id);
        }

        self.active_dashboard_mut()
            .popout
            .iter_mut()
            .for_each(|(id, (_, window_spec))| {
                if let Some(new_window_spec) = windows.get(id) {
                    *window_spec = *new_window_spec;
                }
            });

        self.sidebar.sync_tickers_table_settings();

        let mut ser_layouts = vec![];
        for layout in &self.layout_manager.layouts {
            if let Some(layout) = self.layout_manager.get(layout.id.unique) {
                let serialized_dashboard = data::Dashboard::from(&layout.dashboard);
                ser_layouts.push(data::Layout {
                    name: layout.id.name.clone(),
                    dashboard: serialized_dashboard,
                });
            }
        }

        let layouts = data::Layouts {
            layouts: ser_layouts,
            active_layout: self
                .layout_manager
                .active_layout_id()
                .map(|layout| layout.name.to_string())
                .clone(),
        };

        let main_window_spec = windows
            .iter()
            .find(|(id, _)| **id == self.main_window.id)
            .map(|(_, spec)| *spec);

        let audio_cfg = data::AudioStream::from(&self.audio_stream);

        let state = data::State::from_parts(
            layouts,
            self.theme.clone(),
            self.theme_editor.custom_theme.clone().map(data::Theme),
            main_window_spec,
            self.timezone,
            self.sidebar.state.clone(),
            self.ui_scale_factor,
            audio_cfg,
            self.volume_size_unit,
            self.journal_mode,
            data::chart::kline::user_default_kline_config(),
        );

        // Persist alerts to alerts.json
        data::AlertStore::save();

        // Persist drawings to drawings.json
        data::DrawingStore::save();

        match serde_json::to_string(&state) {
            Ok(layout_str) => {
                let file_name = data::SAVED_STATE_PATH;
                if let Err(e) = data::write_json_to_file(&layout_str, file_name) {
                    log::error!("Failed to write layout state to file: {}", e);
                } else {
                    log::info!("Persisted state to {file_name}");
                }
            }
            Err(e) => log::error!("Failed to serialize layout: {}", e),
        }
    }

    fn restart(&mut self) -> Task<Message> {
        let mut windows_to_close: Vec<window::Id> =
            self.active_dashboard().popout.keys().copied().collect();
        windows_to_close.push(self.main_window.id);

        let close_windows = Task::batch(
            windows_to_close
                .into_iter()
                .map(window::close)
                .collect::<Vec<_>>(),
        );

        let (new_state, init_task) = HawkTerminal::new();
        *self = new_state;

        close_windows.chain(init_task)
    }
}

fn catch_up_offline_alerts_task() -> Task<Message> {
    Task::perform(
        async {
            let active_alerts = data::AlertStore::all()
                .into_iter()
                .filter(|a| a.status == data::chart::alert::AlertStatus::Active)
                .collect::<Vec<_>>();

            if active_alerts.is_empty() {
                return Vec::new();
            }

            let mut unique_tickers: HashMap<exchange::Ticker, (u64, String)> = HashMap::new();
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let max_lookback_ms = 30 * 24 * 60 * 60 * 1000; // 30 days max lookback
            let min_allowed_start = now.saturating_sub(max_lookback_ms);

            for alert in &active_alerts {
                if let Some(ticker) = alert.ticker {
                    let alert_start = alert
                        .last_checked_time
                        .unwrap_or(alert.created_at)
                        .max(min_allowed_start);
                    unique_tickers
                        .entry(ticker)
                        .and_modify(|(earliest, _)| *earliest = (*earliest).min(alert_start))
                        .or_insert((alert_start, alert.ticker_symbol.clone()));
                }
            }

            let mut all_triggered = Vec::new();

            for (ticker, (start_time, _)) in unique_tickers {
                let duration_ms = now.saturating_sub(start_time);
                let timeframe = if duration_ms > 24 * 60 * 60 * 1000 {
                    exchange::Timeframe::H1
                } else {
                    exchange::Timeframe::M1
                };

                let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
                match exchange::adapter::fetch_klines(
                    ticker_info,
                    timeframe,
                    Some((start_time, now)),
                )
                .await
                {
                    Ok(klines) => {
                        let triggered = data::AlertStore::check_offline_klines(&ticker, &klines);
                        all_triggered.extend(triggered);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to fetch offline klines for {}: {e:?}",
                            ticker.display_symbol_and_type().0
                        );
                    }
                }
            }

            all_triggered
        },
        Message::OfflineAlertsChecked,
    )
}
