//! Footprint, candle and cluster drawing for the kline canvas.
//!
//! Split out of `chart/kline.rs` (ROADMAP #131); the code is unchanged apart from
//! `pub(super)` visibility.

use super::*;

pub(super) fn draw_footprint_kline(
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    x_position: f32,
    candle_width: f32,
    kline: &Kline,
    palette: &Extended,
) {
    let y_open = price_to_y(kline.open);
    let y_high = price_to_y(kline.high);
    let y_low = price_to_y(kline.low);
    let y_close = price_to_y(kline.close);

    let body_color = if kline.close >= kline.open {
        palette.success.weak.color
    } else {
        palette.danger.weak.color
    };
    frame.fill_rectangle(
        Point::new(x_position - (candle_width / 8.0), y_open.min(y_close)),
        Size::new(candle_width / 4.0, (y_open - y_close).abs()),
        body_color,
    );

    let wick_color = if kline.close >= kline.open {
        palette.success.weak.color
    } else {
        palette.danger.weak.color
    };
    let marker_line = Stroke::with_color(
        Stroke {
            width: 1.0,
            ..Default::default()
        },
        wick_color.scale_alpha(0.6),
    );
    frame.stroke(
        &Path::line(
            Point::new(x_position, y_high),
            Point::new(x_position, y_low),
        ),
        marker_line,
    );
}

pub(super) fn draw_candle_dp(
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    candle_width: f32,
    palette: &Extended,
    x_position: f32,
    kline: &Kline,
) {
    let y_open = price_to_y(kline.open);
    let y_high = price_to_y(kline.high);
    let y_low = price_to_y(kline.low);
    let y_close = price_to_y(kline.close);

    let body_color = if kline.close >= kline.open {
        palette.success.base.color
    } else {
        palette.danger.base.color
    };
    frame.fill_rectangle(
        Point::new(x_position - (candle_width / 2.0), y_open.min(y_close)),
        Size::new(candle_width, (y_open - y_close).abs()),
        body_color,
    );

    let wick_color = if kline.close >= kline.open {
        palette.success.base.color
    } else {
        palette.danger.base.color
    };
    frame.fill_rectangle(
        Point::new(x_position - (candle_width / 8.0), y_high),
        Size::new(candle_width / 4.0, (y_high - y_low).abs()),
        wick_color,
    );
}

pub(super) fn render_data_source<F>(
    data_source: &PlotData<KlineDataPoint>,
    frame: &mut canvas::Frame,
    earliest: u64,
    latest: u64,
    interval_to_x: impl Fn(u64) -> f32,
    draw_fn: F,
) where
    F: Fn(&mut canvas::Frame, f32, &Kline, &KlineTrades),
{
    match data_source {
        PlotData::TickBased(tick_aggr) => {
            let earliest = earliest as usize;
            let latest = latest as usize;

            tick_aggr
                .datapoints
                .iter()
                .rev()
                .enumerate()
                .filter(|(index, _)| *index <= latest && *index >= earliest)
                .for_each(|(index, tick_aggr)| {
                    let x_position = interval_to_x(index as u64);

                    draw_fn(frame, x_position, &tick_aggr.kline, &tick_aggr.footprint);
                });
        }
        PlotData::TimeBased(timeseries) => {
            if latest < earliest {
                return;
            }

            timeseries
                .datapoints
                .range(earliest..=latest)
                .for_each(|(timestamp, dp)| {
                    let x_position = interval_to_x(*timestamp);

                    draw_fn(frame, x_position, &dp.kline, &dp.footprint);
                });
        }
    }
}

pub(super) struct TableLayout {
    table_left: f32,
    table_right: f32,
    candle_center_x: f32,
}

impl TableLayout {
    fn new(
        content_left: f32,
        content_right: f32,
        candle_width: f32,
        spacing: ContentGaps,
        has_imbalance: bool,
    ) -> Self {
        let (candle_center_x, table_left) = if has_imbalance {
            let ccx = content_left + candle_width / 2.0;
            let tl = (content_left + candle_width + spacing.candle_to_cluster).min(content_right);
            (ccx, tl)
        } else {
            let thin_candle = candle_width * 0.25;
            let ccx = content_left + thin_candle / 2.0;
            let tl = (content_left + thin_candle + spacing.candle_to_cluster).min(content_right);
            (ccx, tl)
        };

        Self {
            table_left,
            table_right: content_right,
            candle_center_x,
        }
    }
}

pub(super) struct TableArea {
    table_left: f32,
    table_right: f32,
}

impl TableArea {
    fn new(
        frame: &mut canvas::Frame,
        price_to_y: &impl Fn(Price) -> f32,
        table_layout: &TableLayout,
        candle_width: f32,
        kline: &Kline,
        palette: &Extended,
    ) -> Self {
        draw_footprint_kline(
            frame,
            price_to_y,
            table_layout.candle_center_x,
            candle_width,
            kline,
            palette,
        );

        Self {
            table_left: table_layout.table_left,
            table_right: table_layout.table_right,
        }
    }

    fn width(&self) -> f32 {
        (self.table_right - self.table_left).max(0.0)
    }
}

#[derive(Clone, Copy)]
pub(super) enum ImbalanceSide {
    Buy,
    Sell,
}

