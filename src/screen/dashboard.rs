pub mod journal;
pub mod pane;
pub mod panel;
pub mod sidebar;
pub mod tickers_table;

pub use sidebar::Sidebar;

use super::DashboardError;
use crate::{
    chart::{self, Chart},
    screen::dashboard::tickers_table::TickersTable,
    style,
    widget::toast::Toast,
    window::{self, Window},
};
use data::{
    UserTimezone,
    layout::{WindowSpec, pane::ContentKind},
};
use exchange::{
    Kline, PushFrequency, StreamPairKind, TickMultiplier, TickerInfo, Timeframe, Trade,
    adapter::{
        self, AdapterError, Exchange, PersistStreamKind, ResolvedStream, StreamConfig, StreamKind,
        StreamTicksize, UniqueStreams, binance, bitcoincounterflow, bybit, hyperliquid, mexc, okex,
    },
    depth::Depth,
    fetcher::{FetchRange, FetchedData, NetOiInterval},
};

use iced::{
    Element, Length, Subscription, Task, Vector,
    task::Straw,
    widget::{
        PaneGrid, center, container,
        pane_grid::{self, Configuration},
    },
};
use iced_futures::futures::TryFutureExt;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant, vec};

#[derive(Debug, Clone)]
pub enum Message {
    Pane(window::Id, pane::Message),
    ChangePaneStatus(uuid::Uuid, pane::Status),
    TradeFetchBatchDone(uuid::Uuid, TickerInfo, u64, u64),
    SavePopoutSpecs(HashMap<window::Id, WindowSpec>),
    ErrorOccurred(Option<uuid::Uuid>, DashboardError),
    Notification(Toast),
    DistributeFetchedData {
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
        stream: StreamKind,
        data: FetchedData,
    },
    ResolveStreams(uuid::Uuid, Vec<PersistStreamKind>),
}

pub struct Dashboard {
    pub panes: pane_grid::State<pane::State>,
    pub focus: Option<(window::Id, pane_grid::Pane)>,
    pub last_focused_kline: Option<(window::Id, pane_grid::Pane)>,
    pub popout: HashMap<window::Id, (pane_grid::State<pane::State>, WindowSpec)>,
    pub streams: UniqueStreams,
    layout_id: uuid::Uuid,
}

