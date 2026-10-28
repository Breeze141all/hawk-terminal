use super::{
    Action, Basis, Caches, Chart, Interaction, Message, PlotConstants, PlotData, PriceAxisBadge,
    TEXT_SIZE, ViewState, indicator, request_fetch, scale::linear::PriceInfoLabel,
};
use crate::chart::indicator::kline::KlineIndicatorImpl;
use crate::{modal::pane::settings::study, style};
use data::aggr::ticks::TickAggr;
use data::aggr::time::TimeSeries;
use data::chart::Autoscale;
use data::chart::alert::{AlertStatus, PriceAlert};
use data::chart::drawing::{Drawing, DrawingKind, DrawingTool};
use data::chart::kline::ClusterScaling;
use data::chart::replay::ReplayState;
use data::chart::{
    KlineChartKind, ViewConfig,
    indicator::{Indicator, KlineIndicator},
    kline::{
        ClusterKind, FootprintStudy, FootprintSummary, GroupedTrades, KlineDataPoint, KlineTrades,
        NPoc, PointOfControl,
    },
    liquidation_heatmap::{LiquidationHeatmap, LiquidationHeatmapConfig, interpolate_color_themed},
};
use data::config::theme::{composite_color, contrast_ratio};
use data::util::{abbr_large_numbers, count_decimals};
use exchange::util::{Price, PriceStep};
use exchange::{
    Exchange, Kline, OpenInterest as OIData, TickerInfo, Trade,
    fetcher::{FetchRange, RequestHandler},
};

