//! User drawings and alerts: hit testing, snapping, position outcomes, rendering.
//!
//! Split out of `chart/kline.rs` (ROADMAP #131); the code is unchanged apart from
//! `pub(super)` visibility.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HitTarget {
    Handle(usize),
    Body,
}

pub(crate) fn snap_to_45_degrees(
    anchor: (u64, f32),
    frame_pos: Point,
    chart: &ViewState,
) -> (u64, f32) {
    let anchor_x = chart.interval_to_x(anchor.0);
    let anchor_y = chart.price_to_y(Price::from_f32(anchor.1));

    let dx = frame_pos.x - anchor_x;
    let dy = frame_pos.y - anchor_y;

    if dx.abs() < 1e-4 && dy.abs() < 1e-4 {
        return anchor;
    }

    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let k = (angle / step).round() as i32;

    let (snapped_x, snapped_y) = match k {
        0 => (frame_pos.x, anchor_y),
        1 => {
            let d = (dx + dy) * 0.5;
            (anchor_x + d, anchor_y + d)
        }
        2 => (anchor_x, frame_pos.y),
        3 => {
            let d = (-dx + dy) * 0.5;
            (anchor_x - d, anchor_y + d)
        }
        4 | -4 => (frame_pos.x, anchor_y),
        -3 => {
            let d = (-dx - dy) * 0.5;
            (anchor_x - d, anchor_y - d)
        }
        -2 => (anchor_x, frame_pos.y),
        -1 => {
            let d = (dx - dy) * 0.5;
            (anchor_x + d, anchor_y - d)
        }
        _ => (frame_pos.x, frame_pos.y),
    };

    let snapped_time = if k == 2 || k == -2 {
        anchor.0
    } else {
        chart.x_to_interval(snapped_x)
    };

    let snapped_price = if k == 0 || k == 4 || k == -4 {
        anchor.1
    } else {
        chart.y_to_price(snapped_y).to_f32()
    };

    (snapped_time, snapped_price)
}

pub(crate) fn snap_to_smart_magnet(
    data_source: &PlotData<KlineDataPoint>,
    chart: &ViewState,
    frame_pos: Point,
) -> (u64, f32) {
    let cursor_time = chart.x_to_interval(frame_pos.x);
    let cursor_price = chart.y_to_price(frame_pos.y).to_f32();

    match data_source {
        PlotData::TimeBased(ts) => {
            let prev = ts.datapoints.range(..=cursor_time).next_back();
            let next = ts.datapoints.range(cursor_time..).next();

            let best_candlestick = match (prev, next) {
                (Some((t1, dp1)), Some((t2, dp2))) => {
                    let x1 = chart.interval_to_x(*t1);
                    let x2 = chart.interval_to_x(*t2);
                    if (frame_pos.x - x1).abs() <= (frame_pos.x - x2).abs() {
                        Some((*t1, &dp1.kline))
                    } else {
                        Some((*t2, &dp2.kline))
                    }
                }
                (Some((t1, dp1)), None) => Some((*t1, &dp1.kline)),
                (None, Some((t2, dp2))) => Some((*t2, &dp2.kline)),
                (None, None) => None,
            };

            if let Some((candle_time, kline)) = best_candlestick {
                let candle_x = chart.interval_to_x(candle_time);
                let max_x_dist = (chart.cell_width * 3.0).max(40.0 / chart.scaling);
                if (frame_pos.x - candle_x).abs() <= max_x_dist {
                    let h = kline.high.to_f32();
                    let l = kline.low.to_f32();
                    let o = kline.open.to_f32();
                    let c = kline.close.to_f32();

                    let levels = [h, l, o, c];
                    let mut best_level = h;
                    let mut min_diff = (cursor_price - h).abs();
                    for &lvl in &levels[1..] {
                        let diff = (cursor_price - lvl).abs();
                        if diff < min_diff {
                            min_diff = diff;
                            best_level = lvl;
                        }
                    }
                    return (candle_time, best_level);
                }
            }

            (cursor_time, cursor_price)
        }
        PlotData::TickBased(tick_aggr) => {
            let n = tick_aggr.datapoints.len();
            if n == 0 {
                return (cursor_time, cursor_price);
            }

            let tick = cursor_time as usize;
            let clamped_idx = tick.min(n - 1);
            let candidates = [
                clamped_idx.saturating_sub(1),
                clamped_idx,
                (clamped_idx + 1).min(n - 1),
            ];

            let mut best_idx = clamped_idx;
            let mut min_x_dist = f32::MAX;
            for &idx in &candidates {
                let x = chart.interval_to_x(idx as u64);
                let dist = (frame_pos.x - x).abs();
                if dist < min_x_dist {
                    min_x_dist = dist;
                    best_idx = idx;
                }
            }

            let max_x_dist = (chart.cell_width * 3.0).max(40.0 / chart.scaling);
            if min_x_dist <= max_x_dist && best_idx < n {
                let kline = &tick_aggr.datapoints[n - 1 - best_idx].kline;
                let h = kline.high.to_f32();
                let l = kline.low.to_f32();
                let o = kline.open.to_f32();
                let c = kline.close.to_f32();

                let levels = [h, l, o, c];
                let mut best_level = h;
                let mut min_diff = (cursor_price - h).abs();
                for &lvl in &levels[1..] {
                    let diff = (cursor_price - lvl).abs();
                    if diff < min_diff {
                        min_diff = diff;
                        best_level = lvl;
                    }
                }
                return (best_idx as u64, best_level);
            }

            (cursor_time, cursor_price)
        }
    }
}

