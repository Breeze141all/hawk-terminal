//! Crosshair tooltip and price overlays (liquidation heatmap, VWAPs).
//!
//! Split out of `chart/kline.rs` (ROADMAP #131); the code is unchanged apart from
//! `pub(super)` visibility.

use super::*;

pub(super) fn draw_crosshair_tooltip(
    data: &PlotData<KlineDataPoint>,
    ticker_info: &TickerInfo,
    frame: &mut canvas::Frame,
    palette: &Extended,
    at_interval: u64,
) {
    let kline_opt = match data {
        PlotData::TimeBased(timeseries) => timeseries
            .datapoints
            .iter()
            .find(|(time, _)| **time == at_interval)
            .map(|(_, dp)| &dp.kline)
            .or_else(|| {
                if timeseries.datapoints.is_empty() {
                    None
                } else {
                    let (last_time, dp) = timeseries.datapoints.last_key_value()?;
                    if at_interval > *last_time {
                        Some(&dp.kline)
                    } else {
                        None
                    }
                }
            }),
        PlotData::TickBased(tick_aggr) => {
            let index = (at_interval / u64::from(tick_aggr.interval.0)) as usize;
            if index < tick_aggr.datapoints.len() {
                Some(&tick_aggr.datapoints[tick_aggr.datapoints.len() - 1 - index].kline)
            } else {
                None
            }
        }
    };

    if let Some(kline) = kline_opt {
        let change_pct = ((kline.close - kline.open).to_f32() / kline.open.to_f32()) * 100.0;
        let change_color = if change_pct >= 0.0 {
            palette.success.base.color
        } else {
            palette.danger.base.color
        };

        let base_color = palette.background.base.text;
        let precision = ticker_info.min_ticksize;

        let segments = [
            ("O", base_color, false),
            (&kline.open.to_string(precision), change_color, true),
            ("H", base_color, false),
            (&kline.high.to_string(precision), change_color, true),
            ("L", base_color, false),
            (&kline.low.to_string(precision), change_color, true),
            ("C", base_color, false),
            (&kline.close.to_string(precision), change_color, true),
            (&format!("{change_pct:+.2}%"), change_color, true),
        ];

        let total_width: f32 = segments
            .iter()
            .map(|(s, _, _)| s.len() as f32 * (TEXT_SIZE * 0.8))
            .sum();

        let position = Point::new(8.0, 8.0);

        let tooltip_rect = Rectangle {
            x: position.x,
            y: position.y,
            width: total_width,
            height: 16.0,
        };

        frame.fill_rectangle(
            tooltip_rect.position(),
            tooltip_rect.size(),
            palette.background.weakest.color.scale_alpha(0.9),
        );

        let mut x = position.x;
        for (text, seg_color, is_value) in segments {
            frame.fill_text(canvas::Text {
                content: text.to_string(),
                position: Point::new(x, position.y),
                size: iced::Pixels(12.0),
                color: seg_color,
                font: style::AZERET_MONO,
                ..canvas::Text::default()
            });
            x += text.len() as f32 * 8.0;
            x += if is_value { 6.0 } else { 2.0 };
        }
    }
}