use iced::task::Handle;
use iced::theme::palette::Extended;
use iced::widget::canvas::{self, Event, Geometry, Path, Stroke};
use iced::{Alignment, Color, Element, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use enum_map::EnumMap;
use std::ops::ControlFlow;
use std::time::{Duration, Instant};

impl Chart for KlineChart {
    type IndicatorKind = KlineIndicator;

    fn y_axis_badges(&self) -> Vec<PriceAxisBadge> {
        self.y_axis_badges()
    }

    fn state(&self) -> &ViewState {
        &self.chart
    }

    fn mut_state(&mut self) -> &mut ViewState {
        &mut self.chart
    }

    fn invalidate_crosshair(&mut self) {
        self.chart.cache.clear_crosshair();
        self.indicators
            .values_mut()
            .filter_map(Option::as_mut)
            .for_each(|indi| indi.clear_crosshair_caches());
        for cache in self.script_panel_caches.values() {
            cache.clear_crosshair();
        }
    }

    fn invalidate_all(&mut self) {
        self.invalidate(None);
    }

    fn view_indicators(&'_ self, enabled: &[Self::IndicatorKind]) -> Vec<Element<'_, Message>> {
        let chart_state = self.state();
        let visible_region = chart_state.visible_region(chart_state.bounds.size());
        let (earliest, raw_latest) = chart_state.interval_range(&visible_region);
        let latest = if let Some(ref rep) = self.replay {
            if rep.active && rep.cutoff_time > 0 {
                raw_latest.min(rep.cutoff_time)
            } else {
                raw_latest
            }
        } else {
            raw_latest
        };
        if earliest > latest {
            return vec![];
        }

        let market = chart_state.ticker_info.market_type();
        let mut elements = vec![];

        // 1. Built-in panel indicators
        for selected_indicator in enabled {
            if !KlineIndicator::for_market(market).contains(selected_indicator) {
                continue;
            }
            if !selected_indicator.is_panel() {
                continue;
            }
            if *selected_indicator == KlineIndicator::RollingVwap
                && !self.config.rolling_vwap_show_panel
            {
                continue;
            }
            if let Some(indi) = self.indicators[*selected_indicator].as_ref() {
                elements.push(indi.element(chart_state, earliest..=latest));
            }
        }

        // 2. Scripted panel indicators
        for instance in self.indicator_instances.instances() {
            let def = instance.kind.definition();
            if def.attachment_mode != data::chart::kline::indicators::IndicatorAttachmentMode::Panel
            {
                continue;
            }
            if !def.supported_markets.is_empty() && !def.supported_markets.contains(&market) {
                continue;
            }
            if let Some(cache) = self.script_panel_caches.get(&instance.id) {
                elements.push(
                    crate::chart::indicator::kline::scripted::render_scripted_panel_row(
                        chart_state,
                        cache,
                        instance,
                        &self.indicator_host,
                        &self.data_source,
                        earliest..=latest,
                    ),
                );
            }
        }

        elements
    }

    fn visible_timerange(&self) -> Option<(u64, u64)> {
        let chart = self.state();
        let region = chart.visible_region(chart.bounds.size());

        if region.width == 0.0 {
            return None;
        }

        match &chart.basis {
            Basis::Time(timeframe) => {
                let interval = timeframe.to_milliseconds();

                let (earliest, latest) = (
                    chart.x_to_interval(region.x).saturating_sub(interval / 2),
                    chart
                        .x_to_interval(region.x + region.width)
                        .saturating_add(interval / 2),
                );

                if earliest > latest {
                    return None;
                }

                Some((earliest, latest))
            }
            Basis::Tick(_) => {
                let (earliest, latest) = chart.interval_range(&region);
                if earliest > latest {
                    return None;
                }
                Some((earliest, latest))
            }
        }
    }

    fn interval_keys(&self) -> Option<Vec<u64>> {
        match &self.data_source {
            PlotData::TimeBased(_) => None,
            PlotData::TickBased(tick_aggr) => Some(
                tick_aggr
                    .datapoints
                    .iter()
                    .map(|dp| dp.kline.time)
                    .collect(),
            ),
        }
    }

    fn autoscaled_coords(&self) -> Vector {
        let chart = self.state();
        let base_x = self.base_x();
        let x_offset = if let Some(rep) = &self.replay
            && rep.active
        {
            self.chart.interval_to_x(rep.cutoff_time)
        } else {
            0.0
        };
        Vector::new(base_x - x_offset, chart.translation.y)
    }

    fn supports_fit_autoscaling(&self) -> bool {
        true
    }

    fn is_empty(&self) -> bool {
        match &self.data_source {
            PlotData::TimeBased(timeseries) => timeseries.datapoints.is_empty(),
            PlotData::TickBased(tick_aggr) => tick_aggr.datapoints.is_empty(),
        }
    }
}

impl PlotConstants for KlineChart {
    fn min_scaling(&self) -> f32 {
        self.kind.min_scaling()
    }

    fn max_scaling(&self) -> f32 {
        self.kind.max_scaling()
    }

    fn max_cell_width(&self) -> f32 {
        self.kind.max_cell_width()
    }

    fn min_cell_width(&self) -> f32 {
        self.kind.min_cell_width()
    }

    fn max_cell_height(&self) -> f32 {
        self.kind.max_cell_height()
    }

    fn min_cell_height(&self) -> f32 {
        self.kind.min_cell_height()
    }

    fn default_cell_width(&self) -> f32 {
        self.kind.default_cell_width()
    }
}

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TpoMenuAction {
    ToggleSplitBrackets(i64),
    MergeNext(i64, i64),
    MergePrev(i64, i64),
    ResetCluster(i64),
}

#[derive(Debug, Clone)]
pub(crate) struct TpoSessionRange {
    pub session_start: i64,
    pub session_end: i64,
    pub prev_start: Option<i64>,
    pub next_start: Option<i64>,
    pub is_clustered: bool,
    pub is_split_brackets: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TpoMenuItem {
    pub label: String,
    pub action: Option<TpoMenuAction>,
    pub rect: Rectangle,
}

#[derive(Debug, Clone)]
pub(crate) struct TpoContextMenu {
    pub rect: Rectangle,
    pub items: Vec<TpoMenuItem>,
}

#[derive(Clone)]
pub(crate) struct TpoCacheEntry {
    candle_count: usize,
    last_candle_high: f64,
    last_candle_low: f64,
    last_candle_time: i64,
    profile: Arc<data::chart::tpo::TpoProfile>,
}

#[derive(Default)]
pub(crate) struct TpoCache {
    tick_size: f64,
    period: Option<data::chart::tpo::SessionPeriod>,
    sessions: HashMap<i64, TpoCacheEntry>,
    pub(crate) trade_tpo_cache: data::chart::tpo::TradeTpoCache,
    pub(crate) session_ranges: Vec<TpoSessionRange>,
    pub(crate) context_menu: Option<TpoContextMenu>,
}

impl TpoCache {
    pub(crate) fn clear(&mut self) {
        self.sessions.clear();
        self.trade_tpo_cache.clear();
        self.session_ranges.clear();
        self.context_menu = None;
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DrawingDrag {
    MovingDrawing {
        start_time: u64,
        start_price: f32,
        initial_drawing: Drawing,
        current_drawing: Drawing,
    },
    MovingHandle {
        handle_idx: usize,
        initial_drawing: Drawing,
        current_drawing: Drawing,
    },
}

impl DrawingDrag {
    pub fn current_drawing(&self) -> &Drawing {
        match self {
            DrawingDrag::MovingDrawing {
                current_drawing, ..
            }
            | DrawingDrag::MovingHandle {
                current_drawing, ..
            } => current_drawing,
        }
    }
}

#[derive(Default)]
pub(crate) struct DrawingState {
    pub active_tool: DrawingTool,
    pub in_progress: Option<Drawing>,
    pub start_point: Option<(u64, f32)>,
    pub selected_drawing: Option<uuid::Uuid>,
    pub drag: Option<DrawingDrag>,
    pub shift_pressed: bool,
    pub ctrl_pressed: bool,
    pub last_cursor_frame_pos: Option<Point>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum DrawingHistoryAction {
    Add(Drawing),
    Delete(Drawing),
    Update { old: Drawing, new: Drawing },
    BatchUpdate(Vec<(Drawing, Drawing)>),
    Clear(Vec<Drawing>),
}

pub struct KlineChart {
    pub ticker_info: TickerInfo,
    chart: ViewState,
    data_source: PlotData<KlineDataPoint>,
    raw_trades: Vec<Trade>,
    indicators: EnumMap<KlineIndicator, Option<Box<dyn KlineIndicatorImpl>>>,
    // === Scripted Indicator Engine Subsystem ===
    pub indicator_instances: data::chart::kline::indicators::IndicatorInstanceStore,
    pub indicator_host: data::chart::kline::indicators::IndicatorHost,
    pub script_panel_caches:
        rustc_hash::FxHashMap<data::chart::kline::indicators::IndicatorInstanceId, Caches>,
    // ===========================================
    fetching_trades: (bool, Vec<Handle>),
    active_trade_fetches: usize,
    pub(crate) kind: KlineChartKind,
    pub(crate) config: data::chart::kline::Config,
    request_handler: RequestHandler,
    study_configurator: study::Configurator<FootprintStudy>,
    last_tick: Instant,
    liquidation_heatmap: LiquidationHeatmap,
    pub(crate) tpo_cache: RefCell<TpoCache>,
    pub(crate) simulated_footprint_cache:
        RefCell<rustc_hash::FxHashMap<u64, (PriceStep, std::sync::Arc<KlineTrades>)>>,
    last_footprint_cache_save: Option<Instant>,
    pub drawings: Vec<Drawing>,
    pub undo_stack: Vec<DrawingHistoryAction>,
    pub redo_stack: Vec<DrawingHistoryAction>,
    pub(crate) drawing_state: RefCell<DrawingState>,
    pub alerts: Vec<PriceAlert>,
    pub alert_drag_state: RefCell<Option<(uuid::Uuid, f32)>>,
    pub replay: Option<ReplayState>,
    pub expanded_indicator: Option<KlineIndicator>,
    pub(crate) history_lookback_input: String,
    pub is_user_panning: bool,
    pub(crate) last_nudge: Option<(Vec<uuid::Uuid>, Instant)>,
}

impl KlineChart {
    pub fn get_or_simulate_footprint(
        &self,
        kline: &exchange::Kline,
        step: PriceStep,
    ) -> std::sync::Arc<KlineTrades> {
        let mut cache = self.simulated_footprint_cache.borrow_mut();
        if let Some((cached_step, footprint)) = cache.get(&kline.time)
            && *cached_step == step
        {
            return std::sync::Arc::clone(footprint);
        }
        if cache.len() > 10_000 {
            cache.clear();
        }
        let sim = std::sync::Arc::new(KlineTrades::simulate_from_kline(kline, step));
        cache.insert(kline.time, (step, std::sync::Arc::clone(&sim)));
        sim
    }
    #[inline]
    pub fn right_margin_offset(&self) -> f32 {
        Self::calc_right_margin_offset(&self.kind, self.chart.cell_width, self.chart.scaling)
    }

    #[inline]
    pub fn calc_right_margin_offset(kind: &KlineChartKind, cell_width: f32, scaling: f32) -> f32 {
        let safe_scaling = scaling.max(0.01);
        match kind {
            KlineChartKind::Footprint { .. } => {
                // Footprint cluster has width cell_width. Center is at 0.5 * cell_width.
                // Keeping center offset at 0.5 * cell_width + 90.0 guarantees 90px of empty space
                // between the right border of the cluster and the price scale ruler.
                (cell_width * 0.5 + 90.0) / safe_scaling
            }
            KlineChartKind::Candles => {
                // TradingView standard: 10 to 14 bars (~120 to 160 screen pixels of right margin padding).
                (12.0 * cell_width.clamp(4.0, 8.0) + 72.0) / safe_scaling
            }
            KlineChartKind::Tpo { .. } => (8.0 * cell_width.clamp(4.0, 12.0) + 64.0) / safe_scaling,
        }
    }

    #[inline]
    pub fn base_x(&self) -> f32 {
        0.5 * (self.chart.bounds.width / self.chart.scaling) - self.right_margin_offset()
    }

    pub fn new(
        layout: ViewConfig,
        basis: Basis,
        tick_size: f32,
        klines_raw: &[Kline],
        raw_trades: Vec<Trade>,
        enabled_indicators: &[KlineIndicator],
        ticker_info: TickerInfo,
        kind: &KlineChartKind,
        config: Option<data::chart::kline::Config>,
    ) -> Self {
        let config = config.unwrap_or_default();
        let history_lookback_input = match kind {
            KlineChartKind::Footprint {
                history_lookback_days: Some(days),
                ..
            } => days.to_string(),
            KlineChartKind::Footprint { .. } => {
                exchange::fetcher::trade_history_lookback_days().to_string()
            }
            _ => String::new(),
        };
        match basis {
            Basis::Time(interval) => {
                let step = PriceStep::from_f32(tick_size);

                let timeseries = TimeSeries::<KlineDataPoint>::new(interval, step, klines_raw)
                    .with_exchange(ticker_info.exchange())
                    .with_trades(&raw_trades);

                let base_price_y = timeseries.base_price();
                let latest_x = timeseries.latest_timestamp().unwrap_or(0);
                let (scale_high, scale_low) = timeseries.price_scale({
                    match kind {
                        KlineChartKind::Footprint { .. } => 12,
                        KlineChartKind::Candles => 60,
                        KlineChartKind::Tpo { .. } => 30,
                    }
                });

                let low_rounded = scale_low.round_to_side_step(true, step);
                let high_rounded = scale_high.round_to_side_step(false, step);

                let y_ticks = Price::steps_between_inclusive(low_rounded, high_rounded, step)
                    .map(|n| n.saturating_sub(1))
                    .unwrap_or(1)
                    .max(1) as f32;

                let cell_width = match kind {
                    KlineChartKind::Footprint { .. } => 80.0,
                    KlineChartKind::Candles => 4.0,
                    KlineChartKind::Tpo { .. } => 8.0,
                };
                let cell_height = match kind {
                    KlineChartKind::Footprint { .. } => 800.0 / y_ticks,
                    KlineChartKind::Candles => 200.0 / y_ticks,
                    KlineChartKind::Tpo { .. } => 400.0 / y_ticks,
                };

                let mut chart = ViewState::new(
                    basis,
                    step,
                    count_decimals(tick_size),
                    ticker_info,
                    ViewConfig {
                        splits: layout.splits,
                        autoscale: Some(Autoscale::FitToVisible),
                    },
                    cell_width,
                    cell_height,
                );
                chart.base_price_y = base_price_y;
                chart.latest_x = latest_x;

                let x_translation = 0.5 * (chart.bounds.width / chart.scaling)
                    - Self::calc_right_margin_offset(kind, chart.cell_width, chart.scaling);
                chart.translation.x = x_translation;

                let data_source = PlotData::TimeBased(timeseries);

                let mut indicators = EnumMap::default();
                for &i in enabled_indicators {
                    let mut indi = indicator::kline::make_empty(i);
                    if i == KlineIndicator::PositionFlow
                        && let Some(any) = indi.as_any_mut()
                        && let Some(pf) = any.downcast_mut::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>()
                    {
                        pf.set_colors(config.position_flow_colors);
                    }
                    if i == KlineIndicator::RollingVwap
                        && let Some(any) = indi.as_any_mut()
                        && let Some(rv) = any.downcast_mut::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
                    {
                        rv.set_colors(config.rolling_vwap_colors);
                        rv.set_window_hours(config.rolling_vwap_window_hours, &data_source);
                    }
                    crate::chart::indicator::kline::apply_chart_config(indi.as_mut(), &config);
                    indi.rebuild_from_source(&data_source);
                    indicators[i] = Some(indi);
                }

                let mut liquidation_heatmap =
                    LiquidationHeatmap::new(LiquidationHeatmapConfig::default());
                let liq_enabled = indicators[KlineIndicator::LiquidationHeatmap].is_some();
                liquidation_heatmap.config.enabled = liq_enabled;
                liquidation_heatmap.config.oi_weighting = config.liq_weight_by_oi;
                // Rebuild heatmap from existing klines
                if liq_enabled && let PlotData::TimeBased(ref ts) = data_source {
                    let kline_data: Vec<_> = ts
                        .datapoints
                        .iter()
                        .map(|(time, dp)| {
                            (
                                *time,
                                dp.kline.open.to_f32(),
                                dp.kline.high.to_f32(),
                                dp.kline.low.to_f32(),
                                dp.kline.close.to_f32(),
                                dp.kline.volume_f32().0,
                                dp.kline.volume_f32().1,
                            )
                        })
                        .collect();
                    liquidation_heatmap.rebuild_from_klines(&kline_data);
                }

                KlineChart {
                    ticker_info,
                    chart,
                    data_source,
                    raw_trades,
                    indicators,
                    indicator_instances:
                        data::chart::kline::indicators::IndicatorInstanceStore::default(),
                    indicator_host: data::chart::kline::indicators::IndicatorHost::default(),
                    script_panel_caches: rustc_hash::FxHashMap::default(),
                    fetching_trades: (false, Vec::new()),
                    active_trade_fetches: 0,
                    request_handler: RequestHandler::new(),
                    kind: kind.clone(),
                    config,
                    study_configurator: study::Configurator::new(),
                    last_tick: Instant::now(),
                    liquidation_heatmap,
                    tpo_cache: RefCell::new(TpoCache::default()),
                    simulated_footprint_cache: RefCell::new(rustc_hash::FxHashMap::default()),
                    last_footprint_cache_save: None,
                    drawings: Vec::new(),
                    undo_stack: Vec::new(),
                    redo_stack: Vec::new(),
                    drawing_state: RefCell::new(DrawingState::default()),
                    alerts: Vec::new(),
                    alert_drag_state: RefCell::new(None),
                    replay: None,
                    expanded_indicator: None,
                    history_lookback_input: history_lookback_input.clone(),
                    is_user_panning: false,
                    last_nudge: None,
                }
            }
            Basis::Tick(interval) => {
                let step = PriceStep::from_f32(tick_size);

                let cell_width = match kind {
                    KlineChartKind::Footprint { .. } => 80.0,
                    KlineChartKind::Candles => 4.0,
                    KlineChartKind::Tpo { .. } => 8.0,
                };
                let cell_height = match kind {
                    KlineChartKind::Footprint { .. } => 90.0,
                    KlineChartKind::Candles => 8.0,
                    KlineChartKind::Tpo { .. } => 16.0,
                };

                let mut chart = ViewState::new(
                    basis,
                    step,
                    count_decimals(tick_size),
                    ticker_info,
                    ViewConfig {
                        splits: layout.splits,
                        autoscale: Some(Autoscale::FitToVisible),
                    },
                    cell_width,
                    cell_height,
                );

                let x_translation = 0.5 * (chart.bounds.width / chart.scaling)
                    - Self::calc_right_margin_offset(kind, chart.cell_width, chart.scaling);
                chart.translation.x = x_translation;

                let data_source = PlotData::TickBased(TickAggr::new(interval, step, &raw_trades));

                let mut indicators = EnumMap::default();
                for &i in enabled_indicators {
                    let mut indi = indicator::kline::make_empty(i);
                    if i == KlineIndicator::PositionFlow
                        && let Some(any) = indi.as_any_mut()
                        && let Some(pf) = any.downcast_mut::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>()
                    {
                        pf.set_colors(config.position_flow_colors);
                    }
                    if i == KlineIndicator::RollingVwap
                        && let Some(any) = indi.as_any_mut()
                        && let Some(rv) = any.downcast_mut::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
                    {
                        rv.set_colors(config.rolling_vwap_colors);
                        rv.set_window_hours(config.rolling_vwap_window_hours, &data_source);
                    }
                    crate::chart::indicator::kline::apply_chart_config(indi.as_mut(), &config);
                    indi.rebuild_from_source(&data_source);
                    indicators[i] = Some(indi);
                }

                // Liquidation heatmap not supported for tick-based charts
                let liquidation_heatmap =
                    LiquidationHeatmap::new(LiquidationHeatmapConfig::default());

                KlineChart {
                    ticker_info,
                    chart,
                    data_source,
                    raw_trades,
                    indicators,
                    indicator_instances:
                        data::chart::kline::indicators::IndicatorInstanceStore::default(),
                    indicator_host: data::chart::kline::indicators::IndicatorHost::default(),
                    script_panel_caches: rustc_hash::FxHashMap::default(),
                    fetching_trades: (false, Vec::new()),
                    active_trade_fetches: 0,
                    request_handler: RequestHandler::new(),
                    kind: kind.clone(),
                    config,
                    study_configurator: study::Configurator::new(),
                    last_tick: Instant::now(),
                    liquidation_heatmap,
                    tpo_cache: RefCell::new(TpoCache::default()),
                    simulated_footprint_cache: RefCell::new(rustc_hash::FxHashMap::default()),
                    last_footprint_cache_save: None,
                    drawings: Vec::new(),
                    undo_stack: Vec::new(),
                    redo_stack: Vec::new(),
                    drawing_state: RefCell::new(DrawingState::default()),
                    alerts: Vec::new(),
                    alert_drag_state: RefCell::new(None),
                    replay: None,
                    expanded_indicator: None,
                    history_lookback_input,
                    is_user_panning: false,
                    last_nudge: None,
                }
            }
        }
    }

    pub fn history_lookback_input(&self) -> &str {
        &self.history_lookback_input
    }

    pub fn effective_history_lookback_days(&self) -> u32 {
        match &self.kind {
            KlineChartKind::Footprint {
                history_lookback_days: Some(days),
                ..
            } => *days,
            _ => exchange::fetcher::trade_history_lookback_days(),
        }
    }

    pub fn set_history_lookback_days(&mut self, days: u32) {
        let days = days.max(1);
        self.history_lookback_input = days.to_string();
        if let KlineChartKind::Footprint {
            history_lookback_days,
            ..
        } = &mut self.kind
        {
            *history_lookback_days = Some(days);
        }
        exchange::fetcher::set_trade_history_lookback_days(days);
        self.reset_request_handler();
        self.invalidate_unfetched_empty_footprints();
        self.invalidate_all();
    }

    pub fn set_history_lookback_input(&mut self, input: String) {
        self.history_lookback_input = input;
        if let Ok(days) = self.history_lookback_input.trim().parse::<u32>()
            && days > 0
        {
            if let KlineChartKind::Footprint {
                history_lookback_days,
                ..
            } = &mut self.kind
            {
                *history_lookback_days = Some(days);
            }
            exchange::fetcher::set_trade_history_lookback_days(days);
            self.reset_request_handler();
            self.invalidate_unfetched_empty_footprints();
            self.invalidate_all();
        }
    }

    pub fn data_source(&self) -> &PlotData<KlineDataPoint> {
        &self.data_source
    }

    pub fn rebuild_liquidation_heatmap(&mut self) {
        if !self.liquidation_heatmap.config.enabled {
            return;
        }
        if let PlotData::TimeBased(ref ts) = self.data_source {
            let kline_data: Vec<_> = ts
                .datapoints
                .iter()
                .map(|(time, dp)| {
                    (
                        *time,
                        dp.kline.open.to_f32(),
                        dp.kline.high.to_f32(),
                        dp.kline.low.to_f32(),
                        dp.kline.close.to_f32(),
                        dp.kline.volume_f32().0,
                        dp.kline.volume_f32().1,
                    )
                })
                .collect();
            self.liquidation_heatmap.rebuild_from_klines(&kline_data);
        }
    }

    pub fn update_latest_kline(&mut self, kline: &Kline) {
        match self.data_source {
            PlotData::TimeBased(ref mut timeseries) => {
                timeseries.insert_klines(&[*kline]);

                self.indicators
                    .values_mut()
                    .filter_map(Option::as_mut)
                    .for_each(|indi| indi.on_insert_klines(&[*kline]));

                if self.liquidation_heatmap.config.enabled {
                    self.liquidation_heatmap.on_candle(
                        kline.time,
                        kline.open.to_f32(),
                        kline.high.to_f32(),
                        kline.low.to_f32(),
                        kline.close.to_f32(),
                        kline.volume_f32().0,
                        kline.volume_f32().1,
                    );
                }

                let chart = self.mut_state();

                if (kline.time) > chart.latest_x {
                    chart.latest_x = kline.time;
                }

                if chart.base_price_y.to_f32_lossy() == 0.0 {
                    chart.base_price_y = kline.close;
                }

                chart.last_price = Some(PriceInfoLabel::new(kline.close, kline.open));
            }
            PlotData::TickBased(_) => {}
        }

        self.invalidate(None);
    }

    pub fn kind(&self) -> &KlineChartKind {
        &self.kind
    }

    pub fn current_price(&self) -> Option<f32> {
        self.chart
            .last_price
            .as_ref()
            .map(|p| p.price().to_f32_lossy())
    }

    fn missing_data_task(&mut self) -> Option<Action> {
        let (timeframe_ms, is_empty, kline_range) = match &self.data_source {
            PlotData::TimeBased(ts) => (
                ts.interval.to_milliseconds(),
                ts.datapoints.is_empty(),
                ts.timerange(),
            ),
            PlotData::TickBased(_) => return None,
        };

        if is_empty || self.chart.latest_x == 0 {
            let latest = chrono::Utc::now().timestamp_millis() as u64;
            let earliest = latest.saturating_sub(450 * timeframe_ms);

            let range = FetchRange::Kline(earliest, latest);
            return request_fetch(&mut self.request_handler, range);
        }

        let (visible_earliest, visible_latest) = self.visible_timerange().unwrap_or_else(|| {
            let latest = if self.chart.latest_x > 0 {
                self.chart.latest_x
            } else {
                chrono::Utc::now().timestamp_millis() as u64
            };
            let earliest = latest.saturating_sub(60 * timeframe_ms);
            (earliest, latest)
        });
        let (kline_earliest, kline_latest) = kline_range;
        let earliest =
            visible_earliest.saturating_sub(visible_latest.saturating_sub(visible_earliest));

        // priority 1, basic kline data fetch
        if let PlotData::TimeBased(ts) = &self.data_source {
            let first_in_view = ts
                .datapoints
                .range(visible_earliest..=visible_latest)
                .next()
                .map(|(&k, _)| k);

            let now_ms = exchange::clock::server_time_ms(Some(self.ticker_info.exchange()));
            if first_in_view.is_none() && visible_earliest > 0 {
                let fetch_start = visible_earliest.saturating_sub(50 * timeframe_ms);
                let fetch_end = fetch_start
                    .saturating_add(1000 * timeframe_ms)
                    .min(now_ms.saturating_add(timeframe_ms * 2));
                if fetch_start < fetch_end {
                    let range = FetchRange::Kline(fetch_start, fetch_end);
                    if let Some(action) = request_fetch(&mut self.request_handler, range) {
                        return Some(action);
                    }
                }
            } else if let Some(first) = first_in_view
                && visible_earliest < first
                && visible_earliest > 0
            {
                if earliest < first {
                    let range = FetchRange::Kline(earliest, first);
                    if let Some(action) = request_fetch(&mut self.request_handler, range) {
                        return Some(action);
                    }
                }
            } else {
                let target_latest = visible_latest.min(now_ms);
                if target_latest > kline_latest.saturating_add(timeframe_ms / 2) {
                    let fetch_start = kline_latest.saturating_sub(timeframe_ms);
                    let fetch_end = target_latest.saturating_add(timeframe_ms * 5);
                    if fetch_start < fetch_end {
                        let range = FetchRange::Kline(fetch_start, fetch_end);
                        if let Some(action) = request_fetch(&mut self.request_handler, range) {
                            return Some(action);
                        }
                    }
                }
            }
        } else if visible_earliest < kline_earliest
            && visible_earliest > 0
            && kline_earliest > 0
            && earliest < kline_earliest
        {
            let range = FetchRange::Kline(earliest, kline_earliest);

            if let Some(action) = request_fetch(&mut self.request_handler, range) {
                return Some(action);
            }
        }

        // priority 2, trades fetch (Footprint only)
        if matches!(self.kind, KlineChartKind::Footprint { .. }) {
            let is_aggregate = self.is_aggregate_footprint();
            let mut trade_fetch_range = None;
            let mut needs_invalidation = false;
            let custom_lookback = match &self.kind {
                KlineChartKind::Footprint {
                    history_lookback_days,
                    ..
                } => *history_lookback_days,
                _ => None,
            };

            if let PlotData::TimeBased(ref mut timeseries) = self.data_source
                && let Some((mut fetch_from, mut fetch_to)) = timeseries
                    .suggest_trade_fetch_range_with_exchange_and_lookback(
                        visible_earliest,
                        visible_latest,
                        Some(self.ticker_info.exchange()),
                        custom_lookback,
                    )
            {
                let (symbol, _) = self.ticker_info.ticker.to_full_symbol_and_type();
                let base_data_path = data::data_path(None);
                let exchange_folder = match self.ticker_info.exchange() {
                    exchange::adapter::Exchange::BinanceSpot
                    | exchange::adapter::Exchange::BinanceLinear
                    | exchange::adapter::Exchange::BinanceInverse => "binance",
                    exchange::adapter::Exchange::BybitSpot
                    | exchange::adapter::Exchange::BybitLinear
                    | exchange::adapter::Exchange::BybitInverse => "bybit",
                    exchange::adapter::Exchange::OkexSpot
                    | exchange::adapter::Exchange::OkexLinear
                    | exchange::adapter::Exchange::OkexInverse => "okex",
                    exchange::adapter::Exchange::HyperliquidSpot
                    | exchange::adapter::Exchange::HyperliquidLinear => "hyperliquid",
                    exchange::adapter::Exchange::MexcSpot
                    | exchange::adapter::Exchange::MexcLinear
                    | exchange::adapter::Exchange::MexcInverse => "mexc",
                    exchange::adapter::Exchange::CoinbaseSpot => "coinbase",
                };
                let trades_data_path =
                    data::data_path(Some(&format!("market_data/{exchange_folder}/")));
                let interval = timeseries.interval;
                let step = self.chart.tick_size;

                let from_d = chrono::DateTime::from_timestamp_millis(fetch_from as i64)
                    .map(|dt| dt.date_naive());
                let to_d = chrono::DateTime::from_timestamp_millis(fetch_to as i64)
                    .map(|dt| dt.date_naive());

                if let (Some(start_d), Some(end_d)) = (from_d, to_d) {
                    let mut cur_d = start_d;
                    let mut loaded_any = false;
                    let today = chrono::Utc::now().date_naive();

                    let cache_symbol = if is_aggregate {
                        format!("{symbol}-AGG")
                    } else {
                        symbol.clone()
                    };

                    while cur_d <= end_d && cur_d <= today {
                        let cache_path = data::chart::kline::footprint_cache_path(
                            &base_data_path,
                            &cache_symbol,
                            interval,
                            step,
                            cur_d,
                        );
                        if cur_d < today
                            && cache_path.exists()
                            && let Some(dps) = data::chart::kline::load_daily_footprint(&cache_path)
                        {
                            timeseries.insert_preaggregated_footprint(dps);
                            loaded_any = true;
                        } else if let Some(trades) =
                            exchange::trades::cache::load_raw_trades_from_cache(
                                &trades_data_path,
                                &self.ticker_info,
                                cur_d,
                            )
                        {
                            let dps =
                                data::aggr::time::aggregate_trades_for_day(&trades, interval, step);
                            let interval_ms = interval.to_milliseconds();
                            let expected_count =
                                86_400_000u64.checked_div(interval_ms).unwrap_or(1) as usize;
                            let is_day_complete = dps.len() >= expected_count
                                && dps.iter().all(|(_, dp)| {
                                    (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) == 0.0
                                        || !dp.footprint.is_empty()
                                });

                            if !dps.is_empty() {
                                if cur_d < today && is_day_complete {
                                    let _ =
                                        data::chart::kline::save_daily_footprint(&cache_path, &dps);
                                }
                                timeseries.insert_preaggregated_footprint(dps);
                                loaded_any = true;
                            }
                        } else if let Some(trades) =
                            exchange::trades::cache::load_intraday_trades_from_cache(
                                &trades_data_path,
                                &self.ticker_info,
                                cur_d,
                            )
                        {
                            let dps =
                                data::aggr::time::aggregate_trades_for_day(&trades, interval, step);
                            if !dps.is_empty() {
                                let interval_ms = interval.to_milliseconds();
                                let expected_count =
                                    86_400_000u64.checked_div(interval_ms).unwrap_or(1) as usize;
                                let is_day_complete = dps.len() >= expected_count
                                    && dps.iter().all(|(_, dp)| {
                                        (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) == 0.0
                                            || !dp.footprint.is_empty()
                                    });

                                if cur_d < today && is_day_complete {
                                    let _ =
                                        data::chart::kline::save_daily_footprint(&cache_path, &dps);
                                    let _ = exchange::trades::cache::save_raw_trades_to_cache(
                                        &trades_data_path,
                                        &self.ticker_info,
                                        cur_d,
                                        &trades,
                                    );
                                }
                                timeseries.insert_preaggregated_footprint(dps);
                                if cur_d == today {
                                    self.raw_trades.extend(trades);
                                    self.raw_trades.sort_by_key(|t| t.time);
                                    self.raw_trades.dedup_by(|a, b| {
                                        a.time == b.time && a.price == b.price && a.qty == b.qty
                                    });
                                }
                                loaded_any = true;
                            }
                        }
                        match cur_d.succ_opt() {
                            Some(next_d) => cur_d = next_d,
                            None => break,
                        }
                    }

                    if loaded_any {
                        needs_invalidation = true;
                        if let Some((new_from, new_to)) = timeseries
                            .suggest_trade_fetch_range_with_exchange_and_lookback(
                                visible_earliest,
                                visible_latest,
                                Some(self.ticker_info.exchange()),
                                custom_lookback,
                            )
                        {
                            fetch_from = new_from;
                            fetch_to = new_to;
                        } else {
                            fetch_from = 0;
                            fetch_to = 0;
                        }
                    }
                }

                if fetch_from < fetch_to
                    && !self.is_fetching_trades()
                    && exchange::fetcher::is_trade_fetch_enabled()
                {
                    trade_fetch_range = Some((fetch_from, fetch_to));
                }
            }

            if needs_invalidation {
                self.invalidate(None);
            }

            if let Some((fetch_from, fetch_to)) = trade_fetch_range {
                let range = FetchRange::Trades(fetch_from, fetch_to);
                if let Some(action) = request_fetch(&mut self.request_handler, range) {
                    self.fetching_trades.0 = true;
                    return Some(action);
                }
            }
        }

        // priority 3, Open Interest data
        let timeframe = match &self.data_source {
            PlotData::TimeBased(ts) => ts.interval,
            PlotData::TickBased(_) => return None,
        };
        let ctx = indicator::kline::FetchCtx {
            main_chart: &self.chart,
            timeframe,
            visible_earliest,
            kline_latest,
            prefetch_earliest: earliest,
        };
        for indi in self.indicators.values_mut().filter_map(Option::as_mut) {
            if let Some(range) = indi.fetch_range(&ctx)
                && let Some(action) = request_fetch(&mut self.request_handler, range)
            {
                return Some(action);
            }
        }

        // priority 4, missing klines & integrity check
        if let PlotData::TimeBased(ref timeseries) = self.data_source
            && let Some(missing_keys) =
                timeseries.check_kline_integrity(kline_earliest, kline_latest, timeframe_ms)
        {
            let latest = missing_keys.iter().max().unwrap_or(&visible_latest) + timeframe_ms;
            let earliest = missing_keys
                .iter()
                .min()
                .unwrap_or(&visible_earliest)
                .saturating_sub(timeframe_ms);

            let range = FetchRange::Kline(earliest, latest);
            if let Some(action) = request_fetch(&mut self.request_handler, range) {
                return Some(action);
            }
        }

        None
    }

    pub fn is_fetching_trades(&self) -> bool {
        self.fetching_trades.0 || self.active_trade_fetches > 0
    }

    pub fn reset_request_handler(&mut self) {
        self.request_handler = RequestHandler::new();
        self.reset_fetching_trades();
    }

    pub fn reset_fetching_trades(&mut self) {
        self.fetching_trades = (false, Vec::new());
        self.active_trade_fetches = 0;
    }

    pub fn raw_trades(&self) -> Vec<Trade> {
        self.raw_trades.clone()
    }

    pub fn set_handle(&mut self, handle: Handle) {
        self.add_trade_fetch_handle(handle);
    }

    pub fn add_trade_fetch_handle(&mut self, handle: Handle) {
        self.fetching_trades.0 = true;
        self.active_trade_fetches += 1;
        self.fetching_trades.1.push(handle);
    }

    pub fn finish_one_trade_fetch(&mut self) {
        self.active_trade_fetches = self.active_trade_fetches.saturating_sub(1);
        if self.active_trade_fetches == 0 {
            self.fetching_trades = (false, Vec::new());
        }
    }

    pub fn mark_trades_fetched(&mut self, from_time: u64, to_time: u64) {
        if let PlotData::TimeBased(ref mut ts) = self.data_source {
            ts.mark_trades_fetched(from_time, to_time);
        }
    }

    pub fn invalidate_unfetched_empty_footprints(&mut self) {
        if let PlotData::TimeBased(ref mut ts) = self.data_source {
            ts.invalidate_unfetched_empty_footprints();
        }
    }

    pub fn tick_size(&self) -> f32 {
        self.chart.tick_size.to_f32_lossy()
    }

    pub fn ticker_info(&self) -> TickerInfo {
        self.ticker_info
    }

    pub fn study_configurator(&self) -> &study::Configurator<FootprintStudy> {
        &self.study_configurator
    }

    pub fn add_drawing(&mut self, drawing: Drawing) {
        self.drawings.push(drawing);
        self.invalidate_all();
    }

    pub fn update_drawing(&mut self, drawing: Drawing) {
        if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == drawing.id) {
            *existing = drawing;
            self.invalidate_all();
        }
    }

    pub fn update_or_add_drawing(&mut self, drawing: Drawing) {
        if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == drawing.id) {
            *existing = drawing;
        } else {
            self.drawings.push(drawing);
        }
        self.invalidate_all();
    }

    pub fn delete_drawing(&mut self, id: uuid::Uuid) {
        self.drawings.retain(|d| d.id != id);
        {
            let mut d = self.drawing_state.borrow_mut();
            if d.selected_drawing == Some(id) {
                d.selected_drawing = None;
            }
        }
        self.invalidate_all();
    }

    pub fn update_alert_price(&mut self, id: uuid::Uuid, new_price: f32) {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == id) {
            alert.target_price = new_price;
        }
        self.invalidate_all();
    }

    pub fn clear_drawings(&mut self) {
        self.drawings.retain(|d| d.is_locked);
        let sel_id = self.drawing_state.borrow().selected_drawing;
        if let Some(id) = sel_id
            && !self.drawings.iter().any(|d| d.id == id)
        {
            self.drawing_state.borrow_mut().selected_drawing = None;
        }
        self.drawing_state.borrow_mut().in_progress = None;
        self.invalidate_all();
    }

    pub fn selected_drawing(&self) -> Option<&Drawing> {
        let sel_id = self.drawing_state.borrow().selected_drawing;
        sel_id.and_then(|id| self.drawings.iter().find(|d| d.id == id))
    }

    pub fn selected_drawing_mut(&mut self) -> Option<&mut Drawing> {
        let sel_id = self.drawing_state.borrow().selected_drawing;
        sel_id.and_then(|id| self.drawings.iter_mut().find(|d| d.id == id))
    }

    pub fn set_selected_drawing(&mut self, id: Option<uuid::Uuid>) {
        self.drawing_state.borrow_mut().selected_drawing = id;
        self.invalidate_all();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn nudge_selected_drawings(
        &mut self,
        time_delta: i64,
        price_delta: f32,
    ) -> Option<(Vec<Drawing>, Vec<Drawing>)> {
        let now = Instant::now();
        let selected_unlocked: Vec<usize> = self
            .drawings
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                (self.drawing_state.borrow().selected_drawing == Some(d.id) || d.is_selected)
                    && !d.is_locked
            })
            .map(|(idx, _)| idx)
            .collect();

        if selected_unlocked.is_empty() {
            return None;
        }

        let current_ids: Vec<uuid::Uuid> = selected_unlocked
            .iter()
            .map(|&idx| self.drawings[idx].id)
            .collect();

        if selected_unlocked.len() == 1 {
            let idx = selected_unlocked[0];
            let id = self.drawings[idx].id;
            let is_burst = self.last_nudge.as_ref().is_some_and(|(prev_ids, prev_t)| {
                prev_ids.len() == 1
                    && prev_ids[0] == id
                    && now.duration_since(*prev_t).as_millis() <= 500
            });
            self.last_nudge = Some((vec![id], now));

            if is_burst
                && let Some(DrawingHistoryAction::Update { old: _, new }) =
                    self.undo_stack.last_mut()
                && new.id == id
            {
                self.drawings[idx].translate(time_delta, price_delta);
                *new = self.drawings[idx].clone();
                self.redo_stack.clear();
            } else {
                let old = self.drawings[idx].clone();
                self.drawings[idx].translate(time_delta, price_delta);
                let new = self.drawings[idx].clone();
                self.push_undo(DrawingHistoryAction::Update { old, new });
            }
            let updated = self.drawings[idx].clone();
            let synced = if updated.is_synced {
                vec![updated.clone()]
            } else {
                Vec::new()
            };
            self.invalidate_all();
            Some((vec![updated], synced))
        } else {
            let is_burst = self.last_nudge.as_ref().is_some_and(|(prev_ids, prev_t)| {
                prev_ids == &current_ids && now.duration_since(*prev_t).as_millis() <= 500
            });
            self.last_nudge = Some((current_ids, now));

            if is_burst
                && let Some(DrawingHistoryAction::BatchUpdate(pairs)) = self.undo_stack.last_mut()
                && pairs.len() == selected_unlocked.len()
            {
                for &idx in &selected_unlocked {
                    self.drawings[idx].translate(time_delta, price_delta);
                }
                for (pair, &idx) in pairs.iter_mut().zip(&selected_unlocked) {
                    pair.1 = self.drawings[idx].clone();
                }
                self.redo_stack.clear();
            } else {
                let old_drawings: Vec<Drawing> = selected_unlocked
                    .iter()
                    .map(|&idx| self.drawings[idx].clone())
                    .collect();

                for &idx in &selected_unlocked {
                    self.drawings[idx].translate(time_delta, price_delta);
                }

                let new_drawings: Vec<Drawing> = selected_unlocked
                    .iter()
                    .map(|&idx| self.drawings[idx].clone())
                    .collect();

                let pairs: Vec<(Drawing, Drawing)> = old_drawings
                    .into_iter()
                    .zip(new_drawings.iter().cloned())
                    .collect();

                self.push_undo(DrawingHistoryAction::BatchUpdate(pairs));
            }

            let new_drawings: Vec<Drawing> = selected_unlocked
                .iter()
                .map(|&idx| self.drawings[idx].clone())
                .collect();

            let synced: Vec<Drawing> = new_drawings
                .iter()
                .filter(|d| d.is_synced)
                .cloned()
                .collect();

            self.invalidate_all();
            Some((new_drawings, synced))
        }
    }

    pub fn push_undo(&mut self, action: DrawingHistoryAction) {
        const MAX_UNDO_STACK: usize = 100;
        if self.undo_stack.len() >= MAX_UNDO_STACK {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> Option<DrawingHistoryAction> {
        let action = self.undo_stack.pop()?;
        match &action {
            DrawingHistoryAction::Add(d) => {
                self.drawings.retain(|existing| existing.id != d.id);
                if self.drawing_state.borrow().selected_drawing == Some(d.id) {
                    self.drawing_state.borrow_mut().selected_drawing = None;
                }
            }
            DrawingHistoryAction::Delete(d) => {
                if !self.drawings.iter().any(|existing| existing.id == d.id) {
                    self.drawings.push(d.clone());
                }
                self.drawing_state.borrow_mut().selected_drawing = Some(d.id);
            }
            DrawingHistoryAction::Update { old, .. } => {
                if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == old.id) {
                    *existing = old.clone();
                } else {
                    self.drawings.push(old.clone());
                }
                self.drawing_state.borrow_mut().selected_drawing = Some(old.id);
            }
            DrawingHistoryAction::BatchUpdate(pairs) => {
                for (old, _) in pairs {
                    if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == old.id) {
                        *existing = old.clone();
                    } else {
                        self.drawings.push(old.clone());
                    }
                }
            }
            DrawingHistoryAction::Clear(drawings) => {
                for d in drawings {
                    if !self.drawings.iter().any(|existing| existing.id == d.id) {
                        self.drawings.push(d.clone());
                    }
                }
            }
        }
        self.redo_stack.push(action.clone());
        self.invalidate_all();
        Some(action)
    }

    pub fn redo(&mut self) -> Option<DrawingHistoryAction> {
        let action = self.redo_stack.pop()?;
        match &action {
            DrawingHistoryAction::Add(d) => {
                if !self.drawings.iter().any(|existing| existing.id == d.id) {
                    self.drawings.push(d.clone());
                }
                self.drawing_state.borrow_mut().selected_drawing = Some(d.id);
            }
            DrawingHistoryAction::Delete(d) => {
                self.drawings.retain(|existing| existing.id != d.id);
                if self.drawing_state.borrow().selected_drawing == Some(d.id) {
                    self.drawing_state.borrow_mut().selected_drawing = None;
                }
            }
            DrawingHistoryAction::Update { new, .. } => {
                if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == new.id) {
                    *existing = new.clone();
                } else {
                    self.drawings.push(new.clone());
                }
                self.drawing_state.borrow_mut().selected_drawing = Some(new.id);
            }
            DrawingHistoryAction::BatchUpdate(pairs) => {
                for (_, new) in pairs {
                    if let Some(existing) = self.drawings.iter_mut().find(|d| d.id == new.id) {
                        *existing = new.clone();
                    } else {
                        self.drawings.push(new.clone());
                    }
                }
            }
            DrawingHistoryAction::Clear(drawings) => {
                let ids: Vec<_> = drawings.iter().map(|d| d.id).collect();
                self.drawings.retain(|d| !ids.contains(&d.id));
                if let Some(sel_id) = self.drawing_state.borrow().selected_drawing
                    && ids.contains(&sel_id)
                {
                    self.drawing_state.borrow_mut().selected_drawing = None;
                }
            }
        }
        self.undo_stack.push(action.clone());
        self.invalidate_all();
        Some(action)
    }

    pub fn find_target_position(&self) -> Option<data::journal::PositionAutofill> {
        let d_state = self.drawing_state.borrow();

        // 1. Dragging handle or drawing of a Position
        if let Some(ref drag) = d_state.drag
            && let Some(autofill) = self.position_drawing_to_autofill(drag.current_drawing())
        {
            return Some(autofill);
        }

        // 2. Currently in-progress drawing of a Position
        if let Some(ref in_prog) = d_state.in_progress
            && let Some(autofill) = self.position_drawing_to_autofill(in_prog)
        {
            return Some(autofill);
        }

        // 3. Selected drawing by UUID or is_selected flag
        if let Some(selected_id) = d_state.selected_drawing
            && let Some(d) = self.drawings.iter().find(|d| d.id == selected_id)
            && let Some(autofill) = self.position_drawing_to_autofill(d)
        {
            return Some(autofill);
        }
        if let Some(d) = self.drawings.iter().find(|d| d.is_selected)
            && let Some(autofill) = self.position_drawing_to_autofill(d)
        {
            return Some(autofill);
        }

        // 4. Most recent Position in chart drawings
        for d in self.drawings.iter().rev() {
            if let Some(autofill) = self.position_drawing_to_autofill(d) {
                return Some(autofill);
            }
        }

        None
    }

    fn position_drawing_to_autofill(
        &self,
        drawing: &Drawing,
    ) -> Option<data::journal::PositionAutofill> {
        if let DrawingKind::Position {
            entry,
            stop_price,
            target_price,
            is_long,
            end_time,
            ..
        } = &drawing.kind
        {
            let side = if *is_long {
                data::journal::TradeSide::Long
            } else {
                data::journal::TradeSide::Short
            };
            let ticker = self.ticker_info.ticker.to_string();
            let exchange = self.ticker_info.ticker.exchange.to_string();

            let default_interval = match &self.data_source {
                PlotData::TimeBased(ts) => ts.interval.to_milliseconds().max(60_000),
                PlotData::TickBased(_) => 1,
            };
            let end_time_val = end_time.unwrap_or(entry.0 + default_interval * 25);
            let cutoff = if let Some(ref rep) = self.replay {
                if rep.active && rep.cutoff_time > 0 {
                    Some(rep.cutoff_time)
                } else {
                    None
                }
            } else {
                None
            };
            let outcome = evaluate_position_outcome(
                &self.data_source,
                entry.0,
                entry.1,
                *target_price,
                *stop_price,
                *is_long,
                end_time_val,
                cutoff,
            );

            let (exit_price, timestamp_close) = if outcome.is_closed {
                (Some(outcome.exit_price as f64), Some(outcome.exit_time))
            } else {
                (None, None)
            };

            Some(data::journal::PositionAutofill::with_outcome(
                ticker,
                exchange,
                side,
                entry.1 as f64,
                *stop_price as f64,
                *target_price as f64,
                entry.0,
                timestamp_close,
                exit_price,
                outcome.is_closed,
                Some(drawing.id),
            ))
        } else {
            None
        }
    }

    pub fn update_selected_drawing_color(&mut self, color: [f32; 4]) {
        if let Some(d) = self.selected_drawing_mut() {
            d.color = color;
            self.invalidate_all();
        }
    }

    pub fn update_selected_position_profit_color(&mut self, color: Option<[f32; 4]>) {
        if let Some(d) = self.selected_drawing_mut() {
            d.set_position_profit_color(color);
            self.invalidate_all();
        }
    }

    pub fn update_selected_position_stop_color(&mut self, color: Option<[f32; 4]>) {
        if let Some(d) = self.selected_drawing_mut() {
            d.set_position_stop_color(color);
            self.invalidate_all();
        }
    }

    pub fn update_selected_position_entry_color(&mut self, color: [f32; 4]) {
        if let Some(d) = self.selected_drawing_mut() {
            d.set_position_entry_color(color);
            self.invalidate_all();
        }
    }

    pub fn toggle_selected_position_price_path(&mut self) {
        if let Some(d) = self.selected_drawing_mut() {
            let current = d.position_style().show_price_path;
            d.set_position_show_price_path(!current);
            self.invalidate_all();
        }
    }

    pub fn update_selected_drawing_width(&mut self, width: f32) {
        if let Some(d) = self.selected_drawing_mut() {
            d.width = width;
            self.invalidate_all();
        }
    }

    pub fn apply_template_to_selected(&mut self, template: &data::chart::drawing::DrawingTemplate) {
        if let Some(d) = self.selected_drawing_mut() {
            d.apply_template(template);
            self.invalidate_all();
        }
    }

    pub fn toggle_selected_drawing_lock(&mut self) {
        if let Some(d) = self.selected_drawing_mut() {
            d.is_locked = !d.is_locked;
            self.invalidate_all();
        }
    }

    pub fn toggle_selected_drawing_sync(&mut self, current_pane_id: uuid::Uuid) {
        if let Some(d) = self.selected_drawing_mut() {
            d.is_synced = !d.is_synced;
            if d.pane_id.is_none() {
                d.pane_id = Some(current_pane_id);
            }
            self.invalidate_all();
        }
    }

    pub fn y_axis_badges(&self) -> Vec<PriceAxisBadge> {
        let mut badges = Vec::new();
        let d_state = self.drawing_state.borrow();

        // 1. Dragging position: uses live dragged coordinates
        let mut dragged_id = None;
        if let Some(ref drag) = d_state.drag {
            let current = drag.current_drawing();
            if let DrawingKind::Position {
                entry,
                target_price,
                stop_price,
                ..
            } = &current.kind
            {
                dragged_id = Some(current.id);
                let style = current.position_style();
                let tp_color = match style.profit_color {
                    Some(c) => {
                        crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                    }
                    None => crate::chart::scale::BadgeColor::ThemeProfit,
                };
                let sl_color = match style.stop_color {
                    Some(c) => {
                        crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                    }
                    None => crate::chart::scale::BadgeColor::ThemeStop,
                };
                let entry_color = Color::from_rgb(
                    style.entry_color[0],
                    style.entry_color[1],
                    style.entry_color[2],
                );

                badges.push(PriceAxisBadge {
                    price: *target_price,
                    background_color: tp_color,
                    text_color: Color::WHITE,
                });
                badges.push(PriceAxisBadge {
                    price: *stop_price,
                    background_color: sl_color,
                    text_color: Color::WHITE,
                });
                badges.push(PriceAxisBadge {
                    price: entry.1,
                    background_color: entry_color.into(),
                    text_color: Color::WHITE,
                });
            }
        }

        // 2. In-progress position drawing
        if let Some(ref in_prog) = d_state.in_progress
            && let DrawingKind::Position {
                entry,
                target_price,
                stop_price,
                ..
            } = &in_prog.kind
        {
            let style = in_prog.position_style();
            let tp_color = match style.profit_color {
                Some(c) => {
                    crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                }
                None => crate::chart::scale::BadgeColor::ThemeProfit,
            };
            let sl_color = match style.stop_color {
                Some(c) => {
                    crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                }
                None => crate::chart::scale::BadgeColor::ThemeStop,
            };
            let entry_color = Color::from_rgb(
                style.entry_color[0],
                style.entry_color[1],
                style.entry_color[2],
            );

            badges.push(PriceAxisBadge {
                price: *target_price,
                background_color: tp_color,
                text_color: Color::WHITE,
            });
            badges.push(PriceAxisBadge {
                price: *stop_price,
                background_color: sl_color,
                text_color: Color::WHITE,
            });
            badges.push(PriceAxisBadge {
                price: entry.1,
                background_color: entry_color.into(),
                text_color: Color::WHITE,
            });
        }

        // 3. All position drawings in self.drawings (skipping the one currently being dragged)
        for d in &self.drawings {
            if Some(d.id) == dragged_id {
                continue;
            }
            if let DrawingKind::Position {
                entry,
                target_price,
                stop_price,
                ..
            } = &d.kind
            {
                let style = d.position_style();
                let tp_color = match style.profit_color {
                    Some(c) => {
                        crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                    }
                    None => crate::chart::scale::BadgeColor::ThemeProfit,
                };
                let sl_color = match style.stop_color {
                    Some(c) => {
                        crate::chart::scale::BadgeColor::Explicit(Color::from_rgb(c[0], c[1], c[2]))
                    }
                    None => crate::chart::scale::BadgeColor::ThemeStop,
                };
                let entry_color = Color::from_rgb(
                    style.entry_color[0],
                    style.entry_color[1],
                    style.entry_color[2],
                );

                badges.push(PriceAxisBadge {
                    price: *target_price,
                    background_color: tp_color,
                    text_color: Color::WHITE,
                });
                badges.push(PriceAxisBadge {
                    price: *stop_price,
                    background_color: sl_color,
                    text_color: Color::WHITE,
                });
                badges.push(PriceAxisBadge {
                    price: entry.1,
                    background_color: entry_color.into(),
                    text_color: Color::WHITE,
                });
            }
        }

        badges
    }

    pub fn set_active_drawing_tool(&mut self, tool: DrawingTool) {
        {
            let mut d = self.drawing_state.borrow_mut();
            d.active_tool = tool;
            d.in_progress = None;
            d.start_point = None;
            d.shift_pressed = false;
            d.ctrl_pressed = false;
            d.last_cursor_frame_pos = None;
        }
        self.invalidate_all();
    }

    pub fn active_drawing_tool(&self) -> DrawingTool {
        self.drawing_state.borrow().active_tool
    }

    pub fn toggle_magnet_mode(&mut self) {
        self.config.magnet_mode = !self.config.magnet_mode;
        self.invalidate_all();
    }

    pub fn is_magnet_active(&self) -> bool {
        self.config.magnet_mode ^ self.drawing_state.borrow().ctrl_pressed
    }

    pub(crate) fn snap_point_to_candle(&self, frame_pos: Point) -> (u64, f32) {
        snap_to_smart_magnet(&self.data_source, self.state(), frame_pos)
    }

    pub(crate) fn apply_handle_movement(
        &self,
        handle_idx: usize,
        initial_drawing: &Drawing,
        current_drawing: &mut Drawing,
        frame_pos: Point,
        shift: bool,
        is_magnet_active: bool,
    ) {
        let chart = self.state();
        let time = chart.x_to_interval(frame_pos.x);
        let price = chart.y_to_price(frame_pos.y).to_f32();
        let snapped = if is_magnet_active {
            self.snap_point_to_candle(frame_pos)
        } else {
            (time, price)
        };

        *current_drawing = initial_drawing.clone();
        match &mut current_drawing.kind {
            DrawingKind::HorizontalLine { price: p } => *p = snapped.1,
            DrawingKind::Trendline { p1, p2 } => {
                if handle_idx == 0 {
                    *p1 = if shift {
                        snap_to_45_degrees(*p2, frame_pos, chart)
                    } else {
                        snapped
                    };
                } else {
                    *p2 = if shift {
                        snap_to_45_degrees(*p1, frame_pos, chart)
                    } else {
                        snapped
                    };
                }
            }
            DrawingKind::Rectangle { p1, p2 } => match handle_idx {
                0 => *p1 = snapped,
                1 => p1.1 = snapped.1,
                2 => {
                    p2.0 = snapped.0;
                    p1.1 = snapped.1;
                }
                3 => p2.0 = snapped.0,
                4 => *p2 = snapped,
                5 => p2.1 = snapped.1,
                6 => {
                    p1.0 = snapped.0;
                    p2.1 = snapped.1;
                }
                7 => p1.0 = snapped.0,
                _ => {}
            },
            DrawingKind::Path { points } => {
                let anchor = if handle_idx > 0 {
                    points.get(handle_idx - 1).copied()
                } else if points.len() > 1 {
                    points.get(1).copied()
                } else {
                    None
                };
                if let Some(pt) = points.get_mut(handle_idx) {
                    *pt = if shift && let Some(anc) = anchor {
                        snap_to_45_degrees(anc, frame_pos, chart)
                    } else {
                        snapped
                    };
                }
            }
            DrawingKind::Brush { .. } => {}
            DrawingKind::Position {
                entry,
                target_price,
                stop_price,
                end_time,
                ..
            } => match handle_idx {
                0 => {
                    let min_interval = match &self.data_source {
                        PlotData::TimeBased(ts) => ts.interval.to_milliseconds().max(60_000),
                        PlotData::TickBased(_) => 1,
                    };
                    let et = end_time.unwrap_or(entry.0 + min_interval * 25);
                    entry.0 = snapped.0.min(et.saturating_sub(min_interval));
                    entry.1 = snapped.1;
                    *end_time = Some(et);
                }
                1 => *target_price = snapped.1,
                2 => *stop_price = snapped.1,
                3 => {
                    let min_interval = match &self.data_source {
                        PlotData::TimeBased(ts) => ts.interval.to_milliseconds().max(60_000),
                        PlotData::TickBased(_) => 1,
                    };
                    let target_time = if is_magnet_active {
                        if time > chart.latest_x {
                            let diff = time - chart.latest_x;
                            let candles = (diff as f64 / min_interval as f64).round() as u64;
                            chart.latest_x + candles * min_interval
                        } else {
                            snapped.0
                        }
                    } else {
                        time
                    };
                    *end_time = Some(target_time.max(entry.0 + min_interval));
                }
                _ => {}
            },
        }
    }

    pub fn add_alert(&mut self, alert: PriceAlert) {
        self.alerts.push(alert);
        self.invalidate_all();
    }

    pub fn remove_alert(&mut self, id: uuid::Uuid) {
        self.alerts.retain(|a| a.id != id);
        self.invalidate_all();
    }

    pub fn toggle_alert(&mut self, id: uuid::Uuid) {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == id) {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            alert.status = match alert.status {
                AlertStatus::Active => AlertStatus::Muted,
                AlertStatus::Muted | AlertStatus::Triggered => {
                    alert.triggered_at = None;
                    alert.last_checked_time = Some(now);
                    AlertStatus::Active
                }
            };
        }
        self.invalidate_all();
    }

    pub fn toggle_replay(&mut self) {
        if self.replay.is_some() {
            self.replay = None;
        } else {
            let cutoff = match &self.data_source {
                PlotData::TimeBased(ts) => ts.latest_timestamp().unwrap_or(0),
                PlotData::TickBased(_) => 0,
            };
            self.replay = Some(ReplayState::new(cutoff));
        }
        self.tpo_cache.borrow_mut().sessions.clear();
        self.recalculate_scripted_indicators_replay();
        self.invalidate_all();
    }

    pub fn is_replay_active(&self) -> bool {
        self.replay.as_ref().is_some_and(|r| r.active)
    }

    pub fn replay_state(&self) -> Option<&ReplayState> {
        self.replay.as_ref()
    }

    pub fn auto_scroll_to_replay_cutoff(&mut self) {
        if let Some(rep) = &self.replay
            && rep.active
        {
            let cutoff = rep.cutoff_time;
            if self.chart.scaling > f32::EPSILON && self.chart.bounds.width > f32::EPSILON {
                let base_x = self.base_x();
                let x_offset = self.chart.interval_to_x(cutoff);
                self.chart.translation.x = base_x - x_offset;
            }
        }
    }

    pub fn on_user_pan(&mut self, translation: Vector) {
        self.is_user_panning = true;
        if let Some((active, cutoff)) = self.replay.as_ref().map(|r| (r.active, r.cutoff_time))
            && active
        {
            let target_x = self.base_x() - self.chart.interval_to_x(cutoff);
            let threshold = (self.chart.cell_width * 2.5).max(50.0);
            if let Some(ref mut rep) = self.replay {
                rep.auto_scroll = (translation.x - target_x).abs() <= threshold;
            }
        }
    }

    pub fn on_pan_released(&mut self) {
        self.is_user_panning = false;
        let mut should_snap = false;
        if let Some((active, cutoff)) = self.replay.as_ref().map(|r| (r.active, r.cutoff_time))
            && active
        {
            let target_x = self.base_x() - self.chart.interval_to_x(cutoff);
            let threshold = (self.chart.cell_width * 2.5).max(50.0);
            if (self.chart.translation.x - target_x).abs() <= threshold {
                if let Some(ref mut rep) = self.replay {
                    rep.auto_scroll = true;
                }
                should_snap = true;
            }
        }
        if should_snap {
            self.auto_scroll_to_replay_cutoff();
            self.invalidate_all();
        }
    }

    pub fn replay_play_pause(&mut self) {
        if let Some(ref mut rep) = self.replay {
            rep.is_playing = !rep.is_playing;
            if rep.is_playing {
                self.last_tick = Instant::now();
                rep.auto_scroll = true;
            }
        }
        if self.replay.as_ref().is_some_and(|r| r.is_playing) {
            self.auto_scroll_to_replay_cutoff();
        }
        self.invalidate_all();
    }

    pub fn replay_step_forward(&mut self) {
        if let Some(ref mut rep) = self.replay {
            let step = match self.chart.basis {
                Basis::Time(tf) => tf.to_milliseconds(),
                Basis::Tick(_) => 10,
            };
            let max_t = match &self.data_source {
                PlotData::TimeBased(ts) => ts.latest_timestamp().unwrap_or(u64::MAX),
                PlotData::TickBased(_) => u64::MAX,
            };
            rep.cutoff_time = rep.cutoff_time.saturating_add(step).min(max_t);
            if rep.auto_scroll {
                self.auto_scroll_to_replay_cutoff();
            }
        }
        self.recalculate_scripted_indicators_replay();
        self.invalidate_all();
    }

    pub fn replay_step_backward(&mut self) {
        if let Some(ref mut rep) = self.replay {
            let step = match self.chart.basis {
                Basis::Time(tf) => tf.to_milliseconds(),
                Basis::Tick(_) => 10,
            };
            let min_t = match &self.data_source {
                PlotData::TimeBased(ts) => ts.timerange().0,
                PlotData::TickBased(_) => 0,
            };
            rep.cutoff_time = rep.cutoff_time.saturating_sub(step).max(min_t);
            if rep.auto_scroll {
                self.auto_scroll_to_replay_cutoff();
            }
        }
        self.recalculate_scripted_indicators_replay();
        self.invalidate_all();
    }

    pub fn replay_set_speed(&mut self, speed_ms: u64) {
        if let Some(ref mut rep) = self.replay {
            rep.speed_ms = speed_ms;
        }
    }

    pub fn replay_set_cutoff(&mut self, cutoff: u64) {
        if let Some(ref mut rep) = self.replay {
            rep.cutoff_time = cutoff;
            rep.auto_scroll = true;
        } else {
            self.replay = Some(ReplayState::new(cutoff));
        }
        self.auto_scroll_to_replay_cutoff();
        self.tpo_cache.borrow_mut().sessions.clear();
        self.recalculate_scripted_indicators_replay();
        self.invalidate_all();
    }

    pub fn scroll_to_timestamp(&mut self, timestamp: u64) {
        if self.chart.scaling > f32::EPSILON && self.chart.bounds.width > f32::EPSILON {
            let base_x = self.base_x();
            let x_offset = self.chart.interval_to_x(timestamp);
            self.chart.translation.x = base_x - x_offset;
        }
        if let Some(ref mut rep) = self.replay {
            rep.auto_scroll = true;
        }
        self.chart.layout.autoscale = Some(Autoscale::FitToVisible);
        self.invalidate_all();
    }

    pub fn replay_set_cutoff_and_jump(&mut self, cutoff: u64) {
        if let Some(ref mut rep) = self.replay {
            rep.cutoff_time = cutoff;
            rep.is_playing = false;
            rep.auto_scroll = true;
        } else {
            self.replay = Some(ReplayState::new(cutoff));
        }
        self.tpo_cache.borrow_mut().sessions.clear();
        self.recalculate_scripted_indicators_replay();
        self.scroll_to_timestamp(cutoff);
    }

    pub fn find_closest_bar_at_or_before(&self, timestamp: u64) -> Option<u64> {
        match &self.data_source {
            PlotData::TimeBased(ts) => {
                let timeframe_ms = ts.interval.to_milliseconds();
                if let Some((&k, _)) = ts.datapoints.range(..=timestamp).next_back()
                    && timestamp.saturating_sub(k) <= 5 * timeframe_ms
                {
                    return Some(k);
                }
                None
            }
            PlotData::TickBased(tick_aggr) => tick_aggr
                .datapoints
                .iter()
                .rfind(|dp| dp.kline.time <= timestamp)
                .map(|dp| dp.kline.time),
        }
    }

    pub fn replay_pick_random_bar(&mut self) -> Option<u64> {
        let timestamp = match &self.data_source {
            PlotData::TimeBased(ts) => {
                let total = ts.datapoints.len();
                if total == 0 {
                    return None;
                }
                let timeframe_ms = ts.interval.to_milliseconds().max(60_000);
                let (_kline_earliest, kline_latest) = ts.timerange();
                let min_ts: u64 = 1_609_459_200_000; // 2021-01-01 00:00:00 UTC
                let now_ts = chrono::Utc::now().timestamp_millis() as u64;
                let max_ts = now_ts.saturating_sub(3 * 24 * 3600 * 1000);

                if kline_latest >= min_ts && max_ts > min_ts {
                    // Pick random timestamp across historical years (2021 to 3 days ago)
                    let span = max_ts.saturating_sub(min_ts).max(timeframe_ms);
                    let rand_bytes = uuid::Uuid::new_v4();
                    let rand_val = u128::from_le_bytes(*rand_bytes.as_bytes());
                    let offset = (rand_val % (span as u128)) as u64;
                    let raw_ts = min_ts + offset;
                    (raw_ts / timeframe_ms) * timeframe_ms
                } else {
                    // Fallback / mock data in unit tests
                    let (start_idx, end_idx) = if total > 40 {
                        (15, total.saturating_sub(15))
                    } else if total > 2 {
                        (0, total - 1)
                    } else {
                        (0, total)
                    };
                    let span = end_idx.saturating_sub(start_idx).max(1);
                    let rand_bytes = uuid::Uuid::new_v4();
                    let rand_val = u128::from_le_bytes(*rand_bytes.as_bytes());
                    let offset = (rand_val % (span as u128)) as usize;
                    let target_idx = start_idx + offset;
                    ts.datapoints.keys().nth(target_idx).copied()?
                }
            }
            PlotData::TickBased(tick_aggr) => {
                let total = tick_aggr.datapoints.len();
                if total == 0 {
                    return None;
                }
                let (start_idx, end_idx) = if total > 40 {
                    (15, total.saturating_sub(15))
                } else if total > 2 {
                    (0, total - 1)
                } else {
                    (0, total)
                };
                let span = end_idx.saturating_sub(start_idx).max(1);
                let rand_bytes = uuid::Uuid::new_v4();
                let rand_val = u128::from_le_bytes(*rand_bytes.as_bytes());
                let offset = (rand_val % (span as u128)) as usize;
                let target_idx = start_idx + offset;
                tick_aggr
                    .datapoints
                    .get(target_idx)
                    .map(|dp| dp.kline.time)?
            }
        };

        self.replay_set_cutoff_and_jump(timestamp);
        Some(timestamp)
    }

    pub fn check_price_alerts(&mut self, prev_price: f32, current_price: f32) -> Vec<PriceAlert> {
        self.check_price_alerts_range(
            prev_price,
            prev_price.min(current_price),
            prev_price.max(current_price),
            current_price,
        )
    }

    pub fn check_price_alerts_range(
        &mut self,
        prev_price: f32,
        low: f32,
        high: f32,
        current_price: f32,
    ) -> Vec<PriceAlert> {
        let mut triggered = Vec::new();
        let now = chrono::Utc::now().timestamp_millis() as u64;
        for alert in &mut self.alerts {
            if alert.check_range_trigger(prev_price, low, high, current_price) {
                alert.status = AlertStatus::Triggered;
                alert.triggered_at = Some(now);
                triggered.push(alert.clone());
            }
        }
        if !triggered.is_empty() {
            self.invalidate_all();
        }
        triggered
    }

    pub fn update_study_configurator(&mut self, message: study::Message<FootprintStudy>) {
        let KlineChartKind::Footprint {
            ref mut studies, ..
        } = self.kind
        else {
            return;
        };

        match self.study_configurator.update(message) {
            Some(study::Action::ToggleStudy(study, is_selected)) => {
                if is_selected {
                    let already_exists = studies.iter().any(|s| s.is_same_type(&study));
                    if !already_exists {
                        studies.push(study);
                    }
                } else {
                    studies.retain(|s| !s.is_same_type(&study));
                }
            }
            Some(study::Action::ConfigureStudy(study)) => {
                if let Some(existing_study) = studies.iter_mut().find(|s| s.is_same_type(&study)) {
                    *existing_study = study;
                }
            }
            None => {}
        }

        self.invalidate(None);
    }

    pub fn chart_layout(&self) -> ViewConfig {
        self.chart.layout()
    }

    pub fn set_cluster_kind(&mut self, new_kind: ClusterKind) {
        if let KlineChartKind::Footprint {
            ref mut clusters, ..
        } = self.kind
        {
            *clusters = new_kind;
        }

        self.invalidate(None);
    }

    pub fn set_cluster_scaling(&mut self, new_scaling: ClusterScaling) {
        if let KlineChartKind::Footprint {
            ref mut scaling, ..
        } = self.kind
        {
            *scaling = new_scaling;
        }

        self.invalidate(None);
    }

    pub fn set_footprint_show_bottom_volume(&mut self, show: bool) {
        if let KlineChartKind::Footprint {
            ref mut show_bottom_volume,
            ..
        } = self.kind
        {
            *show_bottom_volume = show;
        }

        self.invalidate(None);
    }

    pub fn is_aggregate_footprint(&self) -> bool {
        match &self.kind {
            KlineChartKind::Footprint { aggregate, .. } => *aggregate,
            _ => false,
        }
    }

    pub fn reload_footprint_after_aggregate_change(&mut self) {
        self.raw_trades.clear();
        self.ensure_today_trades_loaded();
        self.tpo_cache.borrow_mut().clear();

        match self.data_source {
            PlotData::TimeBased(ref mut timeseries) => {
                for dp in timeseries.datapoints.values_mut() {
                    dp.clear_trades();
                    dp.trades_fetched = false;
                }
                if !self.raw_trades.is_empty() {
                    timeseries.insert_trades_existing_buckets(&self.raw_trades);
                }
            }
            PlotData::TickBased(ref mut tick_aggr) => {
                let interval = tick_aggr.interval;
                let step = self.chart.tick_size;
                *tick_aggr = TickAggr::new(interval, step, &self.raw_trades);
            }
        }

        self.reset_request_handler();
        self.invalidate(Some(std::time::Instant::now()));
    }

    pub fn set_footprint_aggregate(&mut self, aggregate: bool) {
        let changed = if let KlineChartKind::Footprint {
            aggregate: ref mut agg,
            ..
        } = self.kind
        {
            if *agg != aggregate {
                *agg = aggregate;
                true
            } else {
                false
            }
        } else {
            false
        };

        if changed {
            self.reload_footprint_after_aggregate_change();
        } else {
            self.invalidate(None);
        }
    }

    pub fn set_footprint_aggregate_exchange(&mut self, ex: Exchange, enabled: bool) {
        let changed = if let KlineChartKind::Footprint {
            ref mut aggregate_exchanges,
            ..
        } = self.kind
        {
            if enabled {
                if !aggregate_exchanges.contains(&ex) {
                    aggregate_exchanges.push(ex);
                    true
                } else {
                    false
                }
            } else {
                let prev_len = aggregate_exchanges.len();
                aggregate_exchanges.retain(|&e| e != ex);
                prev_len != aggregate_exchanges.len()
            }
        } else {
            false
        };

        if changed {
            self.reload_footprint_after_aggregate_change();
        } else {
            self.invalidate(None);
        }
    }

    pub fn set_tpo_kind(&mut self, new_kind: KlineChartKind) {
        if matches!(new_kind, KlineChartKind::Tpo { .. }) {
            self.kind = new_kind;
            self.tpo_cache.borrow_mut().clear();
            self.invalidate(None);
        }
    }

    pub fn merge_tpo_sessions(&mut self, s1: i64, s2: i64) {
        if let KlineChartKind::Tpo { clusters, .. } = &mut self.kind {
            data::chart::tpo::merge_adjacent_clusters(clusters, s1, s2);
            self.tpo_cache.borrow_mut().clear();
            self.invalidate(None);
        }
    }

    pub fn split_tpo_cluster(&mut self, s: i64) {
        if let KlineChartKind::Tpo { clusters, .. } = &mut self.kind {
            data::chart::tpo::split_cluster(clusters, s);
            self.tpo_cache.borrow_mut().clear();
            self.invalidate(None);
        }
    }

    pub fn toggle_split_brackets(&mut self, session_start: i64) {
        if let KlineChartKind::Tpo { split_sessions, .. } = &mut self.kind {
            if let Some(pos) = split_sessions.iter().position(|&s| s == session_start) {
                split_sessions.remove(pos);
            } else {
                split_sessions.push(session_start);
            }
            self.tpo_cache.borrow_mut().clear();
            self.invalidate(None);
        }
    }

    pub fn basis(&self) -> Basis {
        self.chart.basis
    }

    fn ensure_today_trades_loaded(&mut self) {
        let today = chrono::Utc::now().date_naive();
        let day_start = today
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc().timestamp_millis() as u64)
            .unwrap_or(0);

        let needs_reload = self.raw_trades.is_empty()
            || self
                .raw_trades
                .first()
                .is_some_and(|t| t.time > day_start + 60_000);

        if needs_reload {
            let exchange_folder = match self.ticker_info.exchange() {
                exchange::adapter::Exchange::BinanceSpot
                | exchange::adapter::Exchange::BinanceLinear
                | exchange::adapter::Exchange::BinanceInverse => "binance",
                exchange::adapter::Exchange::BybitSpot
                | exchange::adapter::Exchange::BybitLinear
                | exchange::adapter::Exchange::BybitInverse => "bybit",
                exchange::adapter::Exchange::OkexSpot
                | exchange::adapter::Exchange::OkexLinear
                | exchange::adapter::Exchange::OkexInverse => "okex",
                exchange::adapter::Exchange::HyperliquidSpot
                | exchange::adapter::Exchange::HyperliquidLinear => "hyperliquid",
                exchange::adapter::Exchange::MexcSpot
                | exchange::adapter::Exchange::MexcLinear
                | exchange::adapter::Exchange::MexcInverse => "mexc",
                exchange::adapter::Exchange::CoinbaseSpot => "coinbase",
            };
            let trades_data_path =
                data::data_path(Some(&format!("market_data/{exchange_folder}/")));
            if let Some(disk_trades) = exchange::trades::cache::load_intraday_trades_from_cache(
                &trades_data_path,
                &self.ticker_info,
                today,
            ) && !disk_trades.is_empty()
            {
                let mut merged = std::mem::take(&mut self.raw_trades);
                merged.extend(disk_trades);
                merged.sort_by_key(|t| t.time);
                merged.dedup_by(|a, b| {
                    a.time == b.time && a.price == b.price && (a.qty - b.qty).abs() < 1e-5
                });
                self.raw_trades = merged;
            }
        }
    }

    pub fn change_tick_size(&mut self, new_tick_size: f32) {
        self.ensure_today_trades_loaded();
        self.tpo_cache.borrow_mut().clear();
        self.simulated_footprint_cache.borrow_mut().clear();
        let chart = self.mut_state();

        let step = PriceStep::from_f32(new_tick_size);

        chart.cell_height *= new_tick_size / chart.tick_size.to_f32_lossy();
        chart.tick_size = step;

        match self.data_source {
            PlotData::TickBased(ref mut tick_aggr) => {
                tick_aggr.change_tick_size(new_tick_size, &self.raw_trades);
            }
            PlotData::TimeBased(ref mut timeseries) => {
                timeseries.change_tick_size(new_tick_size, &self.raw_trades);
            }
        }

        self.indicators
            .values_mut()
            .filter_map(Option::as_mut)
            .for_each(|indi| indi.on_ticksize_change(&self.data_source));

        self.reset_request_handler();
        self.invalidate(None);
    }

    pub fn set_basis(&mut self, new_basis: Basis) -> Option<Action> {
        self.ensure_today_trades_loaded();
        self.tpo_cache.borrow_mut().clear();
        self.simulated_footprint_cache.borrow_mut().clear();
        self.chart.last_price = None;
        self.chart.latest_x = 0;
        self.chart.basis = new_basis;

        match new_basis {
            Basis::Time(interval) => {
                let step = self.chart.tick_size;
                let timeseries = TimeSeries::<KlineDataPoint>::new(interval, step, &[])
                    .with_exchange(self.ticker_info.exchange())
                    .with_trades(&self.raw_trades);
                self.data_source = PlotData::TimeBased(timeseries);
            }
            Basis::Tick(tick_count) => {
                let step = self.chart.tick_size;
                let tick_aggr = TickAggr::new(tick_count, step, &self.raw_trades);
                self.data_source = PlotData::TickBased(tick_aggr);
            }
        }

        self.indicators
            .values_mut()
            .filter_map(Option::as_mut)
            .for_each(|indi| indi.on_basis_change(&self.data_source));

        self.reset_request_handler();
        self.invalidate(Some(Instant::now()))
    }

    pub fn studies(&self) -> Option<Vec<FootprintStudy>> {
        match &self.kind {
            KlineChartKind::Footprint { studies, .. } => Some(studies.clone()),
            _ => None,
        }
    }

    pub fn set_studies(&mut self, new_studies: Vec<FootprintStudy>) {
        if let KlineChartKind::Footprint {
            ref mut studies, ..
        } = self.kind
        {
            *studies = new_studies;
        }

        self.invalidate(None);
    }

    pub fn insert_trades_buffer(&mut self, trades_buffer: &[Trade]) {
        if let (Some(last_existing), Some(first_incoming)) =
            (self.raw_trades.last(), trades_buffer.first())
        {
            if first_incoming.time > last_existing.time {
                self.raw_trades.extend_from_slice(trades_buffer);
            } else {
                self.raw_trades =
                    exchange::trades::cache::merge_intraday_trades(&self.raw_trades, trades_buffer);
            }
        } else {
            self.raw_trades.extend_from_slice(trades_buffer);
        }

        const MAX_RAW_TRADES_IN_RAM: usize = 500_000;
        if self.raw_trades.len() > MAX_RAW_TRADES_IN_RAM {
            let excess = self.raw_trades.len() - MAX_RAW_TRADES_IN_RAM;
            self.raw_trades.drain(..excess);
        }

        if matches!(self.kind, KlineChartKind::Tpo { .. })
            || self.indicators[KlineIndicator::Tpo].is_some()
        {
            let base_price = self.chart.base_price_y.to_f32_lossy() as f64;
            let exchange_tick = self.chart.tick_size.to_f32_lossy() as f64;
            let tick_step = match self.kind {
                KlineChartKind::Tpo { tick_step, .. } => tick_step,
                _ => data::chart::kline::TpoTickStep::Auto,
            };
            let tpo_tick = data::chart::tpo::calculate_tpo_tick_size(
                base_price,
                exchange_tick,
                tick_step.multiplier(),
            );
            let anchor = match self.kind.effective_tpo_period() {
                data::chart::tpo::SessionPeriod::Weekly => data::chart::tpo::TpoAnchor::Weekly,
                data::chart::tpo::SessionPeriod::Monthly => data::chart::tpo::TpoAnchor::Monthly,
                _ => data::chart::tpo::TpoAnchor::Daily,
            };
            let mut cache = self.tpo_cache.borrow_mut();
            if cache.trade_tpo_cache.anchor() != anchor {
                cache.trade_tpo_cache.set_anchor(anchor);
            }
            cache.trade_tpo_cache.on_trades(trades_buffer, tpo_tick);
        }

        match self.data_source {
            PlotData::TickBased(ref mut tick_aggr) => {
                let old_dp_len = tick_aggr.datapoints.len();
                tick_aggr.insert_trades(trades_buffer);

                if let Some(last_dp) = tick_aggr.datapoints.last() {
                    self.chart.last_price =
                        Some(PriceInfoLabel::new(last_dp.kline.close, last_dp.kline.open));
                } else {
                    self.chart.last_price = None;
                }

                if !self.indicator_instances.instances().is_empty() {
                    let basis = indicator_engine::TimeOrTick::Tick;
                    let min_tick = Some(self.chart.tick_size.to_f32_lossy() as f64);
                    let new_dp_len = tick_aggr.datapoints.len();
                    if new_dp_len > old_dp_len {
                        let start_idx = match self.indicator_host.committed_index() {
                            Some(last) => last.saturating_add(1),
                            None => 0,
                        };
                        let target_close = new_dp_len.saturating_sub(1);
                        for idx in start_idx..target_close {
                            if let Some(acc) = tick_aggr.datapoints.get(idx) {
                                let frame = data::chart::kline::indicators::kline_to_indicator_frame_with_context(
                                    idx, &acc.kline, Some(&acc.footprint), None, basis, min_tick, true,
                                );
                                self.indicator_host.evaluate_closed(&frame);
                            }
                        }
                    }
                    if let Some(latest_acc) = tick_aggr.datapoints.last() {
                        let preview_idx = new_dp_len.saturating_sub(1);
                        let preview_frame =
                            data::chart::kline::indicators::kline_to_indicator_frame_with_context(
                                preview_idx,
                                &latest_acc.kline,
                                Some(&latest_acc.footprint),
                                None,
                                basis,
                                min_tick,
                                true,
                            );
                        self.indicator_host.preview_open(&preview_frame);
                    }
                }

                self.indicators
                    .values_mut()
                    .filter_map(Option::as_mut)
                    .for_each(|indi| {
                        indi.on_insert_trades(trades_buffer, old_dp_len, &self.data_source)
                    });

                self.invalidate(None);
            }
            PlotData::TimeBased(ref mut timeseries) => {
                let old_dp_len = timeseries.datapoints.len();
                timeseries.insert_realtime_trades(trades_buffer);

                if let Some(last_dp) = timeseries.datapoints.values().last() {
                    let kline_time = last_dp.kline.time;
                    let last_close = last_dp.kline.close;
                    let last_open = last_dp.kline.open;

                    if kline_time > self.chart.latest_x {
                        self.chart.latest_x = kline_time;
                    }
                    self.chart.last_price = Some(PriceInfoLabel::new(last_close, last_open));
                }

                if !self.indicator_instances.instances().is_empty() {
                    let basis = indicator_engine::TimeOrTick::Time;
                    let min_tick = Some(self.chart.tick_size.to_f32_lossy() as f64);
                    let new_dp_len = timeseries.datapoints.len();
                    if new_dp_len > old_dp_len {
                        let start_idx = match self.indicator_host.committed_index() {
                            Some(last) => last.saturating_add(1),
                            None => 0,
                        };
                        let target_close = new_dp_len.saturating_sub(1);
                        if start_idx < target_close {
                            for (offset, dp) in timeseries
                                .datapoints
                                .values()
                                .skip(start_idx)
                                .take(target_close.saturating_sub(start_idx))
                                .enumerate()
                            {
                                let idx = start_idx + offset;
                                let frame =
                                    data::chart::kline::indicators::datapoint_to_indicator_frame(
                                        idx, dp, None, basis, min_tick, true,
                                    );
                                self.indicator_host.evaluate_closed(&frame);
                            }
                        }
                    }
                    if let Some(latest_dp) = timeseries.datapoints.values().last() {
                        let preview_idx = new_dp_len.saturating_sub(1);
                        let preview_frame =
                            data::chart::kline::indicators::datapoint_to_indicator_frame(
                                preview_idx,
                                latest_dp,
                                None,
                                basis,
                                min_tick,
                                true,
                            );
                        self.indicator_host.preview_open(&preview_frame);
                    }
                }

                self.indicators
                    .values_mut()
                    .filter_map(Option::as_mut)
                    .for_each(|indi| {
                        indi.on_insert_trades(trades_buffer, old_dp_len, &self.data_source)
                    });

                self.invalidate(None);
            }
        }

        self.save_footprint_cache(false);
    }

    pub fn save_footprint_cache(&mut self, force: bool) {
        if !matches!(self.kind, KlineChartKind::Footprint { .. }) {
            return;
        }
        if !force
            && let Some(last) = self.last_footprint_cache_save
            && last.elapsed() < Duration::from_secs(60)
        {
            return;
        }

        let PlotData::TimeBased(ref timeseries) = self.data_source else {
            return;
        };

        let (symbol, _) = self.ticker_info.ticker.to_full_symbol_and_type();
        let base_data_path = data::data_path(None);
        let today = chrono::Utc::now().date_naive();
        let interval = timeseries.interval;
        let step = self.chart.tick_size;

        let mut distinct_dates: Vec<chrono::NaiveDate> = Vec::new();
        for (&t, dp) in &timeseries.datapoints {
            if dp.footprint.is_empty() || t < 1577836800000 {
                continue;
            }
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(t as i64) {
                let d = dt.date_naive();
                if !distinct_dates.contains(&d) {
                    distinct_dates.push(d);
                }
            }
        }

        for &date in &distinct_dates {
            if date == today {
                // Today's session is live and in progress.
                // Raw trades are saved contiguously in the daily raw trades cache.
                // Never freeze incomplete/live candles into the immutable daily footprint cache.
                continue;
            }
            if let Some(day_start_dt) = date.and_hms_opt(0, 0, 0) {
                let day_start = day_start_dt.and_utc().timestamp_millis() as u64;
                let day_end = day_start + 86_400_000 - 1;

                let dps: Vec<(u64, KlineDataPoint)> = timeseries
                    .datapoints
                    .range(day_start..=day_end)
                    .map(|(&t, dp)| (t, dp.clone()))
                    .collect();

                let interval_ms = interval.to_milliseconds();
                let expected_count = 86_400_000u64.checked_div(interval_ms).unwrap_or(1) as usize;

                let is_day_complete = dps.len() >= expected_count
                    && !dps.is_empty()
                    && dps.iter().all(|(_, dp)| {
                        (dp.kline.volume_f32().0 + dp.kline.volume_f32().1) == 0.0
                            || (!dp.footprint.is_empty() && dp.trades_fetched)
                    });

                if is_day_complete {
                    let cache_symbol = if self.is_aggregate_footprint() {
                        format!("{symbol}-AGG")
                    } else {
                        symbol.clone()
                    };
                    let cache_path = data::chart::kline::footprint_cache_path(
                        &base_data_path,
                        &cache_symbol,
                        interval,
                        step,
                        date,
                    );
                    let _ = data::chart::kline::save_daily_footprint(&cache_path, &dps);
                }
            }
        }

        if let Some(day_start_dt) = today.and_hms_opt(0, 0, 0) {
            let day_start = day_start_dt.and_utc().timestamp_millis() as u64;
            let today_trades: Vec<Trade> = self
                .raw_trades
                .iter()
                .filter(|t| t.time >= day_start)
                .copied()
                .collect();
            if !today_trades.is_empty() {
                let exchange_folder = match self.ticker_info.exchange() {
                    exchange::adapter::Exchange::BinanceSpot
                    | exchange::adapter::Exchange::BinanceLinear
                    | exchange::adapter::Exchange::BinanceInverse => "binance",
                    exchange::adapter::Exchange::BybitSpot
                    | exchange::adapter::Exchange::BybitLinear
                    | exchange::adapter::Exchange::BybitInverse => "bybit",
                    exchange::adapter::Exchange::OkexSpot
                    | exchange::adapter::Exchange::OkexLinear
                    | exchange::adapter::Exchange::OkexInverse => "okex",
                    exchange::adapter::Exchange::HyperliquidSpot
                    | exchange::adapter::Exchange::HyperliquidLinear => "hyperliquid",
                    exchange::adapter::Exchange::MexcSpot
                    | exchange::adapter::Exchange::MexcLinear
                    | exchange::adapter::Exchange::MexcInverse => "mexc",
                    exchange::adapter::Exchange::CoinbaseSpot => "coinbase",
                };
                let trades_data_path =
                    data::data_path(Some(&format!("market_data/{exchange_folder}/")));
                let _ = exchange::trades::cache::save_intraday_trades_to_cache(
                    &trades_data_path,
                    &self.ticker_info,
                    today,
                    &today_trades,
                );
            }
        }

        self.last_footprint_cache_save = Some(Instant::now());
    }

    pub fn flush_footprint_cache(&mut self) {
        self.save_footprint_cache(true);
    }

    pub fn insert_fetched_trades_buffered(&mut self, trades: &[Trade]) {
        if trades.is_empty() {
            return;
        }
        let (merged, actually_new) =
            exchange::trades::cache::merge_intraday_trades_with_diff(&self.raw_trades, trades);
        self.raw_trades = merged;

        if !actually_new.is_empty()
            && let PlotData::TickBased(ref mut tick_aggr) = self.data_source
        {
            tick_aggr.insert_trades(&actually_new);
        }
    }

    pub fn commit_fetched_trades(&mut self, from_time: u64, to_time: u64) {
        if from_time >= to_time {
            return;
        }
        if let PlotData::TimeBased(ref mut timeseries) = self.data_source {
            let interval_ms = timeseries.interval.to_milliseconds();
            if let Some(nz_interval) = std::num::NonZeroU64::new(interval_ms) {
                let interval_ms = nz_interval.get();
                let start_bucket = (from_time / interval_ms) * interval_ms;
                let end_bucket = (to_time.saturating_sub(1) / interval_ms) * interval_ms;
                let mut affected_buckets = rustc_hash::FxHashSet::default();
                let mut b = start_bucket;
                while b <= end_bucket {
                    affected_buckets.insert(b);
                    b = b.saturating_add(interval_ms);
                }
                timeseries.rebuild_buckets_from_raw_trades(&self.raw_trades, &affected_buckets);
            }
        }

        self.save_footprint_cache(false);

        let today_midnight = exchange::clock::today_midnight_ms(Some(self.ticker_info.exchange()));
        if to_time <= today_midnight {
            self.raw_trades.retain(|t| t.time >= today_midnight);
        } else {
            const MAX_RAW_TRADES_IN_RAM: usize = 500_000;
            if self.raw_trades.len() > MAX_RAW_TRADES_IN_RAM {
                let excess = self.raw_trades.len() - MAX_RAW_TRADES_IN_RAM;
                self.raw_trades.drain(..excess);
            }
        }

        self.recalculate_scripted_indicators();
        self.invalidate_all();
    }

    pub fn insert_raw_trades(&mut self, raw_trades: Vec<Trade>) {
        if raw_trades.is_empty() {
            return;
        }

        let (merged, actually_new) =
            exchange::trades::cache::merge_intraday_trades_with_diff(&self.raw_trades, &raw_trades);
        self.raw_trades = merged;

        if !actually_new.is_empty() {
            match self.data_source {
                PlotData::TickBased(ref mut tick_aggr) => {
                    tick_aggr.insert_trades(&actually_new);
                }
                PlotData::TimeBased(ref mut timeseries) => {
                    let interval_ms = timeseries.interval.to_milliseconds();
                    if interval_ms > 0 {
                        let mut affected_buckets = rustc_hash::FxHashSet::default();
                        for trade in &actually_new {
                            if let Some(q) = trade.time.checked_div(interval_ms) {
                                affected_buckets.insert(q * interval_ms);
                            }
                        }
                        timeseries
                            .rebuild_buckets_from_raw_trades(&self.raw_trades, &affected_buckets);
                    }
                }
            }

            self.save_footprint_cache(false);

            const MAX_RAW_TRADES_IN_RAM: usize = 500_000;
            if self.raw_trades.len() > MAX_RAW_TRADES_IN_RAM {
                let excess = self.raw_trades.len() - MAX_RAW_TRADES_IN_RAM;
                self.raw_trades.drain(..excess);
            }

            self.invalidate(None);
        }
    }

    pub fn insert_hist_klines(&mut self, req_id: uuid::Uuid, klines_raw: &[Kline]) {
        self.tpo_cache.borrow_mut().clear();
        match self.data_source {
            PlotData::TimeBased(ref mut timeseries) => {
                let was_empty = timeseries.datapoints.is_empty() || self.chart.latest_x == 0;

                timeseries.insert_klines(klines_raw);

                if was_empty && !timeseries.datapoints.is_empty() {
                    let latest_x = timeseries.latest_timestamp().unwrap_or(0);
                    self.chart.latest_x = latest_x;
                    self.chart.base_price_y = timeseries.base_price();
                    if let Some(lk) = timeseries.latest_kline() {
                        self.chart.last_price = Some(PriceInfoLabel::new(lk.close, lk.open));
                    }

                    // Recalculate cell_height based on recent candles
                    let (scale_high, scale_low) = timeseries.price_scale(match self.kind {
                        KlineChartKind::Footprint { .. } => 12,
                        KlineChartKind::Candles => 60,
                        KlineChartKind::Tpo { .. } => 30,
                    });
                    let step = self.chart.tick_size;
                    let low_rounded = scale_low.round_to_side_step(true, step);
                    let high_rounded = scale_high.round_to_side_step(false, step);
                    let y_ticks = Price::steps_between_inclusive(low_rounded, high_rounded, step)
                        .map(|n| n.saturating_sub(1))
                        .unwrap_or(1)
                        .max(1) as f32;

                    self.chart.cell_height = match self.kind {
                        KlineChartKind::Footprint { .. } => 800.0 / y_ticks,
                        KlineChartKind::Candles => 200.0 / y_ticks,
                        KlineChartKind::Tpo { .. } => 400.0 / y_ticks,
                    };

                    let x_trans = self.base_x();
                    self.chart.translation.x = x_trans;
                    self.chart.translation.y = -self.chart.bounds.height / 2.0;
                    self.chart.layout.autoscale = Some(Autoscale::FitToVisible);
                } else if let Some(new_latest) = timeseries.latest_timestamp()
                    && new_latest > self.chart.latest_x
                {
                    let old_latest = self.chart.latest_x;
                    self.chart.latest_x = new_latest;

                    if self.replay.is_none() {
                        let base_x = self.base_x();
                        let threshold = (self.chart.cell_width * 3.0).max(60.0);
                        if self.chart.translation.x > base_x + threshold {
                            let step = match self.chart.basis {
                                Basis::Time(tf) => tf.to_milliseconds(),
                                Basis::Tick(_) => 1,
                            };
                            if step > 0 {
                                let shift = ((new_latest - old_latest) as f64 / step as f64
                                    * self.chart.cell_width as f64)
                                    as f32;
                                self.chart.translation.x += shift;
                            }
                        }
                    }
                }

                self.indicators
                    .values_mut()
                    .filter_map(Option::as_mut)
                    .for_each(|indi| indi.rebuild_from_source(&self.data_source));

                self.recalculate_scripted_indicators();

                if self.liquidation_heatmap.config.enabled {
                    self.rebuild_liquidation_heatmap();
                }

                if klines_raw.is_empty() {
                    self.request_handler
                        .mark_failed(req_id, "No data received".to_string());
                } else {
                    self.request_handler.mark_completed(req_id);
                }
                self.invalidate(None);
            }
            PlotData::TickBased(_) => {}
        }
    }

    pub fn insert_open_interest(&mut self, req_id: Option<uuid::Uuid>, oi_data: &[OIData]) {
        if let Some(req_id) = req_id {
            if oi_data.is_empty() {
                self.request_handler
                    .mark_failed(req_id, "No data received".to_string());
            } else {
                self.request_handler.mark_completed(req_id);
            }
        }

        for indi in self.indicators.values_mut().flatten() {
            indi.on_open_interest(oi_data);
        }

        self.liquidation_heatmap
            .add_open_interest(oi_data.iter().map(|oi| (oi.time, oi.value)));
        if self.liquidation_heatmap.config.oi_weighting {
            self.rebuild_liquidation_heatmap();
            self.invalidate(None);
        }
    }

    pub fn insert_funding_rates(
        &mut self,
        req_id: Option<uuid::Uuid>,
        rates: &[exchange::FundingRate],
    ) {
        if let Some(req_id) = req_id {
            if rates.is_empty() {
                self.request_handler
                    .mark_failed(req_id, "No funding rate data received".to_string());
            } else {
                self.request_handler.mark_completed(req_id);
            }
        }

        if let Some(indi) = self.indicators[KlineIndicator::MarketPulse].as_mut() {
            indi.on_funding_rates(rates);
        }
    }

    pub fn insert_spot_klines(
        &mut self,
        req_id: Option<uuid::Uuid>,
        klines: &[exchange::SpotKline],
    ) {
        if let Some(req_id) = req_id {
            if klines.is_empty() {
                self.request_handler
                    .mark_failed(req_id, "No spot kline data received".to_string());
            } else {
                self.request_handler.mark_completed(req_id);
            }
        }

        if let Some(indi) = self.indicators[KlineIndicator::MarketPulse].as_mut() {
            indi.on_spot_klines(klines);
        }
        if let Some(indi) = self.indicators[KlineIndicator::CvdDivergence].as_mut() {
            indi.on_spot_klines(klines);
        }
    }

    pub fn insert_net_oi_data(
        &mut self,
        req_id: Option<uuid::Uuid>,
        data: &[exchange::NetOiDataPoint],
    ) {
        if let Some(req_id) = req_id {
            if data.is_empty() {
                self.request_handler
                    .mark_failed(req_id, "No Net OI data received".to_string());
            } else {
                self.request_handler.mark_completed(req_id);
            }
        }

        if let Some(indi) = self.indicators[KlineIndicator::NetOi].as_mut() {
            indi.on_net_oi_data(data);
        }
    }

    pub fn insert_depth(&mut self, time: u64, depth: &exchange::depth::Depth) {
        if let Some(indi) = self.indicators[KlineIndicator::DepthImbalance].as_mut() {
            indi.on_depth(time, depth);
        }
    }

    fn calc_qty_scales(
        &self,
        earliest: u64,
        latest: u64,
        highest: Price,
        lowest: Price,
        step: PriceStep,
        cluster_kind: ClusterKind,
    ) -> f32 {
        if earliest > latest {
            return 0.0;
        }

        let rounded_highest = highest.round_to_side_step(false, step).add_steps(1, step);

        let rounded_lowest = lowest.round_to_side_step(true, step).add_steps(-1, step);

        let max_qty = match &self.data_source {
            PlotData::TimeBased(timeseries) => timeseries.max_qty_ts_range(
                cluster_kind,
                earliest,
                latest,
                rounded_highest,
                rounded_lowest,
            ),
            PlotData::TickBased(tick_aggr) => {
                let earliest = earliest as usize;
                let latest = latest as usize;

                tick_aggr.max_qty_idx_range(
                    cluster_kind,
                    earliest,
                    latest,
                    rounded_highest,
                    rounded_lowest,
                )
            }
        };

        if self.replay.is_some() {
            if let PlotData::TimeBased(ts) = &self.data_source {
                let sim_max = ts
                    .datapoints
                    .range(earliest..=latest)
                    .filter(|(_, dp)| dp.footprint.trades.is_empty())
                    .map(|(_, dp)| {
                        let sim = self.get_or_simulate_footprint(&dp.kline, step);
                        sim.max_qty_by(rounded_highest, rounded_lowest, |b, s| match cluster_kind {
                            ClusterKind::BidAsk | ClusterKind::Table => b.max(s),
                            ClusterKind::DeltaProfile => (b - s).abs(),
                            ClusterKind::VolumeProfile => b + s,
                        })
                    })
                    .fold(0.0_f32, f32::max);
                max_qty.max(sim_max)
            } else {
                max_qty
            }
        } else {
            max_qty
        }
    }

    pub fn last_update(&self) -> Instant {
        self.last_tick
    }

    pub fn invalidate(&mut self, now: Option<Instant>) -> Option<Action> {
        let mut replay_advanced = false;
        if let Some(t) = now {
            if let Some(ref mut rep) = self.replay
                && rep.active
                && rep.is_playing
            {
                let interval_ms = rep.speed_ms.max(10);
                let elapsed = t.duration_since(self.last_tick).as_millis() as u64;
                if elapsed >= interval_ms {
                    let step = match self.chart.basis {
                        Basis::Time(tf) => tf.to_milliseconds(),
                        Basis::Tick(_) => 10,
                    };
                    let steps = (elapsed / interval_ms).max(1);
                    let max_t = match &self.data_source {
                        PlotData::TimeBased(ts) => ts.latest_timestamp().unwrap_or(u64::MAX),
                        PlotData::TickBased(_) => u64::MAX,
                    };
                    let next_cutoff = rep.cutoff_time.saturating_add(step * steps);
                    if next_cutoff >= max_t {
                        rep.cutoff_time = max_t;
                        rep.is_playing = false;
                    } else {
                        rep.cutoff_time = next_cutoff;
                    }
                    self.last_tick = t;
                    replay_advanced = true;
                }
            } else {
                self.last_tick = t;
            }
        }

        if replay_advanced {
            if let Some(cutoff) = self.replay.as_ref().map(|r| r.cutoff_time) {
                let target_x = self.base_x() - self.chart.interval_to_x(cutoff);
                let threshold = (self.chart.cell_width * 2.5).max(50.0);
                if let Some(ref mut rep) = self.replay
                    && !rep.auto_scroll
                    && (self.chart.translation.x - target_x).abs() <= threshold
                {
                    rep.auto_scroll = true;
                }
            }
            if self.replay.as_ref().is_some_and(|r| r.auto_scroll) && !self.is_user_panning {
                self.auto_scroll_to_replay_cutoff();
            }
            self.tpo_cache.borrow_mut().sessions.clear();
            self.recalculate_scripted_indicators_replay();
        }

        let base_x = self.base_x();
        let chart = &mut self.chart;

        if let Some(autoscale) = chart.layout.autoscale {
            match autoscale {
                super::Autoscale::CenterLatest => {
                    let x_offset = if let Some(rep) = &self.replay
                        && rep.active
                    {
                        chart.interval_to_x(rep.cutoff_time)
                    } else {
                        0.0
                    };
                    if self.replay.as_ref().is_none_or(|r| r.auto_scroll) && !self.is_user_panning {
                        chart.translation.x = base_x - x_offset;
                    }

                    let calculate_target_y = |kline: exchange::Kline| -> f32 {
                        let y_low = chart.price_to_y(kline.low);
                        let y_high = chart.price_to_y(kline.high);
                        let y_close = chart.price_to_y(kline.close);

                        let mut target_y_translation = -(y_low + y_high) / 2.0;

                        if chart.bounds.height > f32::EPSILON && chart.scaling > f32::EPSILON {
                            let visible_half_height = (chart.bounds.height / chart.scaling) / 2.0;

                            let view_center_y_centered = -target_y_translation;

                            let visible_y_top = view_center_y_centered - visible_half_height;
                            let visible_y_bottom = view_center_y_centered + visible_half_height;

                            let padding = chart.cell_height;

                            if y_close < visible_y_top {
                                target_y_translation = -(y_close - padding + visible_half_height);
                            } else if y_close > visible_y_bottom {
                                target_y_translation = -(y_close + padding - visible_half_height);
                            }
                        }
                        target_y_translation
                    };

                    chart.translation.y = self.data_source.latest_y_midpoint(calculate_target_y);
                }
                super::Autoscale::FitToVisible => {
                    let visible_region = chart.visible_region(chart.bounds.size());
                    let (start_interval, mut end_interval) = chart.interval_range(&visible_region);
                    if let Some(rep) = &self.replay
                        && rep.active
                    {
                        end_interval = end_interval.min(rep.cutoff_time);
                    }

                    let price_range = self
                        .data_source
                        .visible_price_range(start_interval, end_interval)
                        .or_else(|| match &self.data_source {
                            PlotData::TimeBased(ts) if !ts.datapoints.is_empty() => {
                                let (hi, lo) = ts.price_scale(60);
                                Some((lo.to_f32_lossy(), hi.to_f32_lossy()))
                            }
                            _ => None,
                        });

                    if let Some((lowest, highest)) = price_range {
                        let padding = (highest - lowest) * 0.05;
                        let price_span = (highest - lowest) + (2.0 * padding);

                        if price_span > 0.0 && chart.bounds.height > f32::EPSILON {
                            let padded_highest = highest + padding;
                            let chart_height = chart.bounds.height;
                            let tick_size = chart.tick_size.to_f32_lossy();

                            if tick_size > 0.0 {
                                chart.cell_height = (chart_height * tick_size) / price_span;
                                chart.base_price_y = Price::from_f32(padded_highest);
                                chart.translation.y = -chart_height / 2.0;
                            }
                        }
                    }
                }
            }
        }

        chart.cache.clear_all();
        for indi in self.indicators.values_mut().filter_map(Option::as_mut) {
            indi.clear_all_caches();
        }
        for cache in self.script_panel_caches.values() {
            cache.clear_all();
        }

        if now.is_some() {
            self.missing_data_task()
        } else {
            None
        }
    }

    pub fn active_panel_indicators_count(&self) -> usize {
        let market = self.chart.ticker_info.market_type();
        let builtin_count = self
            .indicators
            .iter()
            .filter(|(kind, val)| {
                kind.is_panel()
                    && (*kind != KlineIndicator::RollingVwap || self.config.rolling_vwap_show_panel)
                    && KlineIndicator::for_market(market).contains(kind)
                    && val.is_some()
            })
            .count();
        let script_count = self
            .indicator_instances
            .instances()
            .iter()
            .filter(|inst| {
                let def = inst.kind.definition();
                def.attachment_mode
                    == data::chart::kline::indicators::IndicatorAttachmentMode::Panel
                    && (def.supported_markets.is_empty() || def.supported_markets.contains(&market))
            })
            .count();
        builtin_count + script_count
    }

    pub fn recalculate_scripted_indicators(&mut self) {
        let active_ids = self
            .indicator_instances
            .instances()
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>();
        if active_ids.is_empty() {
            return;
        }
        for id in &active_ids {
            self.script_panel_caches.entry(*id).or_default();
        }
        let min_tick = Some(self.chart.tick_size.to_f32_lossy() as f64);
        let frames = data::chart::kline::indicators::plot_data_to_indicator_frames(
            &self.data_source,
            None,
            min_tick,
            true,
        );
        self.indicator_host
            .recalculate(&active_ids, &self.indicator_instances, &frames);
    }

    pub fn recalculate_scripted_indicators_replay(&mut self) {
        let active_ids = self
            .indicator_instances
            .instances()
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>();
        if active_ids.is_empty() {
            return;
        }
        let cutoff = self
            .replay
            .as_ref()
            .filter(|r| r.active)
            .map(|r| r.cutoff_time)
            .unwrap_or(0);
        let min_tick = Some(self.chart.tick_size.to_f32_lossy() as f64);
        let mut frames = data::chart::kline::indicators::plot_data_to_indicator_frames(
            &self.data_source,
            None,
            min_tick,
            true,
        );
        if cutoff > 0 {
            frames.retain(|f| (f.time as u64) <= cutoff);
        }
        self.indicator_host
            .recalculate(&active_ids, &self.indicator_instances, &frames);
    }

    fn sync_panel_splits(&mut self, prev_panel_count: usize) {
        let current_panel_count = self.active_panel_indicators_count();
        if prev_panel_count != current_panel_count {
            if current_panel_count == 0 {
                self.chart.layout.splits.clear();
            } else {
                let main_split = self.chart.layout.splits.first().copied().unwrap_or(0.8);
                self.chart.layout.splits = data::util::calc_panel_splits(
                    main_split,
                    current_panel_count,
                    Some(prev_panel_count),
                );
            }
        }
    }

    pub fn add_scripted_indicator(
        &mut self,
        kind: data::chart::kline::indicators::registry::IndicatorId,
    ) -> data::chart::kline::indicators::IndicatorInstanceId {
        let prev_panel_count = self.active_panel_indicators_count();
        let id = self.indicator_instances.create(kind);
        self.script_panel_caches.entry(id).or_default();
        self.recalculate_scripted_indicators();
        self.sync_panel_splits(prev_panel_count);
        self.invalidate_all();
        id
    }

    pub fn remove_scripted_indicator(
        &mut self,
        id: data::chart::kline::indicators::IndicatorInstanceId,
    ) -> bool {
        let prev_panel_count = self.active_panel_indicators_count();
        let instances = self
            .indicator_instances
            .instances()
            .iter()
            .filter(|i| i.id != id)
            .cloned()
            .collect();
        self.indicator_instances =
            data::chart::kline::indicators::IndicatorInstanceStore::from_instances(
                instances,
                self.chart.basis,
            );
        self.script_panel_caches.remove(&id);
        self.recalculate_scripted_indicators();
        self.sync_panel_splits(prev_panel_count);
        self.invalidate_all();
        true
    }

    pub fn toggle_indicator(&mut self, indicator: KlineIndicator) {
        let prev_panel_count = self.active_panel_indicators_count();

        if self.indicators[indicator].is_some() {
            self.indicators[indicator] = None;
            if self.expanded_indicator == Some(indicator) {
                self.expanded_indicator = None;
            }
            if indicator == KlineIndicator::LiquidationHeatmap {
                self.liquidation_heatmap.config.enabled = false;
                self.liquidation_heatmap.clear();
            }
        } else {
            let mut box_indi = indicator::kline::make_empty(indicator);
            if indicator == KlineIndicator::PositionFlow
                && let Some(any) = box_indi.as_any_mut()
                && let Some(pf) = any.downcast_mut::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>()
            {
                pf.set_colors(self.config.position_flow_colors);
            }
            if indicator == KlineIndicator::RollingVwap
                && let Some(any) = box_indi.as_any_mut()
                && let Some(rv) = any.downcast_mut::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>()
            {
                rv.set_colors(self.config.rolling_vwap_colors);
                rv.set_window_hours(self.config.rolling_vwap_window_hours, &self.data_source);
            }
            if indicator == KlineIndicator::LiquidationHeatmap {
                self.liquidation_heatmap.config.enabled = true;
                self.rebuild_liquidation_heatmap();
            }
            crate::chart::indicator::kline::apply_chart_config(box_indi.as_mut(), &self.config);
            box_indi.rebuild_from_source(&self.data_source);
            self.indicators[indicator] = Some(box_indi);
        }

        self.invalidate(None);

        self.sync_panel_splits(prev_panel_count);
    }

    pub fn toggle_indicator_settings(&mut self, indicator: KlineIndicator) {
        if self.expanded_indicator == Some(indicator) {
            self.expanded_indicator = None;
        } else {
            self.expanded_indicator = Some(indicator);
        }
    }

    pub fn config(&self) -> data::chart::kline::Config {
        self.config
    }

    pub fn set_visual_config(&mut self, config: data::chart::kline::Config) {
        let prev_panel_count = self.active_panel_indicators_count();
        self.config = config;
        data::chart::kline::set_user_default_kline_config(config);

        let current_panel_count = self.active_panel_indicators_count();
        if prev_panel_count != current_panel_count {
            if current_panel_count == 0 {
                self.chart.layout.splits.clear();
            } else {
                let main_split = self.chart.layout.splits.first().copied().unwrap_or(0.8);
                self.chart.layout.splits = data::util::calc_panel_splits(
                    main_split,
                    current_panel_count,
                    Some(prev_panel_count),
                );
            }
        }

        if let Some(indi) = self.indicators[KlineIndicator::PositionFlow].as_mut()
            && let Some(any) = indi.as_any_mut()
            && let Some(pf) = any.downcast_mut::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>()
        {
            pf.set_colors(config.position_flow_colors);
        }

        if let Some(indi) = self.indicators[KlineIndicator::RollingVwap].as_mut()
            && let Some(any) = indi.as_any_mut()
            && let Some(rv) = any
                .downcast_mut::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>(
                )
        {
            rv.set_colors(config.rolling_vwap_colors);
            rv.set_window_hours(config.rolling_vwap_window_hours, &self.data_source);
        }

        for indi in self.indicators.values_mut().flatten() {
            crate::chart::indicator::kline::apply_chart_config(indi.as_mut(), &config);
        }

        if self.liquidation_heatmap.config.oi_weighting != config.liq_weight_by_oi {
            self.liquidation_heatmap.config.oi_weighting = config.liq_weight_by_oi;
            self.rebuild_liquidation_heatmap();
        }

        self.invalidate(None);
    }
}

