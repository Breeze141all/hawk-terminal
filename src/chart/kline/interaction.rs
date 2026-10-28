//! Event handling for the kline canvas, split out of `canvas::Program::update`
//! (ROADMAP #131). Each handler returns `Break(action)` where `update` used to `return`.

use super::*;

impl KlineChart {
    /// Keyboard: drawing shortcuts, nudging, undo/redo and tool hotkeys.
    pub(super) fn handle_keyboard(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> ControlFlow<Option<canvas::Action<Message>>> {
        if let Event::Keyboard(kb_event) = event {
            let mut d_state = self.drawing_state.borrow_mut();
            match kb_event {
                iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    physical_key,
                    ..
                } => {
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift)
                    ) || modifiers.shift()
                    {
                        d_state.shift_pressed = true;
                    }
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(
                            iced::keyboard::key::Named::Control
                                | iced::keyboard::key::Named::Super
                                | iced::keyboard::key::Named::Meta
                        )
                    ) || modifiers.control()
                        || modifiers.command()
                    {
                        d_state.ctrl_pressed = true;
                    }

                    let is_g = matches!(key.as_ref(), iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("g"))
                        || matches!(
                            physical_key,
                            iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyG)
                        );
                    if (modifiers.shift() || d_state.shift_pressed) && is_g {
                        drop(d_state);
                        if let Some(autofill) = self.find_target_position() {
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::AutofillPosition(autofill))
                                    .and_capture(),
                            ));
                        }
                        return ControlFlow::Break(None);
                    }

                    // Escape: cancel in-progress drawing, active tool, drag, or drawing selection
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    ) {
                        let had_in_prog = d_state.in_progress.take().is_some();
                        let had_tool = d_state.active_tool != DrawingTool::None;
                        d_state.active_tool = DrawingTool::None;
                        d_state.start_point = None;

                        if d_state.drag.take().is_some() {
                            drop(d_state);
                            self.chart.cache.clear_crosshair();
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::CrosshairMoved).and_capture(),
                            ));
                        }
                        if d_state.selected_drawing.take().is_some() {
                            drop(d_state);
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::SelectDrawing(None)).and_capture(),
                            ));
                        }
                        if had_in_prog || had_tool {
                            drop(d_state);
                            return ControlFlow::Break(Some(canvas::Action::capture()));
                        }
                    }

                    // Ctrl+Z (Undo) and Ctrl+Y / Ctrl+Shift+Z (Redo)
                    let is_ctrl =
                        modifiers.control() || modifiers.command() || d_state.ctrl_pressed;
                    let is_z = matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Character(c)
                            if c.eq_ignore_ascii_case("z") || c == "я" || c == "Я"
                    ) || matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyZ)
                    );
                    let is_y = matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Character(c)
                            if c.eq_ignore_ascii_case("y") || c == "н" || c == "Н"
                    ) || matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyY)
                    );
                    let is_shift = modifiers.shift() || d_state.shift_pressed;

                    if is_ctrl && !modifiers.alt() {
                        let is_undo = is_z && !is_shift;
                        let is_redo = (is_z && is_shift) || is_y;

                        let is_hovered = cursor.position_in(bounds).is_some();
                        let has_active_state = d_state.in_progress.is_some()
                            || d_state.drag.is_some()
                            || d_state.selected_drawing.is_some()
                            || d_state.active_tool != DrawingTool::None;
                        let should_process =
                            is_hovered || has_active_state || cursor.position().is_none();

                        if is_undo && should_process {
                            // 1. Cancel active drag and restore pre-drag coordinates
                            if d_state.drag.take().is_some() {
                                drop(d_state);
                                self.chart.cache.clear_crosshair();
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }

                            // 2. In-progress multi-point drawing (e.g. Path vertex undo)
                            if let Some(mut in_prog) = d_state.in_progress.take() {
                                if let DrawingKind::Path { ref mut points } = in_prog.kind
                                    && points.len() > 2
                                {
                                    points.remove(points.len() - 2);
                                    d_state.in_progress = Some(in_prog);
                                    drop(d_state);
                                    return ControlFlow::Break(Some(
                                        canvas::Action::request_redraw().and_capture(),
                                    ));
                                }
                                d_state.in_progress = None;
                                d_state.start_point = None;
                                d_state.active_tool = DrawingTool::None;
                                drop(d_state);
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }

                            // 3. Active tool selected with no points placed yet
                            if d_state.active_tool != DrawingTool::None {
                                d_state.active_tool = DrawingTool::None;
                                d_state.start_point = None;
                                drop(d_state);
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }

                            // 4. Committed drawing history undo
                            if !self.undo_stack.is_empty() {
                                drop(d_state);
                                return ControlFlow::Break(Some(
                                    canvas::Action::publish(Message::UndoDrawing).and_capture(),
                                ));
                            }
                        } else if is_redo && should_process && !self.redo_stack.is_empty() {
                            drop(d_state);
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::RedoDrawing).and_capture(),
                            ));
                        }
                    }

                    // Delete / Backspace: delete currently selected drawing
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete)
                            | iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace)
                    ) && let Some(id) = d_state.selected_drawing.take()
                    {
                        drop(d_state);
                        return ControlFlow::Break(Some(
                            canvas::Action::publish(Message::DeleteDrawing(id)).and_capture(),
                        ));
                    }

                    // Arrow keys: micro-nudge selected drawing(s)
                    if !modifiers.control() && !modifiers.command() && !modifiers.alt() {
                        let has_selection = d_state.selected_drawing.is_some()
                            || self.drawings.iter().any(|d| d.is_selected && !d.is_locked);

                        if has_selection {
                            let is_shift = modifiers.shift() || d_state.shift_pressed;
                            let mult = if is_shift { 10.0 } else { 1.0 };
                            let tick_size = f32::from(self.ticker_info.min_ticksize).max(0.0000001);
                            let timeframe_ms = match &self.data_source {
                                PlotData::TimeBased(ts) => ts.interval.to_milliseconds().max(1),
                                PlotData::TickBased(_) => 1,
                            };

                            let nudge = match key.as_ref() {
                                iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                                    Some((0i64, tick_size * mult))
                                }
                                iced::keyboard::Key::Named(
                                    iced::keyboard::key::Named::ArrowDown,
                                ) => Some((0i64, -tick_size * mult)),
                                iced::keyboard::Key::Named(
                                    iced::keyboard::key::Named::ArrowLeft,
                                ) => Some((
                                    -((timeframe_ms as f64 * mult as f64).round() as i64),
                                    0.0,
                                )),
                                iced::keyboard::Key::Named(
                                    iced::keyboard::key::Named::ArrowRight,
                                ) => Some((
                                    ((timeframe_ms as f64 * mult as f64).round() as i64),
                                    0.0,
                                )),
                                _ => None,
                            };

                            if let Some((time_delta, price_delta)) = nudge {
                                drop(d_state);
                                return ControlFlow::Break(Some(
                                    canvas::Action::publish(Message::NudgeSelectedDrawings(
                                        time_delta,
                                        price_delta,
                                    ))
                                    .and_capture(),
                                ));
                            }
                        }
                    }

                    // Tool shortcuts without modifiers: T (Trendline), H (HorizontalLine), B (Brush), R (Rectangle)
                    if !modifiers.control()
                        && !modifiers.command()
                        && !modifiers.alt()
                        && !modifiers.shift()
                        && !d_state.shift_pressed
                        && !d_state.ctrl_pressed
                    {
                        let tool_to_set = match key.as_ref() {
                            iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("t") => {
                                Some(if d_state.active_tool == DrawingTool::Trendline {
                                    DrawingTool::None
                                } else {
                                    DrawingTool::Trendline
                                })
                            }
                            iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("h") => {
                                Some(if d_state.active_tool == DrawingTool::HorizontalLine {
                                    DrawingTool::None
                                } else {
                                    DrawingTool::HorizontalLine
                                })
                            }
                            iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("b") => {
                                Some(if d_state.active_tool == DrawingTool::Brush {
                                    DrawingTool::None
                                } else {
                                    DrawingTool::Brush
                                })
                            }
                            iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("r") => {
                                Some(if d_state.active_tool == DrawingTool::Rectangle {
                                    DrawingTool::None
                                } else {
                                    DrawingTool::Rectangle
                                })
                            }
                            _ => None,
                        };

                        if let Some(tool) = tool_to_set {
                            d_state.active_tool = tool;
                            d_state.in_progress = None;
                            d_state.start_point = None;
                            drop(d_state);
                            return ControlFlow::Break(Some(canvas::Action::capture()));
                        }
                    }
                }
                iced::keyboard::Event::KeyReleased { key, modifiers, .. } => {
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift)
                    ) {
                        d_state.shift_pressed = false;
                    } else {
                        d_state.shift_pressed = modifiers.shift();
                    }
                    if matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(
                            iced::keyboard::key::Named::Control
                                | iced::keyboard::key::Named::Super
                                | iced::keyboard::key::Named::Meta
                        )
                    ) {
                        d_state.ctrl_pressed = false;
                    } else {
                        d_state.ctrl_pressed = modifiers.control() || modifiers.command();
                    }
                }
                iced::keyboard::Event::ModifiersChanged(modifiers) => {
                    d_state.shift_pressed = modifiers.shift();
                    d_state.ctrl_pressed = modifiers.control() || modifiers.command();
                }
            }

            let is_modifier_event = match kb_event {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(
                            iced::keyboard::key::Named::Shift
                                | iced::keyboard::key::Named::Control
                                | iced::keyboard::key::Named::Super
                                | iced::keyboard::key::Named::Meta
                        )
                    ) || modifiers.shift()
                        || modifiers.control()
                        || modifiers.command()
                }
                iced::keyboard::Event::KeyReleased { key, .. } => matches!(
                    key.as_ref(),
                    iced::keyboard::Key::Named(
                        iced::keyboard::key::Named::Shift
                            | iced::keyboard::key::Named::Control
                            | iced::keyboard::key::Named::Super
                            | iced::keyboard::key::Named::Meta
                    )
                ),
                iced::keyboard::Event::ModifiersChanged(_) => true,
            };

            if is_modifier_event
                && (d_state.active_tool != DrawingTool::None
                    || d_state.in_progress.is_some()
                    || d_state.drag.is_some())
            {
                if matches!(*interaction, Interaction::Ruler { .. }) {
                    *interaction = Interaction::None;
                }
                let chart = self.state();
                if let Some(frame_pos) = d_state.last_cursor_frame_pos {
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();
                    let shift = d_state.shift_pressed;
                    let is_magnet_active = self.config.magnet_mode ^ d_state.ctrl_pressed;
                    let snapped = if is_magnet_active {
                        self.snap_point_to_candle(frame_pos)
                    } else {
                        (time, price)
                    };

                    if let Some(ref mut in_prog) = d_state.in_progress {
                        match &mut in_prog.kind {
                            DrawingKind::Trendline { p1, p2 } => {
                                *p2 = if shift {
                                    snap_to_45_degrees(*p1, frame_pos, chart)
                                } else {
                                    snapped
                                };
                            }
                            DrawingKind::Rectangle { p2, .. } => {
                                *p2 = snapped;
                            }
                            DrawingKind::HorizontalLine { price: p } => {
                                *p = snapped.1;
                            }
                            DrawingKind::Path { points } if points.len() >= 2 => {
                                let anchor = points[points.len() - 2];
                                if let Some(last) = points.last_mut() {
                                    *last = if shift {
                                        snap_to_45_degrees(anchor, frame_pos, chart)
                                    } else {
                                        snapped
                                    };
                                }
                            }
                            DrawingKind::Position { stop_price, .. } => {
                                *stop_price = snapped.1;
                            }
                            _ => {}
                        }
                    }

                    if let Some(DrawingDrag::MovingHandle {
                        handle_idx,
                        initial_drawing,
                        current_drawing,
                        ..
                    }) = &mut d_state.drag
                    {
                        self.apply_handle_movement(
                            *handle_idx,
                            initial_drawing,
                            current_drawing,
                            frame_pos,
                            shift,
                            is_magnet_active,
                        );
                    }
                }
                return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
            }
        }

        ControlFlow::Continue(())
    }

    /// TPO session context menu: open, dismiss and run its actions.
    pub(super) fn handle_tpo_context_menu(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> ControlFlow<Option<canvas::Action<Message>>> {
        // 1. Dismiss context menu on Escape key
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
            && matches!(
                key.as_ref(),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            )
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
            return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
        }

        // 2. Dismiss context menu on wheel scroll or panning movement
        if let Event::Mouse(mouse::Event::WheelScrolled { .. }) = event
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
        }
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event
            && matches!(interaction, Interaction::Panning { .. })
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
        }

        // 3. Handle Right Click: open/toggle context menu on the hit session
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(pos) = cursor.position_in(bounds)
            && matches!(self.kind, KlineChartKind::Tpo { .. })
        {
            let chart = self.state();
            let frame_pos = Point::new(
                (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
            );
            let click_time = chart.x_to_interval(frame_pos.x) as i64;
            let mut cache = self.tpo_cache.borrow_mut();

            let matched = cache
                .session_ranges
                .iter()
                .find(|sr| {
                    let sx1 = chart.interval_to_x(sr.session_start as u64);
                    let sx2 = chart.interval_to_x(sr.session_end as u64);
                    let min_x = sx1.min(sx2);
                    let max_x = sx1.max(sx2);
                    (click_time >= sr.session_start && click_time < sr.session_end)
                        || (frame_pos.x >= min_x && frame_pos.x <= max_x)
                })
                .cloned();

            if let Some(session) = matched {
                let menu_w = 220.0;
                let item_h = 28.0;
                let pad_v = 4.0;
                let menu_h = pad_v * 2.0 + 4.0 * item_h;
                let menu_x = pos.x.min(bounds.width - menu_w - 4.0).max(4.0);
                let menu_y = pos.y.min(bounds.height - menu_h - 4.0).max(4.0);
                let menu_rect =
                    Rectangle::new(Point::new(menu_x, menu_y), Size::new(menu_w, menu_h));

                let mut items = Vec::new();
                let split_label = if session.is_split_brackets {
                    "Collapse".to_string()
                } else {
                    "Split Brackets".to_string()
                };
                items.push(TpoMenuItem {
                    label: split_label,
                    action: Some(TpoMenuAction::ToggleSplitBrackets(session.session_start)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Merge with Next Session".to_string(),
                    action: session
                        .next_start
                        .map(|ns| TpoMenuAction::MergeNext(session.session_start, ns)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Merge with Previous Session".to_string(),
                    action: session
                        .prev_start
                        .map(|ps| TpoMenuAction::MergePrev(ps, session.session_start)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h * 2.0),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Reset Cluster".to_string(),
                    action: if session.is_clustered {
                        Some(TpoMenuAction::ResetCluster(session.session_start))
                    } else {
                        None
                    },
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h * 3.0),
                        Size::new(menu_w, item_h),
                    ),
                });

                cache.context_menu = Some(TpoContextMenu {
                    rect: menu_rect,
                    items,
                });
                return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
            } else if cache.context_menu.is_some() {
                cache.context_menu = None;
                return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
            }
        }

        // 4. Handle Left Click: execute menu action or dismiss
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(pos) = cursor.position_in(bounds)
        {
            let mut cache = self.tpo_cache.borrow_mut();
            if let Some(menu) = cache.context_menu.take() {
                if menu.rect.contains(pos) {
                    for item in menu.items {
                        if item.rect.contains(pos) {
                            if let Some(action) = item.action {
                                let msg = match action {
                                    TpoMenuAction::ToggleSplitBrackets(s) => {
                                        Message::ToggleSplitBrackets(s)
                                    }
                                    TpoMenuAction::MergeNext(s1, s2) => {
                                        Message::MergeSessions(s1, s2)
                                    }
                                    TpoMenuAction::MergePrev(s1, s2) => {
                                        Message::MergeSessions(s1, s2)
                                    }
                                    TpoMenuAction::ResetCluster(s) => Message::SplitCluster(s),
                                };
                                return ControlFlow::Break(Some(
                                    canvas::Action::publish(msg).and_capture(),
                                ));
                            } else {
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }
                        }
                    }
                }
                return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
            }
        }

        ControlFlow::Continue(())
    }

    /// Placing new drawings with the active drawing tool.
    pub(super) fn handle_drawing_tools(
        &self,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> ControlFlow<Option<canvas::Action<Message>>> {
        // 5. Handle Drawing Tools interaction
        {
            let mut d_state = self.drawing_state.borrow_mut();
            if d_state.active_tool != DrawingTool::None || d_state.in_progress.is_some() {
                // Cancel on Escape or Right click
                if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                    && matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    )
                {
                    d_state.active_tool = DrawingTool::None;
                    d_state.in_progress = None;
                    d_state.start_point = None;
                    return ControlFlow::Break(Some(
                        canvas::Action::request_redraw().and_capture(),
                    ));
                }
                if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
                    d_state.active_tool = DrawingTool::None;
                    d_state.in_progress = None;
                    d_state.start_point = None;
                    return ControlFlow::Break(Some(
                        canvas::Action::request_redraw().and_capture(),
                    ));
                }

                if let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();
                    let shift = d_state.shift_pressed;
                    let is_magnet_active = self.config.magnet_mode ^ d_state.ctrl_pressed;
                    let snapped = if is_magnet_active {
                        self.snap_point_to_candle(frame_pos)
                    } else {
                        (time, price)
                    };

                    match event {
                        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                            if let Some(ref mut in_prog) = d_state.in_progress {
                                match &mut in_prog.kind {
                                    DrawingKind::Trendline { p1, p2 } => {
                                        *p2 = if shift {
                                            snap_to_45_degrees(*p1, frame_pos, chart)
                                        } else {
                                            snapped
                                        };
                                    }
                                    DrawingKind::Rectangle { p2, .. } => {
                                        *p2 = snapped;
                                    }
                                    DrawingKind::HorizontalLine { price: p } => {
                                        *p = snapped.1;
                                    }
                                    DrawingKind::Brush { points } => {
                                        points.push((time, price));
                                    }
                                    DrawingKind::Path { points } => {
                                        if points.len() >= 2 {
                                            let anchor = points[points.len() - 2];
                                            if let Some(last) = points.last_mut() {
                                                *last = if shift {
                                                    snap_to_45_degrees(anchor, frame_pos, chart)
                                                } else {
                                                    snapped
                                                };
                                            }
                                        } else if let Some(last) = points.last_mut() {
                                            *last = snapped;
                                        }
                                    }
                                    DrawingKind::Position { stop_price, .. } => {
                                        *stop_price = snapped.1;
                                    }
                                }
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                            match d_state.active_tool {
                                DrawingTool::HorizontalLine => {
                                    let drawing = Drawing::horizontal(
                                        snapped.1,
                                        [0.95, 0.65, 0.15, 1.0],
                                        1.5,
                                    );
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return ControlFlow::Break(Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    ));
                                }
                                DrawingTool::Trendline => {
                                    if let Some(start) = d_state.start_point.take() {
                                        let p2 = if d_state.shift_pressed {
                                            snap_to_45_degrees(start, frame_pos, chart)
                                        } else {
                                            snapped
                                        };
                                        let drawing = Drawing::trendline(
                                            start,
                                            p2,
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        );
                                        d_state.active_tool = DrawingTool::None;
                                        d_state.in_progress = None;
                                        return ControlFlow::Break(Some(
                                            canvas::Action::publish(Message::AddDrawing(drawing))
                                                .and_capture(),
                                        ));
                                    } else {
                                        d_state.start_point = Some(snapped);
                                        d_state.in_progress = Some(Drawing::trendline(
                                            snapped,
                                            snapped,
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        ));
                                        return ControlFlow::Break(Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        ));
                                    }
                                }
                                DrawingTool::Rectangle => {
                                    if let Some(start) = d_state.start_point.take() {
                                        let drawing = Drawing::rectangle(
                                            start,
                                            snapped,
                                            [0.3, 0.6, 0.95, 0.8],
                                            1.0,
                                        );
                                        d_state.active_tool = DrawingTool::None;
                                        d_state.in_progress = None;
                                        return ControlFlow::Break(Some(
                                            canvas::Action::publish(Message::AddDrawing(drawing))
                                                .and_capture(),
                                        ));
                                    } else {
                                        d_state.start_point = Some(snapped);
                                        d_state.in_progress = Some(Drawing::rectangle(
                                            snapped,
                                            snapped,
                                            [0.3, 0.6, 0.95, 0.8],
                                            1.0,
                                        ));
                                        return ControlFlow::Break(Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        ));
                                    }
                                }
                                DrawingTool::Brush => {
                                    d_state.start_point = Some((time, price));
                                    d_state.in_progress = Some(Drawing::brush(
                                        vec![(time, price)],
                                        [0.35, 0.7, 1.0, 1.0],
                                        2.0,
                                    ));
                                    return ControlFlow::Break(Some(
                                        canvas::Action::request_redraw().and_capture(),
                                    ));
                                }
                                DrawingTool::Path => {
                                    if let Some(Drawing {
                                        kind: DrawingKind::Path { points },
                                        ..
                                    }) = &mut d_state.in_progress
                                    {
                                        let pt = if shift && points.len() >= 2 {
                                            snap_to_45_degrees(
                                                points[points.len() - 2],
                                                frame_pos,
                                                chart,
                                            )
                                        } else {
                                            snapped
                                        };
                                        if let Some(last) = points.last_mut() {
                                            *last = pt;
                                        }
                                        points.push(pt);
                                        if points.len() >= 4
                                            && let Some(drawing) = d_state.in_progress.take()
                                        {
                                            d_state.active_tool = DrawingTool::None;
                                            return ControlFlow::Break(Some(
                                                canvas::Action::publish(Message::AddDrawing(
                                                    drawing,
                                                ))
                                                .and_capture(),
                                            ));
                                        }
                                        return ControlFlow::Break(Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        ));
                                    } else {
                                        d_state.in_progress = Some(Drawing::path(
                                            vec![snapped, snapped],
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        ));
                                        return ControlFlow::Break(Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        ));
                                    }
                                }
                                DrawingTool::ShortPosition => {
                                    let entry_price = snapped.1;
                                    let stop_price = entry_price * 1.015;
                                    let target_price = entry_price * 0.955;
                                    let interval_ms = match &self.data_source {
                                        PlotData::TimeBased(ts) => {
                                            ts.interval.to_milliseconds().max(60_000)
                                        }
                                        PlotData::TickBased(_) => 1,
                                    };
                                    let end_time = Some(snapped.0 + interval_ms * 25);
                                    let drawing = Drawing::position(
                                        snapped,
                                        stop_price,
                                        target_price,
                                        false,
                                        end_time,
                                        [0.9, 0.3, 0.3, 1.0],
                                        1.0,
                                    );
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return ControlFlow::Break(Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    ));
                                }
                                DrawingTool::LongPosition => {
                                    let entry_price = snapped.1;
                                    let stop_price = entry_price * 0.985;
                                    let target_price = entry_price * 1.045;
                                    let interval_ms = match &self.data_source {
                                        PlotData::TimeBased(ts) => {
                                            ts.interval.to_milliseconds().max(60_000)
                                        }
                                        PlotData::TickBased(_) => 1,
                                    };
                                    let end_time = Some(snapped.0 + interval_ms * 25);
                                    let drawing = Drawing::position(
                                        snapped,
                                        stop_price,
                                        target_price,
                                        true,
                                        end_time,
                                        [0.2, 0.8, 0.4, 1.0],
                                        1.0,
                                    );
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return ControlFlow::Break(Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    ));
                                }
                                DrawingTool::None => {}
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                            if let Some(drawing) = d_state.in_progress.take() {
                                if matches!(drawing.kind, DrawingKind::Brush { .. }) {
                                    d_state.active_tool = DrawingTool::None;
                                    return ControlFlow::Break(Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    ));
                                } else {
                                    d_state.in_progress = Some(drawing);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        ControlFlow::Continue(())
    }

    /// Selecting, moving and reshaping existing drawings.
    pub(super) fn handle_drawing_selection(
        &self,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> ControlFlow<Option<canvas::Action<Message>>> {
        // 5.5. Handle Drawing Selection, Moving & Modification (TradingView style)
        if self.drawing_state.borrow().active_tool == DrawingTool::None {
            // Dragging an existing drawing or handle
            let is_drawing_drag = self.drawing_state.borrow().drag.is_some();
            if is_drawing_drag {
                let chart = self.state();
                if let Some(pos) = cursor.position_in(bounds) {
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();

                    match event {
                        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                            let mut d_state = self.drawing_state.borrow_mut();
                            let shift = d_state.shift_pressed;
                            let ctrl = d_state.ctrl_pressed;
                            let is_magnet_active = self.config.magnet_mode ^ ctrl;
                            if let Some(DrawingDrag::MovingDrawing {
                                start_time,
                                start_price,
                                initial_drawing,
                                current_drawing,
                                ..
                            }) = &mut d_state.drag
                            {
                                let time_delta = time as i64 - *start_time as i64;
                                let price_delta = price - *start_price;
                                *current_drawing = initial_drawing.clone();
                                current_drawing.translate(time_delta, price_delta);
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            } else if let Some(DrawingDrag::MovingHandle {
                                handle_idx,
                                initial_drawing,
                                current_drawing,
                                ..
                            }) = &mut d_state.drag
                            {
                                self.apply_handle_movement(
                                    *handle_idx,
                                    initial_drawing,
                                    current_drawing,
                                    frame_pos,
                                    shift,
                                    is_magnet_active,
                                );
                                return ControlFlow::Break(Some(
                                    canvas::Action::request_redraw().and_capture(),
                                ));
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                            let mut d_state = self.drawing_state.borrow_mut();
                            if let Some(drag) = d_state.drag.take() {
                                let current_drawing = match drag {
                                    DrawingDrag::MovingDrawing {
                                        current_drawing, ..
                                    }
                                    | DrawingDrag::MovingHandle {
                                        current_drawing, ..
                                    } => current_drawing,
                                };
                                self.chart.cache.clear_crosshair();
                                return ControlFlow::Break(Some(
                                    canvas::Action::publish(Message::UpdateDrawing(
                                        current_drawing,
                                    ))
                                    .and_capture(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            } else if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                // Check if an alert is near; if so, let alert dragging take precedence
                let is_near_alert = if let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    self.alerts.iter().any(|a| {
                        let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                        let alert_screen_y =
                            (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                        (pos.y - alert_screen_y).abs() <= 10.0
                    })
                } else {
                    false
                };

                if !is_near_alert && let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();
                    let threshold = 8.0 / chart.scaling;

                    // Check handles of currently selected drawing
                    let mut handle_hit = None;
                    if let Some(sel_id) = self.drawing_state.borrow().selected_drawing
                        && let Some(sel_d) = self.drawings.iter().find(|d| d.id == sel_id)
                        && let Some(HitTarget::Handle(idx)) =
                            hit_test_drawing(sel_d, frame_pos, chart, threshold)
                    {
                        handle_hit = Some((sel_d.clone(), idx));
                    }

                    if let Some((sel_d, idx)) = handle_hit {
                        if !sel_d.is_locked {
                            self.drawing_state.borrow_mut().drag =
                                Some(DrawingDrag::MovingHandle {
                                    handle_idx: idx,
                                    initial_drawing: sel_d.clone(),
                                    current_drawing: sel_d.clone(),
                                });
                            if matches!(sel_d.kind, DrawingKind::Position { .. }) {
                                self.chart.cache.clear_crosshair();
                                return ControlFlow::Break(Some(
                                    canvas::Action::publish(Message::CrosshairMoved).and_capture(),
                                ));
                            }
                        }
                        return ControlFlow::Break(Some(
                            canvas::Action::request_redraw().and_capture(),
                        ));
                    }

                    // Check any drawing
                    let hit = self.drawings.iter().rev().find_map(|d| {
                        hit_test_drawing(d, frame_pos, chart, threshold)
                            .map(|target| (d.clone(), target))
                    });

                    if let Some((drawing, target)) = hit {
                        let mut d_state = self.drawing_state.borrow_mut();
                        d_state.selected_drawing = Some(drawing.id);
                        if !drawing.is_locked {
                            match target {
                                HitTarget::Handle(idx) => {
                                    d_state.drag = Some(DrawingDrag::MovingHandle {
                                        handle_idx: idx,
                                        initial_drawing: drawing.clone(),
                                        current_drawing: drawing.clone(),
                                    });
                                }
                                HitTarget::Body => {
                                    d_state.drag = Some(DrawingDrag::MovingDrawing {
                                        start_time: time,
                                        start_price: price,
                                        initial_drawing: drawing.clone(),
                                        current_drawing: drawing.clone(),
                                    });
                                }
                            }
                            if matches!(drawing.kind, DrawingKind::Position { .. }) {
                                self.chart.cache.clear_crosshair();
                            }
                        }
                        return ControlFlow::Break(Some(
                            canvas::Action::publish(Message::SelectDrawing(Some(drawing.id)))
                                .and_capture(),
                        ));
                    } else {
                        // Clicked empty space: deselect
                        let mut d_state = self.drawing_state.borrow_mut();
                        if d_state.selected_drawing.take().is_some() {
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::SelectDrawing(None)).and_capture(),
                            ));
                        }
                    }
                }
            }
        }

        ControlFlow::Continue(())
    }

    /// Dragging price alert lines.
    pub(super) fn handle_alert_drag(
        &self,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> ControlFlow<Option<canvas::Action<Message>>> {
        // 6. Handle Price Alert Dragging (TradingView style)
        if self.drawing_state.borrow().active_tool == DrawingTool::None {
            // Cancel alert drag on Escape
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                && matches!(
                    key.as_ref(),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                )
                && self.alert_drag_state.borrow_mut().take().is_some()
            {
                return ControlFlow::Break(Some(canvas::Action::request_redraw().and_capture()));
            }

            let is_dragging = self.alert_drag_state.borrow().is_some();
            if is_dragging {
                let chart = self.state();
                let calc_price = |cursor: mouse::Cursor| -> Option<f32> {
                    let pos = cursor.position_from(bounds.position())?;
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    Some(chart.y_to_price(frame_pos.y).to_f32())
                };

                match event {
                    Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if let Some(price) = calc_price(cursor)
                            && let Some(drag) = self.alert_drag_state.borrow_mut().as_mut()
                        {
                            drag.1 = price;
                            return ControlFlow::Break(Some(
                                canvas::Action::request_redraw().and_capture(),
                            ));
                        }
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        if let Some((id, drag_price)) = self.alert_drag_state.borrow_mut().take() {
                            let final_price = calc_price(cursor).unwrap_or(drag_price);
                            return ControlFlow::Break(Some(
                                canvas::Action::publish(Message::UpdateAlertPrice(id, final_price))
                                    .and_capture(),
                            ));
                        }
                    }
                    _ => {}
                }
            } else if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
                && let Some(pos) = cursor.position_in(bounds)
            {
                let chart = self.state();

                // 1. Check if user clicked the '✕' close button on any alert badge
                let close_hit_alert = self.alerts.iter().find(|a| {
                    let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                    let alert_screen_y =
                        (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                    if (pos.y - alert_screen_y).abs() > 10.0 {
                        return false;
                    }
                    pos.x >= bounds.width - 24.0 && pos.x <= bounds.width
                });

                if let Some(alert) = close_hit_alert {
                    let alert_id = alert.id;
                    return ControlFlow::Break(Some(
                        canvas::Action::publish(Message::DeletePriceAlert(alert_id)).and_capture(),
                    ));
                }

                // 2. Otherwise check for alert dragging
                let frame_pos = Point::new(
                    (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                    (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                );
                let current_cursor_price = chart.y_to_price(frame_pos.y).to_f32();

                let found_alert = self.alerts.iter().find(|a| {
                    let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                    let alert_screen_y =
                        (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                    if (pos.y - alert_screen_y).abs() > 8.0 {
                        return false;
                    }
                    if self.config.alert_drag_handle_only {
                        let price_str = format!("{:.*}", chart.decimals, a.target_price);
                        let price_w = 64.0f32.max(price_str.len() as f32 * 8.0 + 16.0);
                        let badge_w = price_w + 18.0;
                        let badge_screen_x = bounds.width - badge_w - 4.0;
                        pos.x >= badge_screen_x - 10.0 && pos.x < bounds.width - 22.0
                    } else {
                        pos.x < bounds.width - 22.0
                    }
                });

                if let Some(alert) = found_alert {
                    *self.alert_drag_state.borrow_mut() = Some((alert.id, current_cursor_price));
                    return ControlFlow::Break(Some(
                        canvas::Action::request_redraw().and_capture(),
                    ));
                }
            }
        }

        ControlFlow::Continue(())
    }
}