impl Default for Dashboard {
    fn default() -> Self {
        let layout_id = uuid::Uuid::new_v4();
        let mut panes = pane_grid::State::with_configuration(Self::default_pane_config());
        for (_, p_state) in panes.iter_mut() {
            p_state.layout_id = Some(layout_id);
        }
        Self {
            panes,
            focus: None,
            last_focused_kline: None,
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Notification(Toast),
    DistributeFetchedData {
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
        data: FetchedData,
        stream: StreamKind,
    },
    ResolveStreams {
        pane_id: uuid::Uuid,
        streams: Vec<PersistStreamKind>,
    },
    AutofillJournal(data::journal::PositionAutofill),
}

impl Dashboard {
    fn default_pane_config() -> Configuration<pane::State> {
        Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: 0.8,
            a: Box::new(Configuration::Split {
                axis: pane_grid::Axis::Horizontal,
                ratio: 0.4,
                a: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(pane::State::default())),
                    b: Box::new(Configuration::Pane(pane::State::default())),
                }),
                b: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(pane::State::default())),
                    b: Box::new(Configuration::Pane(pane::State::default())),
                }),
            }),
            b: Box::new(Configuration::Pane(pane::State::default())),
        }
    }

    pub fn from_config(
        panes: Configuration<pane::State>,
        popout_windows: Vec<(Configuration<pane::State>, WindowSpec)>,
        layout_id: uuid::Uuid,
    ) -> Self {
        let mut panes = pane_grid::State::with_configuration(panes);
        for (_, p_state) in panes.iter_mut() {
            p_state.layout_id = Some(layout_id);
        }

        let mut popout = HashMap::new();

        if window::MULTI_WINDOW_SUPPORTED {
            for (pane, specs) in popout_windows {
                let mut pop_panes = pane_grid::State::with_configuration(pane);
                for (_, p_state) in pop_panes.iter_mut() {
                    p_state.layout_id = Some(layout_id);
                }
                popout.insert(window::Id::unique(), (pop_panes, specs));
            }
        }

        Self {
            panes,
            focus: None,
            last_focused_kline: None,
            streams: UniqueStreams::default(),
            popout,
            layout_id,
        }
    }

    pub fn find_active_position(&self) -> Option<data::journal::PositionAutofill> {
        let check_pane = |win_id: window::Id,
                          pane: pane_grid::Pane|
         -> Option<data::journal::PositionAutofill> {
            let pane_state = self.panes.get(pane).or_else(|| {
                self.popout
                    .get(&win_id)
                    .and_then(|(panes, _)| panes.get(pane))
            });
            if let Some(pane_state) = pane_state
                && let pane::Content::Kline {
                    chart: Some(kline), ..
                } = &pane_state.content
            {
                kline.find_target_position()
            } else {
                None
            }
        };

        // 1. Try last focused kline pane
        if let Some((win_id, pane)) = self.last_focused_kline
            && let Some(autofill) = check_pane(win_id, pane)
        {
            return Some(autofill);
        }
        // 2. Try current focus pane
        if let Some((win_id, pane)) = self.focus
            && let Some(autofill) = check_pane(win_id, pane)
        {
            return Some(autofill);
        }
        // 3. Search all panes in self.panes
        for (_pane, pane_state) in self.panes.iter() {
            if let pane::Content::Kline {
                chart: Some(kline), ..
            } = &pane_state.content
                && let Some(autofill) = kline.find_target_position()
            {
                return Some(autofill);
            }
        }
        // 4. Search popouts
        for (popout_panes, _) in self.popout.values() {
            for (_pane, pane_state) in popout_panes.iter() {
                if let pane::Content::Kline {
                    chart: Some(kline), ..
                } = &pane_state.content
                    && let Some(autofill) = kline.find_target_position()
                {
                    return Some(autofill);
                }
            }
        }
        None
    }

    pub fn toggle_replay_play_pause_active(&mut self) -> bool {
        let check_pane = |pane_state: &mut pane::State| -> bool {
            if pane_state.modal.is_some() {
                return false;
            }
            if let pane::Content::Kline {
                chart: Some(kline), ..
            } = &mut pane_state.content
                && kline.is_replay_active()
            {
                kline.replay_play_pause();
                return true;
            }
            false
        };

        // 1. Try last focused kline pane
        if let Some((win_id, pane)) = self.last_focused_kline {
            let pane_state = self.panes.get_mut(pane).or_else(|| {
                self.popout
                    .get_mut(&win_id)
                    .and_then(|(panes, _)| panes.get_mut(pane))
            });
            if let Some(pane_state) = pane_state
                && check_pane(pane_state)
            {
                return true;
            }
        }
        // 2. Try current focus pane
        if let Some((win_id, pane)) = self.focus {
            let pane_state = self.panes.get_mut(pane).or_else(|| {
                self.popout
                    .get_mut(&win_id)
                    .and_then(|(panes, _)| panes.get_mut(pane))
            });
            if let Some(pane_state) = pane_state
                && check_pane(pane_state)
            {
                return true;
            }
        }
        // 3. Search all panes in self.panes
        for (_pane, pane_state) in self.panes.iter_mut() {
            if check_pane(pane_state) {
                return true;
            }
        }
        // 4. Search popouts
        for (popout_panes, _) in self.popout.values_mut() {
            for (_pane, pane_state) in popout_panes.iter_mut() {
                if check_pane(pane_state) {
                    return true;
                }
            }
        }
        false
    }

    pub fn load_layout(&mut self, main_window: window::Id) -> Task<Message> {
        debug_assert!(window::MULTI_WINDOW_SUPPORTED || self.popout.is_empty());

        let mut open_popouts_tasks: Vec<Task<Message>> = vec![];
        let mut new_popout = Vec::new();
        let mut keys_to_remove = Vec::new();

        for (old_window_id, (_, specs)) in &self.popout {
            keys_to_remove.push((*old_window_id, *specs));
        }

        // remove keys and open new windows
        for (old_window_id, window_spec) in keys_to_remove {
            let (window, task) = window::open(window::Settings {
                position: window::Position::Specific(window_spec.position()),
                size: window_spec.size(),
                exit_on_close_request: false,
                ..window::settings()
            });

            open_popouts_tasks.push(task.then(|_| Task::none()));

            if let Some((removed_pane, specs)) = self.popout.remove(&old_window_id) {
                new_popout.push((window, (removed_pane, specs)));
            }
        }

        // assign new windows to old panes
        for (window, (pane, specs)) in new_popout {
            self.popout.insert(window, (pane, specs));
        }

        Task::batch(open_popouts_tasks).chain(self.refresh_streams(main_window))
    }

    pub fn update(
        &mut self,
        message: Message,
        main_window: &Window,
        layout_id: &uuid::Uuid,
    ) -> (Task<Message>, Option<Event>) {
        match message {
            Message::SavePopoutSpecs(specs) => {
                for (window_id, new_spec) in specs {
                    if let Some((_, spec)) = self.popout.get_mut(&window_id) {
                        *spec = new_spec;
                    }
                }
            }
            Message::ErrorOccurred(pane_id, err) => match pane_id {
                Some(id) => {
                    if let Some(state) = self.get_mut_pane_state_by_uuid(main_window.id, id) {
                        state.status = pane::Status::Ready;
                        let err_str = err.to_string();
                        let is_footprint_archive_err = err_str.contains("Hyperliquid")
                            || err_str.contains("historical archive");
                        if is_footprint_archive_err {
                            state.sync_footprint_exchange_notice();
                        } else if !state.notifications.iter().any(|t| t.body == err_str) {
                            state.notifications.push(Toast::error(err_str));
                        }
                        if let pane::Content::Kline {
                            chart: Some(chart), ..
                        } = &mut state.content
                        {
                            chart.reset_fetching_trades();
                        }
                    }
                }
                _ => {
                    return (
                        Task::done(Message::Notification(Toast::error(err.to_string()))),
                        None,
                    );
                }
            },
            Message::Pane(window, message) => match message {
                pane::Message::PaneClicked(pane) => {
                    self.set_focus(main_window.id, window, pane);
                }
                pane::Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                    if window == main_window.id {
                        self.panes.resize(split, ratio);
                    } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                        popout_panes.resize(split, ratio);
                    }
                }
                pane::Message::PaneDragged(event) => {
                    if let pane_grid::DragEvent::Dropped { pane, target } = event {
                        if window == main_window.id {
                            self.panes.drop(pane, target);
                        } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                            popout_panes.drop(pane, target);
                        }
                    }
                }
                pane::Message::SplitPane(axis, pane) => {
                    let mut new_state = pane::State::new();
                    new_state.layout_id = Some(self.layout_id);
                    let focus_pane = if window == main_window.id {
                        self.panes
                            .split(axis, pane, new_state)
                            .map(|(new_pane, _)| new_pane)
                    } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                        popout_panes
                            .split(axis, pane, new_state)
                            .map(|(new_pane, _)| new_pane)
                    } else {
                        None
                    };

                    if let Some(focus_pane) = focus_pane {
                        self.set_focus(main_window.id, window, focus_pane);
                    }
                }
                pane::Message::ClosePane(pane) => {
                    let sibling = if window == main_window.id {
                        self.panes.close(pane).map(|(_, s)| s)
                    } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                        popout_panes.close(pane).map(|(_, s)| s)
                    } else {
                        None
                    };
                    if let Some(sibling) = sibling {
                        self.set_focus(main_window.id, window, sibling);
                    }
                }
                pane::Message::MaximizePane(pane) => {
                    if window == main_window.id {
                        self.panes.maximize(pane);
                    } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                        popout_panes.maximize(pane);
                    }
                }
                pane::Message::Restore => {
                    if window == main_window.id {
                        self.panes.restore();
                    } else if let Some((popout_panes, _)) = self.popout.get_mut(&window) {
                        popout_panes.restore();
                    }
                }
                pane::Message::ReplacePane(pane) => {
                    let target = if window == main_window.id {
                        self.panes.get_mut(pane)
                    } else {
                        self.popout
                            .get_mut(&window)
                            .and_then(|(p, _)| p.get_mut(pane))
                    };
                    if let Some(pane) = target {
                        *pane = pane::State::new();
                    }

                    return (self.refresh_streams(main_window.id), None);
                }
                pane::Message::VisualConfigChanged(pane, cfg, to_sync) => {
                    if let data::layout::pane::VisualConfig::Kline(ref kline_cfg) = cfg {
                        data::chart::kline::set_user_default_kline_config(*kline_cfg);
                    }
                    if to_sync {
                        if let Some(state) = self.get_pane(main_window.id, window, pane) {
                            let studies_cfg = state.content.studies();
                            let clusters_cfg = state.content.clusters();

                            self.iter_all_panes_mut(main_window.id)
                                .for_each(|(_, _, state)| {
                                    let should_apply = match state.settings.visual_config {
                                        Some(ref current_cfg) => {
                                            std::mem::discriminant(current_cfg)
                                                == std::mem::discriminant(&cfg)
                                        }
                                        None => matches!(
                                            (&cfg, &state.content),
                                            (
                                                data::layout::pane::VisualConfig::Kline(_),
                                                pane::Content::Kline { .. }
                                            ) | (
                                                data::layout::pane::VisualConfig::Heatmap(_),
                                                pane::Content::Heatmap { .. }
                                                    | pane::Content::ShaderHeatmap { .. }
                                            ) | (
                                                data::layout::pane::VisualConfig::TimeAndSales(_),
                                                pane::Content::TimeAndSales(_)
                                            ) | (
                                                data::layout::pane::VisualConfig::Comparison(_),
                                                pane::Content::Comparison(_)
                                            )
                                        ),
                                    };

                                    if should_apply {
                                        state.settings.visual_config = Some(cfg.clone());
                                        state.content.change_visual_config(cfg.clone());

                                        state.apply_synced_settings(&studies_cfg, &clusters_cfg);
                                    }
                                });
                        }
                    } else if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        state.settings.visual_config = Some(cfg.clone());
                        state.content.change_visual_config(cfg);
                    }
                }
                pane::Message::SwitchLinkGroup(pane, group) => {
                    if group.is_none() {
                        if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                            state.link_group = None;
                        }
                        return (Task::none(), None);
                    }

                    let maybe_ticker_info = self
                        .iter_all_panes(main_window.id)
                        .filter(|(w, p, _)| !(*w == window && *p == pane))
                        .find_map(|(_, _, other_state)| {
                            if other_state.link_group == group {
                                other_state.stream_pair()
                            } else {
                                None
                            }
                        });

                    if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        state.link_group = group;
                        state.modal = None;

                        if let Some(ticker_info) = maybe_ticker_info
                            && state.stream_pair() != Some(ticker_info)
                        {
                            let pane_id = state.unique_id();
                            let content_kind = state.content.kind();

                            let streams =
                                state.set_content_and_streams(vec![ticker_info], content_kind);
                            self.streams.extend(streams.iter());

                            for stream in &streams {
                                if let StreamKind::Kline { .. } = stream {
                                    return (
                                        kline_fetch_task(*layout_id, pane_id, *stream, None, None),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
                pane::Message::Popout => {
                    return (self.popout_pane(main_window), None);
                }
                pane::Message::Merge => {
                    return (self.merge_pane(main_window), None);
                }
                pane::Message::PaneEvent(pane, local) => {
                    let is_passive = matches!(
                        &local,
                        pane::Event::ChartInteraction(
                            chart::Message::CrosshairMoved | chart::Message::BoundsChanged(_)
                        )
                    );
                    if !is_passive {
                        self.set_focus(main_window.id, window, pane);
                        if let pane::Event::ContentSelected(
                            ContentKind::CandlestickChart
                            | ContentKind::TpoChart
                            | ContentKind::FootprintChart,
                        ) = &local
                        {
                            self.last_focused_kline = Some((window, pane));
                        }
                    }
                    if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        let Some(effect) = state.update(local) else {
                            return (Task::none(), None);
                        };

                        let task = match effect {
                            pane::Effect::RefreshStreams => self.refresh_streams(main_window.id),
                            pane::Effect::RequestFetch(reqs) => request_fetch_many(
                                state,
                                *layout_id,
                                reqs.into_iter().map(|r| (r.req_id, r.fetch, r.stream)),
                            )
                            .chain(self.refresh_streams(main_window.id)),
                            pane::Effect::SwitchTickersInGroup(ticker_info) => {
                                self.switch_tickers_in_group(main_window.id, ticker_info)
                            }
                            pane::Effect::FocusWidget(id) => {
                                return (iced::widget::operation::focus(id), None);
                            }
                            pane::Effect::TakeScreenshot(window_id) => {
                                let task = window::screenshot(window_id).map(|screenshot| {
                                    match window::copy_screenshot_to_clipboard(&screenshot) {
                                        Ok(()) => Message::Notification(Toast::info(
                                            "Screenshot copied to clipboard",
                                        )),
                                        Err(e) => Message::Notification(Toast::error(format!(
                                            "Screenshot failed: {e}"
                                        ))),
                                    }
                                });
                                return (task, None);
                            }
                            pane::Effect::AutofillJournal(autofill) => {
                                return (Task::none(), Some(Event::AutofillJournal(autofill)));
                            }
                            pane::Effect::SyncDrawing {
                                symbol,
                                drawing,
                                originating_pane_id,
                            } => {
                                self.sync_drawing_to_panes(
                                    main_window.id,
                                    &symbol,
                                    &drawing,
                                    originating_pane_id,
                                );
                                Task::none()
                            }
                            pane::Effect::SyncDrawings {
                                symbol,
                                drawings,
                                originating_pane_id,
                            } => {
                                for drawing in drawings {
                                    self.sync_drawing_to_panes(
                                        main_window.id,
                                        &symbol,
                                        &drawing,
                                        originating_pane_id,
                                    );
                                }
                                Task::none()
                            }
                            pane::Effect::UnsyncDrawing {
                                symbol,
                                drawing_id,
                                originating_pane_id,
                            } => {
                                self.unsync_drawing_from_other_panes(
                                    main_window.id,
                                    &symbol,
                                    drawing_id,
                                    originating_pane_id,
                                );
                                Task::none()
                            }
                            pane::Effect::DeleteSyncedDrawing { symbol, drawing_id } => {
                                self.delete_synced_drawing_from_panes(
                                    main_window.id,
                                    &symbol,
                                    drawing_id,
                                );
                                Task::none()
                            }
                            pane::Effect::ClearSyncedDrawings {
                                symbol,
                                drawing_ids,
                            } => {
                                for drawing_id in drawing_ids {
                                    self.delete_synced_drawing_from_panes(
                                        main_window.id,
                                        &symbol,
                                        drawing_id,
                                    );
                                }
                                Task::none()
                            }
                            pane::Effect::DeleteAlert(id) => {
                                self.iter_all_panes_mut(main_window.id)
                                    .for_each(|(_, _, p)| {
                                        if let pane::Content::Kline { chart: Some(c), .. } =
                                            &mut p.content
                                            && c.alerts.iter().any(|a| a.id == id)
                                        {
                                            c.remove_alert(id);
                                            c.invalidate_all();
                                        }
                                    });
                                Task::none()
                            }
                            pane::Effect::ToggleAlert(id) => {
                                self.iter_all_panes_mut(main_window.id)
                                    .for_each(|(_, _, p)| {
                                        if let pane::Content::Kline { chart: Some(c), .. } =
                                            &mut p.content
                                            && c.alerts.iter().any(|a| a.id == id)
                                        {
                                            c.toggle_alert(id);
                                            c.invalidate_all();
                                        }
                                    });
                                Task::none()
                            }
                            pane::Effect::ClearTriggeredAlerts => {
                                self.iter_all_panes_mut(main_window.id).for_each(|(_, _, p)| {
                                    if let pane::Content::Kline { chart: Some(c), .. } = &mut p.content {
                                        c.alerts.retain(|a| a.status != data::chart::alert::AlertStatus::Triggered);
                                        c.invalidate_all();
                                    }
                                });
                                Task::none()
                            }
                        };
                        return (task, None);
                    }
                }
            },
            Message::ChangePaneStatus(pane_id, status) => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window.id, pane_id) {
                    let is_ready = matches!(status, pane::Status::Ready);
                    pane_state.status = status;
                    if is_ready
                        && let pane::Content::Kline {
                            chart: Some(chart), ..
                        } = &mut pane_state.content
                        && !chart.is_fetching_trades()
                    {
                        chart.reset_fetching_trades();
                    }
                }
            }
            Message::TradeFetchBatchDone(pane_id, batch_ti, from_time, actual_last_trade_t) => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window.id, pane_id) {
                    if let pane::Content::Kline {
                        chart: Some(chart), ..
                    } = &mut pane_state.content
                    {
                        if chart.ticker_info.ticker != batch_ti.ticker {
                            log::warn!(
                                "Dropping stale TradeFetchBatchDone for {:?}, current chart is {:?}",
                                batch_ti.ticker,
                                chart.ticker_info.ticker
                            );
                            if !chart.is_fetching_trades() {
                                pane_state.status = pane::Status::Ready;
                            }
                            return (Default::default(), None);
                        }
                        chart.commit_fetched_trades(from_time, actual_last_trade_t);
                        chart.mark_trades_fetched(from_time, actual_last_trade_t);
                        chart.finish_one_trade_fetch();
                        chart.flush_footprint_cache();
                        let _ = exchange::trades::cache::flush_all_intraday_trades_cache();
                        chart.invalidate_all();
                        if !chart.is_fetching_trades() {
                            pane_state.status = pane::Status::Ready;
                        }
                    } else {
                        pane_state.status = pane::Status::Ready;
                    }
                }
            }
            Message::DistributeFetchedData {
                layout_id,
                pane_id,
                data,
                stream,
            } => {
                return (
                    Task::none(),
                    Some(Event::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }),
                );
            }
            Message::ResolveStreams(pane_id, streams) => {
                return (
                    Task::none(),
                    Some(Event::ResolveStreams { pane_id, streams }),
                );
            }
            Message::Notification(toast) => {
                return (Task::none(), Some(Event::Notification(toast)));
            }
        }

        (Task::none(), None)
    }

    fn new_pane(
        &mut self,
        axis: pane_grid::Axis,
        main_window: &Window,
        pane_state: Option<pane::State>,
    ) -> Task<Message> {
        if self
            .focus
            .filter(|(window, _)| *window == main_window.id)
            .is_some()
        {
            // If there is any focused pane on main window, split it
            return self.split_pane(axis, main_window);
        } else {
            // If there is no focused pane, split the last pane or create a new empty grid
            let pane = self.panes.iter().last().map(|(pane, _)| pane).copied();

            if let Some(pane) = pane {
                let result = self.panes.split(axis, pane, pane_state.unwrap_or_default());

                if let Some((pane, _)) = result {
                    return self.focus_pane(main_window.id, pane);
                }
            } else {
                let (state, pane) = pane_grid::State::new(pane_state.unwrap_or_default());
                self.panes = state;

                return self.focus_pane(main_window.id, pane);
            }
        }

        Task::none()
    }

    fn focus_pane(&mut self, window: window::Id, pane: pane_grid::Pane) -> Task<Message> {
        self.set_focus(window, window, pane);
        Task::none()
    }

    fn split_pane(&mut self, axis: pane_grid::Axis, main_window: &Window) -> Task<Message> {
        if let Some((window, pane)) = self.focus
            && window == main_window.id
        {
            let mut new_state = pane::State::new();
            new_state.layout_id = Some(self.layout_id);
            let result = self.panes.split(axis, pane, new_state);

            if let Some((pane, _)) = result {
                return self.focus_pane(main_window.id, pane);
            }
        }

        Task::none()
    }

    fn popout_pane(&mut self, main_window: &Window) -> Task<Message> {
        if window::MULTI_WINDOW_SUPPORTED
            && let Some((_, id)) = self.focus.take()
            && let Some((pane, _)) = self.panes.close(id)
        {
            let (window, task) = window::open(window::Settings {
                position: main_window
                    .position
                    .map(|point| window::Position::Specific(point + Vector::new(20.0, 20.0)))
                    .unwrap_or_default(),
                exit_on_close_request: false,
                min_size: Some(iced::Size::new(400.0, 300.0)),
                ..window::settings()
            });

            let (state, id) = pane_grid::State::new(pane);
            self.popout.insert(window, (state, WindowSpec::default()));

            return task.then(move |window| {
                Task::done(Message::Pane(window, pane::Message::PaneClicked(id)))
            });
        }

        Task::none()
    }

    fn merge_pane(&mut self, main_window: &Window) -> Task<Message> {
        if let Some((window, pane)) = self.focus.take()
            && let Some(pane_state) = self
                .popout
                .remove(&window)
                .and_then(|(mut panes, _)| panes.panes.remove(&pane))
        {
            let task = self.new_pane(pane_grid::Axis::Horizontal, main_window, Some(pane_state));

            return Task::batch(vec![window::close(window), task]);
        }

        Task::none()
    }

    pub fn get_pane(
        &self,
        main_window: window::Id,
        window: window::Id,
        pane: pane_grid::Pane,
    ) -> Option<&pane::State> {
        if main_window == window {
            self.panes.get(pane)
        } else {
            self.popout
                .get(&window)
                .and_then(|(panes, _)| panes.get(pane))
        }
    }

    fn get_mut_pane(
        &mut self,
        main_window: window::Id,
        window: window::Id,
        pane: pane_grid::Pane,
    ) -> Option<&mut pane::State> {
        if main_window == window {
            self.panes.get_mut(pane)
        } else {
            self.popout
                .get_mut(&window)
                .and_then(|(panes, _)| panes.get_mut(pane))
        }
    }

    fn get_mut_pane_state_by_uuid(
        &mut self,
        main_window: window::Id,
        uuid: uuid::Uuid,
    ) -> Option<&mut pane::State> {
        self.iter_all_panes_mut(main_window)
            .find(|(_, _, state)| state.unique_id() == uuid)
            .map(|(_, _, state)| state)
    }

    fn iter_all_panes(
        &self,
        main_window: window::Id,
    ) -> impl Iterator<Item = (window::Id, pane_grid::Pane, &pane::State)> {
        self.panes
            .iter()
            .map(move |(pane, state)| (main_window, *pane, state))
            .chain(self.popout.iter().flat_map(|(window_id, (panes, _))| {
                panes.iter().map(|(pane, state)| (*window_id, *pane, state))
            }))
    }

    fn iter_all_panes_mut(
        &mut self,
        main_window: window::Id,
    ) -> impl Iterator<Item = (window::Id, pane_grid::Pane, &mut pane::State)> {
        self.panes
            .iter_mut()
            .map(move |(pane, state)| (main_window, *pane, state))
            .chain(self.popout.iter_mut().flat_map(|(window_id, (panes, _))| {
                panes
                    .iter_mut()
                    .map(|(pane, state)| (*window_id, *pane, state))
            }))
    }

    pub fn flush_all_footprint_caches(&mut self, main_window: window::Id) {
        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, state)| {
                if let pane::Content::Kline {
                    chart: Some(chart), ..
                } = &mut state.content
                {
                    chart.flush_footprint_cache();
                }
            });
    }

    pub fn sync_drawing_to_panes(
        &mut self,
        main_window: window::Id,
        target_symbol: &str,
        drawing: &data::chart::drawing::Drawing,
        originating_pane_id: uuid::Uuid,
    ) {
        for (_, _, state) in self.iter_all_panes_mut(main_window) {
            let is_other_pane = state.unique_id() != originating_pane_id;
            if let pane::Content::Kline {
                chart: Some(chart), ..
            } = &mut state.content
            {
                let symbol = chart.ticker_info.ticker.display_symbol_and_type().0;
                if symbol == target_symbol {
                    let mut d = drawing.clone();
                    if is_other_pane {
                        d.is_selected = false;
                    }
                    chart.update_or_add_drawing(d);
                }
            }
        }
    }

    pub fn unsync_drawing_from_other_panes(
        &mut self,
        main_window: window::Id,
        target_symbol: &str,
        drawing_id: uuid::Uuid,
        originating_pane_id: uuid::Uuid,
    ) {
        for (_, _, state) in self.iter_all_panes_mut(main_window) {
            let is_other_pane = state.unique_id() != originating_pane_id;
            if is_other_pane
                && let pane::Content::Kline {
                    chart: Some(chart), ..
                } = &mut state.content
            {
                let symbol = chart.ticker_info.ticker.display_symbol_and_type().0;
                if symbol == target_symbol {
                    chart.delete_drawing(drawing_id);
                }
            }
        }
    }

    pub fn delete_synced_drawing_from_panes(
        &mut self,
        main_window: window::Id,
        target_symbol: &str,
        drawing_id: uuid::Uuid,
    ) {
        for (_, _, state) in self.iter_all_panes_mut(main_window) {
            if let pane::Content::Kline {
                chart: Some(chart), ..
            } = &mut state.content
            {
                let symbol = chart.ticker_info.ticker.display_symbol_and_type().0;
                if symbol == target_symbol {
                    chart.delete_drawing(drawing_id);
                }
            }
        }
    }

    fn set_focus(&mut self, main_window: window::Id, window: window::Id, pane: pane_grid::Pane) {
        self.focus = Some((window, pane));
        if let Some(state) = self.get_pane(main_window, window, pane)
            && matches!(state.content, pane::Content::Kline { .. })
        {
            self.last_focused_kline = Some((window, pane));
        }
    }

    pub fn active_kline_pane(
        &self,
        main_window_id: window::Id,
    ) -> Option<(window::Id, pane_grid::Pane)> {
        let is_kline = |win: window::Id, pane: pane_grid::Pane| -> bool {
            self.get_pane(main_window_id, win, pane)
                .is_some_and(|p| matches!(p.content, pane::Content::Kline { .. }))
        };

        // 1. Current focus if it is a Kline chart
        if let Some((win, pane)) = self.focus
            && is_kline(win, pane)
        {
            return Some((win, pane));
        }

        // 2. Last focused Kline chart if it is still valid
        if let Some((win, pane)) = self.last_focused_kline
            && is_kline(win, pane)
        {
            return Some((win, pane));
        }

        // 3. First initialized Kline chart across all panes
        if let Some((win, pane, _)) = self.iter_all_panes(main_window_id).find(|(_, _, state)| {
            matches!(state.content, pane::Content::Kline { chart: Some(_), .. })
        }) {
            return Some((win, pane));
        }

        // 4. First Kline chart (even if still loading) across all panes
        if let Some((win, pane, _)) = self
            .iter_all_panes(main_window_id)
            .find(|(_, _, state)| matches!(state.content, pane::Content::Kline { .. }))
        {
            return Some((win, pane));
        }

        None
    }

    pub fn view<'a>(
        &'a self,
        main_window: &'a Window,
        tickers_table: &'a TickersTable,
        timezone: UserTimezone,
    ) -> Element<'a, Message> {
        let active_toolbar_pane = self.active_kline_pane(main_window.id);

        let pane_grid: Element<_> = PaneGrid::new(&self.panes, |id, pane, maximized| {
            let is_focused = self.focus == Some((main_window.id, id));
            let show_toolbar = active_toolbar_pane == Some((main_window.id, id));
            pane.view(
                id,
                self.panes.len(),
                is_focused,
                maximized,
                main_window.id,
                main_window,
                timezone,
                tickers_table,
                show_toolbar,
            )
        })
        .min_size(240)
        .on_click(pane::Message::PaneClicked)
        .on_drag(pane::Message::PaneDragged)
        .on_resize(8, pane::Message::PaneResized)
        .spacing(6)
        .style(style::pane_grid)
        .into();

        pane_grid.map(move |message| Message::Pane(main_window.id, message))
    }

    pub fn view_window<'a>(
        &'a self,
        window: window::Id,
        main_window: &'a Window,
        tickers_table: &'a TickersTable,
        timezone: UserTimezone,
    ) -> Element<'a, Message> {
        if let Some((state, _)) = self.popout.get(&window) {
            let active_toolbar_pane = self.active_kline_pane(main_window.id);

            let content = container(
                PaneGrid::new(state, |id, pane, _maximized| {
                    let is_focused = self.focus == Some((window, id));
                    let show_toolbar = active_toolbar_pane == Some((window, id));
                    pane.view(
                        id,
                        state.len(),
                        is_focused,
                        false,
                        window,
                        main_window,
                        timezone,
                        tickers_table,
                        show_toolbar,
                    )
                })
                .on_click(pane::Message::PaneClicked),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8);

            Element::new(content).map(move |message| Message::Pane(window, message))
        } else {
            Element::new(center("No pane found for window"))
                .map(move |message| Message::Pane(window, message))
        }
    }

    pub fn go_back(&mut self, main_window: window::Id) -> bool {
        let Some((window, pane)) = self.focus else {
            return false;
        };

        let Some(state) = self.get_mut_pane(main_window, window, pane) else {
            return false;
        };

        if state.modal.is_some() {
            state.modal = None;
            return true;
        }
        false
    }

    fn handle_error(
        &mut self,
        pane_id: Option<uuid::Uuid>,
        err: &DashboardError,
        main_window: window::Id,
    ) -> Task<Message> {
        match pane_id {
            Some(id) => {
                if let Some(state) = self.get_mut_pane_state_by_uuid(main_window, id) {
                    state.status = pane::Status::Ready;
                    state.notifications.push(Toast::error(err.to_string()));
                }
                Task::none()
            }
            _ => Task::done(Message::Notification(Toast::error(err.to_string()))),
        }
    }

    /// Borrow visual config, studies, and clusters from an existing pane of the same
    /// content kind, so newly created panes inherit the full set of settings
    /// that the manual "Sync all" button would apply.
    ///
    /// `exclude` is the pane that is being initialized, it is skipped so that
    /// a freshly-set placeholder (from `ContentSelected`) doesn't shadow an
    /// already-configured pane of the same kind.
    fn borrow_synced_settings(
        &self,
        main_window: window::Id,
        content_kind: ContentKind,
        exclude: Option<(window::Id, pane_grid::Pane)>,
    ) -> Option<(
        Option<data::layout::pane::VisualConfig>,
        Option<data::chart::Study>,
        Option<data::chart::kline::ClusterKind>,
    )> {
        self.iter_all_panes(main_window)
            .find_map(|(win, pane, state)| {
                if state.content.kind() == content_kind && (exclude != Some((win, pane))) {
                    let visual_config = state.settings.visual_config.clone();
                    let studies = state.content.studies();
                    let clusters = state.content.clusters();
                    Some((visual_config, studies, clusters))
                } else {
                    None
                }
            })
    }

    fn init_pane(
        &mut self,
        main_window: window::Id,
        window: window::Id,
        selected_pane: pane_grid::Pane,
        ticker_info: TickerInfo,
        content_kind: ContentKind,
    ) -> Task<Message> {
        let synced =
            self.borrow_synced_settings(main_window, content_kind, Some((window, selected_pane)));

        let Some(state) = self.get_mut_pane(main_window, window, selected_pane) else {
            return Task::none();
        };

        if let Some((Some(cfg), _, _)) = &synced
            && (state.settings.visual_config.is_none() || state.content.kind() != content_kind)
        {
            state.settings.visual_config = Some(cfg.clone());
        } else if state.content.kind() != content_kind {
            state.settings.visual_config = None;
        }

        let pane_id = state.unique_id();
        let streams = state.set_content_and_streams(vec![ticker_info], content_kind);

        // Apply synced configs now that content is initialized
        if let Some((_, studies, clusters)) = &synced {
            state.apply_synced_settings(studies, clusters);
        }

        self.streams.extend(streams.iter());

        for stream in &streams {
            if let StreamKind::Kline { .. } = stream {
                return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
            }
        }

        Task::none()
    }

    pub fn init_focused_pane(
        &mut self,
        main_window: window::Id,
        ticker_info: TickerInfo,
        content_kind: ContentKind,
    ) -> Task<Message> {
        if self.focus.is_none()
            && self.panes.len() == 1
            && let Some((pane_id, _)) = self.panes.iter().next()
        {
            self.focus = Some((main_window, *pane_id));
        }

        let Some((window, selected_pane)) = self.focus else {
            return Task::done(Message::Notification(Toast::warn(
                "No focused pane found".to_string(),
            )));
        };

        // Inherit settings from an existing pane of the same type
        // (mirrors what the manual "Sync all" button does)
        let synced =
            self.borrow_synced_settings(main_window, content_kind, Some((window, selected_pane)));

        let Some(state) = self.get_mut_pane(main_window, window, selected_pane) else {
            return Task::done(Message::Notification(Toast::warn(
                "No focused pane found".to_string(),
            )));
        };

        if let Some((Some(cfg), _, _)) = &synced
            && (state.settings.visual_config.is_none() || state.content.kind() != content_kind)
        {
            state.settings.visual_config = Some(cfg.clone());
        } else if state.content.kind() != content_kind {
            state.settings.visual_config = None;
        }

        let previous_ticker = state.stream_pair();
        if previous_ticker.is_some() && previous_ticker != Some(ticker_info) {
            state.link_group = None;
        }

        let streams = state.set_content_and_streams(vec![ticker_info], content_kind);

        if let Some((_, studies, clusters)) = &synced {
            state.apply_synced_settings(studies, clusters);
        }

        let pane_id = state.unique_id();
        self.streams.extend(streams.iter());

        for stream in &streams {
            if let StreamKind::Kline { .. } = stream {
                return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
            }
        }

        Task::none()
    }

    pub fn switch_tickers_in_group(
        &mut self,
        main_window: window::Id,
        ticker_info: TickerInfo,
    ) -> Task<Message> {
        if self.focus.is_none()
            && self.panes.len() == 1
            && let Some((pane_id, _)) = self.panes.iter().next()
        {
            self.focus = Some((main_window, *pane_id));
        }

        let link_group = self.focus.and_then(|(window, pane)| {
            self.get_pane(main_window, window, pane)
                .and_then(|state| state.link_group)
        });

        if let Some(group) = link_group {
            let pane_infos: Vec<(window::Id, pane_grid::Pane, ContentKind)> = self
                .iter_all_panes_mut(main_window)
                .filter_map(|(window, pane, state)| {
                    if state.link_group == Some(group) {
                        Some((window, pane, state.content.kind()))
                    } else {
                        None
                    }
                })
                .collect();

            let tasks: Vec<Task<Message>> = pane_infos
                .iter()
                .map(|(window, pane, content_kind)| {
                    self.init_pane(main_window, *window, *pane, ticker_info, *content_kind)
                })
                .collect();

            Task::batch(tasks)
        } else if let Some((window, pane)) = self.focus {
            if let Some(state) = self.get_mut_pane(main_window, window, pane) {
                let content_kind = state.content.kind();
                self.init_focused_pane(main_window, ticker_info, content_kind)
            } else {
                Task::done(Message::Notification(Toast::warn(
                    "Couldn't get focused pane's content".to_string(),
                )))
            }
        } else {
            Task::done(Message::Notification(Toast::warn(
                "No link group or focused pane found".to_string(),
            )))
        }
    }

    pub fn toggle_trade_fetch(&mut self, is_enabled: bool, main_window: &Window) {
        exchange::fetcher::toggle_trade_fetch(is_enabled);

        self.iter_all_panes_mut(main_window.id)
            .for_each(|(_, _, state)| {
                if let pane::Content::Kline { chart, kind, .. } = &mut state.content
                    && matches!(kind, data::chart::KlineChartKind::Footprint { .. })
                    && let Some(c) = chart
                {
                    c.reset_request_handler();

                    if !is_enabled {
                        state.status = pane::Status::Ready;
                    }
                }
                state.sync_footprint_exchange_notice();
            });
    }

    pub fn set_trade_history_lookback(&mut self, days: u32, main_window: &Window) {
        exchange::fetcher::set_trade_history_lookback_days(days);

        self.iter_all_panes_mut(main_window.id)
            .for_each(|(_, _, state)| {
                if let pane::Content::Kline { chart, kind, .. } = &mut state.content
                    && matches!(kind, data::chart::KlineChartKind::Footprint { .. })
                    && let Some(c) = chart
                {
                    c.reset_request_handler();
                    c.invalidate_unfetched_empty_footprints();
                    c.invalidate_all();
                }
                state.sync_footprint_exchange_notice();
            });
    }

    pub fn distribute_fetched_data(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        data: FetchedData,
        stream_type: StreamKind,
    ) -> Task<Message> {
        match data {
            FetchedData::Trades { batch, .. } => {
                let stream_ti = stream_type.ticker_info();
                if !batch.is_empty()
                    && let Err(reason) =
                        self.insert_fetched_trades(main_window, pane_id, stream_ti, &batch)
                {
                    return self.handle_error(Some(pane_id), &reason, main_window);
                }
            }
            FetchedData::Klines { data, req_id } => {
                let mut triggered = Vec::new();
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline {
                        timeframe,
                        ticker_info,
                    } = stream_type
                    {
                        pane_state.insert_hist_klines(req_id, timeframe, ticker_info, &data);
                        triggered =
                            data::AlertStore::check_offline_klines(&ticker_info.ticker, &data);
                    }
                }

                if !triggered.is_empty() {
                    let mut seen_ids = std::collections::HashSet::new();
                    for alert in &triggered {
                        if seen_ids.insert(alert.id) {
                            let notif_body = format!(
                                "{} reached {:.2} ({})",
                                alert.ticker_symbol, alert.target_price, alert.condition
                            );
                            crate::notification::send_desktop_notification(
                                "🔔 Price Alert",
                                &notif_body,
                            );
                        }
                    }

                    self.iter_all_panes_mut(main_window).for_each(|(_, _, ps)| {
                        let pid = ps.unique_id();
                        let mut to_add = Vec::new();
                        if let pane::Content::Kline { chart: Some(c), .. } = &mut ps.content {
                            for alert in &triggered {
                                let matches_pane = alert.is_synced
                                    || alert.pane_id.is_none()
                                    || alert.pane_id == Some(pid);
                                if matches_pane && c.alerts.iter().any(|a| a.id == alert.id) {
                                    c.alerts.iter_mut().for_each(|a| {
                                        if a.id == alert.id {
                                            a.status = data::chart::alert::AlertStatus::Triggered;
                                            a.triggered_at = alert.triggered_at;
                                        }
                                    });
                                    c.invalidate_all();
                                    to_add.push(Toast::info(format!(
                                        "🔔 Price Alert: {} reached {:.2} ({})",
                                        alert.ticker_symbol, alert.target_price, alert.condition
                                    )));
                                }
                            }
                        }
                        if !to_add.is_empty() {
                            ps.notifications.extend(to_add);
                        }
                    });
                }
            }
            FetchedData::OI { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline { .. } = stream_type {
                        pane_state.insert_hist_oi(req_id, &data);
                    }
                }
            }
            FetchedData::FundingRates { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline { .. } = stream_type {
                        pane_state.insert_funding_rates(req_id, &data);
                    }
                }
            }
            FetchedData::SpotKlines { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline { .. } = stream_type {
                        pane_state.insert_spot_klines(req_id, &data);
                    }
                }
            }
            FetchedData::NetOiData { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline { .. } = stream_type {
                        pane_state.insert_net_oi_data(req_id, &data);
                    }
                }
            }
        }

        Task::none()
    }

    fn insert_fetched_trades(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        stream_ti: TickerInfo,
        trades: &[Trade],
    ) -> Result<(), DashboardError> {
        let pane_state = self
            .get_mut_pane_state_by_uuid(main_window, pane_id)
            .ok_or_else(|| {
                DashboardError::Unknown(
                    "No matching pane state found for fetched trades".to_string(),
                )
            })?;

        match &mut pane_state.status {
            pane::Status::Loading(exchange::fetcher::InfoKind::FetchingTrades(count)) => {
                *count += trades.len();
            }
            _ => {
                pane_state.status = pane::Status::Loading(
                    exchange::fetcher::InfoKind::FetchingTrades(trades.len()),
                );
            }
        }

        match &mut pane_state.content {
            pane::Content::Kline { chart, .. } => {
                if let Some(c) = chart {
                    if c.ticker_info.ticker != stream_ti.ticker {
                        log::warn!(
                            "Dropping stale fetched trades for {:?}, current chart is {:?}",
                            stream_ti.ticker,
                            c.ticker_info.ticker
                        );
                        return Ok(());
                    }
                    c.insert_fetched_trades_buffered(trades);
                    Ok(())
                } else {
                    Err(DashboardError::Unknown(
                        "fetched trades but no chart found".to_string(),
                    ))
                }
            }
            _ => Err(DashboardError::Unknown(
                "No matching chart found for fetched trades".to_string(),
            )),
        }
    }

    pub fn update_latest_klines(
        &mut self,
        stream: &StreamKind,
        kline: &Kline,
        main_window: window::Id,
    ) -> Task<Message> {
        let mut found_match = false;
        let mut all_triggered = Vec::new();

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                if pane_state.matches_stream(stream) {
                    match &mut pane_state.content {
                        pane::Content::Kline { chart: Some(c), .. } => {
                            let prev_p = c.current_price().unwrap_or(kline.close.to_f32_lossy());
                            c.update_latest_kline(kline);
                            let cur_p = kline.close.to_f32_lossy();
                            let low_p = kline.low.to_f32_lossy();
                            let high_p = kline.high.to_f32_lossy();
                            let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                            let triggered = data::AlertStore::check_price_triggers_range(
                                &symbol, prev_p, low_p, high_p, cur_p,
                            );
                            all_triggered.extend(triggered);
                        }
                        pane::Content::Comparison(Some(c)) => {
                            c.update_latest_kline(&stream.ticker_info(), kline);
                        }
                        _ => {}
                    }
                    found_match = true;
                }
            });

        if !all_triggered.is_empty() {
            let mut seen_ids = std::collections::HashSet::new();
            for alert in &all_triggered {
                if seen_ids.insert(alert.id) {
                    let notif_body = format!(
                        "{} reached {:.2} ({})",
                        alert.ticker_symbol, alert.target_price, alert.condition
                    );
                    crate::notification::send_desktop_notification("🔔 Price Alert", &notif_body);
                }
            }

            self.iter_all_panes_mut(main_window)
                .for_each(|(_, _, pane_state)| {
                    let pane_id = pane_state.unique_id();
                    let mut triggered_toasts = Vec::new();
                    if let pane::Content::Kline { chart: Some(c), .. } = &mut pane_state.content {
                        for alert in &all_triggered {
                            let matches_pane = alert.is_synced
                                || alert.pane_id.is_none()
                                || alert.pane_id == Some(pane_id);
                            if matches_pane && c.alerts.iter().any(|a| a.id == alert.id) {
                                c.alerts.iter_mut().for_each(|a| {
                                    if a.id == alert.id {
                                        a.status = data::chart::alert::AlertStatus::Triggered;
                                        a.triggered_at = alert.triggered_at;
                                    }
                                });
                                c.invalidate_all();
                                let notif_body = format!(
                                    "{} reached {:.2} ({})",
                                    alert.ticker_symbol, alert.target_price, alert.condition
                                );
                                triggered_toasts
                                    .push(Toast::info(format!("🔔 Price Alert: {notif_body}")));
                            }
                        }
                    }
                    if !triggered_toasts.is_empty() {
                        pane_state.notifications.extend(triggered_toasts);
                    }
                });
        }

        if found_match {
            Task::none()
        } else {
            log::debug!("{stream:?} stream had no matching panes - dropping");
            self.refresh_streams(main_window)
        }
    }

    pub fn update_depth_and_trades(
        &mut self,
        stream: &StreamKind,
        depth_update_t: u64,
        depth: &Arc<Depth>,
        trades_buffer: &[Trade],
        main_window: window::Id,
    ) -> Task<Message> {
        let mut found_match = false;
        let mut all_triggered = Vec::new();

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                let exact_match = pane_state.matches_stream(stream);
                let trade_match = pane_state.matches_trades(stream);

                if exact_match || (trade_match && !trades_buffer.is_empty()) {
                    match &mut pane_state.content {
                        pane::Content::Heatmap { chart: Some(c), .. } if exact_match => {
                            c.insert_datapoint(trades_buffer, depth_update_t, Arc::clone(depth));
                        }
                        pane::Content::ShaderHeatmap { chart: Some(c), .. } if exact_match => {
                            c.insert_datapoint(trades_buffer, depth_update_t, depth);
                        }
                        pane::Content::Kline { chart: Some(c), .. } => {
                            if exact_match {
                                c.insert_depth(depth_update_t, depth);
                            }
                            if !trades_buffer.is_empty() {
                                let prev_p = c.current_price();
                                c.insert_trades_buffer(trades_buffer);
                                let cur_p = c.current_price();
                                if let (Some(prev), Some(cur)) = (prev_p, cur_p) {
                                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                                    let mut min_p = prev.min(cur);
                                    let mut max_p = prev.max(cur);
                                    for t in trades_buffer {
                                        let p = t.price.to_f32_lossy();
                                        if p < min_p {
                                            min_p = p;
                                        }
                                        if p > max_p {
                                            max_p = p;
                                        }
                                    }
                                    let triggered = data::AlertStore::check_price_triggers_range(
                                        &symbol, prev, min_p, max_p, cur,
                                    );
                                    all_triggered.extend(triggered);
                                }
                            }
                        }
                        pane::Content::TimeAndSales(Some(p)) if !trades_buffer.is_empty() => {
                            p.insert_buffer_tagged(&stream.ticker_info(), trades_buffer);
                        }
                        pane::Content::Ladder(Some(panel)) if exact_match => {
                            panel.insert_buffers(depth_update_t, depth, trades_buffer);
                        }
                        _ => {}
                    }
                    found_match = true;
                }
            });

        if !all_triggered.is_empty() {
            let mut seen_ids = std::collections::HashSet::new();
            for alert in &all_triggered {
                if seen_ids.insert(alert.id) {
                    let notif_body = format!(
                        "{} reached {:.2} ({})",
                        alert.ticker_symbol, alert.target_price, alert.condition
                    );
                    crate::notification::send_desktop_notification("🔔 Price Alert", &notif_body);
                }
            }

            self.iter_all_panes_mut(main_window)
                .for_each(|(_, _, pane_state)| {
                    let pane_id = pane_state.unique_id();
                    let mut triggered_toasts = Vec::new();
                    if let pane::Content::Kline { chart: Some(c), .. } = &mut pane_state.content {
                        for alert in &all_triggered {
                            let matches_pane = alert.is_synced
                                || alert.pane_id.is_none()
                                || alert.pane_id == Some(pane_id);
                            if matches_pane && c.alerts.iter().any(|a| a.id == alert.id) {
                                c.alerts.iter_mut().for_each(|a| {
                                    if a.id == alert.id {
                                        a.status = data::chart::alert::AlertStatus::Triggered;
                                        a.triggered_at = alert.triggered_at;
                                    }
                                });
                                c.invalidate_all();
                                let notif_body = format!(
                                    "{} reached {:.2} ({})",
                                    alert.ticker_symbol, alert.target_price, alert.condition
                                );
                                triggered_toasts
                                    .push(Toast::info(format!("🔔 Price Alert: {notif_body}")));
                            }
                        }
                    }
                    if !triggered_toasts.is_empty() {
                        pane_state.notifications.extend(triggered_toasts);
                    }
                });
        }

        if found_match {
            Task::none()
        } else {
            log::debug!("No matching pane found for the stream: {stream:?}");
            self.refresh_streams(main_window)
        }
    }

    pub fn invalidate_all_panes(&mut self, main_window: window::Id) {
        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, state)| {
                let _ = state.invalidate(Instant::now());
            });
    }

    pub fn theme_updated(&mut self, main_window: window::Id, theme: &iced_core::Theme) {
        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, state)| {
                state.content.update_theme(theme);
            });
    }

    pub fn park_for_inactive_layout(&mut self, main_window: window::Id) {
        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, state)| state.park_for_inactive_layout());
    }

    pub fn tick(&mut self, now: Instant, main_window: window::Id) -> Task<Message> {
        let mut tasks = vec![];
        let layout_id = self.layout_id;
        let mut needs_refresh = false;

        self.iter_all_panes_mut(main_window)
            .for_each(|(_window_id, _pane, state)| match state.tick(now) {
                Some(pane::Action::Chart(action)) => match action {
                    chart::Action::ErrorOccurred(err) => {
                        state.status = pane::Status::Ready;
                        state.notifications.push(Toast::error(err.to_string()));
                    }
                    chart::Action::RequestFetch(reqs) => {
                        tasks.push(request_fetch_many(
                            state,
                            layout_id,
                            reqs.into_iter().map(|r| (r.req_id, r.fetch, r.stream)),
                        ));
                    }
                    chart::Action::RequestPalette => {}
                },
                Some(pane::Action::Panel(_action)) => {}
                Some(pane::Action::ResolveStreams(streams)) => {
                    tasks.push(Task::done(Message::ResolveStreams(
                        state.unique_id(),
                        streams,
                    )));
                }
                Some(pane::Action::ResolveContent) => match state.stream_pair_kind() {
                    Some(StreamPairKind::MultiSource(tickers)) => {
                        state.set_content_and_streams(tickers, state.content.kind());
                        needs_refresh = true;
                    }
                    Some(StreamPairKind::SingleSource(ticker)) => {
                        state.set_content_and_streams(vec![ticker], state.content.kind());
                        needs_refresh = true;
                    }
                    None => {}
                },
                None => {}
            });

        if needs_refresh {
            tasks.push(self.refresh_streams(main_window));
        }

        Task::batch(tasks)
    }

    pub fn resolve_streams(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        streams: Vec<StreamKind>,
    ) -> Task<Message> {
        if let Some(state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
            state.streams = ResolvedStream::Ready(streams.clone());
            if let pane::Content::Kline { kind, chart, .. } = &state.content {
                let chart_kind = chart.as_ref().map_or(kind, |c| &c.kind);
                if matches!(chart_kind, data::chart::KlineChartKind::Footprint { .. }) {
                    state.sync_footprint_streams();
                }
            }
        }
        self.refresh_streams(main_window)
    }

    pub fn market_subscriptions(&self) -> Subscription<exchange::Event> {
        let unique_streams = self
            .streams
            .combined_used()
            .flat_map(|(exchange, specs)| {
                let mut subs = vec![];

                if !specs.depth.is_empty() {
                    let depth_subs = specs
                        .depth
                        .iter()
                        .map(|(ticker, aggr, push_freq)| {
                            let tick_mltp = match aggr {
                                StreamTicksize::Client => None,
                                StreamTicksize::ServerSide(tick_mltp) => Some(*tick_mltp),
                            };
                            depth_subscription(*ticker, tick_mltp, *push_freq)
                        })
                        .collect::<Vec<_>>();

                    if !depth_subs.is_empty() {
                        subs.push(Subscription::batch(depth_subs));
                    }
                }

                let kline_params = specs
                    .kline
                    .iter()
                    .map(|(ticker, timeframe)| (*ticker, *timeframe))
                    .collect::<Vec<_>>();

                if !kline_params.is_empty() {
                    subs.push(kline_subscription(exchange, kline_params));
                }

                subs
            })
            .collect::<Vec<Subscription<exchange::Event>>>();

        Subscription::batch(unique_streams)
    }

    fn refresh_streams(&mut self, main_window: window::Id) -> Task<Message> {
        let all_pane_streams = self
            .iter_all_panes(main_window)
            .flat_map(|(_, _, pane_state)| pane_state.streams.ready_iter().into_iter().flatten());
        self.streams = UniqueStreams::from(all_pane_streams);

        Task::none()
    }
}

