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

#[cfg(feature = "shader-heatmap")]
use crate::widget::chart::heatmap::HeatmapShader;
#[cfg(feature = "shader-heatmap")]
use data::chart::heatmap::HeatmapStudy;
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
    adapter::{
        Exchange, MarketKind, PersistStreamKind, ResolvedStream, StreamKind, StreamTicksize,
    },
    fetcher::FetchRequests,
};
use iced::{
    Alignment, Element, Length, Renderer, Theme,
    alignment::Vertical,
    padding,
    widget::{button, center, container, pane_grid, pick_list, row, text, tooltip},
};
use std::time::Instant;

pub mod content;
mod content_impl;
mod history;
mod update;
mod view;

#[derive(Debug, Clone)]
pub enum Effect {
    RefreshStreams,
    RequestFetch(FetchRequests),
    SwitchTickersInGroup(TickerInfo),
    FocusWidget(iced::widget::Id),
    TakeScreenshot(window::Id),
    AutofillJournal(data::journal::PositionAutofill),
    SyncDrawing {
        symbol: String,
        drawing: data::chart::drawing::Drawing,
        originating_pane_id: uuid::Uuid,
    },
    SyncDrawings {
        symbol: String,
        drawings: Vec<data::chart::drawing::Drawing>,
        originating_pane_id: uuid::Uuid,
    },
    UnsyncDrawing {
        symbol: String,
        drawing_id: uuid::Uuid,
        originating_pane_id: uuid::Uuid,
    },
    DeleteSyncedDrawing {
        symbol: String,
        drawing_id: uuid::Uuid,
    },
    ClearSyncedDrawings {
        symbol: String,
        drawing_ids: Vec<uuid::Uuid>,
    },
    DeleteAlert(uuid::Uuid),
    ToggleAlert(uuid::Uuid),
    ClearTriggeredAlerts,
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
    ToggleScriptedIndicator(data::chart::kline::indicators::IndicatorId),
    IndicatorSearchChanged(String),
    ToggleIndicatorSettings(KlineIndicator),
    DeleteNotification(usize),
    ReorderIndicator(column_drag::DragEvent),
    ClusterKindSelected(data::chart::kline::ClusterKind),
    ClusterScalingSelected(data::chart::kline::ClusterScaling),
    StudyConfigurator(modal::pane::settings::study::StudyMessage),
    StreamModifierChanged(modal::stream::Message),
    ComparisonChartInteraction(super::chart::comparison::Message),
    #[cfg(feature = "shader-heatmap")]
    HeatmapShaderInteraction(crate::widget::chart::heatmap::Message),
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
    /// Footprint aggregate toggle
    FootprintAggregateToggled(bool),
    /// Footprint aggregate exchange toggle
    FootprintAggregateExchangeToggled(Exchange, bool),
    /// Footprint history lookback input changed
    FootprintHistoryLookbackInput(String),
    /// Footprint history lookback preset selected
    FootprintHistoryLookbackSet(u32),
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
    /// Undo drawing action (Ctrl+Z)
    UndoDrawing,
    /// Redo drawing action (Ctrl+Y / Ctrl+Shift+Z)
    RedoDrawing,
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
    DrawingSettingsAction(modal::pane::drawing_settings::DrawingSettingsAction),
    ReplayAutoScrollToCutoff,
    /// Price Alerts
    AddPriceAlert(f32),
    DeletePriceAlert(uuid::Uuid),
    TogglePriceAlert(uuid::Uuid),
    AlertPriceInput(String),
    AlertConditionSelected(data::chart::alert::AlertCondition),
    AlertFilterSelected(data::chart::alert::AlertFilter),
    ClearTriggeredAlerts,
    SelectSettingsCategory(modal::pane::settings::SettingsCategory),
}