impl canvas::Program<Message> for KlineChart {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Some(pos) = cursor.position_in(bounds) {
            let chart = self.state();
            let frame_pos = Point::new(
                (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
            );
            self.drawing_state.borrow_mut().last_cursor_frame_pos = Some(frame_pos);
        }

        if let ControlFlow::Break(action) = self.handle_keyboard(interaction, event, bounds, cursor)
        {
            return action;
        }

        if let ControlFlow::Break(action) =
            self.handle_tpo_context_menu(interaction, event, bounds, cursor)
        {
            return action;
        }

        if let ControlFlow::Break(action) = self.handle_drawing_tools(event, bounds, cursor) {
            return action;
        }

        if let ControlFlow::Break(action) = self.handle_drawing_selection(event, bounds, cursor) {
            return action;
        }

        if let ControlFlow::Break(action) = self.handle_alert_drag(event, bounds, cursor) {
            return action;
        }

        super::canvas_interaction(self, interaction, event, bounds, cursor)
    }

    fn draw(
        &self,
        interaction: &Interaction,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let chart = self.state();

        if chart.bounds.width == 0.0 {
            return vec![];
        }

        let bounds_size = bounds.size();
        let palette = theme.extended_palette();

        let klines = chart.cache.main.draw(renderer, bounds_size, |frame| {
            self.draw_main_layer(frame, chart, palette, bounds);
        });

        // Dynamic layer for drawings and alerts (smooth 60fps rubberband & dragging)
        let mut dynamic_frame = canvas::Frame::new(renderer, bounds_size);
        let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);
        dynamic_frame.translate(center);
        dynamic_frame.scale(chart.scaling);
        dynamic_frame.translate(chart.translation);