fn request_fetch(
    state: &mut pane::State,
    layout_id: uuid::Uuid,
    req_id: uuid::Uuid,
    fetch: FetchRange,
    stream: Option<StreamKind>,
) -> Task<Message> {
    let pane_id = state.unique_id();

    match fetch {
        FetchRange::Kline(from, to) => {
            if from >= to {
                return Task::none();
            }

            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return kline_fetch_task(
                    layout_id,
                    pane_uid,
                    stream,
                    Some(req_id),
                    Some((from, to)),
                );
            }
        }
        FetchRange::OpenInterest(from, to) => {
            if from >= to {
                return Task::none();
            }

            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return oi_fetch_task(layout_id, pane_uid, stream, Some(req_id), Some((from, to)));
            }
        }
        FetchRange::Trades(from_time, to_time) => {
            let trade_info = stream
                .and_then(|s| match s {
                    StreamKind::DepthAndTrades { ticker_info, .. } => {
                        Some((ticker_info, pane_id, s))
                    }
                    _ => None,
                })
                .or_else(|| {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::DepthAndTrades { ticker_info, .. } = stream {
                            Some((*ticker_info, pane_id, *stream))
                        } else {
                            None
                        }
                    })
                })
                .or_else(|| {
                    let ti = if let pane::Content::Kline { chart: Some(c), .. } = &state.content {
                        Some(c.ticker_info)
                    } else {
                        state.stream_pair()
                    };
                    ti.map(|ti| {
                        let depth_aggr = if ti.exchange().is_depth_client_aggr() {
                            StreamTicksize::Client
                        } else {
                            StreamTicksize::ServerSide(
                                state
                                    .settings
                                    .tick_multiply
                                    .unwrap_or(exchange::TickMultiplier(1)),
                            )
                        };
                        let fallback_stream = StreamKind::DepthAndTrades {
                            ticker_info: ti,
                            depth_aggr,
                            push_freq: exchange::PushFrequency::ServerDefault,
                        };
                        (ti, pane_id, fallback_stream)
                    })
                });

            if let Some((ticker_info, pane_id, stream)) = trade_info {
                state.sync_footprint_exchange_notice();
                state.sync_footprint_streams();
                let exchange_folder = match ticker_info.exchange() {
                    Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => {
                        "binance"
                    }
                    Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => "bybit",
                    Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => "okex",
                    Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => "hyperliquid",
                    Exchange::MexcSpot | Exchange::MexcLinear | Exchange::MexcInverse => "mexc",
                };
                let data_path = data::data_path(Some(&format!("market_data/{exchange_folder}/")));

                let (task, handle) = Task::sip(
                    fetch_trades_batched(ticker_info, from_time, to_time, data_path),
                    move |batch| {
                        let data = FetchedData::Trades {
                            batch,
                            until_time: to_time,
                        };
                        Message::DistributeFetchedData {
                            layout_id,
                            pane_id,
                            data,
                            stream,
                        }
                    },
                    move |result| match result {
                        Ok(actual_last_trade_t) => Message::TradeFetchBatchDone(
                            pane_id,
                            ticker_info,
                            from_time,
                            actual_last_trade_t,
                        ),
                        Err(err) => Message::ErrorOccurred(
                            Some(pane_id),
                            DashboardError::Fetch(err.to_string()),
                        ),
                    },
                )
                .abortable();

                if let pane::Content::Kline { chart, .. } = &mut state.content
                    && let Some(c) = chart
                {
                    c.add_trade_fetch_handle(handle.abort_on_drop());
                }

                return task;
            }
        }
        FetchRange::FundingRate(from, to) => {
            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return funding_fetch_task(
                    layout_id,
                    pane_uid,
                    stream,
                    Some(req_id),
                    Some((from, to)),
                );
            }
        }
        FetchRange::SpotKline(from, to) => {
            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return spot_kline_fetch_task(
                    layout_id,
                    pane_uid,
                    stream,
                    Some(req_id),
                    Some((from, to)),
                );
            }
        }
        FetchRange::NetOiData { days, interval } => {
            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return net_oi_fetch_task(
                    layout_id,
                    pane_uid,
                    stream,
                    Some(req_id),
                    days,
                    interval,
                );
            }
        }
    }

    Task::none()
}