pub(super) fn draw_liquidation_heatmap(
    heatmap: &LiquidationHeatmap,
    frame: &mut canvas::Frame,
    region: &Rectangle,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    _earliest: u64,
    latest: u64,
    current_price: f32,
    show_bands: bool,
    show_histogram: bool,
    is_dark: bool,
) {
    if !show_bands && !show_histogram {
        return;
    }

    let right_edge_x = region.x + region.width;

    // 1. 2D Time x Price Liquidation Bands
    if show_bands {
        let max_strength = heatmap.max_segment_strength();
        // Noise gate filter: keep low-volume single-candle noise transparent
        let min_ratio = 0.08_f32;

        for seg in heatmap.segments() {
            let ratio = seg.strength / max_strength;
            if ratio < min_ratio {
                continue;
            }

            let y_top = price_to_y(seg.top);
            let y_bottom = price_to_y(seg.bottom);
            let y = y_top.min(y_bottom);
            let h = (y_top - y_bottom).abs().max(1.0);

            if y + h < region.y || y > region.y + region.height {
                continue;
            }

            let start_x = interval_to_x(seg.start_time);
            let end_x = match seg.end_time {
                Some(t_end) => interval_to_x(t_end),
                None => interval_to_x(latest).max(right_edge_x),
            };

            if end_x < region.x || start_x > right_edge_x {
                continue;
            }

            let x1 = start_x.max(region.x);
            let x2 = end_x.min(right_edge_x);
            if x2 <= x1 {
                continue;
            }
            let w = (x2 - x1).max(1.0);

            let norm_ratio = ((ratio - min_ratio) / (1.0 - min_ratio)).clamp(0.0, 1.0);
            let (r, g, b, a) = interpolate_color_themed(norm_ratio, is_dark);
            let band_color = iced::Color::from_rgba(r, g, b, a);

            frame.fill_rectangle(Point::new(x1, y), Size::new(w, h), band_color);
        }
    }

    // 2. Right-side concentration histogram
    if show_histogram {
        let cells = heatmap.visible_cells(current_price, 300);
        if !cells.is_empty() {
            let max_strength = cells
                .iter()
                .map(|c| c.strength)
                .fold(0.0_f32, f32::max)
                .max(0.001);

            let max_bar_width = (region.width * 0.18).clamp(30.0, 140.0);

            for cell in &cells {
                let y_top = price_to_y(cell.top);
                let y_bottom = price_to_y(cell.bottom);
                let y = y_top.min(y_bottom);
                let h = (y_top - y_bottom).abs().max(1.5);

                if y + h < region.y || y > region.y + region.height {
                    continue;
                }

                let ratio = (cell.strength / max_strength).clamp(0.01, 1.0);
                let (r, g, b, a) = interpolate_color_themed(ratio, is_dark);

                let bar_norm = (cell.strength / max_strength).clamp(0.05, 1.0);
                let bar_w = bar_norm * max_bar_width;
                let bar_x = right_edge_x - bar_w;

                let bar_color = iced::Color::from_rgba(r, g, b, (a * 0.85).clamp(0.20, 0.95));
                frame.fill_rectangle(Point::new(bar_x, y), Size::new(bar_w, h), bar_color);

                let border_color = iced::Color::from_rgba(r, g, b, 0.95);
                frame.fill_rectangle(Point::new(bar_x, y), Size::new(1.5, h), border_color);
            }
        }
    }
}

pub(super) fn draw_vwap_overlay(
    data_source: &PlotData<KlineDataPoint>,
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    earliest: u64,
    latest: u64,
) {
    let mut candles: Vec<(i64, f64, f64)> = Vec::new();
    match data_source {
        PlotData::TimeBased(ts) => {
            for (time, dp) in ts.datapoints.range(earliest..=latest) {
                let tp = (dp.kline.high.to_f32() + dp.kline.low.to_f32() + dp.kline.close.to_f32())
                    as f64
                    / 3.0;
                let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
                candles.push((*time as i64, tp, vol));
            }
        }
        PlotData::TickBased(ta) => {
            let start_idx = ta.datapoints.partition_point(|dp| dp.kline.time < earliest);
            let end_idx = ta.datapoints.partition_point(|dp| dp.kline.time <= latest);
            for dp in &ta.datapoints[start_idx..end_idx] {
                let tp = (dp.kline.high.to_f32() + dp.kline.low.to_f32() + dp.kline.close.to_f32())
                    as f64
                    / 3.0;
                let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
                candles.push((dp.kline.time as i64, tp, vol));
            }
        }
    }

    if candles.len() < 2 {
        return;
    }

    let points =
        data::chart::vwap::calculate_vwap_series(&candles, data::chart::vwap::VwapPeriod::Daily);
    let mut prev_pt: Option<(f32, f32, f32, f32, f32, f32)> = None;

    for (i, opt_pt) in points.iter().enumerate() {
        if let Some(pt) = opt_pt {
            let x = interval_to_x(candles[i].0 as u64);
            let y_vwap = price_to_y(Price::from_f32(pt.vwap as f32));
            let y_u1 = price_to_y(Price::from_f32(pt.upper_1sigma as f32));
            let y_l1 = price_to_y(Price::from_f32(pt.lower_1sigma as f32));
            let y_u2 = price_to_y(Price::from_f32(pt.upper_2sigma as f32));
            let y_l2 = price_to_y(Price::from_f32(pt.lower_2sigma as f32));

            if let Some((px, py_v, py_u1, py_l1, py_u2, py_l2)) = prev_pt {
                frame.stroke(
                    &Path::line(Point::new(px, py_v), Point::new(x, y_vwap)),
                    Stroke::default()
                        .with_color(iced::Color::from_rgb(1.0, 0.76, 0.03))
                        .with_width(1.5),
                );
                frame.stroke(
                    &Path::line(Point::new(px, py_u1), Point::new(x, y_u1)),
                    Stroke::default()
                        .with_color(iced::Color::from_rgba(0.13, 0.59, 0.95, 0.7))
                        .with_width(1.0),
                );
                frame.stroke(
                    &Path::line(Point::new(px, py_l1), Point::new(x, y_l1)),
                    Stroke::default()
                        .with_color(iced::Color::from_rgba(0.13, 0.59, 0.95, 0.7))
                        .with_width(1.0),
                );
                frame.stroke(
                    &Path::line(Point::new(px, py_u2), Point::new(x, y_u2)),
                    Stroke::default()
                        .with_color(iced::Color::from_rgba(0.61, 0.15, 0.69, 0.7))
                        .with_width(1.0),
                );
                frame.stroke(
                    &Path::line(Point::new(px, py_l2), Point::new(x, y_l2)),
                    Stroke::default()
                        .with_color(iced::Color::from_rgba(0.61, 0.15, 0.69, 0.7))
                        .with_width(1.0),
                );
            }

            prev_pt = Some((x, y_vwap, y_u1, y_l1, y_u2, y_l2));
        }
    }
}