        let region = chart.visible_region(bounds_size);
        let d_state = self.drawing_state.borrow();
        let selected_drawing_id = d_state.selected_drawing;
        let drag_drawing = d_state.drag.as_ref().map(|d| d.current_drawing());
        let is_magnet_active = self.config.magnet_mode ^ d_state.ctrl_pressed;
        let magnet_indicator = if is_magnet_active
            && (d_state.active_tool != DrawingTool::None
                && d_state.active_tool != DrawingTool::Brush
                || d_state.in_progress.is_some()
                || matches!(d_state.drag, Some(DrawingDrag::MovingHandle { .. })))
        {
            d_state.last_cursor_frame_pos.and_then(|pos| {
                let snapped = self.snap_point_to_candle(pos);
                let candle_x = chart.interval_to_x(snapped.0);
                let max_x_dist = (chart.cell_width * 3.0).max(40.0 / chart.scaling);
                if (pos.x - candle_x).abs() <= max_x_dist {
                    Some(Point::new(
                        candle_x,
                        chart.price_to_y(Price::from_f32(snapped.1)),
                    ))
                } else {
                    None
                }
            })
        } else {
            None
        };
        let cutoff = if let Some(ref rep) = self.replay {
            if rep.active && rep.cutoff_time > 0 {
                Some(rep.cutoff_time)
            } else {
                None
            }
        } else {
            None
        };
        draw_chart_drawings(
            &mut dynamic_frame,
            &self.drawings,
            &d_state.in_progress,
            drag_drawing,
            chart,
            &region,
            selected_drawing_id,
            magnet_indicator,
            &self.data_source,
            cutoff,
            palette,
        );
        draw_chart_alerts(
            &mut dynamic_frame,
            &self.alerts,
            chart,
            &region,
            &self.alert_drag_state.borrow(),
            palette,
        );
        let dynamic_geometry = dynamic_frame.into_geometry();