fn request_fetch_many(
    state: &mut pane::State,
    layout_id: uuid::Uuid,
    reqs: impl IntoIterator<Item = (uuid::Uuid, FetchRange, Option<StreamKind>)>,
) -> Task<Message> {
    let tasks = reqs
        .into_iter()
        .map(|(req_id, fetch, stream)| request_fetch(state, layout_id, req_id, fetch, stream))
        .collect::<Vec<_>>();
    Task::batch(tasks)
}

fn oi_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingOI),
    ));

    let fetch_task = match stream {
        StreamKind::Kline {
            ticker_info,
            timeframe,
        } => Task::perform(
            adapter::fetch_open_interest(ticker_info.ticker, timeframe, range)
                .map_err(|err| format!("{err}")),
            move |result| match result {
                Ok(oi) => {
                    let data = FetchedData::OI { data: oi, req_id };
                    Message::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }
                }
                Err(err) => Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err)),
            },
        ),
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

fn funding_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingMarketPulse),
    ));

    let fetch_task = match stream {
        StreamKind::Kline { ticker_info, .. } => {
            let ticker = ticker_info.ticker;
            Task::perform(
                binance::fetch_funding_rates(ticker, range, Some(500))
                    .map_err(|err| format!("{err}")),
                move |result| match result {
                    Ok(rates) => {
                        let data = FetchedData::FundingRates {
                            data: rates,
                            req_id,
                        };
                        Message::DistributeFetchedData {
                            layout_id,
                            pane_id,
                            data,
                            stream,
                        }
                    }
                    Err(err) => Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err)),
                },
            )
        }
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