impl ImbalanceSide {
    fn volume_bg_color(self, qty: f64, max_qty: f64, palette: &Extended) -> Color {
        const MIN_ALPHA: f32 = 0.04;

        let intensity = if max_qty > 0.0 {
            (qty / max_qty).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let alpha = MIN_ALPHA + intensity * (1.0 - MIN_ALPHA);

        match self {
            ImbalanceSide::Buy => palette.success.base.color.scale_alpha(alpha),
            ImbalanceSide::Sell => palette.danger.base.color.scale_alpha(alpha),
        }
    }

    fn volume_text_color(
        self,
        qty: f64,
        max_qty: f64,
        default_color: Color,
        palette: &Extended,
    ) -> Color {
        let cell_color = self.volume_bg_color(qty, max_qty, palette);
        let cell_background = composite_color(cell_color, palette.background.base.color);
        let inverted_color = palette.background.base.color;

        if contrast_ratio(cell_background, inverted_color)
            > contrast_ratio(cell_background, default_color)
        {
            inverted_color
        } else {
            default_color
        }
    }

    fn color_alpha(
        self,
        footprint: &KlineTrades,
        price: Price,
        qty: f64,
        step: PriceStep,
        threshold: usize,
        color_scale: Option<usize>,
        ignore_zeros: bool,
    ) -> Option<f32> {
        let diagonal_price = match self {
            ImbalanceSide::Buy => price.add_steps(-1, step),
            ImbalanceSide::Sell => price.add_steps(1, step),
        };
        let diagonal_qty = footprint
            .trades
            .get(&diagonal_price)
            .map(|group| match self {
                ImbalanceSide::Buy => group.sell_qty,
                ImbalanceSide::Sell => group.buy_qty,
            })
            .unwrap_or_default();

        if ignore_zeros && (qty <= 0.0 || diagonal_qty <= 0.0) {
            return None;
        }

        let required_qty = diagonal_qty * (100 + threshold) as f64 / 100.0;

        if required_qty <= 0.0 {
            return (qty > 0.0).then_some(1.0);
        }

        if qty <= required_qty {
            return None;
        }

        let ratio = qty / required_qty;
        Some(if let Some(scale) = color_scale {
            let divisor = (scale as f64 / 10.0) - 1.0;
            if divisor.abs() < 1e-4 {
                1.0
            } else {
                (0.2 + 0.8 * ((ratio - 1.0) / divisor).clamp(0.0, 1.0)).min(1.0) as f32
            }
        } else {
            1.0
        })
    }

    fn draw_table_marker(
        self,
        frame: &mut canvas::Frame,
        palette: &Extended,
        alpha: f32,
        qty: f64,
        max_qty: f64,
        cell: Rectangle,
    ) {
        if cell.height <= 0.0 {
            return;
        }

        let bar_width = 2.5;
        let gap = 1.5;

        let volume_intensity = if max_qty > 0.0 {
            (qty / max_qty).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let imbalance_strength = alpha.clamp(0.0, 1.0);
        let marker_alpha = 0.38 + (volume_intensity * 0.24) + (imbalance_strength * 0.38);
        let marker_alpha = marker_alpha.clamp(0.38, 1.0);

        let color = palette.warning.strong.color.scale_alpha(marker_alpha);

        let (x, bar_w) = match self {
            ImbalanceSide::Sell => (cell.x - bar_width - gap, bar_width),
            ImbalanceSide::Buy => (cell.x + cell.width + gap, bar_width),
        };

        frame.fill_rectangle(Point::new(x, cell.y), Size::new(bar_w, cell.height), color);
    }
}

pub(super) struct FootprintSummaryLayout {
    text_size: f32,
    gap: f32,
    line_gap: f32,
}

impl FootprintSummaryLayout {
    fn new(cell_height: f32, scaling: f32) -> FootprintSummaryLayout {
        const MIN_SCREEN_CELL_H_PX: f32 = 6.0;
        const MIN_TEXT_SIZE_PX: f32 = 3.0;
        const SUMMARY_GAP_PX: f32 = 8.0;
        const SUMMARY_LINE_GAP_PX: f32 = 2.0;

        let max_text_size = 9.0;
        let screen_cell_h = cell_height * scaling;
        let text_size = if screen_cell_h < MIN_SCREEN_CELL_H_PX {
            (max_text_size * (screen_cell_h / MIN_SCREEN_CELL_H_PX)).max(MIN_TEXT_SIZE_PX)
        } else {
            max_text_size
        };

        let gap = SUMMARY_GAP_PX / scaling;
        let line_gap = SUMMARY_LINE_GAP_PX / scaling;

        FootprintSummaryLayout {
            text_size,
            gap,
            line_gap,
        }
    }
}

pub(super) fn draw_all_npocs(
    data_source: &PlotData<KlineDataPoint>,
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    candle_width: f32,
    cell_width: f32,
    cell_height: f32,
    palette: &Extended,
    studies: &[FootprintStudy],
    visible_earliest: u64,
    visible_latest: u64,
    cluster_kind: ClusterKind,
    spacing: ContentGaps,
    imb_study_on: bool,
) {
    let Some(lookback) = studies.iter().find_map(|study| {
        if let FootprintStudy::NPoC { lookback } = study {
            Some(*lookback)
        } else {
            None
        }
    }) else {
        return;
    };

    let (filled_color, naked_color) = (
        palette.background.strong.color,
        if palette.is_dark {
            palette.warning.weak.color.scale_alpha(0.5)
        } else {
            palette.warning.strong.color
        },
    );

    let line_height = cell_height.min(1.0);

    let bar_width_factor: f32 = 0.9;
    let inset = (cell_width * (1.0 - bar_width_factor)) / 2.0;

    let candle_lane_factor: f32 = match cluster_kind {
        ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => 0.25,
        ClusterKind::BidAsk | ClusterKind::Table => 1.0,
    };

    let start_x_for = |cell_center_x: f32| -> f32 {
        match cluster_kind {
            ClusterKind::Table => cell_center_x + (cell_width / 2.0) - inset,
            ClusterKind::BidAsk => cell_center_x + (candle_width / 2.0) + spacing.candle_to_cluster,
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
                let content_left = (cell_center_x - (cell_width / 2.0)) + inset;
                let candle_lane_left = content_left
                    + if imb_study_on {
                        candle_width + spacing.marker_to_candle
                    } else {
                        0.0
                    };
                candle_lane_left + candle_width * candle_lane_factor + spacing.candle_to_cluster
            }
        }
    };

    let wick_x_for = |cell_center_x: f32| -> f32 {
        match cluster_kind {
            ClusterKind::BidAsk | ClusterKind::Table => cell_center_x, // not used for BidAsk clustering
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
                let content_left = (cell_center_x - (cell_width / 2.0)) + inset;
                let candle_lane_left = content_left
                    + if imb_study_on {
                        candle_width + spacing.marker_to_candle
                    } else {
                        0.0
                    };
                candle_lane_left + (candle_width * candle_lane_factor) / 2.0
                    - (spacing.candle_to_cluster * 0.5)
            }
        }
    };

