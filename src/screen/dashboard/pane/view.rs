//! Pane rendering: `State::view`, its controls bar, the modal stack and the small modal helpers.
//!
//! Split out of `screen/dashboard/pane.rs` (code unchanged apart from visibility).

use super::*;
// Named explicitly: the glob import of `column` would be ambiguous with `std::column!`.
use iced::widget::column;

impl State {
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
                    let settings_modal = || {
                        modal::pane::settings::timesales_cfg_view(
                            panel.config.clone(),
                            id,
                            trade_input,
                        )
                    };

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
                            self.settings_category,
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
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap {
                chart, indicators, ..
            } => {
                if let Some(chart) = chart {
                    let base = chart.view(timezone).map(move |message| {
                        Message::PaneEvent(id, Event::HeatmapShaderInteraction(message))
                    });

                    let ticker_info = self.stream_pair();
                    let exchange = ticker_info.as_ref().map(|info| info.ticker.exchange);

                    let basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Basis::default_heatmap_time(ticker_info));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(5));

                    let kind = ModifierKind::Heatmap(basis, tick_multiply);
                    let base_ticksize = ticker_info
                        .map(|ti| f32::from(ti.min_ticksize))
                        .unwrap_or_else(|| {
                            tick_multiply.base(chart.tick_size().to_f64_lossy() as f32)
                        });

                    let modifiers = row![
                        basis_modifier(id, basis, modifier, kind),
                        ticksize_modifier(
                            id,
                            base_ticksize,
                            tick_multiply,
                            modifier,
                            kind,
                            exchange,
                        ),
                    ]
                    .spacing(4);

                    stream_info_element = stream_info_element.push(modifiers);

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
                            self.settings_category,
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
                    let base = uninitialized_base(ContentKind::ShaderHeatmap);
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
                    let lookback_input = chart.history_lookback_input();
                    let settings_modal = || {
                        kline_cfg_view(
                            chart.study_configurator(),
                            chart.config(),
                            chart_kind,
                            id,
                            chart.basis(),
                            indicators,
                            chart.expanded_indicator,
                            self.settings_category,
                            lookback_input,
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

    pub(super) fn view_controls(
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

        if window::MULTI_WINDOW_SUPPORTED {
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

    pub(super) fn compose_stack_view<'a, F>(
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

        let mut view: Element<'a, Message> = base;

        if show_toolbar && let Content::Kline { chart: Some(c), .. } = &self.content {
            let active_tool = c.active_drawing_tool();
            let has_drawings = !c.drawings.is_empty();
            let magnet_mode = c.config().magnet_mode;
            let current_cfg = c.config();

            let can_undo = c.can_undo();

            let toolbar = widget::chart::drawing_toolbar::view(
                active_tool,
                has_drawings,
                magnet_mode,
                can_undo,
                move |action| match action {
                    widget::chart::drawing_toolbar::ToolbarAction::SelectTool(tool) => {
                        Message::PaneEvent(pane, Event::SelectDrawingTool(tool))
                    }
                    widget::chart::drawing_toolbar::ToolbarAction::ClearDrawings => {
                        Message::PaneEvent(pane, Event::ClearDrawings)
                    }
                    widget::chart::drawing_toolbar::ToolbarAction::Undo => {
                        Message::PaneEvent(pane, Event::UndoDrawing)
                    }
                    widget::chart::drawing_toolbar::ToolbarAction::ToggleMagnet => {
                        let mut new_cfg = current_cfg;
                        new_cfg.magnet_mode = !new_cfg.magnet_mode;
                        Message::VisualConfigChanged(
                            pane,
                            data::layout::pane::VisualConfig::Kline(new_cfg),
                            false,
                        )
                    }
                },
            );

            let toolbar_pos = self
                .drawing_toolbar_pos
                .unwrap_or(iced::Point::new(16.0, 72.0));
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
                .unwrap_or(iced::Point::new(260.0, 72.0));
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

            let replay_row = row![
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
                tooltip(
                    button(text(play_btn_text).size(10).font(style::AZERET_MONO))
                        .style(|theme, status| style::button::modifier(
                            theme,
                            status,
                            rep.is_playing
                        ))
                        .on_press(Message::PaneEvent(pane, Event::ReplayPlayPause))
                        .padding([3, 10]),
                    Some("Play / Pause (Space)"),
                    tooltip::Position::Top,
                ),
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
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            let mut replay_row = replay_row;
            if !rep.auto_scroll {
                replay_row = replay_row.push(tooltip(
                    button(
                        row![
                            text("TO CUTOFF").size(10).font(style::AZERET_MONO),
                            text(">|").size(10).font(style::AZERET_MONO),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    )
                    .style(|theme, status| style::button::modifier(theme, status, false))
                    .on_press(Message::PaneEvent(pane, Event::ReplayAutoScrollToCutoff))
                    .padding([2, 6]),
                    Some("Jump to cutoff bar and resume following"),
                    tooltip::Position::Top,
                ));
            }

            replay_row = replay_row.push(
                button(icon_text(Icon::Close, 11))
                    .style(|theme, status| style::button::transparent(theme, status, false))
                    .on_press(Message::PaneEvent(pane, Event::ToggleReplay))
                    .padding([3, 5]),
            );

            let replay_bar = container(replay_row)
                .padding([4, 8])
                .style(style::chart_modal);

            let dock = container(replay_bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::End)
                .padding(padding::bottom(10));

            view = iced::widget::stack![view, dock].into();
        }

        // Action modal dialogs are mounted LAST in the stack to guarantee
        // that modals render above floating toolbars and intercept input cleanly.
        match &self.modal {
            Some(Modal::LinkGroup) => {
                let content = link_group_modal(pane, self.link_group);

                stack_modal(
                    view,
                    content,
                    on_blur,
                    padding::right(12).left(4),
                    Alignment::Start,
                )
            }
            Some(Modal::StreamModifier(modifier)) => stack_modal(
                view,
                modifier.view(self.stream_pair_kind()).map(move |message| {
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
                    view,
                    content,
                    Message::PaneEvent(pane, Event::HideModal),
                    padding::left(12),
                    Alignment::Start,
                )
            }
            Some(Modal::DrawingSettings(settings_state)) => {
                let content =
                    modal::pane::drawing_settings::drawing_settings_view(pane, settings_state);
                stack_modal_positioned(
                    view,
                    content,
                    Message::PaneEvent(pane, Event::HideModal),
                    padding::top(70).left(260),
                    Alignment::Start,
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
                let this_chart_alerts =
                    data::AlertStore::for_symbol_and_pane(&ticker_symbol, self.id);

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
                    view,
                    content,
                    on_blur,
                    padding::right(12).left(12),
                    Alignment::End,
                )
            }
            Some(Modal::Settings) => stack_modal(
                view,
                settings_modal(),
                on_blur,
                padding::right(12).left(12),
                Alignment::End,
            ),
            Some(Modal::Indicators) => stack_modal(
                view,
                indicator_modal.unwrap_or_else(|| column![].into()),
                on_blur,
                padding::right(12).left(12),
                Alignment::End,
            ),
            Some(Modal::Controls) => stack_modal(
                view,
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
                view,
                modal::pane::replay_calendar::view(pane, picker_state),
                on_blur,
                padding::bottom(50),
                Alignment::Center,
                Alignment::End,
            ),
            None => view,
        }
    }
}

pub(super) fn link_group_modal<'a>(
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

pub(super) fn ticksize_modifier<'a>(
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

pub(super) fn basis_modifier<'a>(
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
