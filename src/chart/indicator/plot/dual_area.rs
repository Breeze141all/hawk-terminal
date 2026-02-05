//! Dual Area Plot - Two series on the same chart
//!
//! Used for Net OI indicators showing both Net Longs and Net Shorts

use std::ops::RangeInclusive;

use iced::{
    Color, Theme,
    widget::canvas::{self, LineDash, Path, Stroke},
};

use crate::chart::{
    ViewState,
    indicator::plot::{Plot, PlotTooltip, Series, TooltipFn, YScale},
};

/// Dual area chart showing two series with different colors
pub struct DualAreaPlot<V1, V2, T> {
    /// Value extractor for first series (e.g., Net Longs)
    pub value1: V1,
    /// Value extractor for second series (e.g., Net Shorts)
    pub value2: V2,
    pub tooltip: Option<TooltipFn<T>>,
    /// Padding as percentage of value range
    pub padding: f32,
    pub stroke_width: f32,
    /// Draw a dashed line at zero
    pub zero_line: bool,
    /// Fill opacity (0.0 to 1.0)
    pub fill_alpha: f32,
    /// Custom color for series 1 (default: success/green)
    pub color1: Option<Color>,
    /// Custom color for series 2 (default: danger/red)
    pub color2: Option<Color>,
    _phantom: std::marker::PhantomData<T>,
}

#[allow(dead_code)]
impl<V1, V2, T> DualAreaPlot<V1, V2, T> {
    pub fn new(value1: V1, value2: V2) -> Self {
        Self {
            value1,
            value2,
            tooltip: None,
            padding: 0.1,
            stroke_width: 1.5,
            zero_line: true,
            fill_alpha: 0.2,
            color1: None,
            color2: None,
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

    pub fn with_zero_line(mut self, show: bool) -> Self {
        self.zero_line = show;
        self
    }

    pub fn fill_alpha(mut self, alpha: f32) -> Self {
        self.fill_alpha = alpha;
        self
    }

    pub fn color1(mut self, c: Color) -> Self {
        self.color1 = Some(c);
        self
    }

    pub fn color2(mut self, c: Color) -> Self {
        self.color2 = Some(c);
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

impl<S, V1, V2> Plot<S> for DualAreaPlot<V1, V2, S::Y>
where
    S: Series,
    V1: Fn(&S::Y) -> f32,
    V2: Fn(&S::Y) -> f32,
{
    fn y_extents(&self, datapoints: &S, range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;

        datapoints.for_each_in(range, |_, y| {
            let v1 = (self.value1)(y);
            let v2 = (self.value2)(y);

            min_v = min_v.min(v1).min(v2);
            max_v = max_v.max(v1).max(v2);
        });

        if min_v == f32::MAX {
            None
        } else {
            // Ensure zero is included for proper reference
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

        // Series 1: Green (Net Longs)
        let color1 = self.color1.unwrap_or(palette.success.strong.color);
        // Series 2: Red (Net Shorts)
        let color2 = self.color2.unwrap_or(palette.danger.strong.color);

        let stroke1 = Stroke::with_color(
            Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            },
            color1,
        );

        let stroke2 = Stroke::with_color(
            Stroke {
                width: self.stroke_width,
                ..Stroke::default()
            },
            color2,
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

        // Collect points for both series
        let mut points1: Vec<(f32, f32)> = Vec::new();
        let mut points2: Vec<(f32, f32)> = Vec::new();

        datapoints.for_each_in(range.clone(), |x, y| {
            let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);

            let vy1 = (self.value1)(y);
            let sy1 = scale.to_y(vy1);
            points1.push((sx, sy1));

            let vy2 = (self.value2)(y);
            let sy2 = scale.to_y(vy2);
            points2.push((sx, sy2));
        });

        if points1.is_empty() {
            return;
        }

        // Draw line for series 1
        for window in points1.windows(2) {
            let (px, py) = window[0];
            let (sx, sy) = window[1];
            frame.stroke(
                &Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy)),
                stroke1,
            );
        }

        // Draw line for series 2
        for window in points2.windows(2) {
            let (px, py) = window[0];
            let (sx, sy) = window[1];
            frame.stroke(
                &Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy)),
                stroke2,
            );
        }
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<S::Y>> {
        self.tooltip.as_ref()
    }
}