        let crosshair = chart.cache.crosshair.draw(renderer, bounds_size, |frame| {
            if let Some(cursor_position) = cursor.position_in(bounds) {
                let (_, rounded_aggregation) =
                    chart.draw_crosshair(frame, theme, bounds_size, cursor_position, interaction);

                draw_crosshair_tooltip(
                    &self.data_source,
                    &chart.ticker_info,
                    frame,
                    palette,
                    rounded_aggregation,
                );
            }
        });

        let mut layers = vec![klines, dynamic_geometry, crosshair];
        let menu_opt = self.tpo_cache.borrow().context_menu.clone();
        if let Some(menu) = menu_opt {
            let mut frame = canvas::Frame::new(renderer, bounds_size);
            draw_context_menu(&mut frame, &menu, cursor.position_in(bounds), palette);
            layers.push(frame.into_geometry());
        }

        layers
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.alert_drag_state.borrow().is_some() {
            return mouse::Interaction::ResizingVertically;
        }

        if let Some(pos) = cursor.position_in(bounds) {
            let chart = self.state();

            // Hovering over any alert's '✕' close button shows pointer cursor
            let is_over_close_btn = self.alerts.iter().any(|a| {
                let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                let alert_screen_y =
                    (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                (pos.y - alert_screen_y).abs() <= 10.0
                    && pos.x >= bounds.width - 24.0
                    && pos.x <= bounds.width
            });
            if is_over_close_btn && self.drawing_state.borrow().active_tool == DrawingTool::None {
                return mouse::Interaction::Pointer;
            }

            let is_near_alert = self.alerts.iter().any(|a| {
                let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                let alert_screen_y =
                    (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                (pos.y - alert_screen_y).abs() <= 8.0
            });
            if is_near_alert && self.drawing_state.borrow().active_tool == DrawingTool::None {
                return mouse::Interaction::ResizingVertically;
            }
        }

        if self.drawing_state.borrow().active_tool != DrawingTool::None {
            return mouse::Interaction::Pointer;
        }

        if self.drawing_state.borrow().drag.is_some() {
            return mouse::Interaction::Grabbing;
        }

        match interaction {
            Interaction::Panning { .. } => mouse::Interaction::Grabbing,
            Interaction::Zoomin { .. } => mouse::Interaction::ZoomIn,
            Interaction::None | Interaction::Ruler { .. } => {
                let has_selected_drawing = self.drawing_state.borrow().selected_drawing.is_some()
                    || self.drawings.iter().any(|d| d.is_selected);
                if has_selected_drawing && cursor.is_over(bounds) {
                    mouse::Interaction::default()
                } else if cursor.is_over(bounds) {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            }
        }
    }
}

mod drawings;
mod footprint;
mod interaction;
mod main_layer;
mod overlays;
mod tpo;

use drawings::*;
use footprint::*;
use overlays::*;
use tpo::*;

#[cfg(test)]
mod tests;