pub struct State {
    pub id: uuid::Uuid,
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
    pub footprint_notice_dismissed: Option<Exchange>,
    pub settings_category: modal::pane::settings::SettingsCategory,
    pub indicator_search_query: String,
    pub layout_id: Option<uuid::Uuid>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_footprint_exchange_notice(&mut self) {
        let is_footprint = match &self.content {
            Content::Kline { kind, .. } => {
                matches!(kind, data::chart::KlineChartKind::Footprint { .. })
            }
            _ => false,
        };

        if !is_footprint {
            self.notifications
                .retain(|t| !t.title.contains("Footprint"));
            return;
        }

        let exchange = match self.stream_pair() {
            Some(ti) => ti.exchange(),
            None => return,
        };

        if let Some(dismissed_ex) = self.footprint_notice_dismissed
            && dismissed_ex != exchange
        {
            self.footprint_notice_dismissed = None;
        }

        match exchange {
            Exchange::HyperliquidLinear | Exchange::HyperliquidSpot => {
                self.notifications.retain(|t| {
                    t.title == "Hyperliquid Footprint" || !t.title.contains("Footprint")
                });
                if self.footprint_notice_dismissed != Some(exchange)
                    && !self
                        .notifications
                        .iter()
                        .any(|t| t.title == "Hyperliquid Footprint")
                {
                    self.notifications.push(
                        Toast::error("Дані з інших днів не надходитимуть через відсутність сервера (Hyperliquid не надає публічних архівів угод).")
                            .with_title("Hyperliquid Footprint")
                            .persistent()
                            .closable(true),
                    );
                }
            }
            Exchange::BybitLinear | Exchange::BybitInverse | Exchange::BybitSpot => {
                self.notifications
                    .retain(|t| t.title == "Bybit Footprint" || !t.title.contains("Footprint"));
                if self.footprint_notice_dismissed != Some(exchange)
                    && !self
                        .notifications
                        .iter()
                        .any(|t| t.title == "Bybit Footprint")
                {
                    self.notifications.push(
                        Toast::warn("Точні дані за сьогодні завантажуються частково (Bybit обмежує історію до 1000 угод; потрібен сервер).")
                            .with_title("Bybit Footprint")
                            .persistent()
                            .closable(true),
                    );
                }
            }
            Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
                self.notifications
                    .retain(|t| t.title == "OKX Footprint" || !t.title.contains("Footprint"));
                if self.footprint_notice_dismissed != Some(exchange)
                    && !self
                        .notifications
                        .iter()
                        .any(|t| t.title == "OKX Footprint")
                {
                    self.notifications.push(
                        Toast::warn("Дані за минулі дні завантажуються з архівів, але поточний день має обмеження REST API без сервера.")
                            .with_title("OKX Footprint")
                            .persistent()
                            .closable(true),
                    );
                }
            }
            Exchange::BinanceLinear
            | Exchange::BinanceInverse
            | Exchange::BinanceSpot
            | Exchange::CoinbaseSpot => {
                self.notifications
                    .retain(|t| !t.title.contains("Footprint"));
            }
            Exchange::MexcLinear | Exchange::MexcInverse | Exchange::MexcSpot => {
                self.notifications
                    .retain(|t| t.title == "MEXC Footprint" || !t.title.contains("Footprint"));
                if self.footprint_notice_dismissed != Some(exchange)
                    && !self
                        .notifications
                        .iter()
                        .any(|t| t.title == "MEXC Footprint")
                {
                    self.notifications.push(
                        Toast::error("Дані з інших днів не надходитимуть через відсутність сервера (MEXC не надає публічних архівів угод).")
                            .with_title("MEXC Footprint")
                            .persistent()
                            .closable(true),
                    );
                }
            }
        }
    }

    pub fn from_config(
        content: Content,
        mut streams: Vec<PersistStreamKind>,
        mut settings: Settings,
        link_group: Option<LinkGroup>,
    ) -> Self {
        let id = settings.pane_id.unwrap_or_else(uuid::Uuid::new_v4);
        settings.pane_id = Some(id);

        if let Content::Kline { ref kind, .. } = content
            && matches!(
                kind,
                data::chart::KlineChartKind::Footprint { .. }
                    | data::chart::KlineChartKind::Tpo { .. }
            )
            && !streams
                .iter()
                .any(|s| matches!(s, PersistStreamKind::DepthAndTrades(_)))
            && let Some(base_ticker) = streams.iter().find_map(|s| match s {
                PersistStreamKind::Kline(k) => Some(k.ticker),
                _ => None,
            })
        {
            let depth_aggr = if base_ticker.exchange.is_depth_client_aggr() {
                StreamTicksize::Client
            } else {
                StreamTicksize::ServerSide(
                    settings
                        .tick_multiply
                        .unwrap_or(exchange::TickMultiplier(1)),
                )
            };
            streams.push(PersistStreamKind::DepthAndTrades(
                exchange::adapter::PersistDepth {
                    ticker: base_ticker,
                    depth_aggr,
                    push_freq: exchange::PushFrequency::ServerDefault,
                },
            ));
        }

        Self {
            id,
            content,
            settings,
            streams: ResolvedStream::Waiting(streams),
            link_group,
            ..Default::default()
        }
    }

    pub fn stream_pair(&self) -> Option<TickerInfo> {
        self.streams
            .find_ready_map(|stream| match stream {
                StreamKind::DepthAndTrades { ticker_info, .. }
                | StreamKind::Kline { ticker_info, .. } => Some(*ticker_info),
            })
            .or(match &self.content {
                Content::Kline { chart: Some(c), .. } => Some(c.ticker_info),
                _ => None,
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

    pub fn build_footprint_streams(
        base_ticker: TickerInfo,
        kind: &data::chart::KlineChartKind,
        settings: &Settings,
        tf: Option<Timeframe>,
    ) -> Vec<StreamKind> {
        let mut streams = vec![];
        if let Some(tf) = tf {
            streams.push(StreamKind::Kline {
                ticker_info: base_ticker,
                timeframe: tf,
            });
        }

        if let data::chart::KlineChartKind::Footprint {
            aggregate: true,
            aggregate_exchanges,
            ..
        } = kind
        {
            let base_asset = extract_base_symbol(&base_ticker.ticker);
            for &ex in aggregate_exchanges {
                let ex_ti = if ex == base_ticker.exchange() {
                    base_ticker
                } else {
                    ticker_info_for_exchange(&base_asset, ex, Some(&base_ticker))
                };
                let depth_aggr = if ex.is_depth_client_aggr() {
                    StreamTicksize::Client
                } else {
                    StreamTicksize::ServerSide(settings.tick_multiply.unwrap_or(TickMultiplier(1)))
                };
                streams.push(StreamKind::DepthAndTrades {
                    ticker_info: ex_ti,
                    depth_aggr,
                    push_freq: exchange::PushFrequency::ServerDefault,
                });
            }
        } else {
            let depth_aggr = if base_ticker.exchange().is_depth_client_aggr() {
                StreamTicksize::Client
            } else {
                StreamTicksize::ServerSide(settings.tick_multiply.unwrap_or(TickMultiplier(1)))
            };
            streams.push(StreamKind::DepthAndTrades {
                ticker_info: base_ticker,
                depth_aggr,
                push_freq: exchange::PushFrequency::ServerDefault,
            });
        }

        streams
    }

    pub fn sync_footprint_streams(&mut self) {
        if let Content::Kline { chart, kind, .. } = &self.content
            && let Some(base_ticker) = self.stream_pair()
        {
            let chart_kind = chart.as_ref().map_or(kind, |c| &c.kind);
            if matches!(chart_kind, data::chart::KlineChartKind::Footprint { .. }) {
                let tf = match self.settings.selected_basis {
                    Some(Basis::Time(tf)) => Some(tf),
                    None => self
                        .streams
                        .find_ready_map(|s| match s {
                            StreamKind::Kline { timeframe, .. } => Some(*timeframe),
                            _ => None,
                        })
                        .or_else(|| {
                            self.streams.waiting_to_resolve().and_then(|streams| {
                                streams.iter().find_map(|s| match s {
                                    PersistStreamKind::Kline(k) => Some(k.timeframe),
                                    _ => None,
                                })
                            })
                        })
                        .or(Some(Timeframe::M5)),
                    _ => None,
                };
                let streams =
                    Self::build_footprint_streams(base_ticker, chart_kind, &self.settings, tf);
                self.streams = ResolvedStream::Ready(streams);
            } else if matches!(chart_kind, data::chart::KlineChartKind::Tpo { .. }) {
                let tf = match self.settings.selected_basis {
                    Some(Basis::Time(tf)) => tf,
                    _ => self
                        .streams
                        .find_ready_map(|s| match s {
                            StreamKind::Kline { timeframe, .. } => Some(*timeframe),
                            _ => None,
                        })
                        .unwrap_or(Timeframe::M30),
                };
                let k_stream = StreamKind::Kline {
                    ticker_info: base_ticker,
                    timeframe: tf,
                };
                let depth_aggr = if base_ticker.exchange().is_depth_client_aggr() {
                    StreamTicksize::Client
                } else {
                    StreamTicksize::ServerSide(
                        self.settings.tick_multiply.unwrap_or(TickMultiplier(10)),
                    )
                };
                let d_stream = StreamKind::DepthAndTrades {
                    ticker_info: base_ticker,
                    depth_aggr,
                    push_freq: exchange::PushFrequency::ServerDefault,
                };
                self.streams = ResolvedStream::Ready(vec![k_stream, d_stream]);
            }
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

        // Without the feature the WGPU heatmap does not exist; serve the canvas heatmap.
        #[cfg(not(feature = "shader-heatmap"))]
        let kind = match kind {
            ContentKind::ShaderHeatmap => ContentKind::HeatmapChart,
            other => other,
        };

        let Some(&base_ticker) = tickers.first() else {
            return vec![];
        };
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
                #[cfg(not(feature = "shader-heatmap"))]
                ContentKind::ShaderHeatmap => {
                    unreachable!("mapped to HeatmapChart at the top of this function")
                }
                #[cfg(feature = "shader-heatmap")]
                ContentKind::ShaderHeatmap => {
                    let basis = derived_plan
                        .basis
                        .unwrap_or_else(|| Basis::default_heatmap_time(Some(base_ticker)));

                    let (studies, indicators) = if let Content::ShaderHeatmap {
                        chart,
                        studies,
                        indicators,
                    } = &self.content
                    {
                        (
                            chart
                                .as_ref()
                                .map_or(studies.clone(), |c| c.studies.clone()),
                            indicators.clone(),
                        )
                    } else {
                        (
                            vec![HeatmapStudy::VolumeProfile(
                                data::chart::heatmap::ProfileKind::default(),
                            )],
                            vec![HeatmapIndicator::Volume],
                        )
                    };

                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.heatmap());

                    let content = Content::ShaderHeatmap {
                        chart: Some(Box::new(HeatmapShader::new(
                            basis,
                            derived_plan.price_step,
                            base_ticker,
                            studies.clone(),
                            indicators.clone(),
                            config,
                        ))),
                        studies,
                        indicators,
                    };

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
                        self.id,
                        self.layout_id,
                    );

                    let tf = match derived_plan.basis {
                        Some(Basis::Time(t)) => Some(t),
                        None => Some(Timeframe::M5),
                        _ => None,
                    };
                    let chart_kind = match &content {
                        Content::Kline { kind, .. } => kind,
                        _ => unreachable!(),
                    };
                    let streams = Self::build_footprint_streams(
                        derived_plan.ticker_info,
                        chart_kind,
                        &self.settings,
                        tf,
                    );

                    (content, streams)
                }
                ContentKind::CandlestickChart => {
                    let content = Content::new_kline(
                        kind,
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        base_ticker.min_ticksize.into(),
                        self.id,
                        self.layout_id,
                    );

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
                    let content = Content::new_kline(
                        kind,
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        base_ticker.min_ticksize.into(),
                        self.id,
                        self.layout_id,
                    );

                    let tf = match derived_plan.basis {
                        Some(Basis::Time(t)) => t,
                        _ => Timeframe::M30,
                    };
                    let k_stream = kline_stream(derived_plan.ticker_info, tf);
                    let depth_aggr = derived_plan.ticker_info.exchange().stream_ticksize(
                        None,
                        self.settings.tick_multiply.unwrap_or(TickMultiplier(10)),
                    );
                    let temp = PaneSetup {
                        depth_aggr,
                        ..derived_plan
                    };
                    let d_stream = depth_stream(&temp);
                    let streams = vec![k_stream, d_stream];

                    (content, streams)
                }
                ContentKind::TimeAndSales => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.time_and_sales());

                    let mut streams = Vec::new();
                    let temp = PaneSetup {
                        push_freq: exchange::PushFrequency::ServerDefault,
                        ..derived_plan
                    };
                    streams.push(depth_stream(&temp));

                    if let Some(ref cfg) = config {
                        for add_ti in &cfg.additional_tickers {
                            let add_setup = PaneSetup {
                                ticker_info: *add_ti,
                                push_freq: exchange::PushFrequency::ServerDefault,
                                ..derived_plan
                            };
                            streams.push(depth_stream(&add_setup));
                        }
                    }

                    let content = Content::TimeAndSales(Some(TimeAndSales::new(
                        config,
                        derived_plan.ticker_info,
                    )));

                    (content, streams)
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
                        || {
                            log::warn!(
                                "ComparisonChart does not support tick basis; falling back to 1m"
                            );
                            tickers
                                .iter()
                                .copied()
                                .map(|ti| kline_stream(ti, Timeframe::M1))
                                .collect()
                        },
                    );

                    (content, streams)
                }
                ContentKind::Starter => unreachable!(),
            }
        };

        if let Some(base_ticker) = tickers.first()
            && self.footprint_notice_dismissed != Some(base_ticker.exchange())
        {
            self.footprint_notice_dismissed = None;
        }
        self.content = content;
        self.streams = ResolvedStream::Ready(streams.clone());
        self.sync_footprint_exchange_notice();

        streams
    }

    pub fn insert_hist_oi(&mut self, req_id: Option<uuid::Uuid>, oi: &[OpenInterest]) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    log::warn!("Kline chart was uninitialized when inserting open interest");
                    return;
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
                    log::warn!("Kline chart was uninitialized when inserting klines");
                    return;
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
                    let existing_drawings = chart.drawings.clone();
                    let symbol = chart.ticker_info.ticker.display_symbol_and_type().0;

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
                    chart.alerts = data::AlertStore::for_symbol_and_pane(&symbol, self.id);
                    chart.drawings = if !existing_drawings.is_empty() {
                        existing_drawings
                    } else if let Some(lid) = self.layout_id {
                        data::DrawingStore::for_symbol_layout_and_pane(&symbol, lid, self.id)
                    } else {
                        data::DrawingStore::for_symbol_and_pane(&symbol, self.id)
                    };
                }
            }
            Content::Comparison(chart) => {
                let Some(chart) = chart else {
                    log::warn!("Comparison chart was uninitialized when inserting klines");
                    return;
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

    pub fn matches_stream(&self, stream: &StreamKind) -> bool {
        self.streams.matches_stream(stream)
    }

    pub fn matches_trades(&self, stream: &StreamKind) -> bool {
        if self.streams.matches_trades(stream) {
            return true;
        }

        if let Content::TimeAndSales(Some(panel)) = &self.content
            && panel.matches_ticker(&stream.ticker_info())
        {
            return true;
        }

        if let Content::Kline { kind, chart, .. } = &self.content {
            let active_kind = chart.as_ref().map_or(kind, |c| &c.kind);
            if let data::chart::KlineChartKind::Footprint {
                aggregate: true,
                aggregate_exchanges,
                ..
            } = active_kind
            {
                let stream_ti = stream.ticker_info();
                if aggregate_exchanges.contains(&stream_ti.exchange())
                    && let Some(base_ti) = self.stream_pair()
                {
                    let base_sym = extract_base_symbol(&base_ti.ticker);
                    let stream_sym = extract_base_symbol(&stream_ti.ticker);
                    return base_sym == stream_sym;
                }
            }
        }

        false
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
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
        }
    }

    pub fn park_for_inactive_layout(&mut self) {
        #[cfg(feature = "shader-heatmap")]
        if let Content::ShaderHeatmap { chart, .. } = &mut self.content {
            *chart = None;
            self.status = Status::Ready;
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
                    chart.basis_interval().map(|ms| ms.clamp(100, 1000))
                } else {
                    None
                }
            }
            Content::Ladder(_) | Content::TimeAndSales(_) => Some(100),
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { .. } => None,
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

    pub fn apply_synced_settings(
        &mut self,
        studies: &Option<data::chart::Study>,
        clusters: &Option<data::chart::kline::ClusterKind>,
    ) {
        if let Some(studies) = studies {
            self.content.update_studies(studies.clone());
        }
        if let Some(cluster_kind) = clusters
            && let Content::Kline { chart, kind, .. } = &mut self.content
            && let Some(c) = chart
        {
            c.set_cluster_kind(*cluster_kind);
            *kind = c.kind.clone();
        }
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
            footprint_notice_dismissed: None,
            settings_category: modal::pane::settings::SettingsCategory::default(),
            indicator_search_query: String::new(),
            layout_id: None,
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
    #[cfg(feature = "shader-heatmap")]
    ShaderHeatmap {
        chart: Option<Box<HeatmapShader>>,
        indicators: Vec<HeatmapIndicator>,
        studies: Vec<HeatmapStudy>,
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

pub fn extract_base_symbol(ticker: &exchange::Ticker) -> String {
    let raw = ticker.to_full_symbol_and_type().0.to_uppercase();
    let sym = ticker.display_symbol_and_type().0.to_uppercase();

    if ticker.exchange == Exchange::HyperliquidLinear {
        return raw;
    }
    if let Some(prefix) = raw.strip_suffix("-USDT-SWAP") {
        return prefix.to_string();
    }
    if let Some(prefix) = raw.strip_suffix("-USDC-SWAP") {
        return prefix.to_string();
    }
    if let Some(prefix) = raw.strip_suffix("-USDT") {
        return prefix.to_string();
    }
    if let Some(prefix) = raw.strip_suffix("-USDC") {
        return prefix.to_string();
    }
    if let Some(prefix) = raw.strip_suffix("USDT") {
        return prefix.to_string();
    }
    if let Some(prefix) = raw.strip_suffix("USDC") {
        return prefix.to_string();
    }
    if let Some(prefix) = sym.strip_suffix("USDT") {
        return prefix.to_string();
    }
    if let Some(prefix) = sym.strip_suffix("USDC") {
        return prefix.to_string();
    }
    raw
}

pub fn ticker_info_for_exchange(
    base_asset: &str,
    ex: Exchange,
    reference: Option<&TickerInfo>,
) -> TickerInfo {
    match ex {
        Exchange::BinanceLinear => {
            let sym = format!("{base_asset}USDT");
            let ticker = exchange::Ticker::new(&sym, ex);
            let min_tick = reference.map_or(0.1, |r| r.min_ticksize.into());
            TickerInfo::new(ticker, min_tick, 0.001, None)
        }
        Exchange::BybitLinear => {
            let sym = format!("{base_asset}USDT");
            let ticker = exchange::Ticker::new(&sym, ex);
            let min_tick = reference.map_or(0.1, |r| r.min_ticksize.into());
            TickerInfo::new(ticker, min_tick, 0.001, None)
        }
        Exchange::OkexLinear => {
            let sym = format!("{base_asset}-USDT-SWAP");
            let ticker = exchange::Ticker::new(&sym, ex);
            let min_tick = reference.map_or(0.1, |r| r.min_ticksize.into());
            let contract_size = TickerInfo::default_contract_size(&ticker);
            TickerInfo::new(ticker, min_tick, 1.0, contract_size)
        }
        Exchange::HyperliquidLinear => {
            let ticker = exchange::Ticker::new(base_asset, ex);
            let min_tick = reference.map_or(1.0, |r| r.min_ticksize.into());
            TickerInfo::new(ticker, min_tick, 0.0001, None)
        }
        _ => {
            let ticker = exchange::Ticker::new(base_asset, ex);
            TickerInfo::new(ticker, 0.1, 0.001, None)
        }
    }
}

#[cfg(test)]
mod tests;
