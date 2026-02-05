//! Area Plot with filled region under/over the line
//!
//! Used for Net OI indicators with zero line reference

use std::ops::RangeInclusive;

use iced::{
    Color, Theme,
    widget::canvas::{self, LineDash, Path, Stroke},
};

use crate::chart::{
    ViewState,
    indicator::plot::{Plot, PlotTooltip, Series, TooltipFn, YScale},
};

/// Area chart that fills the region between the line and zero
pub struct AreaPlot<V, T> {
    pub value: V,
    pub tooltip: Option<TooltipFn<T>>,
    /// Padding as percentage of value range (0.0 to 1.0)
    pub padding: f32,
    pub stroke_width: f32,
    /// Use success (green) color, otherwise danger (red)
    pub use_success_color: bool,
    /// Draw a dashed line at zero
    pub zero_line: bool,
    /// Custom line color override
    pub line_color: Option<Color>,
    /// Fill opacity (0.0 to 1.0)
    pub fill_alpha: f32,
    _phantom: std::marker::PhantomData<T>,
}

#[allow(dead_code)]
impl<V, T> AreaPlot<V, T> {
    pub fn new(value: V) -> Self {
        Self {
            value,
            tooltip: None,
            padding: 0.1,
            stroke_width: 1.5,
            use_success_color: true,
            zero_line: true,
            line_color: None,
            fill_alpha: 0.2,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn padding(mut self, p: f32) -> Self {
        self.padding = p;
        self
    }

    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    pub fn use_success_color(mut self, success: bool) -> Self {
        self.use_success_color = success;
        self
    }

    pub fn with_zero_line(mut self, show: bool) -> Self {
        self.zero_line = show;
        self
    }

    pub fn line_color(mut self, c: Color) -> Self {
        self.line_color = Some(c);
        self
    }

    pub fn fill_alpha(mut self, alpha: f32) -> Self {
        self.fill_alpha = alpha;
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

impl<S, V> Plot<S> for AreaPlot<V, S::Y>
where
    S: Series,
    V: Fn(&S::Y) -> f32,
{
    fn y_extents(&self, datapoints: &S, range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;

        datapoints.for_each_in(range, |_, y| {
            let v = (self.value)(y);
            if v < min_v {
                min_v = v;
            }
            if v > max_v {
                max_v = v;
            }
        });

        if min_v == f32::MAX {
            None
        } else {
            // Ensure zero is included in the range for proper area fill
            min_v = min_v.min(0.0);
            max_v = max_v.max(0.0);
            Some((min_v, max_v))
        }
    }

    fn adjust_extents(&self, min: f32, max: f32) -> (f32, f32) {
        if self.padding > 0.0 && max > min {
            let range = max - min;
            let pad = range * self.padding;
            (min - pad, max + pad)
        } else {
            (min, max)
        }
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

        // Choose color based on success/danger
        let line_color = self.line_color.unwrap_or(if self.use_success_color {
            palette.success.strong.color
        } else {
            palette.danger.strong.color
        });

        let fill_color = line_color.scale_alpha(self.fill_alpha);

        let stroke = Stroke::with_color(
            Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            },
            line_color,
        );

        // Draw zero line if enabled
        if self.zero_line {
            let zero_y = scale.to_y(0.0);
            let zero_stroke = Stroke::with_color(
                Stroke {
                    width: 1.0,
                    line_dash: LineDash {
                        segments: &[4.0, 4.0],
                        offset: 0,
                    },
                    ..Default::default()
                },
                palette.background.weak.color.scale_alpha(0.6),
            );
            frame.stroke(
                &Path::line(
                    iced::Point::new(0.0, zero_y),
                    iced::Point::new(frame.width(), zero_y),
                ),
                zero_stroke,
            );
        }

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

        // Draw filled area from line to zero
        if points.len() > 1 {
            let zero_y = scale.to_y(0.0);
            let mut builder = canvas::path::Builder::new();

            // Start at zero line
            builder.move_to(iced::Point::new(points[0].0, zero_y));

            // Line to first point, then along all points
            for &(px, py) in &points {
                builder.line_to(iced::Point::new(px, py));
            }

            // Back to zero line at end
            if let Some(&(last_x, _)) = points.last() {
                builder.line_to(iced::Point::new(last_x, zero_y));
            }
            builder.close();

            frame.fill(&builder.build(), fill_color);
        }

        // Draw the line
        for window in points.windows(2) {
            let (px, py) = window[0];
            let (sx, sy) = window[1];
            frame.stroke(
                &Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy)),
                stroke,
            );
        }
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<S::Y>> {
        self.tooltip.as_ref()
    }
}