fn spot_kline_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingMarketPulse),
    ));

    let fetch_task = match stream {
        StreamKind::Kline {
            ticker_info,
            timeframe,
        } => {
            // Convert perp ticker to spot symbol
            if let Some(spot_symbol) = binance::perp_to_spot_symbol(&ticker_info.ticker) {
                Task::perform(
                    async move {
                        binance::fetch_spot_klines(&spot_symbol, timeframe, range, Some(500))
                            .await
                            .map_err(|err| format!("{err}"))
                    },
                    move |result| match result {
                        Ok(klines) => {
                            let data = FetchedData::SpotKlines {
                                data: klines,
                                req_id,
                            };
                            Message::DistributeFetchedData {
                                layout_id,
                                pane_id,
                                data,
                                stream,
                            }
                        }
                        Err(err) => {
                            Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err))
                        }
                    },
                )
            } else {
                Task::none()
            }
        }
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

fn net_oi_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    days: u16,
    interval: NetOiInterval,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingNetOi),
    ));

    let fetch_task = Task::perform(
        async move {
            bitcoincounterflow::fetch_net_oi_data(days, interval)
                .await
                .map_err(|err| format!("{err}"))
        },
        move |result| match result {
            Ok(data) => {
                let data = FetchedData::NetOiData { data, req_id };
                Message::DistributeFetchedData {
                    layout_id,
                    pane_id,
                    data,
                    stream,
                }
            }
            Err(err) => Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err)),
        },
    );

    update_status.chain(fetch_task)
}