pub(super) fn draw_rolling_vwap_overlay(
    rolling_indicator: Option<&crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>,
    data_source: &PlotData<KlineDataPoint>,
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    earliest: u64,
    latest: u64,
    config: &data::chart::kline::Config,
) {
    if !config.rolling_vwap_show_7d
        && !config.rolling_vwap_show_30d
        && !config.rolling_vwap_show_90d
        && !config.rolling_vwap_show_365d
    {
        return;
    }

    let mut prev_7d: Option<(f32, f32)> = None;
    let mut prev_30d: Option<(f32, f32)> = None;
    let mut prev_90d: Option<(f32, f32)> = None;
    let mut prev_365d: Option<(f32, f32)> = None;

    let rgb_7d = config.rolling_vwap_colors.d7_rgb();
    let rgb_30d = config.rolling_vwap_colors.d30_rgb();
    let rgb_90d = config.rolling_vwap_colors.d90_rgb();
    let rgb_365d = config.rolling_vwap_colors.d365_rgb();

    let stroke_7d = Stroke::default()
        .with_color(iced::Color::from_rgb8(rgb_7d[0], rgb_7d[1], rgb_7d[2]))
        .with_width(1.5);
    let stroke_30d = Stroke::default()
        .with_color(iced::Color::from_rgb8(rgb_30d[0], rgb_30d[1], rgb_30d[2]))
        .with_width(1.5);
    let stroke_90d = Stroke::default()
        .with_color(iced::Color::from_rgb8(rgb_90d[0], rgb_90d[1], rgb_90d[2]))
        .with_width(1.5);
    let stroke_365d = Stroke::default()
        .with_color(iced::Color::from_rgb8(
            rgb_365d[0],
            rgb_365d[1],
            rgb_365d[2],
        ))
        .with_width(1.5);

    let max_gap_ms: u64 = match data_source {
        PlotData::TimeBased(ts) => ts.interval.to_milliseconds().saturating_mul(3),
        PlotData::TickBased(_) => u64::MAX,
    };

    let use_cached = rolling_indicator
        .map(|ind| !ind.data.is_empty())
        .unwrap_or(false);

    if use_cached {
        let indicator = rolling_indicator.unwrap();
        let mut prev_time: Option<u64> = None;

        for (&time_u64, pt) in &indicator.data {
            if time_u64 > latest {
                break;
            }

            if prev_time.is_some_and(|last_t| time_u64.saturating_sub(last_t) > max_gap_ms) {
                prev_7d = None;
                prev_30d = None;
                prev_90d = None;
                prev_365d = None;
            }
            prev_time = Some(time_u64);

            let x = interval_to_x(time_u64);

            if config.rolling_vwap_show_7d {
                if let Some(d7) = pt.d7 {
                    let y = price_to_y(Price::from_f32(d7.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_7d
                    {
                        frame.stroke(&Path::line(Point::new(px, py), Point::new(x, y)), stroke_7d);
                    }
                    prev_7d = Some((x, y));
                } else {
                    prev_7d = None;
                }
            }

            if config.rolling_vwap_show_30d {
                if let Some(d30) = pt.d30 {
                    let y = price_to_y(Price::from_f32(d30.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_30d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_30d,
                        );
                    }
                    prev_30d = Some((x, y));
                } else {
                    prev_30d = None;
                }
            }

            if config.rolling_vwap_show_90d {
                if let Some(d90) = pt.d90 {
                    let y = price_to_y(Price::from_f32(d90.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_90d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_90d,
                        );
                    }
                    prev_90d = Some((x, y));
                } else {
                    prev_90d = None;
                }
            }

            if config.rolling_vwap_show_365d {
                if let Some(d365) = pt.d365 {
                    let y = price_to_y(Price::from_f32(d365.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_365d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_365d,
                        );
                    }
                    prev_365d = Some((x, y));
                } else {
                    prev_365d = None;
                }
            }
        }
    } else {
        let mut candles: Vec<(i64, f64, f64)> = Vec::new();
        const MAX_WINDOW_MS: i64 = 365 * 86_400_000;
        let fetch_earliest = (earliest as i64).saturating_sub(MAX_WINDOW_MS) as u64;

        match data_source {
            PlotData::TimeBased(ts) => {
                for (time, dp) in ts.datapoints.range(fetch_earliest..=latest) {
                    let tp = (dp.kline.high.to_f32()
                        + dp.kline.low.to_f32()
                        + dp.kline.close.to_f32()) as f64
                        / 3.0;
                    let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
                    candles.push((*time as i64, tp, vol));
                }
            }
            PlotData::TickBased(ta) => {
                let start_idx = ta
                    .datapoints
                    .partition_point(|dp| dp.kline.time < fetch_earliest);
                let end_idx = ta.datapoints.partition_point(|dp| dp.kline.time <= latest);
                for dp in &ta.datapoints[start_idx..end_idx] {
                    let tp = (dp.kline.high.to_f32()
                        + dp.kline.low.to_f32()
                        + dp.kline.close.to_f32()) as f64
                        / 3.0;
                    let vol = (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) as f64;
                    candles.push((dp.kline.time as i64, tp, vol));
                }
            }
        }

        if candles.len() < 2 {
            return;
        }

        let points = data::chart::vwap::calculate_multi_rolling_vwap_series(&candles);
        let mut prev_time: Option<u64> = None;

        for (i, pt) in points.iter().enumerate() {
            let time_u64 = candles[i].0 as u64;

            if prev_time.is_some_and(|last_t| time_u64.saturating_sub(last_t) > max_gap_ms) {
                prev_7d = None;
                prev_30d = None;
                prev_90d = None;
                prev_365d = None;
            }
            prev_time = Some(time_u64);

            let x = interval_to_x(time_u64);

            if config.rolling_vwap_show_7d {
                if let Some(d7) = pt.d7 {
                    let y = price_to_y(Price::from_f32(d7.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_7d
                    {
                        frame.stroke(&Path::line(Point::new(px, py), Point::new(x, y)), stroke_7d);
                    }
                    prev_7d = Some((x, y));
                } else {
                    prev_7d = None;
                }
            }

            if config.rolling_vwap_show_30d {
                if let Some(d30) = pt.d30 {
                    let y = price_to_y(Price::from_f32(d30.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_30d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_30d,
                        );
                    }
                    prev_30d = Some((x, y));
                } else {
                    prev_30d = None;
                }
            }

            if config.rolling_vwap_show_90d {
                if let Some(d90) = pt.d90 {
                    let y = price_to_y(Price::from_f32(d90.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_90d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_90d,
                        );
                    }
                    prev_90d = Some((x, y));
                } else {
                    prev_90d = None;
                }
            }

            if config.rolling_vwap_show_365d {
                if let Some(d365) = pt.d365 {
                    let y = price_to_y(Price::from_f32(d365.vwap as f32));
                    if time_u64 >= earliest
                        && let Some((px, py)) = prev_365d
                    {
                        frame.stroke(
                            &Path::line(Point::new(px, py), Point::new(x, y)),
                            stroke_365d,
                        );
                    }
                    prev_365d = Some((x, y));
                } else {
                    prev_365d = None;
                }
            }
        }
    }
}
