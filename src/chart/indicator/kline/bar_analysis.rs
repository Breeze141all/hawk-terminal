use crate::{
    chart::{Caches, Interaction, Message, ViewState},
    style,
};

use data::chart::{
    PlotData,
    kline::{FootprintSummary, KlineDataPoint},
};
use data::util::abbr_large_numbers;
use exchange::{Kline, Trade};
use iced::{
    Alignment, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    mouse,
    widget::{
        Canvas,
        canvas::{self, Cache, Geometry},
        container, row, rule, space,
    },
};
use std::{collections::BTreeMap, ops::RangeInclusive};

const TEXT_SIZE: f32 = 12.0;

pub struct BarAnalysisIndicator {
    cache: Caches,
    data: BTreeMap<u64, FootprintSummary>,
    settings: BarAnalysisSettings,
}

impl BarAnalysisIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            settings: BarAnalysisSettings::default(),
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> Element<'a, Message> {
        let canvas = Canvas::new(BarAnalysisCanvas {
            cache: &self.cache.main,
            ctx: main_chart,
            data: &self.data,
            settings: self.settings,
            visible_range,
        })
        .height(Length::Fill)
        .width(Length::Fill);

        row![
            canvas,
            rule::vertical(1).style(style::split_ruler),
            container(space::vertical()).width(main_chart.y_labels_width())
        ]
        .into()
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data.clear();
        match source {
            PlotData::TimeBased(timeseries) => {
                for (timestamp, dp) in &timeseries.datapoints {
                    let summary = FootprintSummary::from_trades(&dp.footprint)
                        .unwrap_or_else(|| FootprintSummary::new(dp.kline.volume.0, dp.kline.volume.1));
                    self.data.insert(*timestamp, summary);
                }
            }
            PlotData::TickBased(tickseries) => {
                for (idx, dp) in tickseries.datapoints.iter().enumerate() {
                    let summary = FootprintSummary::from_trades(&dp.footprint)
                        .unwrap_or_else(|| FootprintSummary::new(dp.kline.volume.0, dp.kline.volume.1));
                    self.data.insert(idx as u64, summary);
                }
            }
        }
        self.clear_all_caches();
    }

    pub fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }
}

impl Default for BarAnalysisIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl super::KlineIndicatorImpl for BarAnalysisIndicator {
    fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }

    fn clear_crosshair_caches(&mut self) {
        self.cache.clear_crosshair();
    }

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> Element<'a, Message> {
        self.indicator_elem(chart, visible_range)
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_insert_klines(&mut self, klines: &[Kline]) {
        for kline in klines {
            let summary = FootprintSummary::new(kline.volume.0, kline.volume.1);
            self.data.insert(kline.time, summary);
        }
        self.clear_all_caches();
    }

    fn on_insert_trades(
        &mut self,
        trades: &[Trade],
        old_dp_len: usize,
        source: &PlotData<KlineDataPoint>,
    ) {
        match source {
            PlotData::TimeBased(timeseries) => {
                let interval_ms = timeseries.interval.to_milliseconds().max(1);
                let mut affected = Vec::new();
                for trade in trades {
                    let rounded = (trade.time / interval_ms) * interval_ms;
                    if !affected.contains(&rounded) {
                        affected.push(rounded);
                    }
                }
                for ts in affected {
                    if let Some(dp) = timeseries.datapoints.get(&ts) {
                        match FootprintSummary::from_trades(&dp.footprint) {
                            Some(summary) => {
                                self.data.insert(ts, summary);
                            }
                            None => {
                                self.data.insert(
                                    ts,
                                    FootprintSummary::new(dp.kline.volume.0, dp.kline.volume.1),
                                );
                            }
                        }
                    } else {
                        self.data.remove(&ts);
                    }
                }
            }
            PlotData::TickBased(tick_aggr) => {
                let new_len = tick_aggr.datapoints.len();
                let start = old_dp_len.saturating_sub(1);
                self.data.retain(|&idx, _| idx < new_len as u64);
                for idx in start..new_len {
                    if let Some(dp) = tick_aggr.datapoints.get(idx) {
                        match FootprintSummary::from_trades(&dp.footprint) {
                            Some(summary) => {
                                self.data.insert(idx as u64, summary);
                            }
                            None => {
                                self.data.insert(
                                    idx as u64,
                                    FootprintSummary::new(dp.kline.volume.0, dp.kline.volume.1),
                                );
                            }
                        }
                    }
                }
            }
        }
        self.clear_all_caches();
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BarAnalysisSettings {
    show_buy_sell: bool,
    show_volume: bool,
    show_delta: bool,
    show_delta_pct: bool,
}

impl Default for BarAnalysisSettings {
    fn default() -> Self {
        Self {
            show_buy_sell: true,
            show_volume: true,
            show_delta: true,
            show_delta_pct: true,
        }
    }
}

struct BarAnalysisCanvas<'a> {
    cache: &'a Cache,
    ctx: &'a ViewState,
    data: &'a BTreeMap<u64, FootprintSummary>,
    settings: BarAnalysisSettings,
    visible_range: RangeInclusive<u64>,
}

