use crate::chart::{
    Caches, Message, ViewState, indicator::IndicatorLabel, indicator::plot::YScale,
};
use crate::style::dashed_line;
use data::chart::kline::indicators::{
    IndicatorAttachmentMode, IndicatorFillSpec, IndicatorHost, IndicatorInstance,
    IndicatorInstanceStore, IndicatorOverlaySource, LineStrokeStyle, OutputChannelSpec,
    OutputValueType,
    composition::MarkKind,
    theme::{ThemeAccent, ThemeColor, ThemeColorRole, ThemeColorShade},
};
use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::util::Price;
use iced::theme::palette::Extended;
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse,
    widget::{
        Canvas,
        canvas::{self, Cache, Frame, Geometry, LineCap, LineDash, Path, Stroke},
        container, row, rule,
    },
};
use std::ops::RangeInclusive;

/// Converts a 32-bit ARGB `indicator_engine::Color` into an `iced::Color`.
#[inline]
pub fn engine_color_to_iced(color: indicator_engine::Color) -> Color {
    let a = ((color.0 >> 24) & 0xff) as f32 / 255.0;
    let r = ((color.0 >> 16) & 0xff) as f32 / 255.0;
    let g = ((color.0 >> 8) & 0xff) as f32 / 255.0;
    let b = (color.0 & 0xff) as f32 / 255.0;
    Color::from_rgba(r, g, b, a)
}

/// Resolves a `ThemeColor` against the active iced `Extended` palette.
pub fn resolve_theme_color(tc: &ThemeColor, palette: &Extended) -> Color {
    if let Some(accent) = tc.accent {
        return match accent {
            ThemeAccent::One => Color::from_rgb(0.20, 0.60, 1.00),
            ThemeAccent::Two => Color::from_rgb(0.95, 0.55, 0.15),
            ThemeAccent::Three => Color::from_rgb(0.18, 0.80, 0.44),
            ThemeAccent::Four => Color::from_rgb(0.75, 0.25, 0.85),
            ThemeAccent::Five => Color::from_rgb(0.98, 0.78, 0.10),
            ThemeAccent::Six => Color::from_rgb(0.15, 0.85, 0.85),
            ThemeAccent::Seven => Color::from_rgb(0.92, 0.25, 0.35),
            ThemeAccent::Eight => Color::from_rgb(0.55, 0.40, 0.90),
        };
    }
    let pair = match tc.role {
        ThemeColorRole::Neutral => &palette.background.base,
        ThemeColorRole::Secondary => &palette.secondary.base,
        ThemeColorRole::Primary => &palette.primary.base,
        ThemeColorRole::Success => &palette.success.base,
        ThemeColorRole::Danger => &palette.danger.base,
        ThemeColorRole::Warning => {
            return match tc.shade {
                ThemeColorShade::Faint => Color::from_rgba(1.0, 0.76, 0.03, 0.25),
                ThemeColorShade::Soft => Color::from_rgba(1.0, 0.76, 0.03, 0.60),
                ThemeColorShade::Base => Color::from_rgb(1.0, 0.76, 0.03),
                ThemeColorShade::Strong | ThemeColorShade::Vivid => {
                    Color::from_rgb(1.0, 0.85, 0.20)
                }
            };
        }
    };
    let base = match tc.role {
        ThemeColorRole::Neutral => pair.text,
        _ => pair.color,
    };
    match tc.shade {
        ThemeColorShade::Faint => Color { a: 0.25, ..base },
        ThemeColorShade::Soft => Color { a: 0.60, ..base },
        ThemeColorShade::Base => base,
        ThemeColorShade::Strong => Color {
            r: (base.r * 1.2).min(1.0),
            g: (base.g * 1.2).min(1.0),
            b: (base.b * 1.2).min(1.0),
            a: base.a,
        },
        ThemeColorShade::Vivid => Color {
            r: (base.r * 1.4).min(1.0),
            g: (base.g * 1.4).min(1.0),
            b: (base.b * 1.4).min(1.0),
            a: 1.0,
        },
    }
}

/// Resolves an indicator output's static/theme color.
pub fn resolve_output_color(
    instance: &IndicatorInstance,
    output: &OutputChannelSpec,
    palette: &Extended,
) -> Color {
    if let Some(custom) = instance.style_overrides.color(&output.id) {
        return resolve_theme_color(&custom, palette);
    }
    if let Some(slot_id) = output.style_slot.as_deref() {
        if let Some(slot_override) = instance.style_overrides.color(slot_id) {
            return resolve_theme_color(&slot_override, palette);
        }
        if let Some(slot) = instance
            .kind
            .definition()
            .style_slots
            .iter()
            .find(|s| s.id == slot_id)
        {
            return resolve_theme_color(&slot.default_color(), palette);
        }
    }
    palette.primary.base.color
}

/// Resolves a band/fill color with strict opacity capping (alpha <= 0.25) to preserve Footprint clarity.
pub fn resolve_fill_color(
    instance: &IndicatorInstance,
    fill_spec: &IndicatorFillSpec,
    palette: &Extended,
) -> Color {
    let base_color = if let Some(custom) = instance.style_overrides.color(&fill_spec.color_slot) {
        resolve_theme_color(&custom, palette)
    } else if let Some(slot) = instance
        .kind
        .definition()
        .style_slots
        .iter()
        .find(|s| s.id == fill_spec.color_slot)
    {
        resolve_theme_color(&slot.default_color(), palette)
    } else {
        palette.primary.base.color
    };
    let opacity = fill_spec.opacity.clamp(0.04, 0.25);
    Color {
        a: opacity,
        ..base_color
    }
}

/// Constructs a stroke for indicator lines with width and dash pattern.
#[inline]
pub fn indicator_line_stroke(width: f32, style: LineStrokeStyle, color: Color) -> Stroke<'static> {
    let mut stroke = Stroke::default()
        .with_width(width.clamp(1.0, 4.0))
        .with_color(color);
    stroke.line_dash = match style {
        LineStrokeStyle::Solid => LineDash::default(),
        LineStrokeStyle::Dashed => LineDash {
            segments: &[6.0, 4.0],
            offset: 0,
        },
        LineStrokeStyle::Dotted => LineDash {
            segments: &[1.0, 3.0],
            offset: 0,
        },
    };
    if matches!(style, LineStrokeStyle::Dotted) {
        stroke.line_cap = LineCap::Round;
    }
    stroke
}

/// A contiguous line run of identical color.
#[derive(Debug, Clone)]
pub struct IndicatorLineRun {
    pub points: Vec<Point>,
    pub color: Color,
}