pub(super) fn dist_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    let proj = Point::new(a.x + t * dx, a.y + t * dy);
    (p.x - proj.x).hypot(p.y - proj.y)
}

pub(super) fn hit_test_drawing(
    drawing: &Drawing,
    frame_pos: Point,
    chart: &ViewState,
    threshold: f32,
) -> Option<HitTarget> {
    // 1. Handles check
    let handles: Vec<Point> = match &drawing.kind {
        DrawingKind::HorizontalLine { price } => {
            vec![Point::new(
                frame_pos.x,
                chart.price_to_y(Price::from_f32(*price)),
            )]
        }
        DrawingKind::Trendline { p1, p2 } => {
            vec![
                Point::new(
                    chart.interval_to_x(p1.0),
                    chart.price_to_y(Price::from_f32(p1.1)),
                ),
                Point::new(
                    chart.interval_to_x(p2.0),
                    chart.price_to_y(Price::from_f32(p2.1)),
                ),
            ]
        }
        DrawingKind::Rectangle { p1, p2 } => {
            let x1 = chart.interval_to_x(p1.0);
            let y1 = chart.price_to_y(Price::from_f32(p1.1));
            let x2 = chart.interval_to_x(p2.0);
            let y2 = chart.price_to_y(Price::from_f32(p2.1));
            let mid_x = (x1 + x2) / 2.0;
            let mid_y = (y1 + y2) / 2.0;
            vec![
                Point::new(x1, y1),
                Point::new(mid_x, y1),
                Point::new(x2, y1),
                Point::new(x2, mid_y),
                Point::new(x2, y2),
                Point::new(mid_x, y2),
                Point::new(x1, y2),
                Point::new(x1, mid_y),
            ]
        }
        DrawingKind::Brush { .. } => Vec::new(),
        DrawingKind::Path { points } => points
            .iter()
            .map(|pt| {
                Point::new(
                    chart.interval_to_x(pt.0),
                    chart.price_to_y(Price::from_f32(pt.1)),
                )
            })
            .collect(),
        DrawingKind::Position {
            entry,
            target_price,
            stop_price,
            end_time,
            ..
        } => {
            let x = chart.interval_to_x(entry.0);
            let default_interval = match chart.basis {
                Basis::Time(tf) => tf.to_milliseconds().max(60_000),
                Basis::Tick(_) => 1,
            };
            let end_time_val = end_time.unwrap_or(entry.0 + default_interval * 25);
            let mut end_x = chart.interval_to_x(end_time_val);
            let min_w = 20.0 / chart.scaling;
            if end_x < x + min_w {
                end_x = x + min_w;
            }
            let mid_x = (x + end_x) / 2.0;
            vec![
                Point::new(x, chart.price_to_y(Price::from_f32(entry.1))),
                Point::new(mid_x, chart.price_to_y(Price::from_f32(*target_price))),
                Point::new(mid_x, chart.price_to_y(Price::from_f32(*stop_price))),
                Point::new(end_x, chart.price_to_y(Price::from_f32(entry.1))),
            ]
        }
    };

    let handle_threshold = (12.0 / chart.scaling).max(threshold);
    for (idx, handle) in handles.iter().enumerate() {
        if (frame_pos.x - handle.x).hypot(frame_pos.y - handle.y) <= handle_threshold {
            return Some(HitTarget::Handle(idx));
        }
    }

    // 2. Body check
    match &drawing.kind {
        DrawingKind::HorizontalLine { price } => {
            let y = chart.price_to_y(Price::from_f32(*price));
            if (frame_pos.y - y).abs() <= threshold {
                return Some(HitTarget::Body);
            }
        }
        DrawingKind::Trendline { p1, p2 } => {
            let a = Point::new(
                chart.interval_to_x(p1.0),
                chart.price_to_y(Price::from_f32(p1.1)),
            );
            let b = Point::new(
                chart.interval_to_x(p2.0),
                chart.price_to_y(Price::from_f32(p2.1)),
            );
            if dist_to_segment(frame_pos, a, b) <= threshold {
                return Some(HitTarget::Body);
            }
        }
        DrawingKind::Rectangle { p1, p2 } => {
            let x1 = chart.interval_to_x(p1.0);
            let y1 = chart.price_to_y(Price::from_f32(p1.1));
            let x2 = chart.interval_to_x(p2.0);
            let y2 = chart.price_to_y(Price::from_f32(p2.1));
            let min_x = x1.min(x2);
            let max_x = x1.max(x2);
            let min_y = y1.min(y2);
            let max_y = y1.max(y2);
            if frame_pos.x >= min_x - threshold
                && frame_pos.x <= max_x + threshold
                && frame_pos.y >= min_y - threshold
                && frame_pos.y <= max_y + threshold
            {
                return Some(HitTarget::Body);
            }
        }
        DrawingKind::Brush { points } | DrawingKind::Path { points } => {
            if points.len() == 1 {
                let pt = Point::new(
                    chart.interval_to_x(points[0].0),
                    chart.price_to_y(Price::from_f32(points[0].1)),
                );
                if (frame_pos.x - pt.x).hypot(frame_pos.y - pt.y) <= threshold {
                    return Some(HitTarget::Body);
                }
            }
            for window in points.windows(2) {
                let a = Point::new(
                    chart.interval_to_x(window[0].0),
                    chart.price_to_y(Price::from_f32(window[0].1)),
                );
                let b = Point::new(
                    chart.interval_to_x(window[1].0),
                    chart.price_to_y(Price::from_f32(window[1].1)),
                );
                if dist_to_segment(frame_pos, a, b) <= threshold {
                    return Some(HitTarget::Body);
                }
            }
        }
        DrawingKind::Position {
            entry,
            target_price,
            stop_price,
            end_time,
            ..
        } => {
            let x = chart.interval_to_x(entry.0);
            let default_interval = match chart.basis {
                Basis::Time(tf) => tf.to_milliseconds().max(60_000),
                Basis::Tick(_) => 1,
            };
            let end_time_val = end_time.unwrap_or(entry.0 + default_interval * 25);
            let mut end_x = chart.interval_to_x(end_time_val);
            let min_w = 20.0 / chart.scaling;
            if end_x < x + min_w {
                end_x = x + min_w;
            }
            let min_x = x.min(end_x);
            let max_x = x.max(end_x);
            let y_entry = chart.price_to_y(Price::from_f32(entry.1));
            let y_tgt = chart.price_to_y(Price::from_f32(*target_price));
            let y_stp = chart.price_to_y(Price::from_f32(*stop_price));
            let min_y = y_tgt.min(y_stp).min(y_entry);
            let max_y = y_tgt.max(y_stp).max(y_entry);
            if frame_pos.x >= min_x - threshold
                && frame_pos.x <= max_x + threshold
                && frame_pos.y >= min_y - threshold
                && frame_pos.y <= max_y + threshold
            {
                return Some(HitTarget::Body);
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PositionOutcome {
    pub is_entered: bool,
    pub exit_time: u64,
    pub exit_price: f32,
    pub is_closed: bool,
    pub is_profit: bool,
}

pub(crate) fn evaluate_position_outcome(
    data_source: &PlotData<KlineDataPoint>,
    entry_time: u64,
    entry_price: f32,
    target_price: f32,
    stop_price: f32,
    is_long: bool,
    end_bound: u64,
    cutoff_time: Option<u64>,
) -> PositionOutcome {
    let mut last_seen_time = entry_time;
    let mut last_seen_price = entry_price;
    let mut has_candles = false;
    let mut is_entered = false;

    match data_source {
        PlotData::TimeBased(ts) => {
            if let Some(cutoff) = cutoff_time
                && entry_time > cutoff
            {
                return PositionOutcome {
                    is_entered: false,
                    exit_time: entry_time,
                    exit_price: entry_price,
                    is_closed: false,
                    is_profit: true,
                };
            }

            let eval_end = if let Some(cutoff) = cutoff_time {
                end_bound.min(cutoff)
            } else {
                end_bound
            };

            let start_candle_t = ts
                .datapoints
                .range(..=entry_time)
                .next_back()
                .map(|(&t, _)| t)
                .unwrap_or(entry_time);

            for (&time, dp) in ts.datapoints.range(start_candle_t..=eval_end) {
                has_candles = true;
                let high = dp.kline.high.to_f32();
                let low = dp.kline.low.to_f32();

                if !is_entered {
                    if low <= entry_price && high >= entry_price {
                        is_entered = true;
                    } else {
                        continue;
                    }
                }

                last_seen_time = time;
                last_seen_price = dp.kline.close.to_f32();

                if is_long {
                    let hit_tp = high >= target_price;
                    let hit_sl = low <= stop_price;
                    if hit_tp && hit_sl {
                        let open = dp.kline.open.to_f32();
                        if (open - target_price).abs() <= (open - stop_price).abs() {
                            return PositionOutcome {
                                is_entered: true,
                                exit_time: time,
                                exit_price: target_price,
                                is_closed: true,
                                is_profit: true,
                            };
                        } else {
                            return PositionOutcome {
                                is_entered: true,
                                exit_time: time,
                                exit_price: stop_price,
                                is_closed: true,
                                is_profit: false,
                            };
                        }
                    } else if hit_tp {
                        return PositionOutcome {
                            is_entered: true,
                            exit_time: time,
                            exit_price: target_price,
                            is_closed: true,
                            is_profit: true,
                        };
                    } else if hit_sl {
                        return PositionOutcome {
                            is_entered: true,
                            exit_time: time,
                            exit_price: stop_price,
                            is_closed: true,
                            is_profit: false,
                        };
                    }
                } else {
                    let hit_tp = low <= target_price;
                    let hit_sl = high >= stop_price;
                    if hit_tp && hit_sl {
                        let open = dp.kline.open.to_f32();
                        if (open - target_price).abs() <= (open - stop_price).abs() {
                            return PositionOutcome {
                                is_entered: true,
                                exit_time: time,
                                exit_price: target_price,
                                is_closed: true,
                                is_profit: true,
                            };
                        } else {
                            return PositionOutcome {
                                is_entered: true,
                                exit_time: time,
                                exit_price: stop_price,
                                is_closed: true,
                                is_profit: false,
                            };
                        }
                    } else if hit_tp {
                        return PositionOutcome {
                            is_entered: true,
                            exit_time: time,
                            exit_price: target_price,
                            is_closed: true,
                            is_profit: true,
                        };
                    } else if hit_sl {
                        return PositionOutcome {
                            is_entered: true,
                            exit_time: time,
                            exit_price: stop_price,
                            is_closed: true,
                            is_profit: false,
                        };
                    }
                }
            }

            if !is_entered {
                return PositionOutcome {
                    is_entered: false,
                    exit_time: entry_time,
                    exit_price: entry_price,
                    is_closed: false,
                    is_profit: true,
                };
            }
        }
        PlotData::TickBased(tick_aggr) => {
            let n = tick_aggr.datapoints.len();
            if n > 0 {
                let start_idx = (entry_time as usize).min(n - 1);
                let end_idx = (end_bound as usize).min(n - 1);
                if start_idx <= end_idx {
                    for idx in start_idx..=end_idx {
                        has_candles = true;
                        let kline = &tick_aggr.datapoints[n - 1 - idx].kline;
                        let high = kline.high.to_f32();
                        let low = kline.low.to_f32();

                        if !is_entered {
                            if low <= entry_price && high >= entry_price {
                                is_entered = true;
                            } else {
                                continue;
                            }
                        }

                        last_seen_time = idx as u64;
                        last_seen_price = kline.close.to_f32();

                        if is_long {
                            let hit_tp = high >= target_price;
                            let hit_sl = low <= stop_price;
                            if hit_tp && hit_sl {
                                let open = kline.open.to_f32();
                                if (open - target_price).abs() <= (open - stop_price).abs() {
                                    return PositionOutcome {
                                        is_entered: true,
                                        exit_time: idx as u64,
                                        exit_price: target_price,
                                        is_closed: true,
                                        is_profit: true,
                                    };
                                } else {
                                    return PositionOutcome {
                                        is_entered: true,
                                        exit_time: idx as u64,
                                        exit_price: stop_price,
                                        is_closed: true,
                                        is_profit: false,
                                    };
                                }
                            } else if hit_tp {
                                return PositionOutcome {
                                    is_entered: true,
                                    exit_time: idx as u64,
                                    exit_price: target_price,
                                    is_closed: true,
                                    is_profit: true,
                                };
                            } else if hit_sl {
                                return PositionOutcome {
                                    is_entered: true,
                                    exit_time: idx as u64,
                                    exit_price: stop_price,
                                    is_closed: true,
                                    is_profit: false,
                                };
                            }
                        } else {
                            let hit_tp = low <= target_price;
                            let hit_sl = high >= stop_price;
                            if hit_tp && hit_sl {
                                let open = kline.open.to_f32();
                                if (open - target_price).abs() <= (open - stop_price).abs() {
                                    return PositionOutcome {
                                        is_entered: true,
                                        exit_time: idx as u64,
                                        exit_price: target_price,
                                        is_closed: true,
                                        is_profit: true,
                                    };
                                } else {
                                    return PositionOutcome {
                                        is_entered: true,
                                        exit_time: idx as u64,
                                        exit_price: stop_price,
                                        is_closed: true,
                                        is_profit: false,
                                    };
                                }
                            } else if hit_tp {
                                return PositionOutcome {
                                    is_entered: true,
                                    exit_time: idx as u64,
                                    exit_price: target_price,
                                    is_closed: true,
                                    is_profit: true,
                                };
                            } else if hit_sl {
                                return PositionOutcome {
                                    is_entered: true,
                                    exit_time: idx as u64,
                                    exit_price: stop_price,
                                    is_closed: true,
                                    is_profit: false,
                                };
                            }
                        }
                    }
                }
            }

            if !is_entered {
                return PositionOutcome {
                    is_entered: false,
                    exit_time: entry_time,
                    exit_price: entry_price,
                    is_closed: false,
                    is_profit: true,
                };
            }
        }
    }

    let is_closed = match data_source {
        PlotData::TimeBased(ts) => {
            let latest_t = if let Some(cutoff) = cutoff_time {
                cutoff
            } else {
                ts.latest_timestamp().unwrap_or(0)
            };
            latest_t > 0 && end_bound <= latest_t && has_candles
        }
        PlotData::TickBased(tick_aggr) => {
            let n = tick_aggr.datapoints.len();
            let latest_idx = if n > 0 { (n - 1) as u64 } else { 0 };
            n > 0 && end_bound <= latest_idx && has_candles
        }
    };

    let is_profit = if is_long {
        last_seen_price >= entry_price
    } else {
        last_seen_price <= entry_price
    };

    PositionOutcome {
        is_entered: true,
        exit_time: last_seen_time.max(entry_time),
        exit_price: last_seen_price,
        is_closed,
        is_profit,
    }
}

/// Fill for the highlighted zone between a position's entry and its current/exit price:
/// the profit color when in profit, the stop color otherwise, with boosted (clamped) alpha.
pub(super) fn position_active_fill(
    is_profit: bool,
    profit_color: [f32; 4],
    stop_color: [f32; 4],
) -> Color {
    let [r, g, b, a] = if is_profit { profit_color } else { stop_color };
    Color::from_rgba(r, g, b, (a * 1.8).clamp(0.25, 0.65))
}

pub(super) fn draw_chart_drawings(
    frame: &mut canvas::Frame,
    drawings: &[Drawing],
    in_progress: &Option<Drawing>,
    drag_drawing: Option<&Drawing>,
    chart: &ViewState,
    region: &Rectangle,
    selected_id: Option<uuid::Uuid>,
    magnet_indicator: Option<Point>,
    data_source: &PlotData<KlineDataPoint>,
    cutoff_time: Option<u64>,
    palette: &Extended,
) {
    let handle_radius = 4.0 / chart.scaling;
    // Stroke width in iced canvas is always in physical screen pixels, do not divide by scaling
    let handle_stroke_w = 1.5;
    let sel_blue = iced::Color::from_rgb(0.16, 0.38, 1.0);
    let handle_bg = iced::Color::from_rgb8(25, 28, 36);

    let draw_round_handle = |frame: &mut canvas::Frame, pt: Point| {
        frame.fill(&Path::circle(pt, handle_radius), handle_bg);
        frame.stroke(
            &Path::circle(pt, handle_radius),
            Stroke {
                style: canvas::Style::Solid(sel_blue),
                width: handle_stroke_w,
                ..Default::default()
            },
        );
    };

    let draw_square_handle = |frame: &mut canvas::Frame, pt: Point| {
        let size = handle_radius * 1.8;
        let rect_pt = Point::new(pt.x - size / 2.0, pt.y - size / 2.0);
        let path = Path::rectangle(rect_pt, Size::new(size, size));
        frame.fill(&path, handle_bg);
        frame.stroke(
            &path,
            Stroke {
                style: canvas::Style::Solid(sel_blue),
                width: handle_stroke_w,
                ..Default::default()
            },
        );
    };

    let drawings_iter = drawings.iter().map(|d| {
        if let Some(drag_d) = drag_drawing
            && drag_d.id == d.id
        {
            return drag_d;
        }
        d
    });

    let current_tf_ms = match data_source {
        PlotData::TimeBased(ts) => Some(ts.interval.to_milliseconds()),
        PlotData::TickBased(_) => None,
    };

    for drawing in drawings_iter.chain(in_progress.iter()) {
        let is_selected = drawing.is_selected || selected_id == Some(drawing.id);
        if let Some(tf_ms) = current_tf_ms
            && !is_selected
            && !drawing.is_visible_for_timeframe(tf_ms)
        {
            continue;
        }

        let color = iced::Color::from_rgba(
            drawing.color[0],
            drawing.color[1],
            drawing.color[2],
            drawing.color[3],
        );

        let line_dash = match drawing.line_style {
            data::chart::drawing::LineStyle::Solid => canvas::stroke::LineDash::default(),
            data::chart::drawing::LineStyle::Dashed => canvas::stroke::LineDash {
                segments: &[8.0, 4.0],
                offset: 0,
            },
            data::chart::drawing::LineStyle::Dotted => canvas::stroke::LineDash {
                segments: &[2.0, 3.0],
                offset: 0,
            },
        };

        let stroke = Stroke {
            style: canvas::Style::Solid(color),
            width: drawing.width,
            line_dash,
            ..Default::default()
        };

        match &drawing.kind {
            DrawingKind::HorizontalLine { price } => {
                let y = chart.price_to_y(Price::from_f32(*price));
                frame.stroke(
                    &Path::line(
                        Point::new(region.x, y),
                        Point::new(region.x + region.width, y),
                    ),
                    stroke,
                );
                if is_selected {
                    draw_round_handle(frame, Point::new(region.x + region.width / 2.0, y));
                }
            }
            DrawingKind::Trendline { p1, p2 } => {
                let x1 = chart.interval_to_x(p1.0);
                let y1 = chart.price_to_y(Price::from_f32(p1.1));
                let x2 = chart.interval_to_x(p2.0);
                let y2 = chart.price_to_y(Price::from_f32(p2.1));

                let mut start_pt = Point::new(x1, y1);
                let mut end_pt = Point::new(x2, y2);
                if (x2 - x1).abs() > 0.001 {
                    let m = (y2 - y1) / (x2 - x1);
                    if drawing.extend_left {
                        let ext_x = region.x;
                        let ext_y = y1 + m * (ext_x - x1);
                        start_pt = Point::new(ext_x, ext_y);
                    }
                    if drawing.extend_right {
                        let ext_x = region.x + region.width;
                        let ext_y = y1 + m * (ext_x - x1);
                        end_pt = Point::new(ext_x, ext_y);
                    }
                }

                frame.stroke(&Path::line(start_pt, end_pt), stroke);

                if drawing.arrow_left {
                    let angle = (y1 - y2).atan2(x1 - x2);
                    let arrow_len = 10.0 / chart.scaling;
                    let arrow = Path::new(|builder| {
                        builder.move_to(Point::new(
                            x1 - arrow_len * (angle - 0.5).cos(),
                            y1 - arrow_len * (angle - 0.5).sin(),
                        ));
                        builder.line_to(Point::new(x1, y1));
                        builder.line_to(Point::new(
                            x1 - arrow_len * (angle + 0.5).cos(),
                            y1 - arrow_len * (angle + 0.5).sin(),
                        ));
                    });
                    frame.stroke(&arrow, stroke);
                }

                if drawing.arrow_right {
                    let angle = (y2 - y1).atan2(x2 - x1);
                    let arrow_len = 10.0 / chart.scaling;
                    let arrow = Path::new(|builder| {
                        builder.move_to(Point::new(
                            x2 - arrow_len * (angle - 0.5).cos(),
                            y2 - arrow_len * (angle - 0.5).sin(),
                        ));
                        builder.line_to(Point::new(x2, y2));
                        builder.line_to(Point::new(
                            x2 - arrow_len * (angle + 0.5).cos(),
                            y2 - arrow_len * (angle + 0.5).sin(),
                        ));
                    });
                    frame.stroke(&arrow, stroke);
                }

                if is_selected {
                    draw_round_handle(frame, Point::new(x1, y1));
                    draw_round_handle(frame, Point::new(x2, y2));
                }
            }
            DrawingKind::Rectangle { p1, p2 } => {
                let x1 = chart.interval_to_x(p1.0);
                let y1 = chart.price_to_y(Price::from_f32(p1.1));
                let x2 = chart.interval_to_x(p2.0);
                let y2 = chart.price_to_y(Price::from_f32(p2.1));

                let min_x = x1.min(x2);
                let min_y = y1.min(y2);
                let w = (x1 - x2).abs();
                let h = (y1 - y2).abs();

                let opacity = drawing.fill_opacity.unwrap_or(0.25).clamp(0.0, 1.0);
                let fill_color = iced::Color::from_rgba(
                    drawing.color[0],
                    drawing.color[1],
                    drawing.color[2],
                    drawing.color[3] * opacity,
                );
                frame.fill_rectangle(Point::new(min_x, min_y), Size::new(w, h), fill_color);
                frame.stroke(
                    &Path::rectangle(Point::new(min_x, min_y), Size::new(w, h)),
                    stroke,
                );
                if is_selected {
                    let mid_x = (x1 + x2) / 2.0;
                    let mid_y = (y1 + y2) / 2.0;
                    draw_round_handle(frame, Point::new(x1, y1));
                    draw_square_handle(frame, Point::new(mid_x, y1));
                    draw_round_handle(frame, Point::new(x2, y1));
                    draw_square_handle(frame, Point::new(x2, mid_y));
                    draw_round_handle(frame, Point::new(x2, y2));
                    draw_square_handle(frame, Point::new(mid_x, y2));
                    draw_round_handle(frame, Point::new(x1, y2));
                    draw_square_handle(frame, Point::new(x1, mid_y));
                }
            }
            DrawingKind::Brush { points } => {
                if points.len() >= 2 {
                    let brush_path = Path::new(|builder| {
                        let first = Point::new(
                            chart.interval_to_x(points[0].0),
                            chart.price_to_y(Price::from_f32(points[0].1)),
                        );
                        builder.move_to(first);
                        for pt in &points[1..] {
                            builder.line_to(Point::new(
                                chart.interval_to_x(pt.0),
                                chart.price_to_y(Price::from_f32(pt.1)),
                            ));
                        }
                    });
                    if is_selected {
                        let highlight_stroke = Stroke {
                            style: canvas::Style::Solid(iced::Color::from_rgba(
                                0.16, 0.38, 1.0, 0.4,
                            )),
                            width: drawing.width + 4.0,
                            ..Default::default()
                        };
                        frame.stroke(&brush_path, highlight_stroke);
                    }
                    frame.stroke(&brush_path, stroke);
                } else if let Some(first) = points.first() {
                    let pt = Point::new(
                        chart.interval_to_x(first.0),
                        chart.price_to_y(Price::from_f32(first.1)),
                    );
                    let r = (drawing.width / 2.0).max(2.0);
                    if is_selected {
                        frame.fill(
                            &Path::circle(pt, r + 2.0 / chart.scaling),
                            iced::Color::from_rgba(0.16, 0.38, 1.0, 0.4),
                        );
                    }
                    frame.fill(&Path::circle(pt, r), color);
                }
            }
            DrawingKind::Path { points } => {
                if points.len() >= 2 {
                    let path = Path::new(|builder| {
                        let first = Point::new(
                            chart.interval_to_x(points[0].0),
                            chart.price_to_y(Price::from_f32(points[0].1)),
                        );
                        builder.move_to(first);
                        for pt in &points[1..] {
                            builder.line_to(Point::new(
                                chart.interval_to_x(pt.0),
                                chart.price_to_y(Price::from_f32(pt.1)),
                            ));
                        }
                    });
                    frame.stroke(&path, stroke);

                    // Arrowhead on last segment
                    let p_end = Point::new(
                        chart.interval_to_x(points[points.len() - 1].0),
                        chart.price_to_y(Price::from_f32(points[points.len() - 1].1)),
                    );
                    let p_prev = Point::new(
                        chart.interval_to_x(points[points.len() - 2].0),
                        chart.price_to_y(Price::from_f32(points[points.len() - 2].1)),
                    );
                    let angle = (p_end.y - p_prev.y).atan2(p_end.x - p_prev.x);
                    let arrow_len = 10.0 / chart.scaling;
                    let arrow = Path::new(|builder| {
                        builder.move_to(Point::new(
                            p_end.x - arrow_len * (angle - 0.5).cos(),
                            p_end.y - arrow_len * (angle - 0.5).sin(),
                        ));
                        builder.line_to(p_end);
                        builder.line_to(Point::new(
                            p_end.x - arrow_len * (angle + 0.5).cos(),
                            p_end.y - arrow_len * (angle + 0.5).sin(),
                        ));
                    });
                    frame.stroke(&arrow, stroke);
                }
                if is_selected {
                    for pt in points {
                        draw_round_handle(
                            frame,
                            Point::new(
                                chart.interval_to_x(pt.0),
                                chart.price_to_y(Price::from_f32(pt.1)),
                            ),
                        );
                    }
                }
            }
            DrawingKind::Position {
                entry,
                target_price,
                stop_price,
                is_long,
                end_time,
                ..
            } => {
                let entry_x = chart.interval_to_x(entry.0);
                let entry_y = chart.price_to_y(Price::from_f32(entry.1));
                let target_y = chart.price_to_y(Price::from_f32(*target_price));
                let stop_y = chart.price_to_y(Price::from_f32(*stop_price));

                let default_interval = match data_source {
                    PlotData::TimeBased(ts) => ts.interval.to_milliseconds().max(60_000),
                    PlotData::TickBased(_) => 1,
                };
                let end_time_val = end_time.unwrap_or(entry.0 + default_interval * 25);
                let mut end_x = chart.interval_to_x(end_time_val);
                let min_w = 20.0 / chart.scaling;
                if end_x < entry_x + min_w {
                    end_x = entry_x + min_w;
                }
                let box_w = end_x - entry_x;

                let pos_style = drawing.position_style();
                let theme_p_col = [
                    palette.success.base.color.r,
                    palette.success.base.color.g,
                    palette.success.base.color.b,
                    0.10,
                ];
                let theme_s_col = [
                    palette.danger.base.color.r,
                    palette.danger.base.color.g,
                    palette.danger.base.color.b,
                    0.10,
                ];
                let p_col = pos_style.profit_color.unwrap_or(theme_p_col);
                let s_col = pos_style.stop_color.unwrap_or(theme_s_col);
                let e_col = pos_style.entry_color;

                let outcome = evaluate_position_outcome(
                    data_source,
                    entry.0,
                    entry.1,
                    *target_price,
                    *stop_price,
                    *is_long,
                    end_time_val,
                    cutoff_time,
                );

                // Profit zone
                let p_min_y = entry_y.min(target_y);
                let p_h = (entry_y - target_y).abs();
                let profit_fill = iced::Color::from_rgba(p_col[0], p_col[1], p_col[2], p_col[3]);
                let profit_stroke = Stroke {
                    style: canvas::Style::Solid(iced::Color::from_rgba(
                        p_col[0],
                        p_col[1],
                        p_col[2],
                        (p_col[3] * 2.0).clamp(0.6, 1.0),
                    )),
                    width: 1.0,
                    ..Default::default()
                };
                frame.fill_rectangle(
                    Point::new(entry_x, p_min_y),
                    Size::new(box_w, p_h),
                    profit_fill,
                );
                frame.stroke(
                    &Path::rectangle(Point::new(entry_x, p_min_y), Size::new(box_w, p_h)),
                    profit_stroke,
                );

                // Stop zone
                let s_min_y = entry_y.min(stop_y);
                let s_h = (entry_y - stop_y).abs();
                let stop_fill = iced::Color::from_rgba(s_col[0], s_col[1], s_col[2], s_col[3]);
                let stop_stroke = Stroke {
                    style: canvas::Style::Solid(iced::Color::from_rgba(
                        s_col[0],
                        s_col[1],
                        s_col[2],
                        (s_col[3] * 2.0).clamp(0.6, 1.0),
                    )),
                    width: 1.0,
                    ..Default::default()
                };
                frame.fill_rectangle(
                    Point::new(entry_x, s_min_y),
                    Size::new(box_w, s_h),
                    stop_fill,
                );
                frame.stroke(
                    &Path::rectangle(Point::new(entry_x, s_min_y), Size::new(box_w, s_h)),
                    stop_stroke,
                );

                // R:R text
                let risk = (entry.1 - stop_price).abs();
                let reward = (target_price - entry.1).abs();
                let rr = if risk > 0.000001 { reward / risk } else { 0.0 };
                let rr_text = format!("R:R {:.2}", rr);
                frame.fill_text(canvas::Text {
                    content: rr_text,
                    position: Point::new(entry_x + box_w / 2.0, entry_y),
                    color: iced::Color::WHITE,
                    size: iced::Pixels(11.0 / chart.scaling),
                    font: style::AZERET_MONO,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });

                // Dynamic active profit/loss highlight zone & tracking path (only once entered)
                if outcome.is_entered {
                    let exit_x = chart.interval_to_x(outcome.exit_time).clamp(entry_x, end_x);
                    let exit_y = chart.price_to_y(Price::from_f32(outcome.exit_price));

                    // Dynamic active profit/loss highlight zone matching position color
                    let active_w = (exit_x - entry_x).max(0.0);
                    let active_min_y = entry_y.min(exit_y);
                    let active_h = (entry_y - exit_y).abs();
                    if active_w > 0.0 && active_h > 0.5 {
                        let active_fill = position_active_fill(outcome.is_profit, p_col, s_col);
                        frame.fill_rectangle(
                            Point::new(entry_x, active_min_y),
                            Size::new(active_w, active_h),
                            active_fill,
                        );
                    }

                    // Tracking dashed line from entry to exit matching position color
                    if pos_style.show_price_path && active_w > 0.5 {
                        let line_color = if outcome.is_profit {
                            iced::Color::from_rgba(
                                p_col[0],
                                p_col[1],
                                p_col[2],
                                (p_col[3] * 3.0).clamp(0.75, 0.95),
                            )
                        } else {
                            iced::Color::from_rgba(
                                s_col[0],
                                s_col[1],
                                s_col[2],
                                (s_col[3] * 3.0).clamp(0.75, 0.95),
                            )
                        };
                        let dash_stroke = Stroke {
                            style: canvas::Style::Solid(line_color),
                            width: 1.5,
                            line_dash: canvas::LineDash {
                                segments: &[4.0, 3.0],
                                offset: 0,
                            },
                            ..Default::default()
                        };
                        frame.stroke(
                            &Path::line(Point::new(entry_x, entry_y), Point::new(exit_x, exit_y)),
                            dash_stroke,
                        );

                        if outcome.is_closed {
                            let dot_color = if outcome.is_profit {
                                iced::Color::from_rgb(p_col[0], p_col[1], p_col[2])
                            } else {
                                iced::Color::from_rgb(s_col[0], s_col[1], s_col[2])
                            };
                            frame.fill(
                                &Path::circle(Point::new(exit_x, exit_y), 3.0 / chart.scaling),
                                dot_color,
                            );
                        }
                    }
                }

                // Entry line is always drawn
                frame.stroke(
                    &Path::line(Point::new(entry_x, entry_y), Point::new(end_x, entry_y)),
                    Stroke {
                        style: canvas::Style::Solid(iced::Color::from_rgba(
                            e_col[0], e_col[1], e_col[2], e_col[3],
                        )),
                        width: drawing.width.max(1.0),
                        ..Default::default()
                    },
                );

                if is_selected {
                    draw_round_handle(frame, Point::new(entry_x, entry_y));
                    draw_round_handle(frame, Point::new(entry_x + box_w / 2.0, target_y));
                    draw_round_handle(frame, Point::new(entry_x + box_w / 2.0, stop_y));
                    draw_round_handle(frame, Point::new(end_x, entry_y));
                }
            }
        }
    }

    if let Some(pt) = magnet_indicator {
        let r = 5.0 / chart.scaling;
        let magnet_color = iced::Color::from_rgb(0.2, 0.65, 1.0);
        frame.stroke(
            &Path::circle(pt, r),
            Stroke {
                style: canvas::Style::Solid(magnet_color),
                width: 1.5,
                ..Default::default()
            },
        );
        frame.fill(&Path::circle(pt, 2.0 / chart.scaling), magnet_color);
    }
}

pub(super) fn draw_chart_alerts(
    frame: &mut canvas::Frame,
    alerts: &[PriceAlert],
    chart: &ViewState,
    region: &Rectangle,
    alert_drag: &Option<(uuid::Uuid, f32)>,
    palette: &Extended,
) {
    for alert in alerts {
        let is_being_dragged = matches!(alert_drag, Some((id, _)) if *id == alert.id);

        let price_val = if is_being_dragged {
            if let Some((_, drag_p)) = alert_drag {
                *drag_p
            } else {
                alert.target_price
            }
        } else {
            alert.target_price
        };

        let y = chart.price_to_y(Price::from_f32(price_val));
        if y >= region.y && y <= region.y + region.height {
            let alert_color = if is_being_dragged {
                iced::Color::from_rgb(0.3, 0.8, 1.0)
            } else if alert.status == data::chart::alert::AlertStatus::Active {
                iced::Color::from_rgb(0.95, 0.75, 0.2)
            } else {
                iced::Color::from_rgb(0.5, 0.5, 0.5)
            };

            // TradingView-style draggable badge on the right edge with '✕' close button
            let price_str = format!("{:.*}", chart.decimals, price_val);
            let btn_w = 18.0 / chart.scaling;
            let badge_h = 18.0 / chart.scaling;
            let price_w = (64.0f32 / chart.scaling)
                .max((price_str.len() as f32 * 8.0 + 16.0) / chart.scaling);
            let badge_w = price_w + btn_w;
            let badge_x = region.x + region.width - badge_w - 4.0 / chart.scaling;
            let badge_y = y - badge_h / 2.0;
            let btn_x = badge_x + price_w;

            let stroke = Stroke {
                style: canvas::Style::Solid(alert_color),
                width: if is_being_dragged { 1.5 } else { 1.0 },
                line_dash: canvas::LineDash {
                    segments: &[5.0, 4.0],
                    offset: 0,
                },
                ..Default::default()
            };
            // Alert line stops at badge boundary so it doesn't cross into or through the badge
            frame.stroke(
                &Path::line(
                    Point::new(region.x, y),
                    Point::new(badge_x.max(region.x), y),
                ),
                stroke,
            );

            // Price area background
            frame.fill_rectangle(
                Point::new(badge_x, badge_y),
                Size::new(price_w, badge_h),
                if is_being_dragged {
                    iced::Color::from_rgba(0.1, 0.35, 0.55, 0.95)
                } else if alert.status == data::chart::alert::AlertStatus::Active {
                    iced::Color::from_rgba(0.18, 0.15, 0.05, 0.92)
                } else {
                    iced::Color::from_rgba(0.15, 0.15, 0.15, 0.92)
                },
            );

            // Close button area background
            frame.fill_rectangle(
                Point::new(btn_x, badge_y),
                Size::new(btn_w, badge_h),
                if is_being_dragged {
                    iced::Color::from_rgba(0.15, 0.25, 0.35, 0.95)
                } else {
                    iced::Color::from_rgba(0.20, 0.12, 0.12, 0.95)
                },
            );

            // Badge outer outline
            frame.stroke(
                &Path::rectangle(Point::new(badge_x, badge_y), Size::new(badge_w, badge_h)),
                Stroke {
                    style: canvas::Style::Solid(alert_color),
                    width: if is_being_dragged { 1.5 } else { 1.0 },
                    ..Default::default()
                },
            );

            // Vertical divider between price and close button
            frame.stroke(
                &Path::line(
                    Point::new(btn_x, badge_y),
                    Point::new(btn_x, badge_y + badge_h),
                ),
                Stroke {
                    style: canvas::Style::Solid(alert_color.scale_alpha(0.6)),
                    width: 1.0,
                    ..Default::default()
                },
            );

            // Price text
            frame.fill_text(canvas::Text {
                content: price_str,
                position: Point::new(badge_x + price_w / 2.0, badge_y + badge_h / 2.0),
                color: palette.background.base.text,
                size: iced::Pixels(10.0 / chart.scaling),
                align_x: Alignment::Center.into(),
                align_y: Alignment::Center.into(),
                font: style::AZERET_MONO,
                ..canvas::Text::default()
            });

            // Cross '✕' icon inside close button
            let cx = btn_x + btn_w / 2.0;
            let cy = badge_y + badge_h / 2.0;
            let r = 3.5 / chart.scaling;
            let cross_color = if alert.status == data::chart::alert::AlertStatus::Active {
                iced::Color::from_rgb(0.9, 0.45, 0.45)
            } else {
                iced::Color::from_rgb(0.7, 0.7, 0.7)
            };
            let cross_stroke = Stroke {
                style: canvas::Style::Solid(cross_color),
                width: 1.2,
                ..Default::default()
            };
            frame.stroke(
                &Path::line(Point::new(cx - r, cy - r), Point::new(cx + r, cy + r)),
                cross_stroke,
            );
            frame.stroke(
                &Path::line(Point::new(cx - r, cy + r), Point::new(cx + r, cy - r)),
                cross_stroke,
            );
        }
    }
}