impl canvas::Program<Message> for BarAnalysisCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let ctx = self.ctx;
        let palette = theme.extended_palette();

        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            if ctx.bounds.width == 0.0 || ctx.scaling <= f32::EPSILON {
                return;
            }

            let pane_height = bounds.height / ctx.scaling;
            let table_top = 0.0;
            let table_height = pane_height.max(24.0);
            let rows_count = {
                let mut count: f32 = 0.0;
                if self.settings.show_buy_sell {
                    count += 2.0;
                }
                if self.settings.show_volume {
                    count += 1.0;
                }
                if self.settings.show_delta {
                    count += 1.0;
                }
                if self.settings.show_delta_pct {
                    count += 1.0;
                }
                count.max(1.0)
            };
            let row_height = table_height / rows_count;
            let column_width = ctx.cell_width;
            let text_size = (row_height * 0.5).clamp(5.0, TEXT_SIZE);
            let border_color = palette.background.strong.color;

            let mut header_labels: Vec<&str> = Vec::with_capacity(5);
            if self.settings.show_buy_sell {
                header_labels.push("Buy");
                header_labels.push("Sell");
            }
            if self.settings.show_volume {
                header_labels.push("Total");
            }
            if self.settings.show_delta {
                header_labels.push("Δ");
            }
            if self.settings.show_delta_pct {
                header_labels.push("Δ%");
            }

            let max_label_chars = header_labels
                .iter()
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(3);
            let header_width = text_size * max_label_chars as f32 * 0.72 + 12.0;
            let screen_text_size = text_size * ctx.scaling;
            let screen_row_height = row_height * ctx.scaling;
            let screen_table_top = table_top * ctx.scaling;
            let screen_header_width = header_width * ctx.scaling;
            let header_x = bounds.width - screen_header_width;

            // Header row labels and separators
            for (idx, label) in header_labels.iter().enumerate() {
                let row_y = screen_table_top + screen_row_height * idx as f32;
                if idx > 0 {
                    frame.fill_rectangle(
                        Point::new(header_x, row_y),
                        Size::new(screen_header_width, 1.0),
                        border_color,
                    );
                }
                draw_text(
                    frame,
                    label,
                    Point::new(
                        header_x + screen_header_width / 2.0,
                        row_y + screen_row_height / 2.0,
                    ),
                    screen_text_size,
                    palette.background.weakest.text,
                );
            }

            // Header vertical border
            frame.fill_rectangle(
                Point::new(header_x, screen_table_top),
                Size::new(1.0, screen_row_height * rows_count),
                border_color,
            );

            let header_left_chart =
                (header_x - bounds.width / 2.0) / ctx.scaling - ctx.translation.x;
            let view_left = -ctx.translation.x - bounds.width / ctx.scaling;
            let view_right =
                (-ctx.translation.x + bounds.width / ctx.scaling).min(header_left_chart);

            let clip_region = Rectangle::new(
                Point::new(0.0, screen_table_top),
                Size::new(
                    bounds.width - screen_header_width,
                    screen_row_height * rows_count,
                ),
            );
            frame.with_clip(clip_region, |frame| {
                let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);
                frame.translate(center);
                frame.scale(ctx.scaling);
                frame.translate(Vector::new(
                    ctx.translation.x,
                    (-bounds.height / ctx.scaling) / 2.0,
                ));

                for (&x, row) in self.data.range(self.visible_range.clone()) {
                    let column_left = ctx.interval_to_x(x) - column_width / 2.0;

                    if column_left > view_right || column_left + column_width < view_left {
                        continue;
                    }

                    frame.fill_rectangle(
                        Point::new(column_left, table_top),
                        Size::new(column_width, table_height),
                        palette.background.weakest.color.scale_alpha(0.22),
                    );

                    let delta_color = if row.delta >= 0.0 {
                        palette.success.base.color
                    } else {
                        palette.danger.base.color
                    };

                    let mut rows: Vec<(String, Color)> = Vec::with_capacity(5);
                    if self.settings.show_buy_sell {
                        rows.push((
                            abbr_large_numbers(row.buy),
                            palette.success.base.color,
                        ));
                        rows.push((
                            abbr_large_numbers(row.sell),
                            palette.danger.base.color,
                        ));
                    }
                    if self.settings.show_volume {
                        rows.push((
                            abbr_large_numbers(row.total),
                            palette.background.weakest.text,
                        ));
                    }
                    if self.settings.show_delta {
                        rows.push((abbr_large_numbers(row.delta), delta_color));
                    }
                    if self.settings.show_delta_pct {
                        rows.push((format!("{:+.1}%", row.delta_pct), delta_color));
                    }

                    for (idx, (label, color)) in rows.iter().enumerate() {
                        let row_y = table_top + row_height * idx as f32;
                        if idx > 0 {
                            frame.fill_rectangle(
                                Point::new(column_left, row_y),
                                Size::new(column_width, 1.0),
                                border_color,
                            );
                        }
                        draw_text(
                            frame,
                            label,
                            Point::new(
                                column_left + column_width / 2.0,
                                row_y + row_height / 2.0,
                            ),
                            text_size,
                            *color,
                        );
                    }

                    frame.fill_rectangle(
                        Point::new(column_left, table_top),
                        Size::new(1.0, table_height),
                        border_color.scale_alpha(0.4),
                    );
                }
            });
        });

        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &Interaction,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