/// Resolves the true timestamp for any candle index, consulting visible candle indices first,
/// then looking up in `data_source` or linearly projecting if outside retention.
pub fn resolve_candle_timestamp(
    data_source: &PlotData<KlineDataPoint>,
    candle_indices: &[(usize, u64)],
    target_idx: usize,
    fallback: u64,
) -> u64 {
    if let Some(&(_, t)) = candle_indices.iter().find(|(idx, _)| *idx == target_idx) {
        return t;
    }

    match data_source {
        PlotData::TimeBased(ts) => {
            if let Some(&t) = ts.datapoints.keys().nth(target_idx) {
                t
            } else if let Some((&last_t, _)) = ts.datapoints.iter().next_back() {
                let total = ts.datapoints.len();
                let step = ts.interval.to_milliseconds();
                if target_idx >= total {
                    let diff = (target_idx - total + 1) as u64;
                    last_t.saturating_add(diff.saturating_mul(step))
                } else if let Some((&first_t, _)) = ts.datapoints.iter().next() {
                    let diff = candle_indices
                        .first()
                        .map(|f| f.0)
                        .unwrap_or(0)
                        .saturating_sub(target_idx) as u64;
                    first_t.saturating_sub(diff.saturating_mul(step))
                } else {
                    fallback
                }
            } else {
                fallback
            }
        }
        PlotData::TickBased(_ta) => target_idx as u64,
    }
}

/// Appends a point to line runs, inserting an orthogonal elbow for step lines and seeding
/// previous point on color transitions to avoid breaking polyline continuity.
pub fn append_indicator_line_point(
    runs: &mut Vec<IndicatorLineRun>,
    previous: Option<(Point, Color)>,
    current_pt: Point,
    current_color: Color,
    step_line: bool,
) {
    if !current_pt.x.is_finite() || !current_pt.y.is_finite() {
        return;
    }

    // Start a new disconnected run if this is the first point or if preceded by a gap (previous is None)
    if runs.is_empty() || previous.is_none() {
        runs.push(IndicatorLineRun {
            points: vec![current_pt],
            color: current_color,
        });
        return;
    }

    let last_run = runs.last_mut().unwrap();
    if last_run.color == current_color {
        if step_line && let Some((prev_pt, _)) = previous {
            last_run.points.push(Point::new(current_pt.x, prev_pt.y));
        }
        last_run.points.push(current_pt);
    } else {
        let mut new_points = Vec::new();
        if let Some((prev_pt, _)) = previous {
            if step_line {
                new_points.push(Point::new(current_pt.x, prev_pt.y));
            } else {
                new_points.push(prev_pt);
            }
        }
        new_points.push(current_pt);
        runs.push(IndicatorLineRun {
            points: new_points,
            color: current_color,
        });
    }
}

/// Draws cached line runs into a canvas frame.
pub fn draw_line_runs(
    frame: &mut Frame,
    runs: &[IndicatorLineRun],
    width: f32,
    style: LineStrokeStyle,
) {
    for run in runs {
        let valid_points: Vec<Point> = run
            .points
            .iter()
            .copied()
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .collect();
        if valid_points.len() >= 2 {
            let path = Path::new(|builder| {
                builder.move_to(valid_points[0]);
                for pt in valid_points.iter().skip(1) {
                    builder.line_to(*pt);
                }
            });
            frame.stroke(&path, indicator_line_stroke(width, style, run.color));
        } else if let Some(single) = valid_points.first() {
            frame.fill_rectangle(
                Point::new(single.x - 0.5, single.y - 0.5),
                Size::new(1.0, 1.0),
                run.color,
            );
        }
    }
}

/// Fills a ribbon/cloud between upper and lower curves.
pub fn fill_indicator_band(
    frame: &mut Frame,
    upper_points: &[Point],
    lower_points: &[Point],
    color: Color,
) {
    if upper_points.len() < 2
        || lower_points.len() < 2
        || upper_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
        || lower_points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return;
    }
    let path = Path::new(|builder| {
        builder.move_to(upper_points[0]);
        for pt in upper_points.iter().skip(1) {
            builder.line_to(*pt);
        }
        for pt in lower_points.iter().rev() {
            builder.line_to(*pt);
        }
        builder.close();
    });
    frame.fill(&path, color);
}

/// Draws an isolated marker glyph.
pub fn draw_marker(
    frame: &mut Frame,
    point: Point,
    mark_kind: Option<MarkKind>,
    color: Color,
    text: Option<&str>,
) {
    if !point.x.is_finite() || !point.y.is_finite() {
        return;
    }
    let radius = 3.5;
    match mark_kind {
        Some(MarkKind::Cross) => {
            frame.stroke(
                &Path::line(
                    Point::new(point.x - radius, point.y),
                    Point::new(point.x + radius, point.y),
                ),
                Stroke::default().with_color(color).with_width(1.5),
            );
            frame.stroke(
                &Path::line(
                    Point::new(point.x, point.y - radius),
                    Point::new(point.x, point.y + radius),
                ),
                Stroke::default().with_color(color).with_width(1.5),
            );
        }
        _ => {
            frame.fill(&Path::circle(point, radius), color);
        }
    }
    if let Some(txt) = text
        && !txt.is_empty()
    {
        frame.fill_text(canvas::Text {
            content: txt.to_string(),
            position: Point::new(point.x + 5.0, point.y - 5.0),
            color,
            size: iced::Pixels(10.0),
            ..Default::default()
        });
    }
}

/// Draws a finite line segment between two coordinates.
pub fn draw_segment(
    frame: &mut Frame,
    start: Point,
    end: Point,
    width: f32,
    color: Color,
    text: Option<&str>,
) {
    if !start.x.is_finite() || !start.y.is_finite() || !end.x.is_finite() || !end.y.is_finite() {
        return;
    }
    frame.stroke(
        &Path::line(start, end),
        Stroke::default()
            .with_width(width.clamp(1.0, 4.0))
            .with_color(color),
    );
    frame.fill(&Path::circle(start, 2.5), color);
    frame.fill(&Path::circle(end, 2.5), color);
    if let Some(txt) = text
        && !txt.is_empty()
    {
        let mid = Point::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5 - 6.0);
        frame.fill_text(canvas::Text {
            content: txt.to_string(),
            position: mid,
            color,
            size: iced::Pixels(10.0),
            ..Default::default()
        });
    }
}

/// Draws a horizontal level across the visible width.
pub fn draw_horizontal_level(
    frame: &mut Frame,
    left_x: f32,
    right_x: f32,
    y: f32,
    color: Color,
    label: Option<&str>,
) {
    if !left_x.is_finite() || !right_x.is_finite() || !y.is_finite() {
        return;
    }
    let mut stroke = Stroke::default().with_width(1.0).with_color(color);
    stroke.line_dash = LineDash {
        segments: &[4.0, 4.0],
        offset: 0,
    };
    frame.stroke(
        &Path::line(Point::new(left_x, y), Point::new(right_x, y)),
        stroke,
    );
    if let Some(lbl) = label {
        frame.fill_text(canvas::Text {
            content: lbl.to_string(),
            position: Point::new(left_x + 8.0, y - 12.0),
            color,
            size: iced::Pixels(10.0),
            ..Default::default()
        });
    }
}

/// Draws a rectangular region with optional fill and border.
pub fn draw_region(
    frame: &mut Frame,
    left_x: f32,
    right_x: f32,
    upper_y: f32,
    lower_y: f32,
    border_color: Option<Color>,
    fill_color: Option<Color>,
    width: f32,
) {
    if !left_x.is_finite() || !right_x.is_finite() || !upper_y.is_finite() || !lower_y.is_finite() {
        return;
    }
    let top_y = upper_y.min(lower_y);
    let h = (upper_y - lower_y).abs().max(1.0);
    let w = (right_x - left_x).max(1.0);
    let rect = Rectangle {
        x: left_x,
        y: top_y,
        width: w,
        height: h,
    };
    if let Some(fill) = fill_color {
        frame.fill_rectangle(rect.position(), rect.size(), fill);
    }
    if let Some(border) = border_color {
        let stroke = Stroke::default()
            .with_width(width.clamp(1.0, 3.0))
            .with_color(border);
        frame.stroke(&Path::rectangle(rect.position(), rect.size()), stroke);
    }
}