    let end_x_for = |cell_center_x: f32| -> f32 {
        match cluster_kind {
            ClusterKind::Table => {
                let content_left = cell_center_x - (cell_width / 2.0) + inset;
                let content_right = cell_center_x + (cell_width / 2.0) - inset;
                let table_layout = TableLayout::new(
                    content_left,
                    content_right,
                    candle_width,
                    spacing,
                    imb_study_on,
                );
                table_layout.table_left
            }
            ClusterKind::BidAsk => cell_center_x - (candle_width / 2.0) - spacing.candle_to_cluster,
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => wick_x_for(cell_center_x),
        }
    };

    let rightmost_cell_center_x = {
        let earliest_x = interval_to_x(visible_earliest);
        let latest_x = interval_to_x(visible_latest);
        if earliest_x > latest_x {
            earliest_x
        } else {
            latest_x
        }
    };

    let mut draw_the_line = |interval: u64, poc: &PointOfControl| {
        let start_x = start_x_for(interval_to_x(interval));

        let (line_width, color) = match poc.status {
            NPoc::Naked => {
                let end_x = end_x_for(rightmost_cell_center_x);
                let line_width = end_x - start_x;
                if line_width.abs() <= cell_width {
                    return;
                }
                (line_width, naked_color)
            }
            NPoc::Filled { at } => {
                let end_x = end_x_for(interval_to_x(at));
                let line_width = end_x - start_x;
                if line_width.abs() <= cell_width {
                    return;
                }
                (line_width, filled_color)
            }
            _ => return,
        };

        frame.fill_rectangle(
            Point::new(start_x, price_to_y(poc.price) - line_height / 2.0),
            Size::new(line_width, line_height),
            color,
        );
    };

    match data_source {
        PlotData::TickBased(tick_aggr) => {
            tick_aggr
                .datapoints
                .iter()
                .rev()
                .enumerate()
                .take(lookback)
                .filter_map(|(index, dp)| dp.footprint.poc.as_ref().map(|poc| (index as u64, poc)))
                .for_each(|(interval, poc)| draw_the_line(interval, poc));
        }
        PlotData::TimeBased(timeseries) => {
            timeseries
                .datapoints
                .iter()
                .rev()
                .take(lookback)
                .filter_map(|(timestamp, dp)| {
                    dp.footprint.poc.as_ref().map(|poc| (*timestamp, poc))
                })
                .for_each(|(interval, poc)| draw_the_line(interval, poc));
        }
    }
}