fn draw_text(frame: &mut canvas::Frame, text: &str, position: Point, size: f32, color: Color) {
    frame.fill_text(canvas::Text {
        content: text.to_string(),
        position,
        size: iced::Pixels(size),
        color,
        align_x: Alignment::Center.into(),
        align_y: Alignment::Center.into(),
        font: style::AZERET_MONO,
        ..canvas::Text::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::indicator::kline::KlineIndicatorImpl;
    use data::chart::kline::KlineTrades;
    use exchange::util::{Price, PriceStep};

    #[test]
    fn test_ter13_bar_analysis_indicator_lifecycle() {
        let mut indicator = BarAnalysisIndicator::new();
        assert!(indicator.data.is_empty());

        let kline = Kline {
            time: 1_700_000_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(110.0),
            low: Price::from_f32(90.0),
            close: Price::from_f32(105.0),
            volume: (60.0, 40.0),
        };
        indicator.on_insert_klines(&[kline]);
        assert_eq!(indicator.data.len(), 1);
        let summary = indicator.data.get(&1_700_000_000).expect("summary exists");
        assert_eq!(summary.buy, 60.0);
        assert_eq!(summary.sell, 40.0);
        assert_eq!(summary.total, 100.0);
        assert_eq!(summary.delta, 20.0);
        assert!((summary.delta_pct - 20.0).abs() < 1e-4);

        let step = PriceStep::from_f32(1.0);
        let mut timeseries = data::aggr::time::TimeSeries::<KlineDataPoint>::new(exchange::Timeframe::M1, step, &[]);
        let simulated_footprint = KlineTrades::simulate_from_kline(&kline, step);
        timeseries.datapoints.insert(
            kline.time,
            KlineDataPoint {
                kline,
                footprint: simulated_footprint,
                trades_fetched: true,
            },
        );

        let plot_data = PlotData::TimeBased(timeseries);
        indicator.rebuild_from_source(&plot_data);
        assert_eq!(indicator.data.len(), 1);
        let rebuilt = indicator.data.get(&1_700_000_000).unwrap();
        assert!((rebuilt.total - 100.0).abs() < 1e-1);
    }
}