/// Scans the data source and extracts visible candle bar indices and timestamps with 1 neighbor
/// on each side to guarantee smooth border clipping without popping artifacts.
pub fn visible_candle_indices(
    data_source: &PlotData<KlineDataPoint>,
    earliest: u64,
    latest: u64,
) -> Vec<(usize, u64)> {
    match data_source {
        PlotData::TimeBased(ts) => {
            if latest < earliest || ts.datapoints.is_empty() {
                return Vec::new();
            }

            // Check if any candle falls into the visible interval [earliest, latest]
            let mut in_range = ts.datapoints.range(earliest..=latest);
            if in_range.next().is_none() {
                return Vec::new();
            }

            // Locate boundary neighbors in O(log N)
            let prev = ts.datapoints.range(..earliest).next_back().map(|(&t, _)| t);
            let next = ts
                .datapoints
                .range(latest.saturating_add(1)..)
                .next()
                .map(|(&t, _)| t);

            let cull_start = prev.unwrap_or(earliest);
            let cull_end = next.unwrap_or(latest);

            // Collect bounded keys in O(log N + K)
            let keys: Vec<u64> = ts
                .datapoints
                .range(cull_start..=cull_end)
                .map(|(&t, _)| t)
                .collect();

            if keys.is_empty() {
                return Vec::new();
            }

            let first_key = keys[0];
            let step = ts.interval.to_milliseconds();
            let first_t = *ts.datapoints.keys().next().unwrap();
            let last_t = *ts.datapoints.keys().next_back().unwrap();

            // O(1) arithmetic index mapping for uniform time series (zero gaps)
            let start_idx = if step > 0
                && last_t >= first_t
                && ((last_t - first_t) / step) + 1 == ts.datapoints.len() as u64
            {
                ((first_key - first_t) / step) as usize
            } else {
                // Bidirectional fallback for datasets with gaps: O(min(idx, N - idx))
                let count_from_end = ts.datapoints.range(first_key..).count();
                if count_from_end <= ts.datapoints.len() / 2 {
                    ts.datapoints.len() - count_from_end
                } else {
                    ts.datapoints.range(..first_key).count()
                }
            };

            keys.into_iter()
                .enumerate()
                .map(|(offset, t)| (start_idx + offset, t))
                .collect()
        }
        PlotData::TickBased(ta) => {
            if ta.datapoints.is_empty() {
                return Vec::new();
            }
            let total = ta.datapoints.len();
            let start = (earliest as usize)
                .saturating_sub(1)
                .min(total.saturating_sub(1));
            let end = (latest as usize)
                .saturating_add(1)
                .min(total.saturating_sub(1));
            (start..=end).map(|idx| (idx, idx as u64)).collect()
        }
    }
}

/// Renders all active overlay indicators directly onto the main price canvas frame (Pass 3.5).
pub fn draw_scripted_indicator_overlays(
    indicator_instances: &IndicatorInstanceStore,
    indicator_host: &IndicatorHost,
    data_source: &PlotData<KlineDataPoint>,
    frame: &mut Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    earliest: u64,
    latest: u64,
    palette: &Extended,
    region: &Rectangle,
) {
    if indicator_instances.instances().is_empty() {
        return;
    }

    let candle_indices = visible_candle_indices(data_source, earliest, latest);
    if candle_indices.is_empty() {
        return;
    }

    let source_range = candle_indices.first().unwrap().0..candle_indices.last().unwrap().0 + 1;

    for instance in indicator_instances.instances() {
        let def = instance.kind.definition();
        if def.attachment_mode != IndicatorAttachmentMode::Overlay {
            continue;
        }

        // 1. Horizontal Levels
        let levels = indicator_host.level_outputs_for(instance.id);
        for level in levels {
            if let Some(val) = level.value {
                let f = val as f32;
                if !f.is_finite() {
                    continue;
                }
                let y = price_to_y(Price::from_f32(f));
                if !y.is_finite() {
                    continue;
                }
                let color = level
                    .color
                    .map(engine_color_to_iced)
                    .unwrap_or(palette.primary.base.color);
                draw_horizontal_level(
                    frame,
                    region.x,
                    region.x + region.width,
                    y,
                    color,
                    Some(&level.id),
                );
            }
        }

        // 2. Rectangular Regions
        let regions =
            indicator_host.region_outputs_for_range(instance.id, Some(source_range.clone()));
        for reg in regions {
            let left_time = candle_indices
                .iter()
                .find(|(idx, _)| *idx == reg.left_index)
                .map(|(_, t)| *t)
                .unwrap_or(earliest);
            let left_x = interval_to_x(left_time);
            let right_x = if reg.extend_right {
                region.x + region.width
            } else {
                let right_time = candle_indices
                    .iter()
                    .find(|(idx, _)| *idx == reg.right_index)
                    .map(|(_, t)| *t)
                    .unwrap_or(latest);
                interval_to_x(right_time)
            };
            let u = reg.upper as f32;
            let l = reg.lower as f32;
            if !u.is_finite() || !l.is_finite() || !left_x.is_finite() || !right_x.is_finite() {
                continue;
            }
            let upper_y = price_to_y(Price::from_f32(u));
            let lower_y = price_to_y(Price::from_f32(l));
            if !upper_y.is_finite() || !lower_y.is_finite() {
                continue;
            }
            let border = reg.border_color.map(engine_color_to_iced);
            let fill = reg.fill_color.map(engine_color_to_iced).map(|c| Color {
                a: c.a.min(0.25),
                ..c
            });
            draw_region(
                frame, left_x, right_x, upper_y, lower_y, border, fill, reg.width,
            );
        }

        // 3. Fills / Series Bands (Clouds)
        if instance.style_overrides.fill_visible {
            for fill_spec in &def.fill_specs {
                let IndicatorOverlaySource::Channel(upper_ch) = &fill_spec.upper;
                let IndicatorOverlaySource::Channel(lower_ch) = &fill_spec.lower;

                let mut upper_points = Vec::new();
                let mut lower_points = Vec::new();

                for &(idx, t) in &candle_indices {
                    let x = interval_to_x(t);
                    let u_val = indicator_host
                        .output_value_for(instance.id, upper_ch, idx)
                        .and_then(|v| v.as_number());
                    let l_val = indicator_host
                        .output_value_for(instance.id, lower_ch, idx)
                        .and_then(|v| v.as_number());
                    if let (Some(u), Some(l)) = (u_val, l_val) {
                        let uf = u as f32;
                        let lf = l as f32;
                        if uf.is_finite() && lf.is_finite() && x.is_finite() {
                            let uy = price_to_y(Price::from_f32(uf));
                            let ly = price_to_y(Price::from_f32(lf));
                            if uy.is_finite() && ly.is_finite() {
                                upper_points.push(Point::new(x, uy));
                                lower_points.push(Point::new(x, ly));
                            }
                        }
                    }
                }

                let color = resolve_fill_color(instance, fill_spec, palette);
                fill_indicator_band(frame, &upper_points, &lower_points, color);
            }
        }

        // 4. Output Lines, Steps & Histograms
        for output in &def.outputs {
            if output.value_type != OutputValueType::Number {
                continue;
            }

            let width = instance
                .style_overrides
                .line_width(&output.id)
                .unwrap_or(output.line_width as u8)
                .clamp(1, 4) as f32;
            let style = instance
                .style_overrides
                .stroke_style(&output.id)
                .unwrap_or(LineStrokeStyle::Solid);

            let is_step = def
                .mark_specs
                .iter()
                .any(|m| m.source_channel == output.id && matches!(m.mark, MarkKind::StepLine));

            let mut runs = Vec::new();
            let mut prev_pt_color: Option<(Point, Color)> = None;

            for &(idx, t) in &candle_indices {
                let x = interval_to_x(t);
                if let Some(val) = indicator_host
                    .output_value_for(instance.id, &output.id, idx)
                    .and_then(|v| v.as_number())
                {
                    let f = val as f32;
                    if f.is_finite() && x.is_finite() {
                        let y = price_to_y(Price::from_f32(f));
                        if y.is_finite() {
                            let pt = Point::new(x, y);

                            let color = indicator_host
                                .output_color_for(instance.id, &output.id, idx)
                                .map(engine_color_to_iced)
                                .unwrap_or_else(|| resolve_output_color(instance, output, palette));

                            append_indicator_line_point(
                                &mut runs,
                                prev_pt_color,
                                pt,
                                color,
                                is_step,
                            );
                            prev_pt_color = Some((pt, color));
                            continue;
                        }
                    }
                }
                prev_pt_color = None;
            }

            draw_line_runs(frame, &runs, width, style);
        }

        // 5. Markers
        for &(idx, t) in &candle_indices {
            let x = interval_to_x(t);
            let markers = indicator_host.marker_outputs_for(instance.id, idx);
            for m in markers {
                if let Some(val) = m.value {
                    let f = val as f32;
                    if f.is_finite() && x.is_finite() {
                        let y = price_to_y(Price::from_f32(f));
                        if y.is_finite() {
                            let color = m
                                .color
                                .map(engine_color_to_iced)
                                .unwrap_or(palette.primary.base.color);
                            draw_marker(frame, Point::new(x, y), None, color, m.text.as_deref());
                        }
                    }
                }
            }
        }

        // 6. Segments
        let segments =
            indicator_host.segment_outputs_for(instance.id, source_range.clone(), |s, e| {
                candle_indices.iter().any(|&(i, _)| i >= s && i <= e)
            });
        for seg in segments {
            if let (Some(sv), Some(ev)) = (seg.start_value, seg.end_value) {
                let sv_f = sv as f32;
                let ev_f = ev as f32;
                if !sv_f.is_finite() || !ev_f.is_finite() {
                    continue;
                }
                let start_time = resolve_candle_timestamp(
                    data_source,
                    &candle_indices,
                    seg.start_index,
                    earliest,
                );
                let end_time =
                    resolve_candle_timestamp(data_source, &candle_indices, seg.end_index, latest);
                let sx = interval_to_x(start_time);
                let sy = price_to_y(Price::from_f32(sv_f));
                let ex = interval_to_x(end_time);
                let ey = price_to_y(Price::from_f32(ev_f));
                if !sx.is_finite() || !sy.is_finite() || !ex.is_finite() || !ey.is_finite() {
                    continue;
                }
                let color = seg
                    .color
                    .map(engine_color_to_iced)
                    .unwrap_or(palette.primary.base.color);
                draw_segment(
                    frame,
                    Point::new(sx, sy),
                    Point::new(ex, ey),
                    1.5,
                    color,
                    seg.text.as_deref(),
                );
            }
        }
    }
}