pub(super) fn effective_cluster_qty(
    scaling: ClusterScaling,
    visible_max: f32,
    footprint: &KlineTrades,
    cluster_kind: ClusterKind,
) -> f32 {
    let individual_max = match cluster_kind {
        ClusterKind::BidAsk | ClusterKind::Table => footprint
            .trades
            .values()
            .map(|group| (group.buy_qty as f32).max(group.sell_qty as f32))
            .fold(0.0_f32, f32::max),
        ClusterKind::DeltaProfile => footprint
            .trades
            .values()
            .map(|group| group.delta_qty().abs())
            .fold(0.0_f32, f32::max),
        ClusterKind::VolumeProfile => footprint
            .trades
            .values()
            .map(|group| group.total_qty())
            .fold(0.0_f32, f32::max),
    };

    let safe = |v: f32| if v <= f32::EPSILON { 1.0 } else { v };

    match scaling {
        ClusterScaling::VisibleRange => safe(visible_max),
        ClusterScaling::Datapoint => safe(individual_max),
        ClusterScaling::Hybrid { weight } => {
            let w = weight.clamp(0.0, 1.0);
            safe(visible_max * w + individual_max * (1.0 - w))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_clusters(
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    x_position: f32,
    cell_width: f32,
    cell_height: f32,
    scaling: f32,
    candle_width: f32,
    max_cluster_qty: f32,
    palette: &Extended,
    text_size: f32,
    tick_size: f32,
    show_text: bool,
    imbalance: Option<(usize, Option<usize>, bool)>,
    kline: &Kline,
    footprint: &KlineTrades,
    cluster_kind: ClusterKind,
    spacing: ContentGaps,
    has_npoc: bool,
    show_footprint_summary: bool,
) {
    let text_color = palette.background.weakest.text;

    let bar_width_factor: f32 = 0.9;
    let inset = (cell_width * (1.0 - bar_width_factor)) / 2.0;

    let cell_left = x_position - (cell_width / 2.0);
    let content_left = cell_left + inset;
    let content_right = x_position + (cell_width / 2.0) - inset;

    if footprint.trades.is_empty() {
        let area_candle_center_x = match cluster_kind {
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
                ProfileArea::new(
                    content_left,
                    content_right,
                    candle_width,
                    spacing,
                    imbalance.is_some(),
                )
                .candle_center_x
            }
            ClusterKind::BidAsk => {
                BidAskArea::new(
                    x_position,
                    content_left,
                    content_right,
                    candle_width,
                    spacing,
                )
                .candle_center_x
            }
            ClusterKind::Table => {
                TableLayout::new(
                    content_left,
                    content_right,
                    candle_width,
                    spacing,
                    imbalance.is_some(),
                )
                .candle_center_x
            }
        };
        draw_footprint_kline(
            frame,
            &price_to_y,
            area_candle_center_x,
            candle_width,
            kline,
            palette,
        );
        return;
    }

    let step = PriceStep::from_f32(tick_size);
    let step_units = step.units.max(1);

    let (dense_min, dense_max) = {
        let mut min_p = Price { units: i64::MAX };
        let mut max_p = Price { units: i64::MIN };
        for &p in footprint.trades.keys() {
            min_p = min_p.min(p);
            max_p = max_p.max(p);
        }
        (min_p, max_p)
    };

    let dense_bin_count = if dense_max.units >= dense_min.units {
        (dense_max.units - dense_min.units) / step_units
    } else {
        0
    };

    let mut table_layout: Option<TableLayout> = None;

    match cluster_kind {
        ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
            let area = ProfileArea::new(
                content_left,
                content_right,
                candle_width,
                spacing,
                imbalance.is_some(),
            );
            let bar_alpha = if show_text { 0.25 } else { 1.0 };

            let mut process_level = |price: Price, group: Option<&GroupedTrades>| {
                let Some(g) = group else {
                    return;
                };
                if g.buy_qty <= 0.0 && g.sell_qty <= 0.0 {
                    return;
                }
                let y = price_to_y(price);

                match cluster_kind {
                    ClusterKind::VolumeProfile => {
                        let (buy_qty, sell_qty, total_qty) =
                            (g.buy_qty as f32, g.sell_qty as f32, g.total_qty());

                        if total_qty > 0.0 {
                            super::super::draw_volume_bar(
                                frame,
                                area.bars_left,
                                y,
                                buy_qty,
                                sell_qty,
                                max_cluster_qty,
                                area.bars_width,
                                cell_height,
                                palette.success.base.color,
                                palette.danger.base.color,
                                bar_alpha,
                                true,
                            );
                        }

                        if show_text && total_qty > 0.0 {
                            let text_str = abbr_large_numbers(total_qty);
                            let fit_size = if !text_str.is_empty() {
                                let max_char_w = (area.bars_width * 0.90) / (text_str.len() as f32);
                                text_size.min(max_char_w / 0.60)
                            } else {
                                text_size
                            };
                            draw_cluster_text(
                                frame,
                                &text_str,
                                Point::new(area.bars_left + 2.0, y),
                                fit_size,
                                text_color,
                                Alignment::Start,
                                Alignment::Center,
                            );
                        }
                    }
                    ClusterKind::DeltaProfile => {
                        let delta = g.delta_qty();
                        if show_text {
                            let text_str = abbr_large_numbers(delta);
                            let fit_size = if !text_str.is_empty() {
                                let max_char_w = (area.bars_width * 0.90) / (text_str.len() as f32);
                                text_size.min(max_char_w / 0.60)
                            } else {
                                text_size
                            };
                            draw_cluster_text(
                                frame,
                                &text_str,
                                Point::new(area.bars_left + 2.0, y),
                                fit_size,
                                text_color,
                                Alignment::Start,
                                Alignment::Center,
                            );
                        }

                        if max_cluster_qty > 0.0 {
                            let bar_width = ((delta.abs() / max_cluster_qty) * area.bars_width)
                                .min(area.bars_width);
                            if bar_width > 0.0 && bar_width.is_finite() {
                                let color = if delta >= 0.0 {
                                    palette.success.base.color.scale_alpha(bar_alpha)
                                } else {
                                    palette.danger.base.color.scale_alpha(bar_alpha)
                                };
                                frame.fill_rectangle(
                                    Point::new(area.bars_left, y - (cell_height / 2.0)),
                                    Size::new(bar_width, cell_height),
                                    color,
                                );
                            }
                        }
                    }
                    _ => {}
                }

                if let Some((threshold, color_scale, ignore_zeros)) = imbalance {
                    let higher_price =
                        Price::from_f32(price.to_f32() + tick_size).round_to_step(step);

                    let rect_w = ((area.imb_marker_width - 1.0) / 2.0).max(1.0);
                    let buyside_x = area.imb_marker_left + area.imb_marker_width - rect_w;
                    let sellside_x =
                        area.imb_marker_left + area.imb_marker_width - (2.0 * rect_w) - 1.0;

                    draw_imbalance_markers(
                        frame,
                        &price_to_y,
                        footprint,
                        price,
                        g.sell_qty as f32,
                        higher_price,
                        threshold,
                        color_scale,
                        ignore_zeros,
                        cell_height,
                        palette,
                        buyside_x,
                        sellside_x,
                        rect_w,
                    );
                }
            };

            if dense_bin_count <= 2000 && dense_min.units <= dense_max.units {
                let mut curr_units = dense_min.units;
                while curr_units <= dense_max.units {
                    let p = Price { units: curr_units };
                    process_level(p, footprint.trades.get(&p));
                    curr_units = curr_units.saturating_add(step_units);
                }
            } else {
                for (price, group) in &footprint.trades {
                    process_level(*price, Some(group));
                }
            }

            draw_footprint_kline(
                frame,
                &price_to_y,
                area.candle_center_x,
                candle_width,
                kline,
                palette,
            );
        }
        ClusterKind::BidAsk => {
            let area = BidAskArea::new(
                x_position,
                content_left,
                content_right,
                candle_width,
                spacing,
            );

            let bar_alpha = if show_text { 0.25 } else { 1.0 };

            let imb_marker_reserve = if imbalance.is_some() {
                ((area.imb_marker_width - 1.0) / 2.0).max(1.0)
            } else {
                0.0
            };

            let right_max_x =
                area.bid_area_right - imb_marker_reserve - (2.0 * spacing.marker_to_bars);
            let right_area_width = (right_max_x - area.bid_area_left).max(0.0);

            let left_min_x =
                area.ask_area_left + imb_marker_reserve + (2.0 * spacing.marker_to_bars);
            let left_area_width = (area.ask_area_right - left_min_x).max(0.0);

            let mut process_level = |price: Price, group: Option<&GroupedTrades>| {
                let Some(g) = group else {
                    return;
                };
                if g.buy_qty <= 0.0 && g.sell_qty <= 0.0 {
                    return;
                }
                let y = price_to_y(price);
                let (buy_qty, sell_qty) = (g.buy_qty as f32, g.sell_qty as f32);

                if right_area_width > 0.0 {
                    if show_text {
                        let text_str = abbr_large_numbers(buy_qty);
                        let fit_size = if !text_str.is_empty() {
                            let max_char_w = (right_area_width * 0.90) / (text_str.len() as f32);
                            text_size.min(max_char_w / 0.60)
                        } else {
                            text_size
                        };
                        draw_cluster_text(
                            frame,
                            &text_str,
                            Point::new(area.bid_area_left + 2.0, y),
                            fit_size,
                            text_color,
                            Alignment::Start,
                            Alignment::Center,
                        );
                    }

                    if max_cluster_qty > 0.0 {
                        let bar_width =
                            ((buy_qty / max_cluster_qty) * right_area_width).min(right_area_width);
                        if bar_width > 0.0 && bar_width.is_finite() {
                            frame.fill_rectangle(
                                Point::new(area.bid_area_left, y - (cell_height / 2.0)),
                                Size::new(bar_width, cell_height),
                                palette.success.base.color.scale_alpha(bar_alpha),
                            );
                        }
                    }
                }
                if left_area_width > 0.0 {
                    if show_text {
                        let text_str = abbr_large_numbers(sell_qty);
                        let fit_size = if !text_str.is_empty() {
                            let max_char_w = (left_area_width * 0.90) / (text_str.len() as f32);
                            text_size.min(max_char_w / 0.60)
                        } else {
                            text_size
                        };
                        draw_cluster_text(
                            frame,
                            &text_str,
                            Point::new(area.ask_area_right - 2.0, y),
                            fit_size,
                            text_color,
                            Alignment::End,
                            Alignment::Center,
                        );
                    }

                    if max_cluster_qty > 0.0 {
                        let bar_width =
                            ((sell_qty / max_cluster_qty) * left_area_width).min(left_area_width);
                        if bar_width > 0.0 && bar_width.is_finite() {
                            let left = (area.ask_area_right - bar_width).max(left_min_x);
                            frame.fill_rectangle(
                                Point::new(left, y - (cell_height / 2.0)),
                                Size::new(bar_width, cell_height),
                                palette.danger.base.color.scale_alpha(bar_alpha),
                            );
                        }
                    }
                }

                if let Some((threshold, color_scale, ignore_zeros)) = imbalance
                    && area.imb_marker_width > 0.0
                {
                    let higher_price =
                        Price::from_f32(price.to_f32() + tick_size).round_to_step(step);

                    let rect_width = ((area.imb_marker_width - 1.0) / 2.0).max(1.0);

                    let buyside_x = area.bid_area_right - rect_width - spacing.marker_to_bars;
                    let sellside_x = area.ask_area_left + spacing.marker_to_bars;

                    draw_imbalance_markers(
                        frame,
                        &price_to_y,
                        footprint,
                        price,
                        g.sell_qty as f32,
                        higher_price,
                        threshold,
                        color_scale,
                        ignore_zeros,
                        cell_height,
                        palette,
                        buyside_x,
                        sellside_x,
                        rect_width,
                    );
                }
            };

            if dense_bin_count <= 2000 && dense_min.units <= dense_max.units {
                let mut curr_units = dense_min.units;
                while curr_units <= dense_max.units {
                    let p = Price { units: curr_units };
                    process_level(p, footprint.trades.get(&p));
                    curr_units = curr_units.saturating_add(step_units);
                }
            } else {
                for (price, group) in &footprint.trades {
                    process_level(*price, Some(group));
                }
            }

            draw_footprint_kline(
                frame,
                &price_to_y,
                area.candle_center_x,
                candle_width,
                kline,
                palette,
            );
        }
        ClusterKind::Table => {
            let tl = TableLayout::new(
                content_left,
                content_right,
                candle_width,
                spacing,
                imbalance.is_some(),
            );
            let area = TableArea::new(frame, &price_to_y, &tl, candle_width, kline, palette);
            table_layout = Some(tl);
            let table_width = area.width();
            let half_width = table_width / 2.0;
            let cell_border = 1.0;
            let grid_color = palette.background.weakest.text.scale_alpha(0.32);

            let mut process_level = |price: Price, group: Option<&GroupedTrades>| {
                let Some(g) = group else {
                    return;
                };
                if g.buy_qty <= 0.0 && g.sell_qty <= 0.0 {
                    return;
                }
                let buy_qty = g.buy_qty;
                let sell_qty = g.sell_qty;
                let y = price_to_y(price);
                let row_top = y - (cell_height / 2.0);

                frame.fill_rectangle(
                    Point::new(area.table_left, row_top),
                    Size::new(half_width, cell_height),
                    ImbalanceSide::Sell.volume_bg_color(sell_qty, max_cluster_qty as f64, palette),
                );
                frame.fill_rectangle(
                    Point::new(area.table_left + half_width, row_top),
                    Size::new(half_width, cell_height),
                    ImbalanceSide::Buy.volume_bg_color(buy_qty, max_cluster_qty as f64, palette),
                );
                let sell_text_color = ImbalanceSide::Sell.volume_text_color(
                    sell_qty,
                    max_cluster_qty as f64,
                    text_color,
                    palette,
                );
                let buy_text_color = ImbalanceSide::Buy.volume_text_color(
                    buy_qty,
                    max_cluster_qty as f64,
                    text_color,
                    palette,
                );

                if let Some((threshold, color_scale, ignore_zeros)) = imbalance {
                    if let Some(alpha) = ImbalanceSide::Sell.color_alpha(
                        footprint,
                        price,
                        sell_qty,
                        step,
                        threshold,
                        color_scale,
                        ignore_zeros,
                    ) {
                        ImbalanceSide::Sell.draw_table_marker(
                            frame,
                            palette,
                            alpha,
                            sell_qty,
                            max_cluster_qty as f64,
                            Rectangle::new(
                                Point::new(area.table_left, row_top),
                                Size::new(half_width, cell_height),
                            ),
                        );
                    }

                    if let Some(alpha) = ImbalanceSide::Buy.color_alpha(
                        footprint,
                        price,
                        buy_qty,
                        step,
                        threshold,
                        color_scale,
                        ignore_zeros,
                    ) {
                        ImbalanceSide::Buy.draw_table_marker(
                            frame,
                            palette,
                            alpha,
                            buy_qty,
                            max_cluster_qty as f64,
                            Rectangle::new(
                                Point::new(area.table_left + half_width, row_top),
                                Size::new(half_width, cell_height),
                            ),
                        );
                    }
                }

                frame.fill_rectangle(
                    Point::new(area.table_left, row_top),
                    Size::new(table_width, cell_border),
                    grid_color,
                );
                frame.fill_rectangle(
                    Point::new(area.table_left, row_top + cell_height - cell_border),
                    Size::new(table_width, cell_border),
                    grid_color,
                );
                frame.fill_rectangle(
                    Point::new(area.table_left, row_top),
                    Size::new(cell_border, cell_height),
                    grid_color,
                );
                frame.fill_rectangle(
                    Point::new(area.table_left + half_width, row_top),
                    Size::new(cell_border, cell_height),
                    grid_color,
                );
                frame.fill_rectangle(
                    Point::new(area.table_right - cell_border, row_top),
                    Size::new(cell_border, cell_height),
                    grid_color,
                );

                if show_text {
                    draw_cluster_text(
                        frame,
                        &abbr_large_numbers(g.sell_qty as f32),
                        Point::new(area.table_left + half_width - 3.0, y),
                        text_size,
                        sell_text_color,
                        Alignment::End,
                        Alignment::Center,
                    );
                    draw_cluster_text(
                        frame,
                        &abbr_large_numbers(g.buy_qty as f32),
                        Point::new(area.table_left + half_width + 3.0, y),
                        text_size,
                        buy_text_color,
                        Alignment::Start,
                        Alignment::Center,
                    );
                }
            };

            if dense_bin_count <= 2000 && dense_min.units <= dense_max.units {
                let mut curr_units = dense_min.units;
                while curr_units <= dense_max.units {
                    let p = Price { units: curr_units };
                    process_level(p, footprint.trades.get(&p));
                    curr_units = curr_units.saturating_add(step_units);
                }
            } else {
                for (price, group) in &footprint.trades {
                    process_level(*price, Some(group));
                }
            }
        }
    }

    // Candle Point of Control (POC) highlight & label when Full LOD is active AND NPoC study is enabled
    if has_npoc
        && show_text
        && let Some(ref poc) = footprint.poc
    {
        let y_poc = price_to_y(poc.price);
        let poc_color = iced::Color::from_rgb(0.98, 0.74, 0.18);
        let poc_border_w = (1.5 / scaling).max(1.0);

        frame.stroke(
            &Path::rectangle(
                Point::new(content_left, y_poc - cell_height / 2.0),
                Size::new(content_right - content_left, cell_height),
            ),
            Stroke::with_color(
                Stroke {
                    width: poc_border_w,
                    ..Default::default()
                },
                poc_color,
            ),
        );

        let poc_badge_size = (7.0 / scaling).clamp(4.5, 9.0);
        let poc_label_pos = match cluster_kind {
            ClusterKind::BidAsk | ClusterKind::Table => Point::new(x_position, y_poc),
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
                Point::new(content_right - 2.0, y_poc)
            }
        };
        let poc_align_x = match cluster_kind {
            ClusterKind::BidAsk | ClusterKind::Table => Alignment::Center,
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => Alignment::End,
        };
        draw_cluster_text(
            frame,
            "POC",
            poc_label_pos,
            poc_badge_size,
            poc_color,
            poc_align_x,
            Alignment::Center,
        );
    }

    if show_footprint_summary && let Some(summary) = FootprintSummary::from_trades(footprint) {
        let summary_layout = FootprintSummaryLayout::new(cell_height, scaling);
        let summary_x = match cluster_kind {
            ClusterKind::Table => {
                if let Some(ref tl) = table_layout {
                    (tl.table_left + tl.table_right) / 2.0
                } else {
                    x_position
                }
            }
            _ => x_position,
        };
        let lowest_trade_price = footprint.trades.keys().min();
        let summary_y = match lowest_trade_price {
            Some(p) => price_to_y(*p) + cell_height / 2.0 + summary_layout.gap,
            None => price_to_y(kline.low) + cell_height / 2.0 + summary_layout.gap,
        };

        draw_cluster_text(
            frame,
            &format!("V: {}", abbr_large_numbers(summary.total)),
            Point::new(summary_x, summary_y),
            summary_layout.text_size,
            palette.background.weakest.text,
            Alignment::Center,
            Alignment::Start,
        );

        let delta_color = if summary.delta >= 0.0 {
            palette.success.base.color
        } else {
            palette.danger.base.color
        };

        draw_cluster_text(
            frame,
            &format!("Δ: {}", abbr_large_numbers(summary.delta)),
            Point::new(
                summary_x,
                summary_y + summary_layout.text_size + summary_layout.line_gap,
            ),
            summary_layout.text_size,
            delta_color,
            Alignment::Center,
            Alignment::Start,
        );
    }
}

