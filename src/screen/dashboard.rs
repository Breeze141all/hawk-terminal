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
        StreamTicksize, UniqueStreams, binance, bitcoincounterflow, bybit, hyperliquid, okex,
    },
    depth::Depth,
    fetcher::{FetchRange, FetchedData, NetOiInterval},
};

use iced::{
    Element, Length, Subscription, Task, Vector,
    task::{Straw, sipper},
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
    TradeFetchBatchDone(uuid::Uuid, u64, u64),
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
        Self {
            panes: pane_grid::State::with_configuration(Self::default_pane_config()),
            focus: None,
            last_focused_kline: None,
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id: uuid::Uuid::new_v4(),
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
        let panes = pane_grid::State::with_configuration(panes);

        let mut popout = HashMap::new();

        for (pane, specs) in popout_windows {
            popout.insert(
                window::Id::unique(),
                (pane_grid::State::with_configuration(pane), specs),
            );
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

    pub fn load_layout(&mut self, main_window: window::Id) -> Task<Message> {
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
                        state.notifications.push(Toast::error(err.to_string()));
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
                    self.panes.resize(split, ratio);
                }
                pane::Message::PaneDragged(event) => {
                    if let pane_grid::DragEvent::Dropped { pane, target } = event {
                        self.panes.drop(pane, target);
                    }
                }
                pane::Message::SplitPane(axis, pane) => {
                    let focus_pane = if let Some((new_pane, _)) =
                        self.panes.split(axis, pane, pane::State::new())
                    {
                        Some(new_pane)
                    } else {
                        None
                    };

                    if let Some(focus_pane) = focus_pane {
                        self.set_focus(main_window.id, window, focus_pane);
                    }
                }
                pane::Message::ClosePane(pane) => {
                    if let Some((_, sibling)) = self.panes.close(pane) {
                        self.set_focus(main_window.id, window, sibling);
                    }
                }
                pane::Message::MaximizePane(pane) => {
                    self.panes.maximize(pane);
                }
                pane::Message::Restore => {
                    self.panes.restore();
                }
                pane::Message::ReplacePane(pane) => {
                    if let Some(pane) = self.panes.get_mut(pane) {
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
                            let clusters_cfg = match &state.content {
                                pane::Content::Kline {
                                    kind: data::chart::KlineChartKind::Footprint { clusters, .. },
                                    ..
                                } => Some(*clusters),
                                _ => None,
                            };

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

                                        if let Some(studies) = &studies_cfg {
                                            state.content.update_studies(studies.clone());
                                        }

                                        if let Some(cluster_kind) = &clusters_cfg
                                            && let pane::Content::Kline { chart, .. } =
                                                &mut state.content
                                            && let Some(c) = chart
                                        {
                                            c.set_cluster_kind(*cluster_kind);
                                        }
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
            Message::TradeFetchBatchDone(pane_id, from_time, actual_last_trade_t) => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window.id, pane_id) {
                    if let pane::Content::Kline {
                        chart: Some(chart), ..
                    } = &mut pane_state.content
                    {
                        chart.mark_trades_fetched(from_time, actual_last_trade_t);
                        chart.finish_one_trade_fetch();
                        chart.flush_footprint_cache();
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
            let result = self.panes.split(axis, pane, pane::State::new());

            if let Some((pane, _)) = result {
                return self.focus_pane(main_window.id, pane);
            }
        }

        Task::none()
    }

    fn popout_pane(&mut self, main_window: &Window) -> Task<Message> {
        if let Some((_, id)) = self.focus.take()
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

    fn init_pane(
        &mut self,
        main_window: window::Id,
        window: window::Id,
        selected_pane: pane_grid::Pane,
        ticker_info: TickerInfo,
        content_kind: ContentKind,
    ) -> Task<Message> {
        if let Some(state) = self.get_mut_pane(main_window, window, selected_pane) {
            let pane_id = state.unique_id();

            let streams = state.set_content_and_streams(vec![ticker_info], content_kind);
            self.streams.extend(streams.iter());

            for stream in &streams {
                if let StreamKind::Kline { .. } = stream {
                    return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
                }
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

        if let Some((window, selected_pane)) = self.focus
            && let Some(state) = self.get_mut_pane(main_window, window, selected_pane)
        {
            let previous_ticker = state.stream_pair();
            if previous_ticker.is_some() && previous_ticker != Some(ticker_info) {
                state.link_group = None;
            }

            let streams = state.set_content_and_streams(vec![ticker_info], content_kind);

            let pane_id = state.unique_id();
            self.streams.extend(streams.iter());

            for stream in &streams {
                if let StreamKind::Kline { .. } = stream {
                    return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
                }
            }
            return Task::none();
        }

        Task::done(Message::Notification(Toast::warn(
            "No focused pane found".to_string(),
        )))
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
                if !batch.is_empty()
                    && let Err(reason) = self.insert_fetched_trades(main_window, pane_id, &batch)
                {
                    return self.handle_error(Some(pane_id), &reason, main_window);
                }
            }
            FetchedData::Klines { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline {
                        timeframe,
                        ticker_info,
                    } = stream_type
                    {
                        pane_state.insert_hist_klines(req_id, timeframe, ticker_info, &data);
                    }
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
                    c.insert_raw_trades(trades.to_owned());
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

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                if pane_state.matches_stream(stream) {
                    match &mut pane_state.content {
                        pane::Content::Kline { chart: Some(c), .. } => {
                            let prev_p = c.current_price().unwrap_or(kline.close.to_f32_lossy());
                            c.update_latest_kline(kline);
                            let cur_p = kline.close.to_f32_lossy();
                            let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                            let triggered =
                                data::AlertStore::check_price_triggers(&symbol, prev_p, cur_p);
                            for alert in &triggered {
                                c.alerts.iter_mut().for_each(|a| {
                                    if a.id == alert.id {
                                        a.status = data::chart::alert::AlertStatus::Triggered;
                                        a.triggered_at = alert.triggered_at;
                                    }
                                });
                                c.invalidate_all();
                                pane_state.notifications.push(Toast::info(format!(
                                    "🔔 Price Alert: {} reached {:.2} ({})",
                                    alert.ticker_symbol, alert.target_price, alert.condition
                                )));
                            }
                        }
                        pane::Content::Comparison(Some(c)) => {
                            c.update_latest_kline(&stream.ticker_info(), kline);
                        }
                        _ => {}
                    }
                    found_match = true;
                }
            });

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

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                let exact_match = pane_state.matches_stream(stream);
                let trade_match = pane_state.matches_trades(stream);

                if exact_match || (trade_match && !trades_buffer.is_empty()) {
                    match &mut pane_state.content {
                        pane::Content::Heatmap { chart: Some(c), .. } if exact_match => {
                            c.insert_datapoint(trades_buffer, depth_update_t, Arc::clone(depth));
                        }
                        pane::Content::Kline { chart: Some(c), .. }
                            if !trades_buffer.is_empty() =>
                        {
                            let prev_p = c.current_price();
                            c.insert_trades_buffer(trades_buffer);
                            let cur_p = c.current_price();
                            if let (Some(prev), Some(cur)) = (prev_p, cur_p) {
                                let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                                let triggered =
                                    data::AlertStore::check_price_triggers(&symbol, prev, cur);
                                for alert in &triggered {
                                    c.alerts.iter_mut().for_each(|a| {
                                        if a.id == alert.id {
                                            a.status = data::chart::alert::AlertStatus::Triggered;
                                            a.triggered_at = alert.triggered_at;
                                        }
                                    });
                                    c.invalidate_all();
                                    pane_state.notifications.push(Toast::info(format!(
                                        "🔔 Price Alert: {} reached {:.2} ({})",
                                        alert.ticker_symbol, alert.target_price, alert.condition
                                    )));
                                }
                            }
                        }
                        pane::Content::TimeAndSales(Some(p)) if !trades_buffer.is_empty() => {
                            p.insert_buffer(trades_buffer);
                        }
                        pane::Content::Ladder(Some(panel)) if exact_match => {
                            panel.insert_buffers(depth_update_t, depth, trades_buffer);
                        }
                        _ => {}
                    }
                    found_match = true;
                }
            });

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

    pub fn tick(&mut self, now: Instant, main_window: window::Id) -> Task<Message> {
        let mut tasks = vec![];
        let layout_id = self.layout_id;

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
                    }
                    Some(StreamPairKind::SingleSource(ticker)) => {
                        state.set_content_and_streams(vec![ticker], state.content.kind());
                    }
                    None => {}
                },
                None => {}
            });

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
            let trade_info = state.streams.find_ready_map(|stream| {
                if let StreamKind::DepthAndTrades { ticker_info, .. } = stream {
                    Some((*ticker_info, pane_id, *stream))
                } else {
                    None
                }
            });

            if let Some((ticker_info, pane_id, stream)) = trade_info {
                let exchange_folder = match ticker_info.exchange() {
                    Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => {
                        "binance"
                    }
                    Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => "bybit",
                    Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => "okex",
                    Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => "hyperliquid",
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
                        Ok(actual_last_trade_t) => {
                            Message::TradeFetchBatchDone(pane_id, from_time, actual_last_trade_t)
                        }
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
    sipper(async move |mut progress| {
        let mut latest_trade_t = from_time;
        let mut actual_last_trade_t = from_time;
        let today_midnight = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        while latest_trade_t < to_time {
            match exchange::trades::fetch_trades(ticker_info, latest_trade_t, data_path.clone())
                .await
            {
                Ok((batch, next_trade_t)) => {
                    let batch_len = batch.len();
                    if batch_len == 0 {
                        break;
                    }
                    if let Some(last_trade) = batch.last() {
                        actual_last_trade_t = actual_last_trade_t.max(last_trade.time);
                    }
                    let () = progress.send(batch).await;

                    let sleep_ms = match ticker_info.exchange() {
                        Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => 200,
                        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => 100,
                        _ => 50,
                    };

                    if latest_trade_t >= today_midnight && batch_len >= 100 {
                        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                    }

                    if next_trade_t <= latest_trade_t {
                        break;
                    }
                    latest_trade_t = next_trade_t;
                }
                Err(err) => return Err(err),
            }
        }

        Ok(actual_last_trade_t)
    })
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
}