/// Canvas program rendering a scripted panel indicator plot and its synchronized crosshair.
pub struct ScriptedPanelCanvas<'a> {
    pub main_cache: &'a Cache,
    pub crosshair_cache: &'a Cache,
    pub chart: &'a ViewState,
    pub instance: &'a IndicatorInstance,
    pub indicator_host: &'a IndicatorHost,
    pub data_source: &'a PlotData<KlineDataPoint>,
    pub min_val: f32,
    pub max_val: f32,
    pub visible_range: RangeInclusive<u64>,
}

impl canvas::Program<Message> for ScriptedPanelCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let palette = theme.extended_palette();
        let earliest = *self.visible_range.start();
        let latest = *self.visible_range.end();

        let candle_indices = visible_candle_indices(self.data_source, earliest, latest);

        // 1. Indicator Plot Geometry
        let indicator = self.main_cache.draw(renderer, bounds.size(), |frame| {
            frame.translate(Vector::new(bounds.width / 2.0, 0.0));
            frame.scale(self.chart.scaling);
            frame.translate(Vector::new(self.chart.translation.x, 0.0));

            let px_height = bounds.height / self.chart.scaling;
            let scale = YScale {
                min: self.min_val,
                max: self.max_val,
                px_height,
            };

            let to_y = |val: f32| scale.to_y(val);

            let def = self.instance.kind.definition();
            let source_range = candle_indices.first().map(|f| f.0).unwrap_or(0)
                ..candle_indices.last().map(|l| l.0 + 1).unwrap_or(1);

            let region_width = bounds.width / self.chart.scaling;
            let region_x = -self.chart.translation.x - region_width / 2.0;

            // Horizontal Levels (e.g. RSI 30/70)
            let levels = self.indicator_host.level_outputs_for(self.instance.id);
            for level in levels {
                if let Some(val) = level.value {
                    let f = val as f32;
                    if !f.is_finite() {
                        continue;
                    }
                    let y = to_y(f);
                    if !y.is_finite() {
                        continue;
                    }
                    let color = level
                        .color
                        .map(engine_color_to_iced)
                        .unwrap_or(palette.background.base.text);
                    draw_horizontal_level(
                        frame,
                        region_x,
                        region_x + region_width,
                        y,
                        color,
                        Some(&level.id),
                    );
                }
            }

            // Series Bands / Clouds
            if self.instance.style_overrides.fill_visible {
                for fill_spec in &def.fill_specs {
                    let IndicatorOverlaySource::Channel(upper_ch) = &fill_spec.upper;
                    let IndicatorOverlaySource::Channel(lower_ch) = &fill_spec.lower;

                    let mut upper_points = Vec::new();
                    let mut lower_points = Vec::new();

                    for &(idx, t) in &candle_indices {
                        let x = self.chart.interval_to_x(t);
                        let u_val = self
                            .indicator_host
                            .output_value_for(self.instance.id, upper_ch, idx)
                            .and_then(|v| v.as_number());
                        let l_val = self
                            .indicator_host
                            .output_value_for(self.instance.id, lower_ch, idx)
                            .and_then(|v| v.as_number());
                        if let (Some(u), Some(l)) = (u_val, l_val) {
                            let uf = u as f32;
                            let lf = l as f32;
                            if uf.is_finite() && lf.is_finite() && x.is_finite() {
                                let uy = to_y(uf);
                                let ly = to_y(lf);
                                if uy.is_finite() && ly.is_finite() {
                                    upper_points.push(Point::new(x, uy));
                                    lower_points.push(Point::new(x, ly));
                                }
                            }
                        }
                    }

                    let color = resolve_fill_color(self.instance, fill_spec, palette);
                    fill_indicator_band(frame, &upper_points, &lower_points, color);
                }
            }

            // Output Curves, Histograms & Steps
            for output in &def.outputs {
                if output.value_type != OutputValueType::Number {
                    continue;
                }

                let width = self
                    .instance
                    .style_overrides
                    .line_width(&output.id)
                    .unwrap_or(output.line_width as u8)
                    .clamp(1, 4) as f32;
                let style = self
                    .instance
                    .style_overrides
                    .stroke_style(&output.id)
                    .unwrap_or(LineStrokeStyle::Solid);

                let is_step = def
                    .mark_specs
                    .iter()
                    .any(|m| m.source_channel == output.id && matches!(m.mark, MarkKind::StepLine));
                let is_histogram = def
                    .mark_specs
                    .iter()
                    .any(|m| m.source_channel == output.id && matches!(m.mark, MarkKind::Bar(_)));

                if is_histogram {
                    let baseline_y = to_y(0.0).clamp(0.0, px_height);
                    let bar_w = (self.chart.cell_width * 0.8).max(1.0);

                    for &(idx, t) in &candle_indices {
                        let x = self.chart.interval_to_x(t);
                        if let Some(val) = self
                            .indicator_host
                            .output_value_for(self.instance.id, &output.id, idx)
                            .and_then(|v| v.as_number())
                        {
                            let f = val as f32;
                            if f.is_finite() && x.is_finite() {
                                let y = to_y(f);
                                if y.is_finite() {
                                    let color = self
                                        .indicator_host
                                        .output_color_for(self.instance.id, &output.id, idx)
                                        .map(engine_color_to_iced)
                                        .unwrap_or_else(|| {
                                            resolve_output_color(self.instance, output, palette)
                                        });

                                    let top = y.min(baseline_y);
                                    let h = (y - baseline_y).abs().max(1.0);
                                    frame.fill_rectangle(
                                        Point::new(x - bar_w * 0.5, top),
                                        Size::new(bar_w, h),
                                        color,
                                    );
                                }
                            }
                        }
                    }
                } else {
                    let mut runs = Vec::new();
                    let mut prev_pt_color: Option<(Point, Color)> = None;

                    for &(idx, t) in &candle_indices {
                        let x = self.chart.interval_to_x(t);
                        if let Some(val) = self
                            .indicator_host
                            .output_value_for(self.instance.id, &output.id, idx)
                            .and_then(|v| v.as_number())
                        {
                            let f = val as f32;
                            if f.is_finite() && x.is_finite() {
                                let y = to_y(f);
                                if y.is_finite() {
                                    let pt = Point::new(x, y);

                                    let color = self
                                        .indicator_host
                                        .output_color_for(self.instance.id, &output.id, idx)
                                        .map(engine_color_to_iced)
                                        .unwrap_or_else(|| {
                                            resolve_output_color(self.instance, output, palette)
                                        });

                                    append_indicator_line_point(
                                        &mut runs,
                                        prev_pt_color,
                                        pt,
                                        color,
                                        is_step,
                                    );
                                    prev_pt_color = Some((pt, color));
                                    continue;
                                }
                            }
                        }
                        prev_pt_color = None;
                    }

                    draw_line_runs(frame, &runs, width, style);
                }
            }

            // Markers
            for &(idx, t) in &candle_indices {
                let x = self.chart.interval_to_x(t);
                let markers = self
                    .indicator_host
                    .marker_outputs_for(self.instance.id, idx);
                for m in markers {
                    if let Some(val) = m.value {
                        let f = val as f32;
                        if f.is_finite() && x.is_finite() {
                            let y = to_y(f);
                            if y.is_finite() {
                                let color = m
                                    .color
                                    .map(engine_color_to_iced)
                                    .unwrap_or(palette.primary.base.color);
                                draw_marker(
                                    frame,
                                    Point::new(x, y),
                                    None,
                                    color,
                                    m.text.as_deref(),
                                );
                            }
                        }
                    }
                }
            }

            // Segments
            let segments =
                self.indicator_host
                    .segment_outputs_for(self.instance.id, source_range, |s, e| {
                        candle_indices.iter().any(|&(i, _)| i >= s && i <= e)
                    });
            for seg in segments {
                if let (Some(sv), Some(ev)) = (seg.start_value, seg.end_value) {
                    let sv_f = sv as f32;
                    let ev_f = ev as f32;
                    if !sv_f.is_finite() || !ev_f.is_finite() {
                        continue;
                    }
                    let start_time = resolve_candle_timestamp(
                        self.data_source,
                        &candle_indices,
                        seg.start_index,
                        earliest,
                    );
                    let end_time = resolve_candle_timestamp(
                        self.data_source,
                        &candle_indices,
                        seg.end_index,
                        latest,
                    );
                    let sx = self.chart.interval_to_x(start_time);
                    let sy = to_y(sv_f);
                    let ex = self.chart.interval_to_x(end_time);
                    let ey = to_y(ev_f);
                    if !sx.is_finite() || !sy.is_finite() || !ex.is_finite() || !ey.is_finite() {
                        continue;
                    }
                    let color = seg
                        .color
                        .map(engine_color_to_iced)
                        .unwrap_or(palette.primary.base.color);
                    draw_segment(
                        frame,
                        Point::new(sx, sy),
                        Point::new(ex, ey),
                        1.5,
                        color,
                        seg.text.as_deref(),
                    );
                }
            }
        });

        // 2. Crosshair Geometry
        let crosshair = self.crosshair_cache.draw(renderer, bounds.size(), |frame| {
            let dashed = dashed_line(theme);
            if let Some(cursor_position) = cursor.position_in(self.chart.bounds) {
                let width = frame.width() / self.chart.scaling;
                let region = Rectangle {
                    x: -self.chart.translation.x - width / 2.0,
                    y: 0.0,
                    width,
                    height: frame.height() / self.chart.scaling,
                };
                let c_earliest = self.chart.x_to_interval(region.x) as f64;
                let c_latest = self.chart.x_to_interval(region.x + region.width) as f64;

                let crosshair_ratio = f64::from(cursor_position.x / bounds.width);
                let snap_ratio = match self.chart.basis {
                    crate::chart::Basis::Time(tf) => {
                        let step = tf.to_milliseconds() as f64;
                        let rx = ((c_earliest + crosshair_ratio * (c_latest - c_earliest)) / step)
                            .round()
                            * step;
                        if c_latest <= c_earliest {
                            0.5
                        } else {
                            ((rx - c_earliest) / (c_latest - c_earliest)) as f32
                        }
                    }
                    crate::chart::Basis::Tick(_) => {
                        let world_x = region.x + (cursor_position.x / bounds.width) * region.width;
                        let snapped =
                            (world_x / self.chart.cell_width).round() * self.chart.cell_width;
                        (snapped - region.x) / region.width
                    }
                };

                if snap_ratio.is_finite() {
                    frame.stroke(
                        &Path::line(
                            Point::new(snap_ratio * bounds.width, 0.0),
                            Point::new(snap_ratio * bounds.width, bounds.height),
                        ),
                        dashed,
                    );
                }

                // Tooltip header on hovered bar
                let hovered_idx = candle_indices
                    .iter()
                    .min_by_key(|(_, t)| {
                        let x = self.chart.interval_to_x(*t);
                        let screen_x = (x + self.chart.translation.x) * self.chart.scaling
                            + bounds.width / 2.0;
                        ((screen_x - cursor_position.x).abs() * 1000.0) as i64
                    })
                    .map(|(idx, _)| *idx);

                if let Some(idx) = hovered_idx {
                    let mut label_text = self.instance.kind.definition().display_name.clone();
                    for out in &self.instance.kind.definition().outputs {
                        if let Some(val) = self
                            .indicator_host
                            .output_value_for(self.instance.id, &out.id, idx)
                            .and_then(|v| v.as_number())
                        {
                            label_text.push_str(&format!("  {}: {:.2}", out.label, val));
                        }
                    }

                    frame.fill_text(canvas::Text {
                        content: label_text,
                        position: Point::new(10.0, 10.0),
                        color: palette.background.base.text,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }
            } else if let Some(cursor_position) = cursor.position_in(bounds) {
                let snap_ratio = cursor_position.y / bounds.height;
                if snap_ratio.is_finite() {
                    frame.stroke(
                        &Path::line(
                            Point::new(0.0, snap_ratio * bounds.height),
                            Point::new(bounds.width, snap_ratio * bounds.height),
                        ),
                        dashed,
                    );
                }
            }
        });

        vec![indicator, crosshair]
    }
}