pub(super) fn draw_cluster_search_highlights(
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    x_position: f32,
    cell_width: f32,
    cell_height: f32,
    scaling: f32,
    footprint: &KlineTrades,
    min_volume: f32,
    min_delta: f32,
    side: data::chart::kline::ClusterSearchSide,
    style: data::chart::kline::HighlightStyle,
    color: data::chart::kline::HighlightColor,
) {
    use data::chart::kline::{ClusterSearchSide, HighlightStyle};

    let rgb = color.to_rgb();
    let base_color = iced::Color::from_rgb(rgb[0], rgb[1], rgb[2]);
    let is_thin = cell_width < (25.0 / scaling);

    for (price, group) in &footprint.trades {
        let total_vol = group.total_qty();
        let delta = group.delta_qty();

        if min_volume > 0.0 && total_vol < min_volume {
            continue;
        }
        if min_delta > 0.0 && delta.abs() < min_delta {
            continue;
        }

        let side_ok = match side {
            ClusterSearchSide::Both => true,
            ClusterSearchSide::BuyOnly => delta > 0.0,
            ClusterSearchSide::SellOnly => delta < 0.0,
        };
        if !side_ok {
            continue;
        }

        let y = price_to_y(*price);

        // Volumetric scaling factor: harmonious scaling with anomaly magnitude
        let threshold_ref = if min_volume > 0.0 {
            min_volume
        } else if min_delta > 0.0 {
            min_delta
        } else {
            100.0
        };
        let anomaly_metric = if min_volume > 0.0 {
            total_vol
        } else {
            delta.abs()
        };
        let vol_ratio = (anomaly_metric / threshold_ref.max(1.0)).max(1.0);
        let scale_factor = vol_ratio.sqrt().clamp(1.0, 1.5);
        let border_w = if is_thin {
            1.0
        } else {
            (1.2 / scaling.sqrt()).clamp(1.0, 1.5)
        };

        match style {
            HighlightStyle::Border => {
                let (rect_x, rect_y, rect_w, rect_h) = if is_thin {
                    let box_w = (cell_width * 2.0).clamp(10.0 / scaling, 28.0 / scaling);
                    let box_h = cell_height.clamp(4.0 / scaling, 16.0 / scaling);
                    (x_position - box_w / 2.0, y - box_h / 2.0, box_w, box_h)
                } else {
                    let inset = border_w / 2.0;
                    (
                        x_position - cell_width / 2.0 + inset,
                        y - cell_height / 2.0 + inset,
                        (cell_width - 2.0 * inset).max(1.0),
                        (cell_height - 2.0 * inset).max(1.0),
                    )
                };
                frame.stroke(
                    &Path::rectangle(Point::new(rect_x, rect_y), Size::new(rect_w, rect_h)),
                    Stroke::with_color(
                        Stroke {
                            width: border_w,
                            ..Default::default()
                        },
                        base_color,
                    ),
                );
            }
            HighlightStyle::Fill => {
                let (rect_x, rect_y, rect_w, rect_h, alpha) = if is_thin {
                    let box_w = (cell_width * 2.0).clamp(10.0 / scaling, 28.0 / scaling);
                    let box_h = cell_height.clamp(4.0 / scaling, 16.0 / scaling);
                    (
                        x_position - box_w / 2.0,
                        y - box_h / 2.0,
                        box_w,
                        box_h,
                        0.85,
                    )
                } else {
                    let inset = border_w / 2.0;
                    (
                        x_position - cell_width / 2.0 + inset,
                        y - cell_height / 2.0 + inset,
                        (cell_width - 2.0 * inset).max(1.0),
                        (cell_height - 2.0 * inset).max(1.0),
                        0.35,
                    )
                };
                frame.fill_rectangle(
                    Point::new(rect_x, rect_y),
                    Size::new(rect_w, rect_h),
                    base_color.scale_alpha(alpha),
                );
                frame.stroke(
                    &Path::rectangle(Point::new(rect_x, rect_y), Size::new(rect_w, rect_h)),
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.90),
                    ),
                );
            }
            HighlightStyle::Circle => {
                let base_r = (cell_height * 0.75).clamp(8.0, 14.0);
                let radius = (base_r * scale_factor).clamp(7.5, 18.0);
                let circle_path = Path::circle(Point::new(x_position, y), radius);
                frame.fill(&circle_path, base_color.scale_alpha(0.35));
                frame.stroke(
                    &circle_path,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Ring => {
                let base_r = (cell_height * 0.75).clamp(8.0, 14.0);
                let radius = (base_r * scale_factor).clamp(7.5, 18.0);
                let ring_path = Path::circle(Point::new(x_position, y), radius);
                frame.stroke(
                    &ring_path,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Concentric => {
                let base_r = (cell_height * 0.70).clamp(7.0, 13.0);
                let inner_radius = (base_r * scale_factor).clamp(6.5, 16.0);
                let inner_path = Path::circle(Point::new(x_position, y), inner_radius);
                frame.fill(&inner_path, base_color.scale_alpha(0.35));
                frame.stroke(
                    &inner_path,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );

                let outer_radius = inner_radius + 4.5;
                let outer_path = Path::circle(Point::new(x_position, y), outer_radius);
                frame.stroke(
                    &outer_path,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.85),
                    ),
                );
            }
            HighlightStyle::Triangle => {
                let base_size = (cell_height * 0.75).clamp(8.0, 14.0);
                let half_size = (base_size * scale_factor).clamp(7.5, 18.0);
                let tri_path = {
                    let mut builder = canvas::path::Builder::new();
                    if delta >= 0.0 {
                        // Pointing UP
                        builder.move_to(Point::new(x_position, y - half_size));
                        builder.line_to(Point::new(
                            x_position + half_size * 0.866,
                            y + half_size * 0.5,
                        ));
                        builder.line_to(Point::new(
                            x_position - half_size * 0.866,
                            y + half_size * 0.5,
                        ));
                    } else {
                        // Pointing DOWN
                        builder.move_to(Point::new(x_position, y + half_size));
                        builder.line_to(Point::new(
                            x_position + half_size * 0.866,
                            y - half_size * 0.5,
                        ));
                        builder.line_to(Point::new(
                            x_position - half_size * 0.866,
                            y - half_size * 0.5,
                        ));
                    }
                    builder.close();
                    builder.build()
                };
                frame.fill(&tri_path, base_color.scale_alpha(0.35));
                frame.stroke(
                    &tri_path,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Square => {
                let base_size = (cell_height * 0.70).clamp(7.0, 13.0);
                let half_size = (base_size * scale_factor).clamp(6.5, 16.0);
                let rect = Path::rectangle(
                    Point::new(x_position - half_size, y - half_size),
                    Size::new(half_size * 2.0, half_size * 2.0),
                );
                frame.fill(&rect, base_color.scale_alpha(0.35));
                frame.stroke(
                    &rect,
                    Stroke::with_color(
                        Stroke {
                            width: 1.0,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::ClassicCircle => {
                let radius = (cell_height * 0.40).clamp(3.5 / scaling, 8.0 / scaling);
                let center_x = if is_thin {
                    x_position
                } else if delta >= 0.0 {
                    x_position + cell_width / 2.0 - radius - (2.0 / scaling)
                } else {
                    x_position - cell_width / 2.0 + radius + (2.0 / scaling)
                };
                let circle_path = Path::circle(Point::new(center_x, y), radius);
                frame.fill(&circle_path, base_color);
            }
            HighlightStyle::ClassicTriangle => {
                let half_size = (cell_height * 0.42).clamp(3.5 / scaling, 8.0 / scaling);
                let center_x = if is_thin {
                    x_position
                } else if delta >= 0.0 {
                    x_position + cell_width / 2.0 - half_size - (2.0 / scaling)
                } else {
                    x_position - cell_width / 2.0 + half_size + (2.0 / scaling)
                };
                let tri_path = {
                    let mut builder = canvas::path::Builder::new();
                    if delta >= 0.0 {
                        builder.move_to(Point::new(center_x, y - half_size));
                        builder.line_to(Point::new(center_x + half_size, y + half_size));
                        builder.line_to(Point::new(center_x - half_size, y + half_size));
                    } else {
                        builder.move_to(Point::new(center_x, y + half_size));
                        builder.line_to(Point::new(center_x + half_size, y - half_size));
                        builder.line_to(Point::new(center_x - half_size, y - half_size));
                    }
                    builder.close();
                    builder.build()
                };
                frame.fill(&tri_path, base_color);
            }
            HighlightStyle::ClassicSquare => {
                let half_size = (cell_height * 0.38).clamp(3.0 / scaling, 7.0 / scaling);
                let center_x = if is_thin {
                    x_position
                } else if delta >= 0.0 {
                    x_position + cell_width / 2.0 - half_size - (2.0 / scaling)
                } else {
                    x_position - cell_width / 2.0 + half_size + (2.0 / scaling)
                };
                frame.fill_rectangle(
                    Point::new(center_x - half_size, y - half_size),
                    Size::new(half_size * 2.0, half_size * 2.0),
                    base_color,
                );
            }
        }
    }
}

pub(super) fn draw_imbalance_markers(
    frame: &mut canvas::Frame,
    price_to_y: &impl Fn(Price) -> f32,
    footprint: &KlineTrades,
    price: Price,
    sell_qty: f32,
    higher_price: Price,
    threshold: usize,
    color_scale: Option<usize>,
    ignore_zeros: bool,
    cell_height: f32,
    palette: &Extended,
    buyside_x: f32,
    sellside_x: f32,
    rect_width: f32,
) {
    if ignore_zeros && sell_qty <= 0.0 {
        return;
    }

    if let Some(group) = footprint.trades.get(&higher_price) {
        let diagonal_buy_qty = group.buy_qty as f32;

        if ignore_zeros && diagonal_buy_qty <= 0.0 {
            return;
        }

        let rect_height = cell_height / 2.0;

        let alpha_from_ratio = |ratio: f32| -> f32 {
            if let Some(scale) = color_scale {
                let divisor = (scale as f32 / 10.0) - 1.0;
                if divisor.abs() < 1e-4 {
                    1.0
                } else {
                    (0.2 + 0.8 * ((ratio - 1.0) / divisor).clamp(0.0, 1.0)).min(1.0)
                }
            } else {
                1.0
            }
        };

        if diagonal_buy_qty >= sell_qty {
            let required_qty = sell_qty * (100 + threshold) as f32 / 100.0;
            if diagonal_buy_qty > required_qty {
                let ratio = diagonal_buy_qty / required_qty;
                let alpha = alpha_from_ratio(ratio);

                let y = price_to_y(higher_price);
                frame.fill_rectangle(
                    Point::new(buyside_x, y - (rect_height / 2.0)),
                    Size::new(rect_width, rect_height),
                    palette.success.weak.color.scale_alpha(alpha),
                );
            }
        } else {
            let required_qty = diagonal_buy_qty * (100 + threshold) as f32 / 100.0;
            if sell_qty > required_qty {
                let ratio = sell_qty / required_qty;
                let alpha = alpha_from_ratio(ratio);

                let y = price_to_y(price);
                frame.fill_rectangle(
                    Point::new(sellside_x, y - (rect_height / 2.0)),
                    Size::new(rect_width, rect_height),
                    palette.danger.weak.color.scale_alpha(alpha),
                );
            }
        }
    }
}

impl ContentGaps {
    pub(super) fn from_view(candle_width: f32, scaling: f32) -> Self {
        let px = |p: f32| p / scaling;
        let base = (candle_width * 0.2).max(px(2.0));
        Self {
            marker_to_candle: base,
            candle_to_cluster: base,
            marker_to_bars: px(2.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ContentGaps {
    /// Space between imb. markers candle body
    marker_to_candle: f32,
    /// Space between candle body and clusters
    candle_to_cluster: f32,
    /// Inner space reserved between imb. markers and clusters (used for BidAsk)
    marker_to_bars: f32,
}

pub(super) fn draw_cluster_text(
    frame: &mut canvas::Frame,
    text: &str,
    position: Point,
    text_size: f32,
    color: iced::Color,
    align_x: Alignment,
    align_y: Alignment,
) {
    frame.fill_text(canvas::Text {
        content: text.to_string(),
        position,
        size: iced::Pixels(text_size),
        color,
        align_x: align_x.into(),
        align_y: align_y.into(),
        font: style::AZERET_MONO,
        ..canvas::Text::default()
    });
}

pub(super) struct ProfileArea {
    imb_marker_left: f32,
    imb_marker_width: f32,
    bars_left: f32,
    bars_width: f32,
    candle_center_x: f32,
}

impl ProfileArea {
    fn new(
        content_left: f32,
        content_right: f32,
        candle_width: f32,
        gaps: ContentGaps,
        has_imbalance: bool,
    ) -> Self {
        let candle_lane_left = if has_imbalance {
            content_left + candle_width + gaps.marker_to_candle
        } else {
            content_left
        };
        let candle_lane_width = candle_width * 0.25;

        let bars_left = candle_lane_left + candle_lane_width + gaps.candle_to_cluster;
        let bars_width = (content_right - bars_left).max(0.0);

        let candle_center_x = candle_lane_left + (candle_lane_width / 2.0);

        Self {
            imb_marker_left: content_left,
            imb_marker_width: if has_imbalance { candle_width } else { 0.0 },
            bars_left,
            bars_width,
            candle_center_x,
        }
    }
}

pub(super) struct BidAskArea {
    bid_area_left: f32,
    bid_area_right: f32,
    ask_area_left: f32,
    ask_area_right: f32,
    candle_center_x: f32,
    imb_marker_width: f32,
}

impl BidAskArea {
    fn new(
        x_position: f32,
        content_left: f32,
        content_right: f32,
        candle_width: f32,
        spacing: ContentGaps,
    ) -> Self {
        let candle_body_width = candle_width * 0.25;

        let candle_left = x_position - (candle_body_width / 2.0);
        let candle_right = x_position + (candle_body_width / 2.0);

        let ask_area_right = candle_left - spacing.candle_to_cluster;
        let bid_area_left = candle_right + spacing.candle_to_cluster;

        Self {
            bid_area_left,
            bid_area_right: content_right,
            ask_area_left: content_left,
            ask_area_right,
            candle_center_x: x_position,
            imb_marker_width: candle_width,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LodLevel {
    /// Full numbers: Bid×Ask / Delta text, POC label
    Full,
    /// Heat blocks only — no text, color-coded by volume/delta
    Heatmap,
    /// Plain thin candles — no cluster content at all, classic bottom volume
    Candle,
}

#[inline]
pub(super) fn lod_level(
    cell_width_unscaled: f32,
    cell_height_unscaled: f32,
    min_w: f32,
) -> LodLevel {
    if cell_width_unscaled < 35.0 {
        LodLevel::Candle
    } else if cell_width_unscaled >= min_w && cell_height_unscaled >= 10.0 {
        LodLevel::Full
    } else {
        LodLevel::Heatmap
    }
}
