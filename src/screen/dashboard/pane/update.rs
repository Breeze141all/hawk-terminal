//! `State::update`: the pane's event handler.
//!
//! Split out of `screen/dashboard/pane.rs` (code unchanged apart from visibility).

use super::*;

impl State {
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
                self.settings.visual_config = None;

                if !matches!(kind, ContentKind::Starter) {
                    self.streams = ResolvedStream::Waiting(vec![]);
                    let modal = Modal::MiniTickersList(MiniPanel::new());

                    if let Some(effect) = self.show_modal_with_focus(modal) {
                        return Some(effect);
                    }
                }
            }
            Event::ChartInteraction(msg) => return self.on_chart_interaction(msg),
            Event::PanelInteraction(msg) => match &mut self.content {
                Content::Ladder(Some(p)) => super::panel::update(p, msg),
                Content::TimeAndSales(Some(p)) => super::panel::update(p, msg),
                _ => {}
            },
            #[cfg(feature = "shader-heatmap")]
            Event::HeatmapShaderInteraction(message) => {
                if let Content::ShaderHeatmap { chart: Some(c), .. } = &mut self.content {
                    c.update(message);
                }
            }
            Event::ToggleIndicator(ind) => {
                self.content.toggle_indicator(ind);
            }
            Event::ToggleScriptedIndicator(ind) => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    let existing = c
                        .indicator_instances
                        .instances()
                        .iter()
                        .find(|i| i.kind == ind)
                        .map(|i| i.id);
                    if let Some(instance_id) = existing {
                        c.remove_scripted_indicator(instance_id);
                    } else {
                        c.add_scripted_indicator(ind);
                    }
                }
            }
            Event::IndicatorSearchChanged(query) => {
                self.indicator_search_query = query;
            }
            Event::ToggleIndicatorSettings(ind) => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.toggle_indicator_settings(ind);
                }
            }
            Event::DeleteNotification(idx) => {
                if idx < self.notifications.len() {
                    let removed = self.notifications.remove(idx);
                    if removed.title.contains("Footprint")
                        && let Some(ti) = self.stream_pair()
                    {
                        self.footprint_notice_dismissed = Some(ti.exchange());
                    }
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
                self.sync_footprint_exchange_notice();
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
            }
            Event::ClusterScalingSelected(scaling) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_scaling(scaling);
                    *kind = c.kind.clone();
                }
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
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
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
            }
            Event::FootprintShowBottomVolumeToggled(show) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_footprint_show_bottom_volume(show);
                    *kind = c.kind.clone();
                }
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
            }
            Event::FootprintAggregateToggled(agg) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content {
                    if let Some(c) = chart {
                        c.set_footprint_aggregate(agg);
                        *kind = c.kind.clone();
                    } else if let data::chart::KlineChartKind::Footprint { aggregate, .. } = kind {
                        *aggregate = agg;
                    }
                }
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
            }
            Event::FootprintAggregateExchangeToggled(ex, enabled) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content {
                    if let Some(c) = chart {
                        c.set_footprint_aggregate_exchange(ex, enabled);
                        *kind = c.kind.clone();
                    } else if let data::chart::KlineChartKind::Footprint {
                        aggregate_exchanges,
                        ..
                    } = kind
                    {
                        if enabled {
                            if !aggregate_exchanges.contains(&ex) {
                                aggregate_exchanges.push(ex);
                            }
                        } else {
                            aggregate_exchanges.retain(|&e| e != ex);
                        }
                    }
                }
                self.sync_footprint_streams();
                return Some(Effect::RefreshStreams);
            }
            Event::FootprintHistoryLookbackInput(input) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content {
                    if let Some(c) = chart {
                        c.set_history_lookback_input(input);
                        *kind = c.kind.clone();
                    } else if let data::chart::KlineChartKind::Footprint {
                        history_lookback_days,
                        ..
                    } = kind
                        && let Ok(days) = input.trim().parse::<u32>()
                        && days > 0
                    {
                        *history_lookback_days = Some(days);
                        exchange::fetcher::set_trade_history_lookback_days(days);
                    }
                }
            }
            Event::FootprintHistoryLookbackSet(days) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content {
                    if let Some(c) = chart {
                        c.set_history_lookback_days(days);
                        *kind = c.kind.clone();
                    } else if let data::chart::KlineChartKind::Footprint {
                        history_lookback_days,
                        ..
                    } = kind
                    {
                        *history_lookback_days = Some(days);
                        exchange::fetcher::set_trade_history_lookback_days(days);
                    }
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
                    #[cfg(feature = "shader-heatmap")]
                    if let Content::ShaderHeatmap { chart, studies, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                        *studies = c.studies.clone();
                    }
                }
            },
            Event::SelectSettingsCategory(cat) => {
                self.settings_category = cat;
            }
            Event::StreamModifierChanged(message) => {
                return self.on_stream_modifier_changed(message);
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
            Event::HeatmapTradeSizeInput(input) => match &mut self.content {
                Content::Heatmap { chart: Some(c), .. } => {
                    c.set_trade_size_input(input);
                }
                #[cfg(feature = "shader-heatmap")]
                Content::ShaderHeatmap { chart: Some(c), .. } => {
                    c.set_trade_size_input(input);
                }
                _ => {}
            },
            Event::HeatmapOrderSizeInput(input) => match &mut self.content {
                Content::Heatmap { chart: Some(c), .. } => {
                    c.set_order_size_input(input);
                }
                #[cfg(feature = "shader-heatmap")]
                Content::ShaderHeatmap { chart: Some(c), .. } => {
                    c.set_order_size_input(input);
                }
                _ => {}
            },
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
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    let cleared_drawings: Vec<data::chart::drawing::Drawing> = c
                        .drawings
                        .iter()
                        .filter(|d| !d.is_locked)
                        .cloned()
                        .collect();
                    let cleared_synced_ids: Vec<uuid::Uuid> = cleared_drawings
                        .iter()
                        .filter(|d| d.is_synced)
                        .map(|d| d.id)
                        .collect();
                    if !cleared_drawings.is_empty() {
                        c.push_undo(crate::chart::kline::DrawingHistoryAction::Clear(
                            cleared_drawings,
                        ));
                    }
                    c.clear_drawings();
                    data::DrawingStore::clear_for_symbol_and_pane(&symbol, self.id);
                    self.selected_drawing_show_settings = false;
                    if !cleared_synced_ids.is_empty() {
                        return Some(Effect::ClearSyncedDrawings {
                            symbol,
                            drawing_ids: cleared_synced_ids,
                        });
                    }
                }
            }
            Event::UndoDrawing => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    return Self::apply_undo_action(
                        self.id,
                        &mut self.selected_drawing_show_settings,
                        c,
                        &symbol,
                    );
                }
            }
            Event::RedoDrawing => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    return Self::apply_redo_action(
                        self.id,
                        &mut self.selected_drawing_show_settings,
                        c,
                        &symbol,
                    );
                }
            }
            Event::DrawingToolbarMoved(pos) => {
                self.drawing_toolbar_pos = Some(pos);
            }
            Event::SelectedDrawingToolbarMoved(pos) => {
                self.selected_drawing_toolbar_pos = Some(pos);
            }
            Event::SelectedDrawingAction(action) => return self.on_selected_drawing_action(action),
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
            Event::DrawingSettingsAction(action) => return self.on_drawing_settings_action(action),

            Event::ReplayDatePickerAction(action) => {
                return self.on_replay_date_picker_action(action);
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
            Event::ReplayAutoScrollToCutoff => {
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.auto_scroll_to_replay_cutoff();
                    if let Some(ref mut rep) = c.replay {
                        rep.auto_scroll = true;
                    }
                    c.invalidate_all();
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
                let mut alert = data::chart::alert::PriceAlert::with_details(
                    ticker,
                    symbol,
                    price,
                    initial_price,
                    self.alert_condition,
                );
                alert.pane_id = Some(self.id);
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
                return Some(Effect::DeleteAlert(id));
            }
            Event::TogglePriceAlert(id) => {
                data::AlertStore::toggle(id);
                if let Content::Kline { chart: Some(c), .. } = &mut self.content {
                    c.toggle_alert(id);
                }
                return Some(Effect::ToggleAlert(id));
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
                return Some(Effect::ClearTriggeredAlerts);
            }
        }
        None
    }

    /// Forwards a chart canvas message to the pane's chart.
    fn on_chart_interaction(&mut self, msg: crate::chart::Message) -> Option<Effect> {
        match &mut self.content {
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
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    let mut drawing = drawing.clone();
                    drawing.pane_id = Some(self.id);
                    drawing.is_synced = false;
                    c.add_drawing(drawing.clone());
                    c.push_undo(crate::chart::kline::DrawingHistoryAction::Add(
                        drawing.clone(),
                    ));
                    data::DrawingStore::add(&symbol, drawing.clone());
                }
                chart::Message::UpdateDrawing(drawing) => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    let old_drawing = c.drawings.iter().find(|d| d.id == drawing.id).cloned();
                    c.update_drawing(drawing.clone());
                    if let Some(old) = old_drawing
                        && old != *drawing
                    {
                        c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                            old,
                            new: drawing.clone(),
                        });
                    }
                    data::DrawingStore::update(&symbol, drawing.clone());
                    if drawing.is_synced {
                        return Some(Effect::SyncDrawing {
                            symbol,
                            drawing: drawing.clone(),
                            originating_pane_id: self.id,
                        });
                    }
                }
                chart::Message::DeleteDrawing(id) => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    let old_drawing = c.drawings.iter().find(|d| d.id == *id).cloned();
                    let was_synced = c
                        .drawings
                        .iter()
                        .find(|d| d.id == *id)
                        .is_some_and(|d| d.is_synced);
                    c.delete_drawing(*id);
                    if let Some(old) = old_drawing {
                        c.push_undo(crate::chart::kline::DrawingHistoryAction::Delete(old));
                    }
                    data::DrawingStore::remove(*id);
                    self.selected_drawing_show_settings = false;
                    if was_synced {
                        return Some(Effect::DeleteSyncedDrawing {
                            symbol,
                            drawing_id: *id,
                        });
                    }
                }
                chart::Message::ClearDrawings => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    let cleared_drawings: Vec<data::chart::drawing::Drawing> = c
                        .drawings
                        .iter()
                        .filter(|d| !d.is_locked)
                        .cloned()
                        .collect();
                    let cleared_synced_ids: Vec<uuid::Uuid> = cleared_drawings
                        .iter()
                        .filter(|d| d.is_synced)
                        .map(|d| d.id)
                        .collect();
                    if !cleared_drawings.is_empty() {
                        c.push_undo(crate::chart::kline::DrawingHistoryAction::Clear(
                            cleared_drawings,
                        ));
                    }
                    c.clear_drawings();
                    if let Some(lid) = self.layout_id {
                        data::DrawingStore::clear_for_symbol_layout_and_pane(&symbol, lid, self.id);
                    } else {
                        data::DrawingStore::clear_for_symbol_and_pane(&symbol, self.id);
                    }
                    self.selected_drawing_show_settings = false;
                    if !cleared_synced_ids.is_empty() {
                        return Some(Effect::ClearSyncedDrawings {
                            symbol,
                            drawing_ids: cleared_synced_ids,
                        });
                    }
                }
                chart::Message::UndoDrawing => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    return Self::apply_undo_action(
                        self.id,
                        &mut self.selected_drawing_show_settings,
                        c,
                        &symbol,
                    );
                }
                chart::Message::RedoDrawing => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    return Self::apply_redo_action(
                        self.id,
                        &mut self.selected_drawing_show_settings,
                        c,
                        &symbol,
                    );
                }
                chart::Message::NudgeSelectedDrawings(time_delta, price_delta) => {
                    let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                    return Self::apply_nudge_action(
                        self.id,
                        c,
                        &symbol,
                        *time_delta,
                        *price_delta,
                    );
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
                chart::Message::DeletePriceAlert(id) => {
                    data::AlertStore::remove(*id);
                    c.remove_alert(*id);
                    return Some(Effect::DeleteAlert(*id));
                }
                chart::Message::AutofillPosition(autofill) => {
                    return Some(Effect::AutofillJournal(autofill.clone()));
                }
                chart::Message::Translated(translation) => {
                    c.on_user_pan(*translation);
                    super::chart::update(c, &msg);
                }
                chart::Message::PanReleased => {
                    c.on_pan_released();
                    super::chart::update(c, &msg);
                }
                chart::Message::DoubleClick(chart::AxisScaleClicked::X) => {
                    if let Some(ref mut rep) = c.replay {
                        rep.auto_scroll = true;
                    }
                    super::chart::update(c, &msg);
                }
                chart::Message::AutoscaleToggled => {
                    if let Some(ref mut rep) = c.replay {
                        rep.auto_scroll = true;
                    }
                    super::chart::update(c, &msg);
                }
                _ => {
                    super::chart::update(c, &msg);
                }
            },
            _ => {}
        }
        None
    }

    /// Basis / tick size / ticker changes from the stream modifier modal.
    fn on_stream_modifier_changed(&mut self, message: modal::stream::Message) -> Option<Effect> {
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
                                    c.change_tick_size(tm.multiply_with_min_tick_size(ticker));
                                    c.reset_request_handler();
                                }
                                Content::Heatmap { chart: Some(c), .. } => {
                                    c.change_tick_size(tm.multiply_with_min_tick_size(ticker));
                                }
                                #[cfg(feature = "shader-heatmap")]
                                Content::ShaderHeatmap {
                                    chart: Some(c),
                                    indicators,
                                    studies,
                                    ..
                                } => {
                                    let saved_config = c.config;
                                    **c = HeatmapShader::new(
                                        c.basis,
                                        tm.multiply_with_min_tick_step(ticker),
                                        c.ticker_info,
                                        studies.clone(),
                                        indicators.clone(),
                                        Some(saved_config),
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
                        if let Content::Kline { chart, kind, .. } = &self.content {
                            let chart_kind = chart.as_ref().map_or(kind, |c| &c.kind);
                            if matches!(
                                chart_kind,
                                data::chart::KlineChartKind::Footprint { .. }
                                    | data::chart::KlineChartKind::Tpo { .. }
                            ) {
                                self.sync_footprint_streams();
                                effect = Some(Effect::RefreshStreams);
                            }
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
                                        it.find(|s| matches!(s, StreamKind::DepthAndTrades { .. }))
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
                                            *push_freq = exchange::PushFrequency::ServerDefault
                                        }
                                    }
                                }

                                effect = Some(Effect::RefreshStreams);
                            }
                            #[cfg(feature = "shader-heatmap")]
                            Content::ShaderHeatmap {
                                chart: Some(c),
                                indicators,
                                ..
                            } => {
                                let saved_config = c.config;
                                let saved_studies = c.studies.clone();
                                **c = HeatmapShader::new(
                                    new_basis,
                                    c.tick_size(),
                                    c.ticker_info,
                                    saved_studies,
                                    indicators.clone(),
                                    Some(saved_config),
                                );

                                if let Some(stream_type) =
                                    self.streams.ready_iter_mut().and_then(|mut it| {
                                        it.find(|s| matches!(s, StreamKind::DepthAndTrades { .. }))
                                    })
                                    && let StreamKind::DepthAndTrades {
                                        push_freq,
                                        ticker_info,
                                        ..
                                    } = stream_type
                                    && ticker_info.exchange().is_custom_push_freq()
                                {
                                    match new_basis {
                                        Basis::Time(tf)
                                            if ticker_info
                                                .exchange()
                                                .supports_heatmap_timeframe(tf) =>
                                        {
                                            *push_freq = exchange::PushFrequency::Custom(tf);
                                        }
                                        _ => {
                                            *push_freq = exchange::PushFrequency::ServerDefault;
                                        }
                                    }
                                }

                                effect = Some(Effect::RefreshStreams);
                            }
                            Content::Kline { chart, kind, .. } => {
                                if let Some(base_ticker) = base_ticker {
                                    let chart_kind = chart.as_ref().map_or(&*kind, |c| &c.kind);
                                    match new_basis {
                                        Basis::Time(tf) => {
                                            let streams = if matches!(
                                                chart_kind,
                                                data::chart::KlineChartKind::Footprint { .. }
                                            ) {
                                                Self::build_footprint_streams(
                                                    base_ticker,
                                                    chart_kind,
                                                    &self.settings,
                                                    Some(tf),
                                                )
                                            } else if matches!(
                                                chart_kind,
                                                data::chart::KlineChartKind::Tpo { .. }
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
                                                            .unwrap_or(TickMultiplier(10)),
                                                    )
                                                };
                                                vec![
                                                    StreamKind::Kline {
                                                        ticker_info: base_ticker,
                                                        timeframe: tf,
                                                    },
                                                    StreamKind::DepthAndTrades {
                                                        ticker_info: base_ticker,
                                                        depth_aggr,
                                                        push_freq:
                                                            exchange::PushFrequency::ServerDefault,
                                                    },
                                                ]
                                            } else {
                                                vec![StreamKind::Kline {
                                                    ticker_info: base_ticker,
                                                    timeframe: tf,
                                                }]
                                            };

                                            self.streams = ResolvedStream::Ready(streams);
                                            let action =
                                                chart.as_mut().and_then(|c| c.set_basis(new_basis));

                                            if let Some(chart::Action::RequestFetch(fetch)) = action
                                            {
                                                effect = Some(Effect::RequestFetch(fetch));
                                            } else {
                                                effect = Some(Effect::RefreshStreams);
                                            }
                                        }
                                        Basis::Tick(_) => {
                                            let streams = if matches!(
                                                chart_kind,
                                                data::chart::KlineChartKind::Footprint { .. }
                                            ) {
                                                Self::build_footprint_streams(
                                                    base_ticker,
                                                    chart_kind,
                                                    &self.settings,
                                                    None,
                                                )
                                            } else {
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
                                                vec![StreamKind::DepthAndTrades {
                                                    ticker_info: base_ticker,
                                                    depth_aggr,
                                                    push_freq:
                                                        exchange::PushFrequency::ServerDefault,
                                                }]
                                            };

                                            self.streams = ResolvedStream::Ready(streams);
                                            if let Some(c) = chart {
                                                c.set_basis(new_basis);
                                            }
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

                                    if let Some(chart::Action::RequestFetch(fetch)) = action {
                                        effect = Some(Effect::RequestFetch(fetch));
                                    } else {
                                        effect = Some(Effect::RefreshStreams);
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
        None
    }

    /// Actions from the toolbar of the selected drawing.
    fn on_selected_drawing_action(
        &mut self,
        action: widget::chart::drawing_selection_toolbar::SelectionToolbarAction,
    ) -> Option<Effect> {
        if let Content::Kline { chart: Some(c), .. } = &mut self.content {
            let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
            match action {
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ToggleSettings => {
                            self.selected_drawing_show_settings = !self.selected_drawing_show_settings;
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::OpenAdvancedSettings => {
                            if let Some(d) = c.selected_drawing() {
                                let st = modal::pane::drawing_settings::DrawingSettingsState::new(d);
                                self.modal = Some(Modal::DrawingSettings(Box::new(st)));
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ToggleLock => {
                            let old = c.selected_drawing().cloned();
                            c.toggle_selected_drawing_lock();
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ToggleSync => {
                            let old = c.selected_drawing().cloned();
                            c.toggle_selected_drawing_sync(self.id);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                } else {
                                    return Some(Effect::UnsyncDrawing {
                                        symbol,
                                        drawing_id: d.id,
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::Delete => {
                            if let Some(d) = c.selected_drawing().cloned() {
                                let id = d.id;
                                let was_synced = d.is_synced;
                                c.delete_drawing(id);
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Delete(d));
                                data::DrawingStore::remove(id);
                                self.selected_drawing_show_settings = false;
                                if was_synced {
                                    return Some(Effect::DeleteSyncedDrawing {
                                        symbol,
                                        drawing_id: id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetColor(color) => {
                            let old = c.selected_drawing().cloned();
                            c.update_selected_drawing_color(color);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetWidth(w) => {
                            let old = c.selected_drawing().cloned();
                            c.update_selected_drawing_width(w);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetPositionProfitColor(color) => {
                            let old = c.selected_drawing().cloned();
                            c.update_selected_position_profit_color(color);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetPositionStopColor(color) => {
                            let old = c.selected_drawing().cloned();
                            c.update_selected_position_stop_color(color);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::SetPositionEntryColor(color) => {
                            let old = c.selected_drawing().cloned();
                            c.update_selected_position_entry_color(color);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::TogglePositionPricePath => {
                            let old = c.selected_drawing().cloned();
                            c.toggle_selected_position_price_path();
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::AutofillJournal => {
                            if let Some(autofill) = c.find_target_position() {
                                return Some(Effect::AutofillJournal(autofill));
                            }
                        }
                        widget::chart::drawing_selection_toolbar::SelectionToolbarAction::ApplyTemplate(template) => {
                            let old = c.selected_drawing().cloned();
                            c.apply_template_to_selected(&template);
                            if let Some(new) = c.selected_drawing().cloned()
                                && let Some(old) = old
                                && old != new
                            {
                                c.push_undo(crate::chart::kline::DrawingHistoryAction::Update {
                                    old,
                                    new: new.clone(),
                                });
                            }
                            if let Some(d) = c.selected_drawing() {
                                data::DrawingStore::update(&symbol, d.clone());
                                if d.is_synced {
                                    return Some(Effect::SyncDrawing {
                                        symbol,
                                        drawing: d.clone(),
                                        originating_pane_id: self.id,
                                    });
                                }
                            }
                        }
                    }
        }
        None
    }

    /// Edits from the drawing settings modal.
    fn on_drawing_settings_action(
        &mut self,
        action: modal::pane::drawing_settings::DrawingSettingsAction,
    ) -> Option<Effect> {
        if let Some(Modal::DrawingSettings(ref mut st)) = self.modal {
            let mut needs_update = false;
            let mut close_after = false;
            match action {
                modal::pane::drawing_settings::DrawingSettingsAction::SelectTab(t) => {
                    st.active_tab = t;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::HexChanged(h) => {
                    st.hex_input = h;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ApplyHex => {
                    if let Some(color) = data::chart::drawing::hex_to_color(&st.hex_input) {
                        st.drawing.color = color;
                        data::chart::drawing::push_recent_color(color);
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::RgbChanged(r, g, b) => {
                    st.drawing.color[0] = r;
                    st.drawing.color[1] = g;
                    st.drawing.color[2] = b;
                    st.hex_input = data::chart::drawing::color_to_hex(st.drawing.color);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SelectRecentColor(c_arr) => {
                    st.drawing.color = c_arr;
                    st.hex_input = data::chart::drawing::color_to_hex(c_arr);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetLineStyle(ls) => {
                    st.drawing.line_style = ls;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetWidth(w) => {
                    st.drawing.width = w;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetOpacity(a) => {
                    st.drawing.color[3] = a.clamp(0.05, 1.0);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleExtendLeft => {
                    st.drawing.extend_left = !st.drawing.extend_left;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleExtendRight => {
                    st.drawing.extend_right = !st.drawing.extend_right;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleArrowLeft => {
                    st.drawing.arrow_left = !st.drawing.arrow_left;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleArrowRight => {
                    st.drawing.arrow_right = !st.drawing.arrow_right;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetFillOpacity(op) => {
                    st.drawing.fill_opacity = Some(op.clamp(0.0, 1.0));
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetPositionProfitColor(
                    color,
                ) => {
                    st.drawing.set_position_profit_color(color);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetPositionStopColor(
                    color,
                ) => {
                    st.drawing.set_position_stop_color(color);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetPositionEntryColor(
                    color,
                ) => {
                    st.drawing.set_position_entry_color(color);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::TogglePositionPricePath => {
                    let cur = st.drawing.position_style().show_price_path;
                    st.drawing.set_position_show_price_path(!cur);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::TextChanged(txt) => {
                    st.text_input = txt.clone();
                    let mut annot = st.drawing.annotation.clone().unwrap_or_default();
                    annot.text = txt;
                    st.drawing.annotation = Some(annot);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::TextHexChanged(th) => {
                    st.text_hex_input = th;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ApplyTextHex => {
                    if let Some(c_arr) = data::chart::drawing::hex_to_color(&st.text_hex_input) {
                        let mut annot = st.drawing.annotation.clone().unwrap_or_default();
                        annot.color = c_arr;
                        st.drawing.annotation = Some(annot);
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetFontSize(sz) => {
                    let mut annot = st.drawing.annotation.clone().unwrap_or_default();
                    annot.font_size = sz;
                    st.drawing.annotation = Some(annot);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetTextAlignH(ah) => {
                    let mut annot = st.drawing.annotation.clone().unwrap_or_default();
                    annot.align_h = ah;
                    st.drawing.annotation = Some(annot);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::SetTextAlignV(av) => {
                    let mut annot = st.drawing.annotation.clone().unwrap_or_default();
                    annot.align_v = av;
                    st.drawing.annotation = Some(annot);
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::CoordP1Changed(p1_s) => {
                    st.coord_p1 = p1_s.clone();
                    if let Ok(price) = p1_s.trim().parse::<f32>() {
                        match &mut st.drawing.kind {
                            data::chart::drawing::DrawingKind::HorizontalLine { price: p } => {
                                *p = price
                            }
                            data::chart::drawing::DrawingKind::Trendline { p1, .. } => p1.1 = price,
                            data::chart::drawing::DrawingKind::Rectangle { p1, .. } => p1.1 = price,
                            data::chart::drawing::DrawingKind::Position { entry, .. } => {
                                entry.1 = price
                            }
                            _ => {}
                        }
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::CoordP2Changed(p2_s) => {
                    st.coord_p2 = p2_s.clone();
                    if let Ok(price) = p2_s.trim().parse::<f32>() {
                        match &mut st.drawing.kind {
                            data::chart::drawing::DrawingKind::Trendline { p2, .. } => p2.1 = price,
                            data::chart::drawing::DrawingKind::Rectangle { p2, .. } => p2.1 = price,
                            data::chart::drawing::DrawingKind::Position { stop_price, .. } => {
                                *stop_price = price
                            }
                            _ => {}
                        }
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::CoordTargetPriceChanged(
                    tp_s,
                ) => {
                    st.coord_target_price = tp_s.clone();
                    if let Ok(price) = tp_s.trim().parse::<f32>()
                        && let data::chart::drawing::DrawingKind::Position {
                            ref mut target_price,
                            ..
                        } = st.drawing.kind
                    {
                        *target_price = price;
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::CoordT1Changed(t1_s) => {
                    st.coord_t1 = t1_s.clone();
                    if let Ok(time_ms) = t1_s.trim().parse::<u64>() {
                        match &mut st.drawing.kind {
                            data::chart::drawing::DrawingKind::Trendline { p1, .. } => {
                                p1.0 = time_ms
                            }
                            data::chart::drawing::DrawingKind::Rectangle { p1, .. } => {
                                p1.0 = time_ms
                            }
                            data::chart::drawing::DrawingKind::Position { entry, .. } => {
                                entry.0 = time_ms
                            }
                            _ => {}
                        }
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::CoordT2Changed(t2_s) => {
                    st.coord_t2 = t2_s.clone();
                    if let Ok(time_ms) = t2_s.trim().parse::<u64>() {
                        match &mut st.drawing.kind {
                            data::chart::drawing::DrawingKind::Trendline { p2, .. } => {
                                p2.0 = time_ms
                            }
                            data::chart::drawing::DrawingKind::Rectangle { p2, .. } => {
                                p2.0 = time_ms
                            }
                            data::chart::drawing::DrawingKind::Position { end_time, .. } => {
                                *end_time = Some(time_ms)
                            }
                            _ => {}
                        }
                        needs_update = true;
                    }
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleVisibilitySeconds => {
                    st.drawing.visibility.seconds = !st.drawing.visibility.seconds;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleVisibilityMinutes => {
                    st.drawing.visibility.minutes = !st.drawing.visibility.minutes;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleVisibilityHours => {
                    st.drawing.visibility.hours = !st.drawing.visibility.hours;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleVisibilityDays => {
                    st.drawing.visibility.days = !st.drawing.visibility.days;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::ToggleVisibilityWeeks => {
                    st.drawing.visibility.weeks = !st.drawing.visibility.weeks;
                    needs_update = true;
                }
                modal::pane::drawing_settings::DrawingSettingsAction::Close => {
                    close_after = true;
                }
            }

            if close_after {
                self.modal = None;
                return None;
            }

            if needs_update && let Content::Kline { chart: Some(c), .. } = &mut self.content {
                let symbol = c.ticker_info.ticker.display_symbol_and_type().0;
                let updated_drawing = st.drawing.clone();
                c.update_drawing(updated_drawing.clone());
                data::DrawingStore::update(&symbol, updated_drawing.clone());
                if updated_drawing.is_synced {
                    return Some(Effect::SyncDrawing {
                        symbol,
                        drawing: updated_drawing,
                        originating_pane_id: self.id,
                    });
                }
            }
        }
        None
    }

    /// Replay calendar: date selection and replay start.
    fn on_replay_date_picker_action(
        &mut self,
        action: modal::pane::replay_calendar::Action,
    ) -> Option<Effect> {
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
                    *state = modal::pane::replay_calendar::ReplayDatePickerState::new(rand_ts);
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
                    *state = modal::pane::replay_calendar::ReplayDatePickerState::new(target_ts);
                }
            }
            Action::Close => {
                self.modal = None;
            }
        }
        None
    }
}
