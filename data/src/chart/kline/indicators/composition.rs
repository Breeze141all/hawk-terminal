use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelRole {
    Primary,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PanelPresentation {
    #[default]
    Plot,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LayerPurpose {
    #[default]
    Plot,
    TableBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistogramMode {
    Plain,
    SignedOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarMode {
    Regular,
    Histogram(HistogramMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    Candle,
    Bar(BarMode),
    Line,
    StepLine,
    Area,
    Point,
    Cross,
    Segment,
    HorizontalLevel,
}

impl MarkKind {
    pub const fn supports_line_styling(self) -> bool {
        matches!(
            self,
            Self::Line | Self::StepLine | Self::Area | Self::Segment | Self::HorizontalLevel
        )
    }

    pub const fn is_line_like(self) -> bool {
        matches!(
            self,
            Self::Line | Self::StepLine | Self::Area | Self::Segment | Self::HorizontalLevel
        )
    }

    pub const fn is_series_line_like(self) -> bool {
        matches!(
            self,
            Self::Line | Self::StepLine | Self::Area | Self::Segment | Self::HorizontalLevel
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerDataKind {
    Ohlc,
    Scalar,
    Histogram,
    Range,
}

pub const fn default_bar_mode_for_data_kind(data_kind: LayerDataKind) -> BarMode {
    match data_kind {
        LayerDataKind::Histogram => BarMode::Histogram(HistogramMode::Plain),
        LayerDataKind::Ohlc | LayerDataKind::Scalar | LayerDataKind::Range => BarMode::Regular,
    }
}

pub const fn default_mark_for_data_kind(data_kind: LayerDataKind) -> MarkKind {
    match data_kind {
        LayerDataKind::Ohlc => MarkKind::Candle,
        LayerDataKind::Scalar | LayerDataKind::Range => MarkKind::Line,
        LayerDataKind::Histogram => MarkKind::Bar(BarMode::Histogram(HistogramMode::Plain)),
    }
}

const fn supports_mark(data_kind: LayerDataKind, mark: MarkKind) -> bool {
    match data_kind {
        LayerDataKind::Ohlc => matches!(mark, MarkKind::Line | MarkKind::Candle),
        LayerDataKind::Scalar => matches!(
            mark,
            MarkKind::Line
                | MarkKind::StepLine
                | MarkKind::Area
                | MarkKind::Point
                | MarkKind::Cross
                | MarkKind::Segment
                | MarkKind::HorizontalLevel
                | MarkKind::Bar(_)
        ),
        LayerDataKind::Range => matches!(mark, MarkKind::Line),
        LayerDataKind::Histogram => matches!(
            mark,
            MarkKind::Line
                | MarkKind::StepLine
                | MarkKind::Area
                | MarkKind::Point
                | MarkKind::Cross
                | MarkKind::Segment
                | MarkKind::HorizontalLevel
                | MarkKind::Bar(_)
        ),
    }
}

const fn supports_bar_mode(data_kind: LayerDataKind, mode: BarMode) -> bool {
    match mode {
        BarMode::Regular => true,
        BarMode::Histogram(HistogramMode::Plain) => matches!(data_kind, LayerDataKind::Histogram),
        BarMode::Histogram(HistogramMode::SignedOverlay) => {
            matches!(data_kind, LayerDataKind::Histogram)
        }
    }
}

pub fn resolve_mark_for_data_kind(mark: MarkKind, data_kind: LayerDataKind) -> MarkKind {
    let mut resolved = if supports_mark(data_kind, mark) {
        mark
    } else {
        default_mark_for_data_kind(data_kind)
    };

    if let MarkKind::Bar(mode) = resolved
        && !supports_bar_mode(data_kind, mode)
    {
        resolved = MarkKind::Bar(default_bar_mode_for_data_kind(data_kind));
    }

    resolved
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisBinding {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelScaleMode {
    Absolute,
    FitVisible,
    FitVisibleIncludeZero,
    Logarithmic,
    PercentFromBase,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelValuePrecision {
    BaseTickerMinTick,
    BaseTickerMinQty,
    FixedPower10(exchange::util::Power10<-8, 8>),
    FixedStep(f64),
}

impl PanelValuePrecision {
    pub fn quantization_step(self, base_ticker: Option<exchange::TickerInfo>) -> Option<f64> {
        let step = match self {
            Self::BaseTickerMinTick => {
                base_ticker.map(|ticker| ticker.min_ticksize.as_f32() as f64)
            }
            Self::BaseTickerMinQty => base_ticker.map(|ticker| ticker.min_qty.as_f32() as f64),
            Self::FixedPower10(step) => Some(step.as_f32() as f64),
            Self::FixedStep(step) => Some(step),
        }?;
        Some(step.abs().max(1e-8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelValueLabelMode {
    Compact,
    Commas,
    Abbreviated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelValueLabelPolicy {
    pub axis_mode: PanelValueLabelMode,
    pub header_mode: PanelValueLabelMode,
    pub max_decimals: Option<u8>,
}

impl Default for PanelValueLabelPolicy {
    fn default() -> Self {
        Self {
            axis_mode: PanelValueLabelMode::Compact,
            header_mode: PanelValueLabelMode::Commas,
            max_decimals: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PanelComparisonPolicy {
    pub force_percent_scale_on_multi_source: bool,
    pub force_line_for_non_base_sources: bool,
}
