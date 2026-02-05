//! MTM Tension Index Plot with threshold line and fixed 0-100 range

use std::ops::RangeInclusive;

use iced::{
    Color, Theme,
    widget::canvas::{self, LineDash, Path, Stroke},
};

use crate::chart::{
    ViewState,
    indicator::plot::{Plot, PlotTooltip, Series, TooltipFn, YScale},
};

/// MTM-specific plot with threshold line and optional fill
pub struct MtmPlot<V, T> {
    pub value: V,
    pub tooltip: Option<TooltipFn<T>>,
    pub threshold: f32,
    pub stroke_width: f32,
    /// Color for the MTM line
    pub line_color: Option<Color>,
    /// Color for the threshold line
    pub threshold_color: Option<Color>,
    /// Whether to fill area under the line
    pub fill_area: bool,
    _phantom: std::marker::PhantomData<T>,
}

impl<V, T> MtmPlot<V, T> {
    pub fn new(value: V, threshold: f32) -> Self {
        Self {
            value,
            tooltip: None,
            threshold,
            stroke_width: 1.5,
            line_color: None,
            threshold_color: None,
            fill_area: true,
            _phantom: std::marker::PhantomData,
        }
    }

    #[allow(dead_code)]
    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    #[allow(dead_code)]
    pub fn line_color(mut self, c: Color) -> Self {
        self.line_color = Some(c);
        self
    }

    #[allow(dead_code)]
    pub fn threshold_color(mut self, c: Color) -> Self {
        self.threshold_color = Some(c);
        self
    }

    #[allow(dead_code)]
    pub fn fill_area(mut self, fill: bool) -> Self {
        self.fill_area = fill;
        self
    }

    pub fn with_tooltip<F>(mut self, tooltip: F) -> Self
    where
        F: Fn(&T, Option<&T>) -> PlotTooltip + 'static,
    {
        self.tooltip = Some(Box::new(tooltip));
        self
    }
}

impl<S, V> Plot<S> for MtmPlot<V, S::Y>
where
    S: Series,
    V: Fn(&S::Y) -> f32,
{
    fn y_extents(&self, _datapoints: &S, _range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        // Fixed 0-100 range for MTM index
        Some((0.0, 100.0))
    }

    fn adjust_extents(&self, min: f32, max: f32) -> (f32, f32) {
        // No adjustment needed, keep 0-100
        (min, max)
    }

    fn draw(
        &self,
        frame: &mut canvas::Frame,
        ctx: &ViewState,
        theme: &Theme,
        datapoints: &S,
        range: RangeInclusive<u64>,
        scale: &YScale,
    ) {
        let palette = theme.extended_palette();

        // MTM line color - use a cyan/teal color for visibility
        let line_color = self.line_color.unwrap_or(palette.primary.strong.color);

        // Threshold line color - orange/warning color
        let threshold_color = self.threshold_color.unwrap_or(palette.danger.base.color);

        let line_stroke = Stroke::with_color(
            Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            },
            line_color,
        );

        // Draw threshold line (dashed)
        let threshold_y = scale.to_y(self.threshold);
        let threshold_stroke = Stroke::with_color(
            Stroke {
                width: 1.0,
                line_dash: LineDash {
                    segments: &[5.0, 5.0],
                    offset: 0,
                },
                ..Default::default()
            },
            threshold_color.scale_alpha(0.8),
        );

        frame.stroke(
            &Path::line(
                iced::Point::new(0.0, threshold_y),
                iced::Point::new(frame.width(), threshold_y),
            ),
            threshold_stroke,
        );

        // Collect points for the line and fill
        let mut points: Vec<(f32, f32)> = Vec::new();
        datapoints.for_each_in(range.clone(), |x, y| {
            let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);
            let vy = (self.value)(y);
            let sy = scale.to_y(vy);
            points.push((sx, sy));
        });

        if points.is_empty() {
            return;
        }

        // Draw fill area under the line
        if self.fill_area && points.len() > 1 {
            let bottom_y = scale.to_y(0.0);
            let fill_color = line_color.scale_alpha(0.15);

            let mut builder = canvas::path::Builder::new();
            builder.move_to(iced::Point::new(points[0].0, bottom_y));

            for &(px, py) in &points {
                builder.line_to(iced::Point::new(px, py));
            }

            if let Some(&(last_x, _)) = points.last() {
                builder.line_to(iced::Point::new(last_x, bottom_y));
            }
            builder.close();

            frame.fill(&builder.build(), fill_color);
        }

        // Draw the MTM line
        for window in points.windows(2) {
            let (px, py) = window[0];
            let (sx, sy) = window[1];
            frame.stroke(
                &Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy)),
                line_stroke,
            );
        }

        // Draw alert markers where value > threshold
        let alert_color = palette.danger.strong.color;
        datapoints.for_each_in(range, |x, y| {
            let vy = (self.value)(y);
            if vy > self.threshold {
                let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);
                let sy = scale.to_y(vy);

                // Draw a small circle marker
                frame.fill(&Path::circle(iced::Point::new(sx, sy), 3.0), alert_color);
            }
        });
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<S::Y>> {
        self.tooltip.as_ref()
    }
}
