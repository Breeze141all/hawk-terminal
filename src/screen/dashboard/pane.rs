use crate::{
    chart::{self, Chart, comparison::ComparisonChart, heatmap::HeatmapChart, kline::KlineChart},
    modal::{
        self, ModifierKind,
        pane::{
            Modal,
            mini_tickers_list::MiniPanel,
            settings::{comparison_cfg_view, heatmap_cfg_view, kline_cfg_view},
            stack_modal, stack_modal_positioned,
        },
    },
    screen::dashboard::{
        panel::{self, ladder::Ladder, timeandsales::TimeAndSales},
        tickers_table::TickersTable,
    },
    style::{self, Icon, icon_text},
    widget::{self, button_with_tooltip, column_drag, link_group_button, toast::Toast},
    window::{self, Window},
};
use data::{
    UserTimezone,
    chart::{
        Basis, ViewConfig,
        indicator::{HeatmapIndicator, Indicator, KlineIndicator, UiIndicator},
    },
    layout::pane::{ContentKind, LinkGroup, PaneSetup, Settings, VisualConfig},
};
use exchange::{
    Kline, OpenInterest, StreamPairKind, TickMultiplier, TickerInfo, Timeframe,
    adapter::{MarketKind, PersistStreamKind, ResolvedStream, StreamKind, StreamTicksize},
    fetcher::FetchRequests,
};
use iced::{
    Alignment, Element, Length, Renderer, Theme,
    alignment::Vertical,
    padding,
    widget::{button, center, column, container, pane_grid, pick_list, row, text, tooltip},
};
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum Effect {
    RefreshStreams,
    RequestFetch(FetchRequests),
    SwitchTickersInGroup(TickerInfo),
    FocusWidget(iced::widget::Id),
    TakeScreenshot(window::Id),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Status {
    #[default]
    Ready,
    Loading(exchange::fetcher::InfoKind),
    Stale(String),
}

pub enum Action {
    Chart(chart::Action),
    Panel(panel::Action),
    ResolveStreams(Vec<PersistStreamKind>),
    ResolveContent,
}

#[derive(Debug, Clone)]
pub enum Message {
    PaneClicked(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    ClosePane(pane_grid::Pane),
    SplitPane(pane_grid::Axis, pane_grid::Pane),
    MaximizePane(pane_grid::Pane),
    Restore,
    ReplacePane(pane_grid::Pane),
    Popout,
    Merge,
    SwitchLinkGroup(pane_grid::Pane, Option<LinkGroup>),
    VisualConfigChanged(pane_grid::Pane, VisualConfig, bool),
    PaneEvent(pane_grid::Pane, Event),
}

#[derive(Debug, Clone)]
pub enum Event {
    ShowModal(Modal),
    HideModal,
    ContentSelected(ContentKind),
    ChartInteraction(super::chart::Message),
    PanelInteraction(super::panel::Message),
    ToggleIndicator(UiIndicator),
    DeleteNotification(usize),
    ReorderIndicator(column_drag::DragEvent),
    ClusterKindSelected(data::chart::kline::ClusterKind),
    ClusterScalingSelected(data::chart::kline::ClusterScaling),
    StudyConfigurator(modal::pane::settings::study::StudyMessage),
    StreamModifierChanged(modal::stream::Message),
    ComparisonChartInteraction(super::chart::comparison::Message),
    MiniTickersListInteraction(modal::pane::mini_tickers_list::Message),
    /// Heatmap trade size filter input changed
    HeatmapTradeSizeInput(String),
    /// Heatmap order size filter input changed
    HeatmapOrderSizeInput(String),
    /// Time & Sales trade size filter input changed
    TimeAndSalesTradeSizeInput(String),
    /// TPO settings / kind changed
    TpoKindChanged(data::chart::KlineChartKind),
    /// Footprint bottom volume toggle
    FootprintShowBottomVolumeToggled(bool),
    /// Select drawing tool
    SelectDrawingTool(data::chart::drawing::DrawingTool),
    /// Drawing toolbar dragged to new position
    DrawingToolbarMoved(iced::Point),
    /// Selected drawing toolbar dragged to new position
    SelectedDrawingToolbarMoved(iced::Point),
    /// Action on selected drawing toolbar
    SelectedDrawingAction(widget::chart::drawing_selection_toolbar::SelectionToolbarAction),
    /// Clear all drawings
    ClearDrawings,
    /// Screenshot to clipboard
    TakeScreenshot(window::Id),
    /// Market Replay
    ToggleReplay,
    ReplayPlayPause,
    ReplayStepForward,
    ReplayStepBackward,
    ReplaySetSpeed(u64),
    ToggleReplayDatePicker,
    ReplayDatePickerAction(modal::pane::replay_calendar::Action),
    /// Price Alerts
    AddPriceAlert(f32),
    DeletePriceAlert(uuid::Uuid),
    TogglePriceAlert(uuid::Uuid),
    AlertPriceInput(String),
    AlertConditionSelected(data::chart::alert::AlertCondition),
    AlertFilterSelected(data::chart::alert::AlertFilter),
    ClearTriggeredAlerts,
}

pub struct State {
    id: uuid::Uuid,
    pub modal: Option<Modal>,
    pub content: Content,
    pub settings: Settings,
    pub notifications: Vec<Toast>,
    pub streams: ResolvedStream,
    pub status: Status,
    pub link_group: Option<LinkGroup>,
    pub alert_price_input: String,
    pub alert_condition: data::chart::alert::AlertCondition,
    pub alert_filter: data::chart::alert::AlertFilter,
    pub drawing_toolbar_pos: Option<iced::Point>,
    pub selected_drawing_toolbar_pos: Option<iced::Point>,
    pub selected_drawing_show_settings: bool,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_config(
        content: Content,
        streams: Vec<PersistStreamKind>,
        settings: Settings,
        link_group: Option<LinkGroup>,
    ) -> Self {
        Self {
            content,
            settings,
            streams: ResolvedStream::Waiting(streams),
            link_group,
            ..Default::default()
        }
    }

    pub fn stream_pair(&self) -> Option<TickerInfo> {
        self.streams.find_ready_map(|stream| match stream {
            StreamKind::DepthAndTrades { ticker_info, .. }
            | StreamKind::Kline { ticker_info, .. } => Some(*ticker_info),
        })
    }

    pub fn stream_pair_kind(&self) -> Option<StreamPairKind> {
        let ready_streams = self.streams.ready_iter()?;
        let mut unique = vec![];

        for stream in ready_streams {
            let ticker = stream.ticker_info();
            if !unique.contains(&ticker) {
                unique.push(ticker);
            }
        }

        match unique.len() {
            0 => None,
            1 => Some(StreamPairKind::SingleSource(unique[0])),
            _ => Some(StreamPairKind::MultiSource(unique)),
        }
    }

    pub fn set_content_and_streams(
        &mut self,
        tickers: Vec<TickerInfo>,
        kind: ContentKind,
    ) -> Vec<StreamKind> {
        if !(self.content.kind() == kind) {
            self.settings.selected_basis = None;
            self.settings.tick_multiply = None;
        }

        let base_ticker = tickers[0];
        let prev_base_ticker = self.stream_pair();

        let derived_plan = PaneSetup::new(
            kind,
            base_ticker,
            prev_base_ticker,
            self.settings.selected_basis,
            self.settings.tick_multiply,
        );

        self.settings.selected_basis = derived_plan.basis;
        self.settings.tick_multiply = derived_plan.tick_multiplier;

        let (content, streams) = {
            let kline_stream = |ti: TickerInfo, tf: Timeframe| StreamKind::Kline {
                ticker_info: ti,
                timeframe: tf,
            };
            let depth_stream = |derived_plan: &PaneSetup| StreamKind::DepthAndTrades {
                ticker_info: derived_plan.ticker_info,
                depth_aggr: derived_plan.depth_aggr,
                push_freq: derived_plan.push_freq,
            };

            match kind {
                ContentKind::HeatmapChart => {
                    let content = Content::new_heatmap(
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        derived_plan.tick_size,
                    );

                    let streams = vec![depth_stream(&derived_plan)];

                    (content, streams)
                }
                ContentKind::FootprintChart => {
                    let content = Content::new_kline(
                        kind,
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        derived_plan.tick_size,
                    );

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M5,
                        |tf| {
                            vec![
                                depth_stream(&derived_plan),
                                kline_stream(derived_plan.ticker_info, tf),
                            ]
                        },
                        || vec![depth_stream(&derived_plan)],
                    );

                    (content, streams)
                }
                ContentKind::CandlestickChart => {
                    let content = {
                        let base_ticker = tickers[0];
                        Content::new_kline(
                            kind,
                            &self.content,
                            derived_plan.ticker_info,
                            &self.settings,
                            base_ticker.min_ticksize.into(),
                        )
                    };

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M15,
                        |tf| vec![kline_stream(derived_plan.ticker_info, tf)],
                        || {
                            let depth_aggr = derived_plan
                                .ticker_info
                                .exchange()
                                .stream_ticksize(None, TickMultiplier(50));
                            let temp = PaneSetup {
                                depth_aggr,
                                ..derived_plan
                            };
                            vec![depth_stream(&temp)]
                        },
                    );

                    (content, streams)
                }
                ContentKind::TpoChart => {
                    let content = {
                        let base_ticker = tickers[0];
                        Content::new_kline(
                            kind,
                            &self.content,
                            derived_plan.ticker_info,
                            &self.settings,
                            base_ticker.min_ticksize.into(),
                        )
                    };

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M30,
                        |tf| vec![kline_stream(derived_plan.ticker_info, tf)],
                        || {
                            let depth_aggr = derived_plan
                                .ticker_info
                                .exchange()
                                .stream_ticksize(None, TickMultiplier(10));
                            let temp = PaneSetup {
                                depth_aggr,
                                ..derived_plan
                            };
                            vec![depth_stream(&temp)]
                        },
                    );

                    (content, streams)
                }
                ContentKind::TimeAndSales => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.time_and_sales());
                    let content = Content::TimeAndSales(Some(TimeAndSales::new(
                        config,
                        derived_plan.ticker_info,
                    )));

                    let temp = PaneSetup {
                        push_freq: exchange::PushFrequency::ServerDefault,
                        ..derived_plan
                    };

                    (content, vec![depth_stream(&temp)])
                }
                ContentKind::Ladder => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.ladder());
                    let content = Content::Ladder(Some(Ladder::new(
                        config,
                        derived_plan.ticker_info,
                        derived_plan.tick_size,
                    )));

                    (content, vec![depth_stream(&derived_plan)])
                }
                ContentKind::ComparisonChart => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.comparison());
                    let basis = derived_plan.basis.unwrap_or(Basis::Time(Timeframe::M15));
                    let content =
                        Content::Comparison(Some(ComparisonChart::new(basis, &tickers, config)));

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M15,
                        |tf| {
                            tickers
                                .iter()
                                .copied()
                                .map(|ti| kline_stream(ti, tf))
                                .collect()
                        },
                        || todo!("WIP: ComparisonChart does not support tick basis"),
                    );

                    (content, streams)
                }
                ContentKind::Starter => unreachable!(),
            }
        };

        self.content = content;
        self.streams = ResolvedStream::Ready(streams.clone());

        streams
    }

    pub fn insert_hist_oi(&mut self, req_id: Option<uuid::Uuid>, oi: &[OpenInterest]) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    panic!("Kline chart wasn't initialized when inserting open interest");
                };
                chart.insert_open_interest(req_id, oi);
            }
            _ => {
                log::error!("pane content not candlestick");
            }
        }
    }

    pub fn insert_funding_rates(
        &mut self,
        req_id: Option<uuid::Uuid>,
        rates: &[exchange::FundingRate],
    ) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    log::error!("Kline chart wasn't initialized when inserting funding rates");
                    return;
                };
                chart.insert_funding_rates(req_id, rates);
            }
            _ => {
                log::error!("pane content not candlestick");
            }
        }
    }

    pub fn insert_spot_klines(
        &mut self,
        req_id: Option<uuid::Uuid>,
        klines: &[exchange::SpotKline],
    ) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    log::error!("Kline chart wasn't initialized when inserting spot klines");
                    return;
                };
                chart.insert_spot_klines(req_id, klines);
            }
            _ => {
                log::error!("pane content not candlestick");
            }
        }
    }

    pub fn insert_net_oi_data(
        &mut self,
        req_id: Option<uuid::Uuid>,
        data: &[exchange::NetOiDataPoint],
    ) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    log::error!("Kline chart wasn't initialized when inserting net OI data");
                    return;
                };
                chart.insert_net_oi_data(req_id, data);
            }
            _ => {
                log::error!("pane content not candlestick");
            }
        }
    }

    pub fn insert_hist_klines(
        &mut self,
        req_id: Option<uuid::Uuid>,
        timeframe: Timeframe,
        ticker_info: TickerInfo,
        klines: &[Kline],
    ) {
        match &mut self.content {
            Content::Kline {
                chart, indicators, ..
            } => {
                let Some(chart) = chart else {
                    panic!("chart wasn't initialized when inserting klines");
                };

                if let Some(id) = req_id {
                    if chart.basis() != Basis::Time(timeframe) {
                        log::warn!(
                            "Ignoring stale kline fetch for timeframe {:?}; chart basis = {:?}",
                            timeframe,
                            chart.basis()
                        );
                        return;
                    }
                    chart.insert_hist_klines(id, klines);
                } else {
                    let (raw_trades, tick_size) = (chart.raw_trades(), chart.tick_size());
                    let layout = chart.chart_layout();
                    let current_config = chart.config();

                    *chart = KlineChart::new(
                        layout,
                        Basis::Time(timeframe),
                        tick_size,
                        klines,
                        raw_trades,
                        indicators,
                        ticker_info,
                        chart.kind(),
                        Some(current_config),
                    );
                }
            }
            Content::Comparison(chart) => {
                let Some(chart) = chart else {
                    panic!("Comparison chart wasn't initialized when inserting klines");
                };

                if let Some(id) = req_id {
                    if chart.timeframe != timeframe {
                        log::warn!(
                            "Ignoring stale kline fetch for timeframe {:?}; chart timeframe = {:?}",
                            timeframe,
                            chart.timeframe
                        );
                        return;
                    }
                    chart.insert_history(id, ticker_info, klines);
                } else {
                    *chart = ComparisonChart::new(
                        Basis::Time(timeframe),
                        &[ticker_info],
                        Some(chart.serializable_config()),
                    );
                }
            }
            _ => {
                log::error!("pane content not candlestick or footprint");
            }
        }
    }

    fn has_stream(&self) -> bool {
        match &self.streams {
            ResolvedStream::Ready(streams) => !streams.is_empty(),
            ResolvedStream::Waiting(streams) => !streams.is_empty(),
        }
    }

    pub fn view<'a>(
        &'a self,
        id: pane_grid::Pane,
        panes: usize,
        is_focused: bool,
        maximized: bool,
        window: window::Id,
        main_window: &'a Window,
        timezone: UserTimezone,
        tickers_table: &'a TickersTable,
        show_toolbar: bool,
    ) -> pane_grid::Content<'a, Message, Theme, Renderer> {
        let mut stream_info_element = if Content::Starter == self.content {
            row![]
        } else {
            row![link_group_button(id, self.link_group, |id| {
                Message::PaneEvent(id, Event::ShowModal(Modal::LinkGroup))
            })]
        };

        if let Some(kind) = self.stream_pair_kind() {
            let (base_ti, extra) = match kind {
                StreamPairKind::MultiSource(list) => (list[0], list.len().saturating_sub(1)),
                StreamPairKind::SingleSource(ti) => (ti, 0),
            };

            let exchange_icon = icon_text(style::exchange_icon(base_ti.ticker.exchange), 14);
            let mut label = {
                let symbol = base_ti.ticker.display_symbol_and_type().0;
                match base_ti.ticker.market_type() {
                    MarketKind::Spot => symbol,
                    MarketKind::LinearPerps | MarketKind::InversePerps => symbol + " PERP",
                }
            };
            if extra > 0 {
                label = format!("{label} +{extra}");
            }

            let content = row![exchange_icon, text(label).size(14)]
                .align_y(Vertical::Center)
                .spacing(4);

            let tickers_list_btn = button(content)
                .on_press(Message::PaneEvent(
                    id,
                    Event::ShowModal(Modal::MiniTickersList(MiniPanel::new())),
                ))
                .style(|theme, status| {
                    style::button::modifier(
                        theme,
                        status,
                        !matches!(self.modal, Some(Modal::MiniTickersList(_))),
                    )
                })
                .padding([4, 10]);

            stream_info_element = stream_info_element.push(tickers_list_btn);
        } else if !matches!(self.content, Content::Starter) && !self.has_stream() {
            let content = row![text("Choose a ticker").size(13)]
                .align_y(Alignment::Center)
                .spacing(4);

            let tickers_list_btn = button(content)
                .on_press(Message::PaneEvent(
                    id,
                    Event::ShowModal(Modal::MiniTickersList(MiniPanel::new())),
                ))
                .style(|theme, status| {
                    style::button::modifier(
                        theme,
                        status,
                        !matches!(self.modal, Some(Modal::MiniTickersList(_))),
                    )
                })
                .padding([4, 10]);

            stream_info_element = stream_info_element.push(tickers_list_btn);
        }

        let modifier: Option<modal::stream::Modifier> = self.modal.clone().and_then(|m| {
            if let Modal::StreamModifier(modifier) = m {
                Some(modifier)
            } else {
                None
            }
        });

        let compact_controls = if self.modal == Some(Modal::Controls) {
            Some(
                container(self.view_controls(
                    id,
                    panes,
                    maximized,
                    window != main_window.id,
                    window,
                ))
                .style(style::chart_modal)
                .into(),
            )
        } else {
            None
        };

        let uninitialized_base = |kind: ContentKind| -> Element<'a, Message> {
            if self.has_stream() {
                center(text("Loading…").size(16)).into()
            } else {
                let content = column![
                    text(kind.to_string()).size(16),
                    text("No ticker selected").size(14)
                ]
                .spacing(8)
                .align_x(Alignment::Center);

                center(content).into()
            }
        };

        let body = match &self.content {
            Content::Starter => {
                let content_picklist =
                    pick_list(ContentKind::ALL, Some(ContentKind::Starter), move |kind| {
                        Message::PaneEvent(id, Event::ContentSelected(kind))
                    });

                let base: Element<_> = widget::toast::Manager::new(
                    center(
                        column![
                            text("Choose a view to get started").size(16),
                            content_picklist
                        ]
                        .align_x(Alignment::Center)
                        .spacing(12),
                    ),
                    &self.notifications,
                    Alignment::End,
                    move |msg| Message::PaneEvent(id, Event::DeleteNotification(msg)),
                )
                .into();

                self.compose_stack_view(
                    base,
                    id,
                    None,
                    compact_controls,
                    || column![].into(),
                    None,
                    tickers_table,
                    false,
                )
            }
            Content::Comparison(chart) => {
                if let Some(c) = chart {
                    let selected_basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Timeframe::M15.into());
                    let kind = ModifierKind::Comparison(selected_basis);

                    let modifiers =
                        row![basis_modifier(id, selected_basis, modifier, kind),].spacing(4);

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = c.view(timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ComparisonChartInteraction(message))
                    });

                    let settings_modal = || comparison_cfg_view(id, c);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        Some(c.selected_tickers()),
                        tickers_table,
                        false,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::ComparisonChart);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                        false,
                    )
                }
            }
            Content::TimeAndSales(panel) => {
                if let Some(panel) = panel {
                    let base = panel::view(panel, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::PanelInteraction(message))
                    });
                    let trade_input = panel.size_filter_input();
                    let settings_modal =
                        || modal::pane::settings::timesales_cfg_view(panel.config, id, trade_input);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                        false,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::TimeAndSales);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                        false,
                    )
                }
            }
            Content::Ladder(panel) => {
                if let Some(panel) = panel {
                    let basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Basis::default_heatmap_time(self.stream_pair()));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(1));

                    let kind = ModifierKind::Orderbook(basis, tick_multiply);

                    let base_ticksize = tick_multiply.base(panel.tick_size());
                    let exchange = self.stream_pair().map(|ti| ti.ticker.exchange);

                    let modifiers = ticksize_modifier(
                        id,
                        base_ticksize,
                        tick_multiply,
                        modifier,
                        kind,
                        exchange,
                    );

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = panel::view(panel, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::PanelInteraction(message))
                    });

                    let settings_modal =
                        || modal::pane::settings::ladder_cfg_view(panel.config, id);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                        false,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::Ladder);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                        false,
                    )
                }
            }
            Content::Heatmap {
                chart, indicators, ..
            } => {
                if let Some(chart) = chart {
                    let ticker_info = self.stream_pair();
                    let exchange = ticker_info.as_ref().map(|info| info.ticker.exchange);

                    let basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Basis::default_heatmap_time(ticker_info));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(5));

                    let kind = ModifierKind::Heatmap(basis, tick_multiply);
                    let base_ticksize = tick_multiply.base(chart.tick_size());

                    let modifiers = row![
                        basis_modifier(id, basis, modifier, kind),
                        ticksize_modifier(
                            id,
                            base_ticksize,
                            tick_multiply,
                            modifier,
                            kind,
                            exchange
                        ),
                    ]
                    .spacing(4);

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = chart::view(chart, indicators, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ChartInteraction(message))
                    });
                    let (trade_input, order_input) = chart.size_filter_inputs();
                    let settings_modal = || {
                        heatmap_cfg_view(
                            chart.visual_config(),
                            id,
                            chart.study_configurator(),
                            &chart.studies,
                            basis,
                            trade_input,
                            order_input,
                        )
                    };

                    let indicator_modal = if self.modal == Some(Modal::Indicators) {
                        Some(modal::indicators::view(
                            id,
                            self,
                            indicators,
                            self.stream_pair().map(|i| i.ticker.market_type()),
                        ))
                    } else {
                        None
                    };

                    self.compose_stack_view(
                        base,
                        id,
                        indicator_modal,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                        false,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::HeatmapChart);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                        false,
                    )
                }
            }
            Content::Kline {
                chart,
                indicators,
                kind: chart_kind,
                ..
            } => {
                if let Some(chart) = chart {
                    match chart_kind {
                        data::chart::KlineChartKind::Footprint { .. } => {
                            let basis =
                                self.settings.selected_basis.unwrap_or(Timeframe::M5.into());
                            let tick_multiply =
                                self.settings.tick_multiply.unwrap_or(TickMultiplier(10));

                            let kind = ModifierKind::Footprint(basis, tick_multiply);
                            let base_ticksize = tick_multiply.base(chart.tick_size());

                            let exchange =
                                self.stream_pair().as_ref().map(|info| info.ticker.exchange);

                            let modifiers = row![
                                basis_modifier(id, basis, modifier, kind),
                                ticksize_modifier(
                                    id,
                                    base_ticksize,
                                    tick_multiply,
                                    modifier,
                                    kind,
                                    exchange
                                ),
                            ]
                            .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                        data::chart::KlineChartKind::Candles => {
                            let selected_basis = self
                                .settings
                                .selected_basis
                                .unwrap_or(Timeframe::M15.into());
                            let kind = ModifierKind::Candlestick(selected_basis);

                            let modifiers =
                                row![basis_modifier(id, selected_basis, modifier, kind),]
                                    .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                        data::chart::KlineChartKind::Tpo {
                            show_candles,
                            show_letters,
                            show_ib,
                            show_va,
                            show_poc,
                            show_single_prints,
                            tick_step,
                            period,
                            clusters,
                            split_sessions,
                            color_scheme,
                            ib_color,
                            poc_color,
                            single_prints_color,
                        } => {
                            let selected_basis = self
                                .settings
                                .selected_basis
                                .unwrap_or(Timeframe::M30.into());
                            let kind = ModifierKind::Candlestick(selected_basis);

                            let sc = *show_candles;
                            let sl = *show_letters;
                            let sib = *show_ib;
                            let sva = *show_va;
                            let spoc = *show_poc;
                            let ssp = *show_single_prints;
                            let st = *tick_step;
                            let cur_period = *period;
                            let cs = *color_scheme;
                            let cib = *ib_color;
                            let cpoc = *poc_color;
                            let csp = *single_prints_color;
                            let cur_clusters = clusters.clone();
                            let cur_split_sessions = split_sessions.clone();

                            // 1. ViewMode Toggle: [Pure TPO] vs [Combined]
                            let s_sessions_toggle = cur_split_sessions.clone();
                            let view_mode_btn =
                                button(text(if sc { "Combined" } else { "Pure TPO" }))
                                    .style(move |theme, status| {
                                        style::button::modifier(theme, status, false)
                                    })
                                    .on_press(Message::PaneEvent(
                                        id,
                                        Event::TpoKindChanged(data::chart::KlineChartKind::Tpo {
                                            show_candles: !sc,
                                            show_letters: sl,
                                            show_ib: sib,
                                            show_va: sva,
                                            show_poc: spoc,
                                            show_single_prints: ssp,
                                            tick_step: st,
                                            period: cur_period,
                                            clusters: cur_clusters.clone(),
                                            split_sessions: s_sessions_toggle,
                                            color_scheme: cs,
                                            ib_color: cib,
                                            poc_color: cpoc,
                                            single_prints_color: csp,
                                        }),
                                    ));

                            // 2. Period Selector PickList (unlocked in both Pure TPO and Combined)
                            let clusters_for_period = cur_clusters.clone();
                            let s_sessions_period = cur_split_sessions.clone();
                            let period_element: Element<'a, Message> = pick_list(
                                data::chart::tpo::SessionPeriod::ALL,
                                Some(cur_period),
                                move |new_p| {
                                    Message::PaneEvent(
                                        id,
                                        Event::TpoKindChanged(data::chart::KlineChartKind::Tpo {
                                            show_candles: sc,
                                            show_letters: sl,
                                            show_ib: sib,
                                            show_va: sva,
                                            show_poc: spoc,
                                            show_single_prints: ssp,
                                            tick_step: st,
                                            period: new_p,
                                            clusters: clusters_for_period.clone(),
                                            split_sessions: s_sessions_period.clone(),
                                            color_scheme: cs,
                                            ib_color: cib,
                                            poc_color: cpoc,
                                            single_prints_color: csp,
                                        }),
                                    )
                                },
                            )
                            .into();

                            let modifiers = row![
                                basis_modifier(id, selected_basis, modifier, kind),
                                view_mode_btn,
                                period_element,
                            ]
                            .align_y(Alignment::Center)
                            .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                    }

                    let base = chart::view(chart, indicators, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ChartInteraction(message))
                    });
                    let settings_modal = || {
                        kline_cfg_view(
                            chart.study_configurator(),
                            chart.config(),
                            chart_kind,
                            id,
                            chart.basis(),
                            indicators,
                        )
                    };

                    let indicator_modal = if self.modal == Some(Modal::Indicators) {
                        Some(modal::indicators::view(
                            id,
                            self,
                            indicators,
                            self.stream_pair().map(|i| i.ticker.market_type()),
                        ))
                    } else {
                        None
                    };

                    self.compose_stack_view(
                        base,
                        id,
                        indicator_modal,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                        show_toolbar,
                    )
                } else {
                    let content_kind = match chart_kind {
                        data::chart::KlineChartKind::Candles => ContentKind::CandlestickChart,
                        data::chart::KlineChartKind::Footprint { .. } => {
                            ContentKind::FootprintChart
                        }
                        data::chart::KlineChartKind::Tpo { .. } => ContentKind::TpoChart,
                    };
                    let base = uninitialized_base(content_kind);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                        false,
                    )
                }
            }
        };

        match &self.status {
            Status::Loading(exchange::fetcher::InfoKind::FetchingKlines) => {
                stream_info_element = stream_info_element.push(text("Fetching Klines..."));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingTrades(count)) => {
                stream_info_element =
                    stream_info_element.push(text(format!("Fetching Trades... {count} fetched")));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingOI) => {
                stream_info_element = stream_info_element.push(text("Fetching Open Interest..."));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingMarketPulse) => {
                stream_info_element = stream_info_element.push(text("Fetching Market Pulse..."));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingNetOi) => {
                stream_info_element = stream_info_element.push(text("Fetching Net OI..."));
            }
            Status::Stale(msg) => {
                stream_info_element = stream_info_element.push(text(msg));
            }
            Status::Ready => {}
        }

        let content = pane_grid::Content::new(body)
            .style(move |theme| style::pane_background(theme, is_focused));

        let controls = {
            let compact_control = container(
                button(text("...").size(13).align_y(Alignment::End))
                    .on_press(Message::PaneEvent(id, Event::ShowModal(Modal::Controls)))
                    .style(move |theme, status| {
                        style::button::transparent(
                            theme,
                            status,
                            self.modal == Some(Modal::Controls)
                                || self.modal == Some(Modal::Settings),
                        )
                    }),
            )
            .align_y(Alignment::Center)
            .height(Length::Fixed(32.0))
            .padding(4);

            if self.modal == Some(Modal::Controls) {
                pane_grid::Controls::new(compact_control)
            } else {
                pane_grid::Controls::dynamic(
                    self.view_controls(id, panes, maximized, window != main_window.id, window),
                    compact_control,
                )
            }
        };

        let title_bar = pane_grid::TitleBar::new(
            stream_info_element
                .padding(padding::left(4).top(1))
                .align_y(Vertical::Center)
                .spacing(8)
                .height(Length::Fixed(32.0)),
        )
        .controls(controls)
        .style(style::pane_title_bar);

        content.title_bar(if self.modal.is_none() {
            title_bar
        } else {
            title_bar.always_show_controls()
        })
    }

    pub fn update(&mut self, msg: Event) -> Option<Effect> {
        match msg {
            Event::ShowModal(requested_modal) => {
                return self.show_modal_with_focus(requested_modal);
            }
            Event::HideModal => {
                self.modal = None;
            }
            Event::ContentSelected(kind) => {
                self.content = Content::placeholder(kind);

                if !matches!(kind, ContentKind::Starter) {
                    self.streams = ResolvedStream::Waiting(vec![]);
                    let modal = Modal::MiniTickersList(MiniPanel::new());

                    if let Some(effect) = self.show_modal_with_focus(modal) {
                        return Some(effect);
                    }
                }
            }
            Event::ChartInteraction(msg) => match &mut self.content {
                Content::Heatmap { chart: Some(c), .. } => {
                    super::chart::update(c, &msg);
                }
                Content::Kline {
                    chart: Some(c),
                    kind,
                    ..
                } => match &msg {
                    chart::Message::MergeSessions(s1, s2) => {
                        c.merge_tpo_sessions(*s1, *s2);
                        *kind = c.kind.clone();
                    }
                    chart::Message::SplitCluster(s) => {
                        c.split_tpo_cluster(*s);
                        *kind = c.kind.clone();
                    }
                    chart::Message::ToggleSplitBrackets(s) => {
                        c.toggle_split_brackets(*s);
                        *kind = c.kind.clone();
                    }
                    chart::Message::AddDrawing(drawing) => {
                        c.add_drawing(drawing.clone());
                    }
                    chart::Message::UpdateDrawing(drawing) => {
                        c.update_drawing(drawing.clone());
                    }
                    chart::Message::DeleteDrawing(id) => {
                        c.delete_drawing(*id);
                        self.selected_drawing_show_settings = false;
                    }
                    chart::Message::ClearDrawings => {
                        c.clear_drawings();
                        self.selected_drawing_show_settings = false;
                    }
                    chart::Message::SelectDrawing(id) => {
                        c.set_selected_drawing(*id);
                        if id.is_none() {
                            self.selected_drawing_show_settings = false;
                        }
                    }
                    chart::Message::UpdateAlertPrice(id, price) => {
                        c.update_alert_price(*id, *price);
                        data::AlertStore::update_price(*id, *price);
                    }
                    _ => {
                        super::chart::update(c, &msg);
                    }
                },
                _ => {}
            },
            Event::PanelInteraction(msg) => match &mut self.content {
                Content::Ladder(Some(p)) => super::panel::update(p, msg),
                Content::TimeAndSales(Some(p)) => super::panel::update(p, msg),
                _ => {}
            },
            Event::ToggleIndicator(ind) => {
                self.content.toggle_indicator(ind);
            }
            Event::DeleteNotification(idx) => {
                if idx < self.notifications.len() {
                    self.notifications.remove(idx);
                }
            }
            Event::ReorderIndicator(e) => {
                self.content.reorder_indicators(&e);
            }
            Event::ClusterKindSelected(kind) => {
                if let Content::Kline {
                    chart, kind: cur, ..
                } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_kind(kind);
                    *cur = c.kind.clone();
                }
            }
            Event::ClusterScalingSelected(scaling) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_scaling(scaling);
                    *kind = c.kind.clone();
                }
            }
            Event::TpoKindChanged(new_kind) => {
                if let Content::Kline {
                    chart, kind: cur, ..
                } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_tpo_kind(new_kind.clone());
                    *cur = new_kind;
                }
            }
            Event::FootprintShowBottomVolumeToggled(show) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_footprint_show_bottom_volume(show);
                    *kind = c.kind.clone();
                }
            }
            Event::StudyConfigurator(study_msg) => match study_msg {
                modal::pane::settings::study::StudyMessage::Footprint(m) => {
                    if let Content::Kline { chart, kind, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                        *kind = c.kind.clone();
                    }
                }
                modal::pane::settings::study::StudyMessage::Heatmap(m) => {
                    if let Content::Heatmap { chart, studies, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                        *studies = c.studies.clone();
                    }
                }
            },
            Event::StreamModifierChanged(message) => {
                if let Some(Modal::StreamModifier(mut modifier)) = self.modal.take() {
                    let mut effect: Option<Effect> = None;

                    if let Some(action) = modifier.update(message) {
                        match action {
                            modal::stream::Action::TabSelected(tab) => {
                                modifier.tab = tab;
                            }
                            modal::stream::Action::TicksizeSelected(tm) => {
                                modifier.update_kind_with_multiplier(tm);
                                self.settings.tick_multiply = Some(tm);

                                if let Some(ticker) = self.stream_pair() {
                                    match &mut self.content {
                                        Content::Kline { chart: Some(c), .. } => {
                                            c.change_tick_size(
                                                tm.multiply_with_min_tick_size(ticker),
                                            );
                                            c.reset_request_handler();
                                        }
                                        Content::Heatmap { chart: Some(c), .. } => {
                                            c.change_tick_size(
                                                tm.multiply_with_min_tick_size(ticker),
                                            );
                                        }
                                        Content::Ladder(Some(p)) => {
                                            p.set_tick_size(tm.multiply_with_min_tick_size(ticker));
                                        }
                                        _ => {}
                                    }
                                }

                                let is_client = self
                                    .stream_pair()
                                    .map(|ti| ti.exchange().is_depth_client_aggr())
                                    .unwrap_or(false);

                                if let Some(mut it) = self.streams.ready_iter_mut() {
                                    for s in &mut it {
                                        if let StreamKind::DepthAndTrades { depth_aggr, .. } = s {
                                            *depth_aggr = if is_client {
                                                StreamTicksize::Client
                                            } else {
                                                StreamTicksize::ServerSide(tm)
                                            };
                                        }
                                    }
                                }
                                if !is_client {
                                    effect = Some(Effect::RefreshStreams);
                                }
                            }
                            modal::stream::Action::BasisSelected(new_basis) => {
                                modifier.update_kind_with_basis(new_basis);
                                self.settings.selected_basis = Some(new_basis);

                                let base_ticker = self.stream_pair();

                                match &mut self.content {
                                    Content::Heatmap { chart: Some(c), .. } => {
                                        c.set_basis(new_basis);

                                        if let Some(stream_type) =
                                            self.streams.ready_iter_mut().and_then(|mut it| {
                                                it.find(|s| {
                                                    matches!(s, StreamKind::DepthAndTrades { .. })
                                                })
                                            })
                                            && let StreamKind::DepthAndTrades {
                                                push_freq,
                                                ticker_info,
                                                ..
                                            } = stream_type
                                            && ticker_info.exchange().is_custom_push_freq()
                                        {
                                            match new_basis {
                                                Basis::Time(tf) => {
                                                    *push_freq = exchange::PushFrequency::Custom(tf)
                                                }
                                                Basis::Tick(_) => {
                                                    *push_freq =
                                                        exchange::PushFrequency::ServerDefault
                                                }
                                            }
                                        }

                                        effect = Some(Effect::RefreshStreams);
                                    }
                                    Content::Kline { chart: Some(c), .. } => {
                                        if let Some(base_ticker) = base_ticker {
                                            match new_basis {
                                                Basis::Time(tf) => {
                                                    let kline_stream = StreamKind::Kline {
                                                        ticker_info: base_ticker,
                                                        timeframe: tf,
                                                    };
                                                    let mut streams = vec![kline_stream];

                                                    if matches!(
                                                        c.kind,
                                                        data::chart::KlineChartKind::Footprint { .. }
                                                    ) {
                                                        let depth_aggr = if base_ticker
                                                            .exchange()
                                                            .is_depth_client_aggr()
                                                        {
                                                            StreamTicksize::Client
                                                        } else {
                                                            StreamTicksize::ServerSide(
                                                                self.settings
                                                                    .tick_multiply
                                                                    .unwrap_or(TickMultiplier(1)),
                                                            )
                                                        };
                                                        streams.push(StreamKind::DepthAndTrades {
                                                            ticker_info: base_ticker,
                                                            depth_aggr,
                                                            push_freq: exchange::PushFrequency::ServerDefault,
                                                        });
                                                    }

                                                    self.streams = ResolvedStream::Ready(streams);
                                                    let action = c.set_basis(new_basis);

                                                    if let Some(chart::Action::RequestFetch(
                                                        fetch,
                                                    )) = action
                                                    {
                                                        effect = Some(Effect::RequestFetch(fetch));
                                                    }
                                                }
                                                Basis::Tick(_) => {
                                                    let depth_aggr = if base_ticker
                                                        .exchange()
                                                        .is_depth_client_aggr()
                                                    {
                                                        StreamTicksize::Client
                                                    } else {
                                                        StreamTicksize::ServerSide(
                                                            self.settings
                                                                .tick_multiply
                                                                .unwrap_or(TickMultiplier(1)),
                                                        )
                                                    };

                                                    self.streams = ResolvedStream::Ready(vec![
                                                        StreamKind::DepthAndTrades {
                                                            ticker_info: base_ticker,
                                                            depth_aggr,
                                                            push_freq: exchange::PushFrequency::ServerDefault,
                                                        },
                                                    ]);
                                                    c.set_basis(new_basis);
                                                    effect = Some(Effect::RefreshStreams);
                                                }
                                            }
                                        }
                                    }
                                    Content::Comparison(Some(c)) => {
                                        if let Basis::Time(tf) = new_basis {
                                            let streams: Vec<StreamKind> = c
                                                .selected_tickers()
                                                .iter()
                                                .copied()
                                                .map(|ti| StreamKind::Kline {
                                                    ticker_info: ti,
                                                    timeframe: tf,
                                                })
                                                .collect();

                                            self.streams = ResolvedStream::Ready(streams);
                                            let action = c.set_basis(new_basis);

                                            if let Some(chart::Action::RequestFetch(fetch)) = action
                                            {
                                                effect = Some(Effect::RequestFetch(fetch));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    self.modal = Some(Modal::StreamModifier(modifier));

                    if let Some(e) = effect {
                        return Some(e);
                    }
                }
            }
            Event::ComparisonChartInteraction(message) => {
                if let Content::Comparison(chart_opt) = &mut self.content
                    && let Some(chart) = chart_opt
                    && let Some(action) = chart.update(message)
                {
                    match action {
                        super::chart::comparison::Action::SeriesColorChanged(t, color) => {
                            chart.set_series_color(t, color);
                        }
                        super::chart::comparison::Action::SeriesNameChanged(t, name) => {
                            chart.set_series_name(t, name);
                        }
                        super::chart::comparison::Action::OpenSeriesEditor => {
                            self.modal = Some(Modal::Settings);
                        }
                        super::chart::comparison::Action::RemoveSeries(ti) => {
                            let rebuilt = chart.remove_ticker(&ti);
                            self.streams = ResolvedStream::Ready(rebuilt);

                            return Some(Effect::RefreshStreams);
                        }
                    }
                }
            }
            Event::MiniTickersListInteraction(message) => {
                if let Some(Modal::MiniTickersList(ref mut mini_panel)) = self.modal
                    && let Some(action) = mini_panel.update(message)
                {
                    self.modal = Some(Modal::MiniTickersList(mini_panel.clone()));

                    let crate::modal::pane::mini_tickers_list::Action::RowSelected(sel) = action;
                    match sel {
                        crate::modal::pane::mini_tickers_list::RowSelection::Add(ti) => {
                            if let Content::Comparison(chart) = &mut self.content
                                && let Some(c) = chart
                            {
                                let rebuilt = c.add_ticker(&ti);
                                self.streams = ResolvedStream::Ready(rebuilt);
                                return Some(Effect::RefreshStreams);
                            }
                        }
                        crate::modal::pane::mini_tickers_list::RowSelection::Remove(ti) => {
                            if let Content::Comparison(chart) = &mut self.content
                                && let Some(c) = chart
                            {
                                let rebuilt = c.remove_ticker(&ti);
                                self.streams = ResolvedStream::Ready(rebuilt);
                                return Some(Effect::RefreshStreams);
                            }
                        }
                        crate::modal::pane::mini_tickers_list::RowSelection::Switch(ti) => {
                            return Some(Effect::SwitchTickersInGroup(ti));
                        }
                    }
                }
            }
            Event::HeatmapTradeSizeInput(input) => {
                if let Content::Heatmap { chart: Some(c), .. } = &mut self.content {
                    c.set_trade_size_input(input);
                }
            }
            Event::HeatmapOrderSizeInput(input) => {
                if let Content::Heatmap { chart: Some(c), .. } = &mut self.content {
                    c.set_order_size_input(input);
                }
            }
            Event::TimeAndSalesTradeSizeInput(input) => {
                if let Content::TimeAndSales(Some(panel)) = &mut self.content {
                    panel.set_trade_size_input(input);
                }
            }
            Event::SelectDrawingTool(tool) => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.set_active_drawing_tool(tool);
                }
            }
            Event::ClearDrawings => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.clear_drawings();
                    self.selected_drawing_show_settings = false;
                }
            }
            Event::DrawingToolbarMoved(pos) => {
                self.drawing_toolbar_pos = Some(pos);
            }
            Event::SelectedDrawingToolbarMoved(pos) => {
                self.selected_drawing_toolbar_pos = Some(pos);
            }
            Event::SelectedDrawingAction(action) => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    match action {
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ToggleSettings => {
                            self.selected_drawing_show_settings = !self.selected_drawing_show_settings;
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ToggleLock => {
                            c.toggle_selected_drawing_lock();
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::Delete => {
                            if let Some(d) = c.selected_drawing() {
                                let id = d.id;
                                c.delete_drawing(id);
                                self.selected_drawing_show_settings = false;
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetColor(color) => {
                            c.update_selected_drawing_color(color);
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetWidth(w) => {
                            c.update_selected_drawing_width(w);
                        }
                    }
                }
            }
            Event::TakeScreenshot(window_id) => {
                return Some(Effect::TakeScreenshot(window_id));
            }
            Event::ToggleReplay => {
                if matches!(self.modal, Some(Modal::ReplayDatePicker(_))) {
                    self.modal = None;
                }
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.toggle_replay();
                }
            }
            Event::ToggleReplayDatePicker => {
                if matches!(self.modal, Some(Modal::ReplayDatePicker(_))) {
                    self.modal = None;
                } else if let Content::Kline { chart: Some(c), .. } = &self.content {
                    let cutoff = c.replay_state().map(|r| r.cutoff_time).unwrap_or(0);
                    let state = modal::pane::replay_calendar::ReplayDatePickerState::new(cutoff);
                    self.modal = Some(Modal::ReplayDatePicker(state));
                }
            }
            Event::ReplayDatePickerAction(action) => {
                use modal::pane::replay_calendar::Action;
                match action {
                    Action::PrevMonth => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            if state.view_month == 1 {
                                state.view_year -= 1;
                                state.view_month = 12;
                            } else {
                                state.view_month -= 1;
                            }
                        }
                    }
                    Action::NextMonth => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            if state.view_month == 12 {
                                state.view_year += 1;
                                state.view_month = 1;
                            } else {
                                state.view_month += 1;
                            }
                        }
                    }
                    Action::PrevYear => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            state.view_year -= 1;
                        }
                    }
                    Action::NextYear => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            state.view_year += 1;
                        }
                    }
                    Action::SelectDate(date) => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            state.selected_date = date;
                            if let Some(target_ts) = state.to_timestamp_millis()
                                && let Content::Kline { chart: Some(c), .. } = &mut self.content
                            {
                                let actual_ts = c
                                    .find_closest_bar_at_or_before(target_ts)
                                    .unwrap_or(target_ts);
                                c.replay_set_cutoff_and_jump(actual_ts);
                            }
                        }
                    }
                    Action::AdjustHour(delta) => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            let curr = state.hour as i32;
                            state.hour = (curr + delta).rem_euclid(24) as u32;
                            if let Some(target_ts) = state.to_timestamp_millis()
                                && let Content::Kline { chart: Some(c), .. } = &mut self.content
                            {
                                let actual_ts = c
                                    .find_closest_bar_at_or_before(target_ts)
                                    .unwrap_or(target_ts);
                                c.replay_set_cutoff_and_jump(actual_ts);
                            }
                        }
                    }
                    Action::AdjustMinute(delta) => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            let curr = state.minute as i32;
                            state.minute = (curr + delta).rem_euclid(60) as u32;
                            if let Some(target_ts) = state.to_timestamp_millis()
                                && let Content::Kline { chart: Some(c), .. } = &mut self.content
                            {
                                let actual_ts = c
                                    .find_closest_bar_at_or_before(target_ts)
                                    .unwrap_or(target_ts);
                                c.replay_set_cutoff_and_jump(actual_ts);
                            }
                        }
                    }
                    Action::SetTime(h, m) => {
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            state.hour = h.min(23);
                            state.minute = m.min(59);
                            if let Some(target_ts) = state.to_timestamp_millis()
                                && let Content::Kline { chart: Some(c), .. } = &mut self.content
                            {
                                let actual_ts = c
                                    .find_closest_bar_at_or_before(target_ts)
                                    .unwrap_or(target_ts);
                                c.replay_set_cutoff_and_jump(actual_ts);
                            }
                        }
                    }
                    Action::RandomBar => {
                        if let Content::Kline { chart: Some(c), .. } = &mut self.content
                            && let Some(rand_ts) = c.replay_pick_random_bar()
                            && let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal
                        {
                            *state =
                                modal::pane::replay_calendar::ReplayDatePickerState::new(rand_ts);
                        }
                    }
                    Action::JumpToToday => {
                        let now = chrono::Utc::now();
                        let target_ts = now.timestamp_millis() as u64;
                        if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                            let actual_ts = c
                                .find_closest_bar_at_or_before(target_ts)
                                .unwrap_or(target_ts);
                            c.replay_set_cutoff_and_jump(actual_ts);
                        }
                        if let Some(Modal::ReplayDatePicker(ref mut state)) = self.modal {
                            *state =
                                modal::pane::replay_calendar::ReplayDatePickerState::new(target_ts);
                        }
                    }
                    Action::Close => {
                        self.modal = None;
                    }
                }
            }
            Event::ReplayPlayPause => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.replay_play_pause();
                }
            }
            Event::ReplayStepForward => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.replay_step_forward();
                }
            }
            Event::ReplayStepBackward => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.replay_step_backward();
                }
            }
            Event::ReplaySetSpeed(speed) => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.replay_set_speed(speed);
                }
            }
            Event::AddPriceAlert(price) => {
                let ticker_info = self.stream_pair();
                let ticker = ticker_info.map(|ti| ti.ticker);
                let symbol = ticker_info
                    .map(|ti| ti.ticker.display_symbol_and_type().0)
                    .unwrap_or_else(|| "Symbol".to_string());
                let initial_price = if let Content::Kline { chart: Some(c), .. } = &self.content {
                    c.current_price()
                } else {
                    None
                };
                let alert = data::chart::alert::PriceAlert::with_details(
                    ticker,
                    symbol,
                    price,
                    initial_price,
                    self.alert_condition,
                );
                data::AlertStore::add(alert.clone());
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.add_alert(alert);
                }
                self.alert_price_input.clear();
            }
            Event::DeletePriceAlert(id) => {
                data::AlertStore::remove(id);
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.remove_alert(id);
                }
            }
            Event::TogglePriceAlert(id) => {
                data::AlertStore::toggle(id);
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.toggle_alert(id);
                }
            }
            Event::AlertPriceInput(val) => {
                self.alert_price_input = val;
            }
            Event::AlertConditionSelected(cond) => {
                self.alert_condition = cond;
            }
            Event::AlertFilterSelected(filter) => {
                self.alert_filter = filter;
            }
            Event::ClearTriggeredAlerts => {
                data::AlertStore::clear_triggered();
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.alerts
                        .retain(|a| a.status != data::chart::alert::AlertStatus::Triggered);
                    c.invalidate_all();
                }
            }
        }
        None
    }

    fn view_controls(
        &'_ self,
        pane: pane_grid::Pane,
        total_panes: usize,
        is_maximized: bool,
        is_popout: bool,
        window: window::Id,
    ) -> Element<'_, Message> {
        let modal_btn_style = |modal: Modal| {
            let is_active = self.modal == Some(modal);
            move |theme: &Theme, status: button::Status| {
                style::button::transparent(theme, status, is_active)
            }
        };

        let control_btn_style = |is_active: bool| {
            move |theme: &Theme, status: button::Status| {
                style::button::transparent(theme, status, is_active)
            }
        };

        let treat_as_starter =
            matches!(&self.content, Content::Starter) || !self.content.initialized();

        let tooltip_pos = tooltip::Position::Bottom;
        let mut buttons = row![];

        let show_modal = |modal: Modal| Message::PaneEvent(pane, Event::ShowModal(modal));

        if !treat_as_starter {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Cog, 12),
                show_modal(Modal::Settings),
                None,
                tooltip_pos,
                modal_btn_style(Modal::Settings),
            ));
        }
        if !treat_as_starter
            && matches!(
                &self.content,
                Content::Heatmap { .. } | Content::Kline { .. }
            )
        {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::ChartOutline, 12),
                show_modal(Modal::Indicators),
                Some("Indicators"),
                tooltip_pos,
                modal_btn_style(Modal::Indicators),
            ));
        }

        if !treat_as_starter && matches!(&self.content, Content::Kline { .. }) {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::SpeakerHigh, 12),
                show_modal(Modal::Alerts),
                Some("Price Alerts"),
                tooltip_pos,
                modal_btn_style(Modal::Alerts),
            ));

            let is_replay_active = if let Content::Kline { chart: Some(c), .. } = &self.content {
                c.is_replay_active()
            } else {
                false
            };
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Return, 12),
                Message::PaneEvent(pane, Event::ToggleReplay),
                Some("Market Replay"),
                tooltip_pos,
                control_btn_style(is_replay_active),
            ));
        }

        buttons = buttons.push(button_with_tooltip(
            icon_text(Icon::Clone, 12),
            Message::PaneEvent(pane, Event::TakeScreenshot(window)),
            Some("Screenshot to Clipboard"),
            tooltip_pos,
            control_btn_style(false),
        ));

        if is_popout {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Merge,
                Some("Merge"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        } else if total_panes > 1 {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Popout,
                Some("Pop out"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        }

        if total_panes > 1 {
            let (resize_icon, message) = if is_maximized {
                (Icon::ResizeSmall, Message::Restore)
            } else {
                (Icon::ResizeFull, Message::MaximizePane(pane))
            };

            buttons = buttons.push(button_with_tooltip(
                icon_text(resize_icon, 12),
                message,
                None,
                tooltip_pos,
                control_btn_style(is_maximized),
            ));

            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Close, 12),
                Message::ClosePane(pane),
                None,
                tooltip_pos,
                control_btn_style(false),
            ));
        }

        buttons
            .padding(padding::right(4).left(4))
            .align_y(Vertical::Center)
            .height(Length::Fixed(32.0))
            .into()
    }

    fn compose_stack_view<'a, F>(
        &'a self,
        base: Element<'a, Message>,
        pane: pane_grid::Pane,
        indicator_modal: Option<Element<'a, Message>>,
        compact_controls: Option<Element<'a, Message>>,
        settings_modal: F,
        selected_tickers: Option<&'a [TickerInfo]>,
        tickers_table: &'a TickersTable,
        show_toolbar: bool,
    ) -> Element<'a, Message>
    where
        F: FnOnce() -> Element<'a, Message>,
    {
        let base =
            widget::toast::Manager::new(base, &self.notifications, Alignment::End, move |msg| {
                Message::PaneEvent(pane, Event::DeleteNotification(msg))
            })
            .into();

        let on_blur = Message::PaneEvent(pane, Event::HideModal);

        let mut view: Element<'a, Message> = match &self.modal {
            Some(Modal::LinkGroup) => {
                let content = link_group_modal(pane, self.link_group);

                stack_modal(
                    base,
                    content,
                    on_blur,
                    padding::right(12).left(4),
                    Alignment::Start,
                )
            }
            Some(Modal::StreamModifier(modifier)) => stack_modal(
                base,
                modifier.view(self.stream_pair()).map(move |message| {
                    Message::PaneEvent(pane, Event::StreamModifierChanged(message))
                }),
                Message::PaneEvent(pane, Event::HideModal),
                padding::right(12).left(48),
                Alignment::Start,
            ),
            Some(Modal::MiniTickersList(panel)) => {
                let mini_list = panel
                    .view(tickers_table, selected_tickers, self.stream_pair())
                    .map(move |msg| {
                        Message::PaneEvent(pane, Event::MiniTickersListInteraction(msg))
                    });

                let content: Element<_> = container(mini_list)
                    .max_width(260)
                    .padding(16)
                    .style(style::chart_modal)
                    .into();

                stack_modal(
                    base,
                    content,
                    Message::PaneEvent(pane, Event::HideModal),
                    padding::left(12),
                    Alignment::Start,
                )
            }
            Some(Modal::Alerts) => {
                let current_price = if let Content::Kline { chart: Some(c), .. } = &self.content {
                    c.current_price()
                } else {
                    None
                };
                let ticker_symbol = self
                    .stream_pair()
                    .map(|ti| ti.ticker.display_symbol_and_type().0)
                    .unwrap_or_else(|| "Symbol".to_string());

                let all_alerts = data::AlertStore::all();
                let this_chart_alerts: Vec<_> = all_alerts
                    .iter()
                    .filter(|a| a.ticker_symbol == ticker_symbol)
                    .cloned()
                    .collect();

                let content = crate::modal::pane::alerts::alerts_view(
                    pane,
                    &ticker_symbol,
                    current_price,
                    &this_chart_alerts,
                    &all_alerts,
                    self.alert_filter,
                    &self.alert_price_input,
                    self.alert_condition,
                );

                stack_modal(
                    base,
                    content,
                    on_blur,
                    padding::right(12).left(12),
                    Alignment::End,
                )
            }
            Some(Modal::Settings) => stack_modal(
                base,
                settings_modal(),
                on_blur,
                padding::right(12).left(12),
                Alignment::End,
            ),
            Some(Modal::Indicators) => stack_modal(
                base,
                indicator_modal.unwrap_or_else(|| column![].into()),
                on_blur,
                padding::right(12).left(12),
                Alignment::End,
            ),
            Some(Modal::Controls) => stack_modal(
                base,
                if let Some(controls) = compact_controls {
                    controls
                } else {
                    column![].into()
                },
                on_blur,
                padding::left(12),
                Alignment::End,
            ),
            Some(Modal::ReplayDatePicker(picker_state)) => stack_modal_positioned(
                base,
                modal::pane::replay_calendar::view(pane, picker_state),
                on_blur,
                padding::bottom(50),
                Alignment::Center,
                Alignment::End,
            ),
            None => base,
        };

        if show_toolbar && let Content::Kline { chart: Some(c), .. } = &self.content {
            let active_tool = c.active_drawing_tool();
            let has_drawings = !c.drawings.is_empty();

            let toolbar =
                widget::chart::drawing_toolbar::view(active_tool, has_drawings, move |action| {
                    match action {
                        widget::chart::drawing_toolbar::ToolbarAction::SelectTool(tool) => {
                            Message::PaneEvent(pane, Event::SelectDrawingTool(tool))
                        }
                        widget::chart::drawing_toolbar::ToolbarAction::ClearDrawings => {
                            Message::PaneEvent(pane, Event::ClearDrawings)
                        }
                    }
                });

            let toolbar_pos = self
                .drawing_toolbar_pos
                .unwrap_or(iced::Point::new(16.0, 48.0));
            let draggable_toolbar =
                widget::DraggableOverlay::new(toolbar, toolbar_pos, move |new_pos| {
                    Message::PaneEvent(pane, Event::DrawingToolbarMoved(new_pos))
                })
                .drag_handle_width(32.0);

            view = iced::widget::stack![view, draggable_toolbar].into();
        }

        if let Content::Kline { chart: Some(c), .. } = &self.content
            && let Some(sel_d) = c.selected_drawing()
        {
            let sel_toolbar = widget::chart::drawing_selection_toolbar::view(
                sel_d,
                self.selected_drawing_show_settings,
                move |action| Message::PaneEvent(pane, Event::SelectedDrawingAction(action)),
            );

            let sel_toolbar_pos = self
                .selected_drawing_toolbar_pos
                .unwrap_or(iced::Point::new(260.0, 48.0));
            let draggable_sel_toolbar =
                widget::DraggableOverlay::new(sel_toolbar, sel_toolbar_pos, move |new_pos| {
                    Message::PaneEvent(pane, Event::SelectedDrawingToolbarMoved(new_pos))
                })
                .drag_handle_width(32.0);

            view = iced::widget::stack![view, draggable_sel_toolbar].into();
        }

        if let Content::Kline { chart: Some(c), .. } = &self.content
            && let Some(rep) = c.replay_state()
            && rep.active
        {
            let dt_str = chrono::DateTime::from_timestamp_millis(rep.cutoff_time as i64)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "--".to_string());

            let speed_btn = |speed_ms: u64, label: &'static str| {
                let is_selected = rep.speed_ms == speed_ms;
                button(text(label).size(10).font(style::AZERET_MONO))
                    .style(move |theme, status| style::button::modifier(theme, status, is_selected))
                    .on_press(Message::PaneEvent(pane, Event::ReplaySetSpeed(speed_ms)))
                    .padding([2, 6])
            };

            let play_btn_text = if rep.is_playing { "PAUSE" } else { "PLAY" };
            let is_picker_open = matches!(self.modal, Some(Modal::ReplayDatePicker(_)));

            let replay_bar = container(
                row![
                    container(text("REPLAY").size(10).font(style::AZERET_MONO))
                        .padding([2, 6])
                        .style(|theme: &Theme| {
                            let p = theme.extended_palette();
                            container::Style {
                                background: Some(p.primary.weak.color.into()),
                                text_color: Some(p.primary.weak.text),
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                    button(modal::pane::replay_calendar::step_backward_icon(
                        None, 12.0, 10.0
                    ))
                    .style(|theme, status| style::button::transparent(theme, status, false))
                    .on_press(Message::PaneEvent(pane, Event::ReplayStepBackward))
                    .padding([3, 7]),
                    button(text(play_btn_text).size(10).font(style::AZERET_MONO))
                        .style(|theme, status| style::button::modifier(
                            theme,
                            status,
                            rep.is_playing
                        ))
                        .on_press(Message::PaneEvent(pane, Event::ReplayPlayPause))
                        .padding([3, 10]),
                    button(modal::pane::replay_calendar::step_forward_icon(
                        None, 12.0, 10.0
                    ))
                    .style(|theme, status| style::button::transparent(theme, status, false))
                    .on_press(Message::PaneEvent(pane, Event::ReplayStepForward))
                    .padding([3, 7]),
                    row![
                        speed_btn(1000, "1x"),
                        speed_btn(500, "2x"),
                        speed_btn(200, "5x"),
                        speed_btn(100, "10x"),
                    ]
                    .spacing(2),
                    button(
                        row![
                            modal::pane::replay_calendar::calendar_icon(None, is_picker_open, 12.0),
                            text(dt_str).size(11).font(style::AZERET_MONO),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([2, 8])
                    .style(move |theme: &Theme, status| {
                        let p = theme.extended_palette();
                        let is_hovered = matches!(status, button::Status::Hovered);
                        let bg = if is_picker_open {
                            p.primary.weak.color
                        } else if is_hovered {
                            p.background.strong.color
                        } else {
                            p.background.weak.color
                        };
                        let border_color = if is_picker_open {
                            p.primary.strong.color
                        } else {
                            p.background.strong.color
                        };
                        button::Style {
                            background: Some(bg.into()),
                            text_color: if is_picker_open {
                                p.primary.weak.text
                            } else {
                                p.background.base.text
                            },
                            border: iced::Border {
                                radius: 2.0.into(),
                                width: 1.0,
                                color: border_color,
                            },
                            ..Default::default()
                        }
                    })
                    .on_press(Message::PaneEvent(pane, Event::ToggleReplayDatePicker)),
                    button(modal::pane::replay_calendar::random_icon(None, 12.0))
                        .style(|theme, status| style::button::transparent(theme, status, false))
                        .on_press(Message::PaneEvent(
                            pane,
                            Event::ReplayDatePickerAction(
                                modal::pane::replay_calendar::Action::RandomBar
                            ),
                        ))
                        .padding([3, 6]),
                    button(icon_text(Icon::Close, 11))
                        .style(|theme, status| style::button::transparent(theme, status, false))
                        .on_press(Message::PaneEvent(pane, Event::ToggleReplay))
                        .padding([3, 5]),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(style::chart_modal);

            let dock = container(replay_bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::End)
                .padding(padding::bottom(10));

            iced::widget::stack![view, dock].into()
        } else {
            view
        }
    }

    pub fn matches_stream(&self, stream: &StreamKind) -> bool {
        self.streams.matches_stream(stream)
    }

    pub fn matches_trades(&self, stream: &StreamKind) -> bool {
        self.streams.matches_trades(stream)
    }

    fn show_modal_with_focus(&mut self, requested_modal: Modal) -> Option<Effect> {
        let should_toggle_close = match (&self.modal, &requested_modal) {
            (Some(Modal::StreamModifier(open)), Modal::StreamModifier(req)) => {
                open.view_mode == req.view_mode
            }
            (Some(open), req) => core::mem::discriminant(open) == core::mem::discriminant(req),
            _ => false,
        };

        if should_toggle_close {
            self.modal = None;
            return None;
        }

        let focus_widget_id = match &requested_modal {
            Modal::MiniTickersList(m) => Some(m.search_box_id.clone()),
            _ => None,
        };

        self.modal = Some(requested_modal);
        focus_widget_id.map(Effect::FocusWidget)
    }

    pub fn invalidate(&mut self, now: Instant) -> Option<Action> {
        match &mut self.content {
            Content::Heatmap { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
            Content::Kline { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
            Content::TimeAndSales(panel) => panel
                .as_mut()
                .and_then(|p| p.invalidate(Some(now)).map(Action::Panel)),
            Content::Ladder(panel) => panel
                .as_mut()
                .and_then(|p| p.invalidate(Some(now)).map(Action::Panel)),
            Content::Starter => None,
            Content::Comparison(chart) => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
        }
    }

    pub fn update_interval(&self) -> Option<u64> {
        match &self.content {
            Content::Kline { chart, .. } => {
                if let Some(chart) = chart
                    && let Some(rep) = &chart.replay
                    && rep.active
                    && rep.is_playing
                {
                    return Some(rep.speed_ms.min(100));
                }
                Some(1000)
            }
            Content::Comparison(_) => Some(1000),
            Content::Heatmap { chart, .. } => {
                if let Some(chart) = chart {
                    chart.basis_interval()
                } else {
                    None
                }
            }
            Content::Ladder(_) | Content::TimeAndSales(_) => Some(100),
            Content::Starter => None,
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        self.content.last_tick()
    }

    pub fn tick(&mut self, now: Instant) -> Option<Action> {
        let invalidate_interval: Option<u64> = self.update_interval();
        let last_tick: Option<Instant> = self.last_tick();

        if let Some(streams) = self.streams.waiting_to_resolve()
            && !streams.is_empty()
        {
            return Some(Action::ResolveStreams(streams.to_vec()));
        }

        if !self.content.initialized() {
            return Some(Action::ResolveContent);
        }

        match (invalidate_interval, last_tick) {
            (Some(interval_ms), Some(previous_tick_time)) => {
                if interval_ms > 0 {
                    let interval_duration = std::time::Duration::from_millis(interval_ms);
                    if now.duration_since(previous_tick_time) >= interval_duration {
                        return self.invalidate(now);
                    }
                }
            }
            (Some(interval_ms), None) => {
                if interval_ms > 0 {
                    return self.invalidate(now);
                }
            }
            (None, _) => {}
        }

        None
    }

    pub fn unique_id(&self) -> uuid::Uuid {
        self.id
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            modal: None,
            content: Content::Starter,
            settings: Settings::default(),
            streams: ResolvedStream::Waiting(vec![]),
            notifications: vec![],
            status: Status::Ready,
            link_group: None,
            alert_price_input: String::new(),
            alert_condition: data::chart::alert::AlertCondition::Crossing,
            alert_filter: data::chart::alert::AlertFilter::ThisChart,
            drawing_toolbar_pos: None,
            selected_drawing_toolbar_pos: None,
            selected_drawing_show_settings: false,
        }
    }
}

#[derive(Default)]
#[allow(clippy::large_enum_variant)]
pub enum Content {
    #[default]
    Starter,
    Heatmap {
        chart: Option<HeatmapChart>,
        indicators: Vec<HeatmapIndicator>,
        layout: data::chart::ViewConfig,
        studies: Vec<data::chart::heatmap::HeatmapStudy>,
    },
    Kline {
        chart: Option<KlineChart>,
        indicators: Vec<KlineIndicator>,
        layout: data::chart::ViewConfig,
        kind: data::chart::KlineChartKind,
    },
    TimeAndSales(Option<TimeAndSales>),
    Ladder(Option<Ladder>),
    Comparison(Option<ComparisonChart>),
}

impl Content {
    fn new_heatmap(
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (enabled_indicators, layout, prev_studies) = if let Content::Heatmap {
            chart,
            indicators,
            studies,
            layout,
        } = current_content
        {
            (
                indicators.clone(),
                chart
                    .as_ref()
                    .map(|c| c.chart_layout())
                    .unwrap_or(layout.clone()),
                chart
                    .as_ref()
                    .map_or(studies.clone(), |c| c.studies.clone()),
            )
        } else {
            (
                vec![HeatmapIndicator::Volume],
                ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
                vec![],
            )
        };

        let basis = settings
            .selected_basis
            .unwrap_or_else(|| Basis::default_heatmap_time(Some(ticker_info)));
        let config = settings.visual_config.clone().and_then(|cfg| cfg.heatmap());

        let chart = HeatmapChart::new(
            layout.clone(),
            basis,
            tick_size,
            &enabled_indicators,
            ticker_info,
            config,
            prev_studies.clone(),
        );

        Content::Heatmap {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            studies: prev_studies,
        }
    }

    fn new_kline(
        content_kind: ContentKind,
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (prev_indis, prev_layout, prev_kind_opt, prev_config) = if let Content::Kline {
            chart,
            indicators,
            kind,
            layout,
        } = current_content
        {
            (
                Some(indicators.clone()),
                Some(chart.as_ref().map_or(layout.clone(), |c| c.chart_layout())),
                Some(chart.as_ref().map_or(kind.clone(), |c| c.kind().clone())),
                chart.as_ref().map(|c| c.config()),
            )
        } else {
            (None, None, None, None)
        };

        let prev_was_footprint = prev_kind_opt
            .as_ref()
            .is_some_and(|k| matches!(k, data::chart::KlineChartKind::Footprint { .. }));

        let (default_tf, determined_chart_kind) = match content_kind {
            ContentKind::FootprintChart => (
                Timeframe::M5,
                prev_kind_opt
                    .filter(|k| matches!(k, data::chart::KlineChartKind::Footprint { .. }))
                    .unwrap_or_else(|| data::chart::KlineChartKind::Footprint {
                        clusters: data::chart::kline::ClusterKind::default(),
                        scaling: data::chart::kline::ClusterScaling::default(),
                        studies: vec![],
                        show_bottom_volume: false,
                    }),
            ),
            ContentKind::CandlestickChart => (Timeframe::M15, data::chart::KlineChartKind::Candles),
            ContentKind::TpoChart => (
                Timeframe::M30,
                prev_kind_opt
                    .filter(|k| matches!(k, data::chart::KlineChartKind::Tpo { .. }))
                    .unwrap_or(data::chart::KlineChartKind::Tpo {
                        show_candles: true,
                        show_letters: true,
                        show_ib: true,
                        show_va: true,
                        show_poc: true,
                        show_single_prints: true,
                        tick_step: Default::default(),
                        period: Default::default(),
                        clusters: Vec::new(),
                        split_sessions: Vec::new(),
                        color_scheme: Default::default(),
                        ib_color: Default::default(),
                        poc_color: Default::default(),
                        single_prints_color: Default::default(),
                    }),
            ),
            _ => unreachable!("invalid content kind for kline chart"),
        };

        let basis = settings.selected_basis.unwrap_or(Basis::Time(default_tf));

        let enabled_indicators = {
            let available = KlineIndicator::for_market(ticker_info.market_type());
            let is_footprint = matches!(content_kind, ContentKind::FootprintChart);

            if is_footprint && !prev_was_footprint {
                vec![]
            } else {
                prev_indis.map_or_else(
                    || {
                        if is_footprint {
                            vec![]
                        } else {
                            vec![KlineIndicator::Volume]
                        }
                    },
                    |indis| {
                        indis
                            .into_iter()
                            .filter(|i| available.contains(i))
                            .collect()
                    },
                )
            }
        };

        let splits = if enabled_indicators.is_empty() {
            vec![]
        } else {
            let main_chart_split: f32 = 0.8;
            let mut splits_vec = vec![main_chart_split];
            let num_indicators = enabled_indicators.len();

            let indicator_total_height_ratio = 1.0 - main_chart_split;
            let height_per_indicator_pane = indicator_total_height_ratio / num_indicators as f32;

            let mut current_split_pos = main_chart_split;
            for _ in 0..(num_indicators - 1) {
                current_split_pos += height_per_indicator_pane;
                splits_vec.push(current_split_pos);
            }
            splits_vec
        };

        let layout = prev_layout
            .filter(|l| l.splits.len() == splits.len())
            .unwrap_or(ViewConfig {
                splits,
                autoscale: Some(data::chart::Autoscale::FitToVisible),
            });

        let kline_config = settings
            .visual_config
            .as_ref()
            .and_then(|cfg| cfg.kline())
            .or(prev_config);

        let mut chart = KlineChart::new(
            layout.clone(),
            basis,
            tick_size,
            &[],
            vec![],
            &enabled_indicators,
            ticker_info,
            &determined_chart_kind,
            kline_config,
        );
        chart.alerts =
            data::AlertStore::for_symbol(&ticker_info.ticker.display_symbol_and_type().0);

        Content::Kline {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            kind: determined_chart_kind,
        }
    }

    fn placeholder(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Starter => Content::Starter,
            ContentKind::CandlestickChart => Content::Kline {
                chart: None,
                indicators: vec![KlineIndicator::Volume],
                kind: data::chart::KlineChartKind::Candles,
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::TpoChart => Content::Kline {
                chart: None,
                indicators: vec![KlineIndicator::Volume],
                kind: data::chart::KlineChartKind::Tpo {
                    show_candles: true,
                    show_letters: true,
                    show_ib: true,
                    show_va: true,
                    show_poc: true,
                    show_single_prints: true,
                    tick_step: Default::default(),
                    period: Default::default(),
                    clusters: Vec::new(),
                    split_sessions: Vec::new(),
                    color_scheme: Default::default(),
                    ib_color: Default::default(),
                    poc_color: Default::default(),
                    single_prints_color: Default::default(),
                },
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::FootprintChart => Content::Kline {
                chart: None,
                indicators: vec![],
                kind: data::chart::KlineChartKind::Footprint {
                    clusters: data::chart::kline::ClusterKind::default(),
                    scaling: data::chart::kline::ClusterScaling::default(),
                    studies: vec![],
                    show_bottom_volume: false,
                },
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::HeatmapChart => Content::Heatmap {
                chart: None,
                indicators: vec![HeatmapIndicator::Volume],
                studies: vec![],
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
            },
            ContentKind::ComparisonChart => Content::Comparison(None),
            ContentKind::TimeAndSales => Content::TimeAndSales(None),
            ContentKind::Ladder => Content::Ladder(None),
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        match self {
            Content::Heatmap { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::Kline { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::TimeAndSales(panel) => Some(panel.as_ref()?.last_update()),
            Content::Ladder(panel) => Some(panel.as_ref()?.last_update()),
            Content::Comparison(chart) => Some(chart.as_ref()?.last_update()),
            Content::Starter => None,
        }
    }

    pub fn chart_kind(&self) -> Option<data::chart::KlineChartKind> {
        match self {
            Content::Kline { chart, .. } => Some(chart.as_ref()?.kind().clone()),
            _ => None,
        }
    }

    pub fn toggle_indicator(&mut self, indicator: UiIndicator) {
        match (self, indicator) {
            (
                Content::Heatmap {
                    chart, indicators, ..
                },
                UiIndicator::Heatmap(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            (
                Content::Kline {
                    chart, indicators, ..
                },
                UiIndicator::Kline(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            _ => panic!("indicator toggle on {indicator:?} pane",),
        }
    }

    pub fn reorder_indicators(&mut self, event: &column_drag::DragEvent) {
        match self {
            Content::Heatmap { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::Kline { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => {
                panic!("indicator reorder on {} pane", self)
            }
        }
    }

    pub fn change_visual_config(&mut self, config: VisualConfig) {
        match (self, config) {
            (Content::Kline { chart: Some(c), .. }, VisualConfig::Kline(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::Heatmap { chart: Some(c), .. }, VisualConfig::Heatmap(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::TimeAndSales(Some(panel)), VisualConfig::TimeAndSales(cfg)) => {
                panel.config = cfg;
            }
            (Content::Ladder(Some(panel)), VisualConfig::Ladder(cfg)) => {
                panel.config = cfg;
            }
            (Content::Comparison(Some(chart)), VisualConfig::Comparison(cfg)) => {
                chart.config = cfg;
            }
            _ => {}
        }
    }

    pub fn studies(&self) -> Option<data::chart::Study> {
        match &self {
            Content::Heatmap { studies, .. } => Some(data::chart::Study::Heatmap(studies.clone())),
            Content::Kline { kind, .. } => {
                if let data::chart::KlineChartKind::Footprint { studies, .. } = kind {
                    Some(data::chart::Study::Footprint(studies.clone()))
                } else {
                    None
                }
            }
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => None,
        }
    }

    pub fn update_studies(&mut self, studies: data::chart::Study) {
        match (self, studies) {
            (
                Content::Heatmap {
                    chart,
                    studies: previous,
                    ..
                },
                data::chart::Study::Heatmap(studies),
            ) => {
                chart
                    .as_mut()
                    .expect("heatmap chart not initialized")
                    .studies = studies.clone();
                *previous = studies;
            }
            (Content::Kline { chart, kind, .. }, data::chart::Study::Footprint(studies)) => {
                chart
                    .as_mut()
                    .expect("kline chart not initialized")
                    .set_studies(studies.clone());
                if let data::chart::KlineChartKind::Footprint {
                    studies: k_studies, ..
                } = kind
                {
                    *k_studies = studies;
                }
            }
            _ => {}
        }
    }

    pub fn kind(&self) -> ContentKind {
        match self {
            Content::Heatmap { .. } => ContentKind::HeatmapChart,
            Content::Kline { kind, .. } => match kind {
                data::chart::KlineChartKind::Footprint { .. } => ContentKind::FootprintChart,
                data::chart::KlineChartKind::Candles => ContentKind::CandlestickChart,
                data::chart::KlineChartKind::Tpo { .. } => ContentKind::TpoChart,
            },
            Content::TimeAndSales(_) => ContentKind::TimeAndSales,
            Content::Ladder(_) => ContentKind::Ladder,
            Content::Comparison(_) => ContentKind::ComparisonChart,
            Content::Starter => ContentKind::Starter,
        }
    }

    fn initialized(&self) -> bool {
        match self {
            Content::Heatmap { chart, .. } => chart.is_some(),
            Content::Kline { chart, .. } => chart.is_some(),
            Content::TimeAndSales(panel) => panel.is_some(),
            Content::Ladder(panel) => panel.is_some(),
            Content::Comparison(chart) => chart.is_some(),
            Content::Starter => true,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind())
    }
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Content::Starter, Content::Starter)
                | (Content::Heatmap { .. }, Content::Heatmap { .. })
                | (Content::Kline { .. }, Content::Kline { .. })
                | (Content::TimeAndSales(_), Content::TimeAndSales(_))
                | (Content::Ladder(_), Content::Ladder(_))
        )
    }
}

fn link_group_modal<'a>(
    pane: pane_grid::Pane,
    selected_group: Option<LinkGroup>,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(4);
    let rows = LinkGroup::ALL.chunks(3);

    for row_groups in rows {
        let mut button_row = row![].spacing(4);

        for &group in row_groups {
            let is_selected = selected_group == Some(group);
            let btn_content = text(group.to_string()).font(style::AZERET_MONO);

            let btn = if is_selected {
                button_with_tooltip(
                    btn_content.align_x(iced::Alignment::Center),
                    Message::SwitchLinkGroup(pane, None),
                    Some("Unlink"),
                    tooltip::Position::Bottom,
                    move |theme, status| style::button::menu_body(theme, status, true),
                )
            } else {
                button(btn_content.align_x(iced::Alignment::Center))
                    .on_press(Message::SwitchLinkGroup(pane, Some(group)))
                    .style(move |theme, status| style::button::menu_body(theme, status, false))
                    .into()
            };

            button_row = button_row.push(btn);
        }

        grid = grid.push(button_row);
    }

    container(grid)
        .max_width(240)
        .padding(16)
        .style(style::chart_modal)
        .into()
}

fn ticksize_modifier<'a>(
    id: pane_grid::Pane,
    base_ticksize: f32,
    multiplier: TickMultiplier,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
    exchange: Option<exchange::adapter::Exchange>,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_ticksize_view(base_ticksize, multiplier, exchange),
    );

    let is_active = modifier.is_some_and(|m| {
        matches!(
            m.view_mode,
            modal::stream::ViewMode::TicksizeSelection { .. }
        )
    });

    button(text(multiplier.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn basis_modifier<'a>(
    id: pane_grid::Pane,
    selected_basis: Basis,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_view_mode(modal::stream::ViewMode::BasisSelection),
    );

    let is_active =
        modifier.is_some_and(|m| m.view_mode == modal::stream::ViewMode::BasisSelection);

    button(text(selected_basis.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn by_basis_default<T>(
    basis: Option<Basis>,
    default_tf: Timeframe,
    on_time: impl FnOnce(Timeframe) -> T,
    on_tick: impl FnOnce() -> T,
) -> T {
    match basis.unwrap_or(Basis::Time(default_tf)) {
        Basis::Time(tf) => on_time(tf),
        Basis::Tick(_) => on_tick(),
    }
}