fn kline_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingKlines),
    ));

    let fetch_task = match stream {
        StreamKind::Kline {
            ticker_info,
            timeframe,
        } => Task::perform(
            adapter::fetch_klines(ticker_info, timeframe, range)
                .map_err(|err| err.to_user_message()),
            move |result| match result {
                Ok(klines) => {
                    let data = FetchedData::Klines {
                        data: klines,
                        req_id,
                    };
                    Message::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }
                }
                Err(err) => {
                    Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err.to_string()))
                }
            },
        ),
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

pub fn fetch_trades_batched(
    ticker_info: TickerInfo,
    from_time: u64,
    to_time: u64,
    data_path: PathBuf,
) -> impl Straw<u64, Vec<Trade>, AdapterError> {
    let server = crate::connector::fetcher::active_server_client();
    crate::connector::fetcher::fetch_trades_paged(
        server,
        ticker_info,
        from_time,
        to_time,
        data_path,
    )
}

pub fn depth_subscription(
    ticker_info: TickerInfo,
    tick_mlpt: Option<TickMultiplier>,
    push_freq: PushFrequency,
) -> Subscription<exchange::Event> {
    let exchange = ticker_info.exchange();

    let config = StreamConfig::new(ticker_info, exchange, tick_mlpt, push_freq);

    match exchange {
        Exchange::BinanceSpot | Exchange::BinanceInverse | Exchange::BinanceLinear => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                binance::connect_market_stream(cfg.id, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                bybit::connect_market_stream(cfg.id, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                hyperliquid::connect_market_stream(cfg.id, cfg.tick_mltp, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
            let builder =
                |cfg: &StreamConfig<TickerInfo>| okex::connect_market_stream(cfg.id, cfg.push_freq);
            Subscription::run_with(config, builder)
        }
        Exchange::MexcLinear | Exchange::MexcInverse | Exchange::MexcSpot => {
            let builder =
                |cfg: &StreamConfig<TickerInfo>| mexc::connect_market_stream(cfg.id, cfg.push_freq);
            Subscription::run_with(config, builder)
        }
    }
}

pub fn kline_subscription(
    exchange: Exchange,
    kline_subs: Vec<(TickerInfo, Timeframe)>,
) -> Subscription<exchange::Event> {
    let config = StreamConfig::new(kline_subs, exchange, None, PushFrequency::ServerDefault);
    match exchange {
        Exchange::BinanceSpot | Exchange::BinanceInverse | Exchange::BinanceLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                binance::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::BybitSpot | Exchange::BybitInverse | Exchange::BybitLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                bybit::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                hyperliquid::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                okex::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::MexcLinear | Exchange::MexcInverse | Exchange::MexcSpot => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                mexc::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_chart() -> crate::chart::kline::KlineChart {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &data::chart::KlineChartKind::Candles,
            None,
        )
    }

    #[test]
    fn test_single_active_toolbar_pane() {
        let mut dashboard = Dashboard::default();
        let main_window_id = window::Id::unique();

        // 1. Initially default panes are Starter; active_kline_pane returns None
        assert_eq!(dashboard.active_kline_pane(main_window_id), None);

        // 2. Set one pane to uninitialized Kline content
        let (first_pane, _) = dashboard.panes.iter().next().unwrap();
        let first_pane = *first_pane;
        if let Some(state) = dashboard.panes.get_mut(first_pane) {
            state.content = pane::Content::Kline {
                chart: None,
                indicators: Default::default(),
                layout: Default::default(),
                kind: data::chart::KlineChartKind::Candles,
            };
        }

        // Active kline pane finds the uninitialized Kline pane as fallback
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, first_pane))
        );

        // 3. Set another pane to initialized Kline chart
        let second_pane = dashboard
            .panes
            .iter()
            .find(|(p, _)| **p != first_pane)
            .map(|(p, _)| *p)
            .unwrap();
        if let Some(state) = dashboard.panes.get_mut(second_pane) {
            state.content = pane::Content::Kline {
                chart: Some(make_test_chart()),
                indicators: Default::default(),
                layout: Default::default(),
                kind: data::chart::KlineChartKind::Candles,
            };
        }

        // Active kline pane should prefer the initialized Kline chart when neither is focused
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, second_pane))
        );

        // 3b. Focusing the uninitialized pane takes priority over initialized pane
        dashboard.set_focus(main_window_id, main_window_id, first_pane);
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, first_pane))
        );

        // 4. Initialize first pane as well; when first pane is focused, focus takes priority
        if let Some(state) = dashboard.panes.get_mut(first_pane) {
            state.content = pane::Content::Kline {
                chart: Some(make_test_chart()),
                indicators: Default::default(),
                layout: Default::default(),
                kind: data::chart::KlineChartKind::Candles,
            };
        }
        dashboard.set_focus(main_window_id, main_window_id, first_pane);
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, first_pane))
        );

        // 5. If focus switches to a non-Kline pane, last_focused_kline preserves the toolbar on first_pane
        let third_pane = dashboard
            .panes
            .iter()
            .find(|(p, _)| **p != first_pane && **p != second_pane)
            .map(|(p, _)| *p)
            .unwrap();
        dashboard.focus = Some((main_window_id, third_pane));
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, first_pane))
        );

        // 6. When second_pane receives focus, the single toolbar transfers to second_pane
        dashboard.set_focus(main_window_id, main_window_id, second_pane);
        assert_eq!(
            dashboard.active_kline_pane(main_window_id),
            Some((main_window_id, second_pane))
        );
    }

    #[test]
    fn test_drawing_isolation_and_cross_pane_sync() {
        let mut dashboard = Dashboard::default();
        let main_window_id = window::Id::unique();

        // 1. Setup two panes with BTCUSDT kline charts
        let panes: Vec<pane_grid::Pane> = dashboard.panes.iter().map(|(p, _)| *p).collect();
        let p1 = panes[0];
        let p2 = panes[1];

        let p1_id = dashboard.panes.get(p1).unwrap().unique_id();
        let p2_id = dashboard.panes.get(p2).unwrap().unique_id();
        assert_ne!(p1_id, p2_id);

        dashboard.panes.get_mut(p1).unwrap().content = pane::Content::Kline {
            chart: Some(make_test_chart()),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };
        dashboard.panes.get_mut(p2).unwrap().content = pane::Content::Kline {
            chart: Some(make_test_chart()),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };

        // 2. Add an unsynced drawing to pane 1
        let mut drawing =
            data::chart::drawing::Drawing::horizontal(65000.0, [1.0, 1.0, 1.0, 1.0], 1.0);
        drawing.pane_id = Some(p1_id);
        drawing.is_synced = false;
        let drawing_id = drawing.id;

        if let pane::Content::Kline { chart: Some(c), .. } =
            &mut dashboard.panes.get_mut(p1).unwrap().content
        {
            c.add_drawing(drawing.clone());
        }

        // Verify pane 1 has 1 drawing and pane 2 has 0 drawings (isolated)
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.drawings.len(), 1);
        }
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p2).unwrap().content
        {
            assert_eq!(c.drawings.len(), 0);
        }

        // 3. Sync drawing to all charts of the same ticker
        drawing.is_synced = true;
        dashboard.sync_drawing_to_panes(main_window_id, "BTCUSDT", &drawing, p1_id);

        // Verify both panes now have the drawing
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.drawings.len(), 1);
            assert!(c.drawings[0].is_synced);
        }
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p2).unwrap().content
        {
            assert_eq!(c.drawings.len(), 1);
            assert_eq!(c.drawings[0].id, drawing_id);
            assert!(c.drawings[0].is_synced);
        }

        // 4. Unsync from other panes
        dashboard.unsync_drawing_from_other_panes(main_window_id, "BTCUSDT", drawing_id, p1_id);

        // Verify pane 1 retains drawing, pane 2 no longer has it
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.drawings.len(), 1);
        }
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p2).unwrap().content
        {
            assert_eq!(c.drawings.len(), 0);
        }

        // 5. Re-sync and then delete synced drawing
        dashboard.sync_drawing_to_panes(main_window_id, "BTCUSDT", &drawing, p1_id);
        dashboard.delete_synced_drawing_from_panes(main_window_id, "BTCUSDT", drawing_id);

        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.drawings.len(), 0);
        }
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p2).unwrap().content
        {
            assert_eq!(c.drawings.len(), 0);
        }
    }

    #[test]
    fn test_alert_isolation_between_panes_same_symbol() {
        let mut dashboard = Dashboard::default();
        let main_window_id = window::Id::unique();

        let test_symbol = format!("TEST_{}", &uuid::Uuid::new_v4().to_string()[..8]);

        // Cleanup any potential pre-existing alerts for this test symbol or legacy TESTALERT
        for a in data::AlertStore::for_symbol(&test_symbol) {
            data::AlertStore::remove(a.id);
        }
        for a in data::AlertStore::for_symbol("TESTALERT") {
            data::AlertStore::remove(a.id);
        }

        // 1. Setup two panes with the same ticker: Pane 1 (TPO) and Pane 2 (Footprint)
        let panes: Vec<pane_grid::Pane> = dashboard.panes.iter().map(|(p, _)| *p).collect();
        let p1 = panes[0];
        let p2 = panes[1];

        let p1_id = dashboard.panes.get(p1).unwrap().unique_id();
        let p2_id = dashboard.panes.get(p2).unwrap().unique_id();
        assert_ne!(p1_id, p2_id);

        let ticker_info = exchange::TickerInfo::new(
            exchange::Ticker::new(&test_symbol, exchange::adapter::Exchange::BinanceLinear),
            0.1,
            0.001,
            None,
        );

        let stream = exchange::adapter::StreamKind::Kline {
            ticker_info,
            timeframe: exchange::Timeframe::M15,
        };

        let tpo_kind = data::chart::KlineChartKind::Tpo {
            show_candles: true,
            show_letters: true,
            show_ib: true,
            show_va: true,
            show_poc: true,
            show_single_prints: true,
            tick_step: Default::default(),
            period: Default::default(),
            clusters: vec![],
            split_sessions: vec![],
            color_scheme: Default::default(),
            ib_color: Default::default(),
            poc_color: Default::default(),
            single_prints_color: Default::default(),
        };

        let footprint_kind = data::chart::KlineChartKind::Footprint {
            clusters: Default::default(),
            scaling: Default::default(),
            studies: vec![],
            show_bottom_volume: false,
            aggregate: false,
            aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
            history_lookback_days: None,
        };

        let chart_p1 = crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::M15),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &tpo_kind,
            None,
        );

        let chart_p2 = crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::M15),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &footprint_kind,
            None,
        );

        dashboard.panes.get_mut(p1).unwrap().streams =
            exchange::adapter::ResolvedStream::Ready(vec![stream]);
        dashboard.panes.get_mut(p1).unwrap().content = pane::Content::Kline {
            chart: Some(chart_p1),
            indicators: Default::default(),
            layout: Default::default(),
            kind: tpo_kind,
        };

        dashboard.panes.get_mut(p2).unwrap().streams =
            exchange::adapter::ResolvedStream::Ready(vec![stream]);
        dashboard.panes.get_mut(p2).unwrap().content = pane::Content::Kline {
            chart: Some(chart_p2),
            indicators: Default::default(),
            layout: Default::default(),
            kind: footprint_kind,
        };

        // 2. Add an alert on Pane 1 (TPO)
        let alert_price = 65000.0;
        dashboard
            .panes
            .get_mut(p1)
            .unwrap()
            .update(pane::Event::AddPriceAlert(alert_price));

        // Verify Pane 1 has 1 alert, Pane 2 has 0 alerts
        let p1_alerts_len = match &dashboard.panes.get(p1).unwrap().content {
            pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
            _ => 0,
        };
        let p2_alerts_len = match &dashboard.panes.get(p2).unwrap().content {
            pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
            _ => 0,
        };
        assert_eq!(p1_alerts_len, 1);
        assert_eq!(p2_alerts_len, 0);

        // Verify AlertStore has it bound to p1_id
        let for_p1 = data::AlertStore::for_symbol_and_pane(&test_symbol, p1_id);
        let for_p2 = data::AlertStore::for_symbol_and_pane(&test_symbol, p2_id);
        assert_eq!(for_p1.len(), 1);
        assert_eq!(for_p1[0].pane_id, Some(p1_id));
        assert_eq!(for_p2.len(), 0);

        // 3. Simulate kline reload / app startup on Pane 2 (Footprint)
        dashboard.panes.get_mut(p2).unwrap().insert_hist_klines(
            None,
            exchange::Timeframe::M15,
            ticker_info,
            &[],
        );

        // Pane 2 must STILL have 0 alerts
        let p2_alerts_len_after = match &dashboard.panes.get(p2).unwrap().content {
            pane::Content::Kline { chart: Some(c), .. } => c.alerts.len(),
            _ => 0,
        };
        assert_eq!(p2_alerts_len_after, 0);

        // 4. Initial price below alert (64000.0)
        let kline_init = exchange::Kline {
            time: 1700000000000,
            open: exchange::util::Price::from_f32(64000.0),
            high: exchange::util::Price::from_f32(64100.0),
            low: exchange::util::Price::from_f32(63900.0),
            close: exchange::util::Price::from_f32(64000.0),
            volume: (10.0, 10.0),
        };
        let _ = dashboard.update_latest_klines(&stream, &kline_init, main_window_id);

        // Next price crossing 65000.0
        let kline_trigger = exchange::Kline {
            time: 1700000060000,
            open: exchange::util::Price::from_f32(64000.0),
            high: exchange::util::Price::from_f32(65200.0),
            low: exchange::util::Price::from_f32(64000.0),
            close: exchange::util::Price::from_f32(65100.0),
            volume: (10.0, 10.0),
        };
        let _ = dashboard.update_latest_klines(&stream, &kline_trigger, main_window_id);

        // Pane 1 must receive the notification and have its alert marked Triggered
        let p1_notifications = &dashboard.panes.get(p1).unwrap().notifications;
        let p2_notifications = &dashboard.panes.get(p2).unwrap().notifications;
        assert_eq!(p1_notifications.len(), 1);
        assert!(p1_notifications[0].body.contains("Price Alert"));
        assert_eq!(
            p2_notifications.len(),
            0,
            "Pane 2 must not receive notification for Pane 1's alert"
        );

        // Clean up AlertStore for this test symbol
        for a in data::AlertStore::for_symbol(&test_symbol) {
            data::AlertStore::remove(a.id);
        }
        let after_remove = data::AlertStore::for_symbol(&test_symbol);
        assert!(after_remove.is_empty());
    }

    #[test]
    fn test_dashboard_toggle_replay_play_pause() {
        let mut p1_state = pane::State::default();
        let chart = make_test_chart();
        assert!(!chart.is_replay_active());
        p1_state.content = pane::Content::Kline {
            chart: Some(chart),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Candles,
        };

        let (panes, p1) = pane_grid::State::new(p1_state);
        let win_id = window::Id::unique();

        let mut dashboard = Dashboard {
            panes,
            focus: Some((win_id, p1)),
            last_focused_kline: Some((win_id, p1)),
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id: uuid::Uuid::new_v4(),
        };

        // When replay is not active, toggle returns false
        assert!(!dashboard.toggle_replay_play_pause_active());

        // Activate replay on pane
        if let pane::Content::Kline { chart: Some(c), .. } =
            &mut dashboard.panes.get_mut(p1).unwrap().content
        {
            c.toggle_replay();
            assert!(c.is_replay_active());
        }

        // Toggle play/pause via dashboard (Space hotkey logic)
        assert!(dashboard.toggle_replay_play_pause_active());
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert!(c.replay_state().unwrap().is_playing);
        }

        // Toggle again to pause
        assert!(dashboard.toggle_replay_play_pause_active());
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert!(!c.replay_state().unwrap().is_playing);
        }

        // When a modal is open on the pane (e.g. Settings), space does not trigger replay
        dashboard.panes.get_mut(p1).unwrap().modal = Some(crate::modal::pane::Modal::Settings);
        assert!(!dashboard.toggle_replay_play_pause_active());
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert!(!c.replay_state().unwrap().is_playing);
        }
    }

    #[test]
    fn test_stale_trade_fetch_discarded_on_ticker_mismatch() {
        let mut p1_state = pane::State::default();
        let eth_ticker =
            exchange::Ticker::new("ETHUSDT", exchange::adapter::Exchange::BinanceLinear);
        let eth_info = exchange::TickerInfo::new(eth_ticker, 0.01, 0.001, None);
        let chart = crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::H1),
            1.0,
            &[],
            Vec::new(),
            &[],
            eth_info,
            &data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
            None,
        );
        p1_state.content = pane::Content::Kline {
            chart: Some(chart),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
        };

        let (panes, p1) = pane_grid::State::new(p1_state);
        let win_id = window::Id::unique();

        let mut dashboard = Dashboard {
            panes,
            focus: Some((win_id, p1)),
            last_focused_kline: Some((win_id, p1)),
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id: uuid::Uuid::new_v4(),
        };

        let p1_uuid = dashboard.panes.get(p1).unwrap().unique_id();

        // Create SOL trade and try inserting it into ETH pane
        let sol_ticker =
            exchange::Ticker::new("SOLUSDT", exchange::adapter::Exchange::BinanceLinear);
        let sol_info = exchange::TickerInfo::new(sol_ticker, 0.01, 0.001, None);
        let sol_trade = exchange::Trade {
            time: 1700000000000,
            price: exchange::util::Price::from_f32(120.0),
            qty: 1.0,
            is_sell: false,
        };

        // 1. insert_fetched_trades with SOL must drop the trade and return Ok(())
        let res = dashboard.insert_fetched_trades(win_id, p1_uuid, sol_info, &[sol_trade]);
        assert!(res.is_ok());

        // Verify ETH chart buffer did NOT accept SOL trade
        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.raw_trades().len(), 0);
        }

        // 2. update Message::TradeFetchBatchDone with SOL must be ignored
        let main_win = Window::new(win_id);
        let layout_id = dashboard.layout_id;
        let (_task, event) = dashboard.update(
            Message::TradeFetchBatchDone(p1_uuid, sol_info, 1700000000000, 1700000001000),
            &main_win,
            &layout_id,
        );
        assert!(event.is_none());

        // 3. insert_fetched_trades with matching ETH trade is accepted
        let eth_trade = exchange::Trade {
            time: 1700000000000,
            price: exchange::util::Price::from_f32(2700.0),
            qty: 2.0,
            is_sell: false,
        };
        let res2 = dashboard.insert_fetched_trades(win_id, p1_uuid, eth_info, &[eth_trade]);
        assert!(res2.is_ok());

        if let pane::Content::Kline { chart: Some(c), .. } =
            &dashboard.panes.get(p1).unwrap().content
        {
            assert_eq!(c.raw_trades().len(), 1);
        }
    }

    #[test]
    fn test_fetch_range_trades_falls_back_to_chart_ticker_info() {
        let mut p1_state = pane::State::default();
        let btc_ticker =
            exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let btc_info = exchange::TickerInfo::new(btc_ticker, 0.1, 0.001, None);
        let chart = crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            btc_info,
            &data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
            None,
        );
        p1_state.content = pane::Content::Kline {
            chart: Some(chart),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
        };

        // Explicitly set streams to only contain Kline stream (no DepthAndTrades)
        p1_state.streams = exchange::adapter::ResolvedStream::Ready(vec![StreamKind::Kline {
            ticker_info: btc_info,
            timeframe: exchange::Timeframe::M5,
        }]);

        let layout_id = uuid::Uuid::new_v4();
        let req_id = uuid::Uuid::new_v4();
        let fetch = FetchRange::Trades(1700000000000, 1700000060000);

        // request_fetch should NOT return Task::none() or drop request,
        // and should synchronize footprint streams so DepthAndTrades is registered.
        let _task = request_fetch(&mut p1_state, layout_id, req_id, fetch, None);

        // Verify DepthAndTrades is now in p1_state.streams
        let has_depth_and_trades = p1_state.streams.find_ready_map(|s| {
            if let StreamKind::DepthAndTrades { ticker_info, .. } = s {
                Some(*ticker_info)
            } else {
                None
            }
        });
        assert_eq!(has_depth_and_trades, Some(btc_info));
    }

    #[test]
    fn test_fetch_range_trades_explicit_stream_priority() {
        let btc_ticker = exchange::Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let btc_info = TickerInfo::new(btc_ticker, 0.1, 0.001, None);
        let bybit_ticker = exchange::Ticker::new("BTCUSDT", Exchange::BybitLinear);
        let bybit_info = TickerInfo::new(bybit_ticker, 0.1, 0.001, None);

        let mut p1_state = pane::State::default();
        let chart = crate::chart::kline::KlineChart::new(
            data::chart::ViewConfig::default(),
            data::chart::Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            btc_info,
            &data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: true,
                aggregate_exchanges: vec![Exchange::BinanceLinear, Exchange::BybitLinear],
                history_lookback_days: None,
            },
            None,
        );
        p1_state.content = pane::Content::Kline {
            chart: Some(chart),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Footprint {
                clusters: Default::default(),
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: true,
                aggregate_exchanges: vec![Exchange::BinanceLinear, Exchange::BybitLinear],
                history_lookback_days: None,
            },
        };

        p1_state.streams =
            exchange::adapter::ResolvedStream::Ready(vec![StreamKind::DepthAndTrades {
                ticker_info: btc_info,
                depth_aggr: StreamTicksize::Client,
                push_freq: PushFrequency::ServerDefault,
            }]);

        let explicit_stream = StreamKind::DepthAndTrades {
            ticker_info: bybit_info,
            depth_aggr: StreamTicksize::Client,
            push_freq: PushFrequency::ServerDefault,
        };

        let layout_id = uuid::Uuid::new_v4();
        let req_id = uuid::Uuid::new_v4();
        let fetch = FetchRange::Trades(1700000000000, 1700000060000);

        // When explicit stream is passed, request_fetch must not panic or drop request
        let _task = request_fetch(
            &mut p1_state,
            layout_id,
            req_id,
            fetch,
            Some(explicit_stream),
        );
        assert!(p1_state.streams.matches_trades(&explicit_stream));
    }

    #[test]
    fn test_ter06_popout_window_grid_routing_isolation() {
        let (main_panes, _) = pane_grid::State::new(pane::State::default());
        let (popout_panes, popout_p1) = pane_grid::State::new(pane::State::default());

        let popout_win_id = window::Id::unique();

        let mut popout_map = HashMap::new();
        popout_map.insert(popout_win_id, (popout_panes, WindowSpec::default()));

        let mut dashboard = Dashboard {
            panes: main_panes,
            focus: None,
            last_focused_kline: None,
            streams: UniqueStreams::default(),
            popout: popout_map,
            layout_id: uuid::Uuid::new_v4(),
        };

        // Split pane in popout window
        let (new_popout_pane, _) = dashboard
            .popout
            .get_mut(&popout_win_id)
            .unwrap()
            .0
            .split(pane_grid::Axis::Horizontal, popout_p1, pane::State::new())
            .unwrap();

        assert_eq!(dashboard.popout.get(&popout_win_id).unwrap().0.len(), 2);
        assert_eq!(
            dashboard.panes.len(),
            1,
            "Main grid must remain unaffected by popout split"
        );

        dashboard
            .popout
            .get_mut(&popout_win_id)
            .unwrap()
            .0
            .maximize(new_popout_pane);

        assert_eq!(
            dashboard.panes.len(),
            1,
            "Main grid must remain unaffected by popout maximize"
        );
    }

    #[test]
    fn test_ter14_auto_inherit_pane_config_on_init() {
        let win_id = window::Id::unique();

        // Pane 1: existing configured Kline pane with DeltaProfile
        let mut p1_state = pane::State::default();
        let mut chart1 = make_test_chart();
        chart1.set_cluster_kind(data::chart::kline::ClusterKind::DeltaProfile);
        p1_state.settings.visual_config = Some(data::layout::pane::VisualConfig::Kline(
            data::chart::kline::Config {
                rolling_vwap_window_hours: 42,
                ..Default::default()
            },
        ));
        p1_state.content = pane::Content::Kline {
            chart: Some(chart1),
            indicators: Default::default(),
            layout: Default::default(),
            kind: data::chart::KlineChartKind::Footprint {
                clusters: data::chart::kline::ClusterKind::DeltaProfile,
                scaling: Default::default(),
                studies: vec![],
                show_bottom_volume: false,
                aggregate: false,
                aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                history_lookback_days: None,
            },
        };

        // Pane 2: newly created placeholder pane
        let p2_state = pane::State::default();

        let (mut panes, p1) = pane_grid::State::new(p1_state);
        let (p2, _) = panes
            .split(pane_grid::Axis::Horizontal, p1, p2_state)
            .unwrap();

        let mut dashboard = Dashboard {
            panes,
            focus: Some((win_id, p2)),
            last_focused_kline: Some((win_id, p1)),
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id: uuid::Uuid::new_v4(),
        };

        let ticker = exchange::Ticker::new("ETHUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ti = exchange::TickerInfo::new(ticker, 0.01, 0.001, None);

        // Initialize pane 2 as FootprintChart
        let _ = dashboard.init_pane(
            win_id,
            win_id,
            p2,
            ti,
            data::layout::pane::ContentKind::FootprintChart,
        );

        let p2_state = dashboard.panes.get(p2).unwrap();
        // Check visual config was inherited
        match &p2_state.settings.visual_config {
            Some(data::layout::pane::VisualConfig::Kline(cfg)) => {
                assert_eq!(cfg.rolling_vwap_window_hours, 42);
            }
            _ => {
                panic!("Expected inherited VisualConfig::Kline with rolling_vwap_window_hours = 42")
            }
        }
        // Check cluster kind was inherited
        assert_eq!(
            p2_state.content.clusters(),
            Some(data::chart::kline::ClusterKind::DeltaProfile)
        );
    }

    #[test]
    fn test_dat17_dashboard_layout_id_propagation_and_clone_isolation() {
        let dash = Dashboard::default();
        let original_layout_id = dash.layout_id;

        // Verify layout_id is propagated to all default panes
        for (_, pane_state) in dash.panes.iter() {
            assert_eq!(pane_state.layout_id, Some(original_layout_id));
        }

        // Clone layout simulation as in src/main.rs
        let mut ser_dashboard = data::Dashboard::from(&dash);

        fn reset_pane_ids(pane: &mut data::Pane) {
            match pane {
                data::Pane::Split { a, b, .. } => {
                    reset_pane_ids(a);
                    reset_pane_ids(b);
                }
                data::Pane::KlineChart { settings, .. }
                | data::Pane::HeatmapChart { settings, .. }
                | data::Pane::ShaderHeatmap { settings, .. }
                | data::Pane::ComparisonChart { settings, .. }
                | data::Pane::TimeAndSales { settings, .. }
                | data::Pane::Ladder { settings, .. } => {
                    settings.pane_id = Some(uuid::Uuid::new_v4());
                }
                data::Pane::Starter { .. } => {}
            }
        }

        reset_pane_ids(&mut ser_dashboard.pane);
        let new_uid = uuid::Uuid::new_v4();
        assert_ne!(new_uid, original_layout_id);

        let cloned_dash = Dashboard::from_config(
            crate::layout::configuration(ser_dashboard.pane),
            vec![],
            new_uid,
        );

        assert_eq!(cloned_dash.layout_id, new_uid);
        for (_, p_state) in cloned_dash.panes.iter() {
            assert_eq!(p_state.layout_id, Some(new_uid));
            // Ensure no pane ID collision with original dashboard
            for (_, orig_state) in dash.panes.iter() {
                assert_ne!(p_state.id, orig_state.id);
            }
        }
    }
}