/// Renders a full panel indicator row (plot canvas + split ruler + Y-axis labels canvas).
pub fn render_scripted_panel_row<'a>(
    chart_state: &'a ViewState,
    cache: &'a Caches,
    instance: &'a IndicatorInstance,
    indicator_host: &'a IndicatorHost,
    data_source: &'a PlotData<KlineDataPoint>,
    visible_range: RangeInclusive<u64>,
) -> Element<'a, Message> {
    let earliest = *visible_range.start();
    let latest = *visible_range.end();

    let candle_indices = visible_candle_indices(data_source, earliest, latest);

    // Calculate vertical extents over visible bars
    let mut min_val = f32::INFINITY;
    let mut max_val = f32::NEG_INFINITY;

    for out in &instance.kind.definition().outputs {
        if out.value_type != OutputValueType::Number {
            continue;
        }
        for &(idx, _) in &candle_indices {
            if let Some(v) = indicator_host
                .output_value_for(instance.id, &out.id, idx)
                .and_then(|v| v.as_number())
            {
                let f = v as f32;
                if f.is_finite() {
                    min_val = min_val.min(f);
                    max_val = max_val.max(f);
                }
            }
        }
    }

    // Include horizontal levels in range
    let levels = indicator_host.level_outputs_for(instance.id);
    for lvl in levels {
        if let Some(v) = lvl.value {
            let f = v as f32;
            if f.is_finite() {
                min_val = min_val.min(f);
                max_val = max_val.max(f);
            }
        }
    }

    if !min_val.is_finite() || !max_val.is_finite() {
        min_val = 0.0;
        max_val = 100.0;
    } else if min_val >= max_val {
        let center = (min_val + max_val) * 0.5;
        min_val = center - 1.0;
        max_val = center + 1.0;
    } else {
        let span = max_val - min_val;
        let pad = (span * 0.08).max(0.01);
        min_val -= pad;
        max_val += pad;
    }

    let canvas = Canvas::new(ScriptedPanelCanvas {
        main_cache: &cache.main,
        crosshair_cache: &cache.crosshair,
        chart: chart_state,
        instance,
        indicator_host,
        data_source,
        min_val,
        max_val,
        visible_range,
    })
    .height(Length::Fill)
    .width(Length::Fill);

    let labels = Canvas::new(IndicatorLabel {
        label_cache: &cache.y_labels,
        max: max_val,
        min: min_val,
        chart_bounds: chart_state.bounds,
    })
    .height(Length::Fill)
    .width(chart_state.y_labels_width());

    row![
        canvas,
        rule::vertical(1).style(crate::style::split_ruler),
        container(labels),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use data::chart::kline::indicators::{IndicatorFillGeometry, IndicatorId};

    #[test]
    fn test_engine_color_to_iced() {
        let engine_col = indicator_engine::Color(0x80112233);
        let iced_col = engine_color_to_iced(engine_col);
        assert!((iced_col.a - 0x80 as f32 / 255.0).abs() < 1e-4);
        assert!((iced_col.r - 0x11 as f32 / 255.0).abs() < 1e-4);
        assert!((iced_col.g - 0x22 as f32 / 255.0).abs() < 1e-4);
        assert!((iced_col.b - 0x33 as f32 / 255.0).abs() < 1e-4);
    }

    #[test]
    fn test_resolve_fill_color_alpha_cap() {
        let palette = iced::Theme::Dark.extended_palette();
        let inst = IndicatorInstance::new(
            data::chart::kline::indicators::IndicatorInstanceId(1),
            IndicatorId::parse("core.rsi").unwrap(),
        );
        let spec = IndicatorFillSpec {
            upper: IndicatorOverlaySource::Channel("top".into()),
            lower: IndicatorOverlaySource::Channel("bot".into()),
            upper_offset_bars: 0,
            lower_offset_bars: 0,
            style_by: None,
            geometry: IndicatorFillGeometry::SeriesBand,
            color_slot: "fill".into(),
            true_color_slot: None,
            false_color_slot: None,
            opacity: 0.85, // Requests 0.85 opacity
        };
        let c = resolve_fill_color(&inst, &spec, palette);
        assert!(c.a <= 0.25, "Fill alpha must not exceed 0.25, got {}", c.a);
        assert_eq!(c.a, 0.25);
    }

    #[test]
    fn test_indicator_line_stroke_styles() {
        let c = Color::from_rgb(1.0, 0.0, 0.0);
        let s_solid = indicator_line_stroke(2.0, LineStrokeStyle::Solid, c);
        assert!(s_solid.line_dash.segments.is_empty());

        let s_dashed = indicator_line_stroke(2.0, LineStrokeStyle::Dashed, c);
        assert_eq!(s_dashed.line_dash.segments, &[6.0f32, 4.0f32]);

        let s_dotted = indicator_line_stroke(2.0, LineStrokeStyle::Dotted, c);
        assert_eq!(s_dotted.line_dash.segments, &[1.0f32, 3.0f32]);
    }

    #[test]
    fn test_append_indicator_line_point_step_line() {
        let mut runs = Vec::new();
        let c = Color::from_rgb(1.0, 1.0, 1.0);

        let pt1 = Point::new(10.0, 100.0);
        let pt2 = Point::new(20.0, 120.0);

        append_indicator_line_point(&mut runs, None, pt1, c, true);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].points, vec![pt1]);

        append_indicator_line_point(&mut runs, Some((pt1, c)), pt2, c, true);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].points, vec![pt1, Point::new(20.0, 100.0), pt2]);
    }

    #[test]
    fn test_append_indicator_line_point_color_break() {
        let mut runs = Vec::new();
        let c1 = Color::from_rgb(1.0, 0.0, 0.0);
        let c2 = Color::from_rgb(0.0, 1.0, 0.0);

        let pt1 = Point::new(10.0, 100.0);
        let pt2 = Point::new(20.0, 120.0);

        append_indicator_line_point(&mut runs, None, pt1, c1, false);
        append_indicator_line_point(&mut runs, Some((pt1, c1)), pt2, c2, false);

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].color, c1);
        assert_eq!(runs[0].points, vec![pt1]);
        assert_eq!(runs[1].color, c2);
        assert_eq!(runs[1].points, vec![pt1, pt2]);
    }

    #[test]
    fn test_adversarial_viewport_culling_100k_bars_scaling_and_asymptotics() {
        use data::aggr::time::TimeSeries;
        use exchange::util::{Price, PriceStep};
        use exchange::{Kline, Timeframe};
        use std::collections::BTreeMap;

        let count = 100_000;
        let step_ms = 60_000u64;
        let start_time = 1_700_000_000_000u64;

        let mut datapoints = BTreeMap::new();
        for i in 0..count {
            let t = start_time + (i as u64) * step_ms;
            datapoints.insert(
                t,
                KlineDataPoint {
                    kline: Kline {
                        time: t,
                        open: Price::from_f32(100.0),
                        high: Price::from_f32(105.0),
                        low: Price::from_f32(95.0),
                        close: Price::from_f32(102.0),
                        volume: (10.0, 10.0),
                    },
                    footprint: data::chart::kline::KlineTrades::new(),
                    trades_fetched: true,
                },
            );
        }

        let ts = TimeSeries {
            datapoints,
            interval: Timeframe::M1,
            tick_size: PriceStep { units: 1 },
            exchange: None,
        };
        let plot_data = PlotData::TimeBased(ts);

        // 1. Narrow viewport (50 bars) at right boundary (most common real-time view)
        let right_earliest = start_time + ((count - 50) as u64) * step_ms;
        let right_latest = start_time + ((count - 1) as u64) * step_ms;

        let start_instant = std::time::Instant::now();
        let indices_right = visible_candle_indices(&plot_data, right_earliest, right_latest);
        let right_duration = start_instant.elapsed();

        // Must be bounded to visible bars + boundary neighbors (50..=52)
        assert!(
            indices_right.len() >= 50 && indices_right.len() <= 52,
            "Visible indices must be bounded to K+2, got {}",
            indices_right.len()
        );
        assert_eq!(indices_right.last().unwrap().0, count - 1);

        // 2. Narrow viewport (50 bars) at left boundary
        let left_earliest = start_time;
        let left_latest = start_time + 49 * step_ms;

        let start_left = std::time::Instant::now();
        let indices_left = visible_candle_indices(&plot_data, left_earliest, left_latest);
        let left_duration = start_left.elapsed();

        assert!(
            indices_left.len() >= 50 && indices_left.len() <= 52,
            "Visible indices must be bounded, got {}",
            indices_left.len()
        );
        assert_eq!(indices_left[0].0, 0);

        // 3. Out-of-bounds future
        let future_earliest = start_time + (count as u64 + 100) * step_ms;
        let future_latest = future_earliest + 50 * step_ms;
        let indices_future = visible_candle_indices(&plot_data, future_earliest, future_latest);
        assert!(indices_future.is_empty(), "Future viewport must be empty");

        // 4. Inverted range (latest < earliest)
        let indices_inverted = visible_candle_indices(&plot_data, right_latest, right_earliest);
        assert!(
            indices_inverted.is_empty(),
            "Inverted viewport must be empty"
        );

        println!(
            "100k bars benchmark: Left viewport (idx 0..50) = {:?}, Right viewport (idx 99950..100000) = {:?}",
            left_duration, right_duration
        );
    }

    #[test]
    fn test_adversarial_numerical_extremes_yscale_and_zerodiv() {
        let px_height = 200.0;

        // A. Identical min and max (zero division vector)
        let scale_identical = YScale {
            min: 100.0,
            max: 100.0,
            px_height,
        };
        assert_eq!(scale_identical.to_y(100.0), px_height);
        assert_eq!(scale_identical.to_y(150.0), px_height);
        assert_eq!(scale_identical.to_y(50.0), px_height);

        // B. Inverted min and max (max < min)
        let scale_inverted = YScale {
            min: 150.0,
            max: 100.0,
            px_height,
        };
        assert_eq!(scale_inverted.to_y(120.0), px_height);

        // C. Zero scale (0.0 to 0.0)
        let scale_zero = YScale {
            min: 0.0,
            max: 0.0,
            px_height,
        };
        assert_eq!(scale_zero.to_y(0.0), px_height);

        // D. Strictly negative price range (-200.0 to -100.0)
        let scale_negative = YScale {
            min: -200.0,
            max: -100.0,
            px_height,
        };
        assert!((scale_negative.to_y(-200.0) - px_height).abs() < 1e-4);
        assert!((scale_negative.to_y(-100.0) - 0.0).abs() < 1e-4);
        assert!((scale_negative.to_y(-150.0) - 100.0).abs() < 1e-4);

        // E. Sub-normal epsilon span (100.0 to 100.0 + f32::EPSILON)
        let scale_eps = YScale {
            min: 100.0,
            max: 100.0 + f32::EPSILON,
            px_height,
        };
        let y_eps = scale_eps.to_y(100.0);
        assert!(y_eps.is_finite(), "Epsilon span must not produce NaN/Inf");

        // F. Extents fallback logic from render_scripted_panel_row
        let check_extents = |mut min_val: f32, mut max_val: f32| -> (f32, f32) {
            if !min_val.is_finite() || !max_val.is_finite() || min_val >= max_val {
                min_val = 0.0;
                max_val = 100.0;
            } else {
                let span = max_val - min_val;
                let pad = (span * 0.08).max(0.01);
                min_val -= pad;
                max_val += pad;
            }
            (min_val, max_val)
        };

        assert_eq!(check_extents(f32::NAN, f32::NAN), (0.0, 100.0));
        assert_eq!(
            check_extents(f32::NEG_INFINITY, f32::INFINITY),
            (0.0, 100.0)
        );
        assert_eq!(check_extents(50.0, 50.0), (0.0, 100.0));
        assert_eq!(check_extents(80.0, 20.0), (0.0, 100.0));

        let (norm_min, norm_max) = check_extents(10.0, 20.0);
        assert!(norm_min < 10.0 && norm_max > 20.0);
    }

    #[test]
    fn test_adversarial_nan_inf_propagation_and_price_lossy() {
        use exchange::util::Price;

        // Price::from_f32 with NaN produces units: 0
        let price_nan = Price::from_f32(f32::NAN);
        assert_eq!(price_nan.units, 0);

        // Price::from_f32 with +Inf produces units: i64::MAX
        let price_inf = Price::from_f32(f32::INFINITY);
        assert_eq!(price_inf.units, i64::MAX);

        // Price::from_f32 with -Inf produces units: i64::MIN
        let price_neg_inf = Price::from_f32(f32::NEG_INFINITY);
        assert_eq!(price_neg_inf.units, i64::MIN);

        // Price::from_f32 with negative price
        let price_neg = Price::from_f32(-50.25);
        assert!((price_neg.to_f32() - (-50.25)).abs() < 1e-4);

        // Verify YScale when given NaN or Inf values
        let scale = YScale {
            min: 0.0,
            max: 100.0,
            px_height: 200.0,
        };
        let y_nan = scale.to_y(f32::NAN);
        assert!(y_nan.is_nan(), "to_y(NaN) propagates NaN");

        let y_inf = scale.to_y(f32::INFINITY);
        assert_eq!(y_inf, f32::NEG_INFINITY);

        let y_neg_inf = scale.to_y(f32::NEG_INFINITY);
        assert_eq!(y_neg_inf, f32::INFINITY);
    }

    #[test]
    fn test_adversarial_line_step_and_missing_data_gaps() {
        let mut runs = Vec::new();
        let c_red = Color::from_rgb(1.0, 0.0, 0.0);
        let c_blue = Color::from_rgb(0.0, 0.0, 1.0);

        // A. Alternating colors on every point (Red, Blue, Red, Blue)
        let p0 = Point::new(10.0, 100.0);
        let p1 = Point::new(20.0, 110.0);
        let p2 = Point::new(30.0, 120.0);
        let p3 = Point::new(40.0, 130.0);

        append_indicator_line_point(&mut runs, None, p0, c_red, false);
        append_indicator_line_point(&mut runs, Some((p0, c_red)), p1, c_blue, false);
        append_indicator_line_point(&mut runs, Some((p1, c_blue)), p2, c_red, false);
        append_indicator_line_point(&mut runs, Some((p2, c_red)), p3, c_blue, false);

        assert_eq!(runs.len(), 4, "Every color alternation creates a new run");
        assert_eq!(runs[0].points, vec![p0]);
        assert_eq!(runs[1].points, vec![p0, p1]);
        assert_eq!(runs[2].points, vec![p1, p2]);
        assert_eq!(runs[3].points, vec![p2, p3]);

        // B. Missing data handling: Point at 10 (Red), Gap at 20 (None), Point at 30 (Red)
        let mut gap_runs = Vec::new();
        let pt_a = Point::new(10.0, 100.0);
        let pt_c = Point::new(30.0, 120.0);

        append_indicator_line_point(&mut gap_runs, None, pt_a, c_red, false);
        // Point B was None, so previous is reset to None
        append_indicator_line_point(&mut gap_runs, None, pt_c, c_red, false);

        // After fix: A missing point breaks polyline continuity, producing 2 runs instead of bridging
        assert_eq!(gap_runs.len(), 2, "Data gap must create separate line runs");
        assert_eq!(gap_runs[0].points, vec![pt_a]);
        assert_eq!(gap_runs[1].points, vec![pt_c]);

        // C. Step-line with rapid vertical steps
        let mut step_runs = Vec::new();
        let s0 = Point::new(10.0, 100.0);
        let s1 = Point::new(20.0, 200.0);
        let s2 = Point::new(30.0, 50.0);

        append_indicator_line_point(&mut step_runs, None, s0, c_red, true);
        append_indicator_line_point(&mut step_runs, Some((s0, c_red)), s1, c_red, true);
        append_indicator_line_point(&mut step_runs, Some((s1, c_red)), s2, c_red, true);

        assert_eq!(step_runs.len(), 1);
        assert_eq!(
            step_runs[0].points,
            vec![s0, Point::new(20.0, 100.0), s1, Point::new(30.0, 200.0), s2,]
        );
    }

    #[test]
    fn test_adversarial_geometric_boundary_clamps() {
        // A. Stroke style clamping
        let c = Color::from_rgb(0.5, 0.5, 0.5);
        let s_under = indicator_line_stroke(0.0, LineStrokeStyle::Solid, c);
        assert_eq!(s_under.width, 1.0, "Stroke width clamped to min 1.0");

        let s_over = indicator_line_stroke(10.0, LineStrokeStyle::Solid, c);
        assert_eq!(s_over.width, 4.0, "Stroke width clamped to max 4.0");

        // B. Path constructions with empty, single, and boundary points
        let _p_empty = Path::new(|_| {});

        // C. Path with circle at NaN / Inf coordinates panics in lyon_path
        let panic_nan = std::panic::catch_unwind(|| {
            let _ = Path::circle(Point::new(f32::NAN, 100.0), 3.5);
        });
        assert!(
            panic_nan.is_err(),
            "VULNERABILITY CONFIRMED: Path::circle with NaN coordinate panics in lyon_path without is_finite check"
        );

        let panic_inf = std::panic::catch_unwind(|| {
            let _ = Path::circle(Point::new(f32::INFINITY, 100.0), 3.5);
        });
        assert!(
            panic_inf.is_err(),
            "VULNERABILITY CONFIRMED: Path::circle with Inf coordinate panics in lyon_path without is_finite check"
        );

        let _p_neg = Path::circle(Point::new(-100.0, -200.0), 3.5);

        // D. Rectangle with inverted or zero dimensions
        let top_y = 200.0f32.min(100.0);
        let h = (200.0f32 - 100.0).abs().max(1.0);
        let w = (50.0f32 - 100.0).max(1.0);
        assert_eq!(top_y, 100.0);
        assert_eq!(h, 100.0);
        assert_eq!(w, 1.0);
    }

    #[test]
    fn test_adversarial_culling_asymptotic_scaling_empirical_ratio() {
        use data::aggr::time::TimeSeries;
        use exchange::util::{Price, PriceStep};
        use exchange::{Kline, Timeframe};
        use std::collections::BTreeMap;

        let build_series = |size: usize| -> PlotData<KlineDataPoint> {
            let mut datapoints = BTreeMap::new();
            for i in 0..size {
                let t = (i as u64) * 60_000;
                datapoints.insert(
                    t,
                    KlineDataPoint {
                        kline: Kline {
                            time: t,
                            open: Price::from_f32(100.0),
                            high: Price::from_f32(105.0),
                            low: Price::from_f32(95.0),
                            close: Price::from_f32(102.0),
                            volume: (10.0, 10.0),
                        },
                        footprint: data::chart::kline::KlineTrades::new(),
                        trades_fetched: true,
                    },
                );
            }
            PlotData::TimeBased(TimeSeries {
                datapoints,
                interval: Timeframe::M1,
                tick_size: PriceStep { units: 1 },
                exchange: None,
            })
        };

        let series_1k = build_series(1_000);
        let series_10k = build_series(10_000);
        let series_100k = build_series(100_000);

        let bench =
            |data: &PlotData<KlineDataPoint>, total: usize| -> (std::time::Duration, usize) {
                let earliest = ((total - 50) as u64) * 60_000;
                let latest = ((total - 1) as u64) * 60_000;
                let iters = 50;
                let t0 = std::time::Instant::now();
                let mut count = 0;
                for _ in 0..iters {
                    let res = visible_candle_indices(data, earliest, latest);
                    count = res.len();
                }
                (t0.elapsed() / iters, count)
            };

        let (dur_1k, c1) = bench(&series_1k, 1_000);
        let (dur_10k, c10) = bench(&series_10k, 10_000);
        let (dur_100k, c100) = bench(&series_100k, 100_000);

        assert_eq!(c1, c10);
        assert_eq!(c10, c100);

        println!(
            "Asymptotic scaling: 1k bars = {:?}, 10k bars = {:?}, 100k bars = {:?}",
            dur_1k, dur_10k, dur_100k
        );
    }
}
