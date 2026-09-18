use super::{
    Action, Basis, Chart, Interaction, Message, PlotConstants, PlotData, TEXT_SIZE, ViewState,
    indicator, request_fetch, scale::linear::PriceInfoLabel,
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
    kline::{ClusterKind, FootprintStudy, KlineDataPoint, KlineTrades, NPoc, PointOfControl},
    liquidation_heatmap::{LiquidationHeatmap, LiquidationHeatmapConfig, interpolate_color_themed},
};
use data::util::{abbr_large_numbers, count_decimals};
use exchange::util::{Price, PriceStep};
use exchange::{
    Kline, OpenInterest as OIData, TickerInfo, Trade,
    fetcher::{FetchRange, RequestHandler},
};

use iced::task::Handle;
use iced::theme::palette::Extended;
use iced::widget::canvas::{self, Event, Geometry, Path, Stroke};
use iced::{Alignment, Element, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use enum_map::EnumMap;
use std::time::{Duration, Instant};

impl Chart for KlineChart {
    type IndicatorKind = KlineIndicator;

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

        for selected_indicator in enabled {
            if !KlineIndicator::for_market(market).contains(selected_indicator) {
                continue;
            }
            if !selected_indicator.is_panel() {
                continue;
            }
            if let Some(indi) = self.indicators[*selected_indicator].as_ref() {
                elements.push(indi.element(chart_state, earliest..=latest));
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
                unimplemented!()
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
        let x_translation = match &self.kind {
            KlineChartKind::Footprint { .. } => {
                0.5 * (chart.bounds.width / chart.scaling) - (chart.cell_width / chart.scaling)
            }
            KlineChartKind::Candles => {
                0.5 * (chart.bounds.width / chart.scaling)
                    - (8.0 * chart.cell_width / chart.scaling)
            }
            KlineChartKind::Tpo { .. } => {
                0.5 * (chart.bounds.width / chart.scaling)
                    - (8.0 * chart.cell_width / chart.scaling)
            }
        };
        Vector::new(x_translation, chart.translation.y)
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
    pub(crate) session_ranges: Vec<TpoSessionRange>,
    pub(crate) context_menu: Option<TpoContextMenu>,
}

impl TpoCache {
    pub(crate) fn clear(&mut self) {
        self.sessions.clear();
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
}

pub struct KlineChart {
    pub ticker_info: TickerInfo,
    chart: ViewState,
    data_source: PlotData<KlineDataPoint>,
    raw_trades: Vec<Trade>,
    indicators: EnumMap<KlineIndicator, Option<Box<dyn KlineIndicatorImpl>>>,
    fetching_trades: (bool, Vec<Handle>),
    active_trade_fetches: usize,
    pub(crate) kind: KlineChartKind,
    pub(crate) config: data::chart::kline::Config,
    request_handler: RequestHandler,
    study_configurator: study::Configurator<FootprintStudy>,
    last_tick: Instant,
    liquidation_heatmap: LiquidationHeatmap,
    pub(crate) tpo_cache: RefCell<TpoCache>,
    last_footprint_cache_save: Option<Instant>,
    pub drawings: Vec<Drawing>,
    pub(crate) drawing_state: RefCell<DrawingState>,
    pub alerts: Vec<PriceAlert>,
    pub alert_drag_state: RefCell<Option<(uuid::Uuid, f32)>>,
    pub replay: Option<ReplayState>,
}

impl KlineChart {
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
        match basis {
            Basis::Time(interval) => {
                let step = PriceStep::from_f32(tick_size);

                let timeseries = TimeSeries::<KlineDataPoint>::new(interval, step, klines_raw)
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

                let x_translation = match &kind {
                    KlineChartKind::Footprint { .. } => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (chart.cell_width / chart.scaling)
                    }
                    KlineChartKind::Candles => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (8.0 * chart.cell_width / chart.scaling)
                    }
                    KlineChartKind::Tpo { .. } => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (8.0 * chart.cell_width / chart.scaling)
                    }
                };
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
                        rv.set_window_hours(config.rolling_vwap_window_hours, &data_source);
                    }
                    indi.rebuild_from_source(&data_source);
                    indicators[i] = Some(indi);
                }

                let mut liquidation_heatmap =
                    LiquidationHeatmap::new(LiquidationHeatmapConfig::default());
                let liq_enabled = indicators[KlineIndicator::LiquidationHeatmap].is_some();
                liquidation_heatmap.config.enabled = liq_enabled;
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
                                dp.kline.volume.0,
                                dp.kline.volume.1,
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
                    fetching_trades: (false, Vec::new()),
                    active_trade_fetches: 0,
                    request_handler: RequestHandler::new(),
                    kind: kind.clone(),
                    config,
                    study_configurator: study::Configurator::new(),
                    last_tick: Instant::now(),
                    liquidation_heatmap,
                    tpo_cache: RefCell::new(TpoCache::default()),
                    last_footprint_cache_save: None,
                    drawings: Vec::new(),
                    drawing_state: RefCell::new(DrawingState::default()),
                    alerts: Vec::new(),
                    alert_drag_state: RefCell::new(None),
                    replay: None,
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

                let x_translation = match &kind {
                    KlineChartKind::Footprint { .. } => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (chart.cell_width / chart.scaling)
                    }
                    KlineChartKind::Candles => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (8.0 * chart.cell_width / chart.scaling)
                    }
                    KlineChartKind::Tpo { .. } => {
                        0.5 * (chart.bounds.width / chart.scaling)
                            - (8.0 * chart.cell_width / chart.scaling)
                    }
                };
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
                        rv.set_window_hours(config.rolling_vwap_window_hours, &data_source);
                    }
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
                    fetching_trades: (false, Vec::new()),
                    active_trade_fetches: 0,
                    request_handler: RequestHandler::new(),
                    kind: kind.clone(),
                    config,
                    study_configurator: study::Configurator::new(),
                    last_tick: Instant::now(),
                    liquidation_heatmap,
                    tpo_cache: RefCell::new(TpoCache::default()),
                    last_footprint_cache_save: None,
                    drawings: Vec::new(),
                    drawing_state: RefCell::new(DrawingState::default()),
                    alerts: Vec::new(),
                    alert_drag_state: RefCell::new(None),
                    replay: None,
                }
            }
        }
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
                        dp.kline.volume.0,
                        dp.kline.volume.1,
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
                        kline.volume.0,
                        kline.volume.1,
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

        let (visible_earliest, visible_latest) = self.visible_timerange()?;
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

            if first_in_view.is_none() && visible_earliest > 0 {
                let fetch_start = visible_earliest.saturating_sub(50 * timeframe_ms);
                let fetch_end = fetch_start.saturating_add(1000 * timeframe_ms);
                let range = FetchRange::Kline(fetch_start, fetch_end);
                if let Some(action) = request_fetch(&mut self.request_handler, range) {
                    return Some(action);
                }
            } else if let Some(first) = first_in_view
                && visible_earliest < first
                && visible_earliest > 0
            {
                let range = FetchRange::Kline(earliest, first);
                if let Some(action) = request_fetch(&mut self.request_handler, range) {
                    return Some(action);
                }
            } else {
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                let target_latest = visible_latest.min(now_ms);
                if target_latest > kline_latest.saturating_add(timeframe_ms / 2) {
                    let fetch_start = kline_latest.saturating_sub(timeframe_ms);
                    let fetch_end = target_latest.saturating_add(timeframe_ms * 5);
                    let range = FetchRange::Kline(fetch_start, fetch_end);
                    if let Some(action) = request_fetch(&mut self.request_handler, range) {
                        return Some(action);
                    }
                }
            }
        } else if visible_earliest < kline_earliest && visible_earliest > 0 && kline_earliest > 0 {
            let range = FetchRange::Kline(earliest, kline_earliest);

            if let Some(action) = request_fetch(&mut self.request_handler, range) {
                return Some(action);
            }
        }

        // priority 2, trades fetch (Footprint only)
        if matches!(self.kind, KlineChartKind::Footprint { .. }) {
            let mut trade_fetch_range = None;
            let mut needs_invalidation = false;

            if let PlotData::TimeBased(ref mut timeseries) = self.data_source
                && let Some((mut fetch_from, mut fetch_to)) =
                    timeseries.suggest_trade_fetch_range(visible_earliest, visible_latest)
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

                    while cur_d <= end_d && cur_d <= today {
                        let cache_path = data::chart::kline::footprint_cache_path(
                            &base_data_path,
                            &symbol,
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
                                    (dp.kline.volume.0 + dp.kline.volume.1) == 0.0
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
                                        (dp.kline.volume.0 + dp.kline.volume.1) == 0.0
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
                        if let Some((new_from, new_to)) =
                            timeseries.suggest_trade_fetch_range(visible_earliest, visible_latest)
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
            alert.status = AlertStatus::Active;
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

    pub fn update_selected_drawing_color(&mut self, color: [f32; 4]) {
        if let Some(d) = self.selected_drawing_mut() {
            d.color = color;
            self.invalidate_all();
        }
    }

    pub fn update_selected_drawing_width(&mut self, width: f32) {
        if let Some(d) = self.selected_drawing_mut() {
            d.width = width;
            self.invalidate_all();
        }
    }

    pub fn toggle_selected_drawing_lock(&mut self) {
        if let Some(d) = self.selected_drawing_mut() {
            d.is_locked = !d.is_locked;
            self.invalidate_all();
        }
    }

    pub fn set_active_drawing_tool(&mut self, tool: DrawingTool) {
        {
            let mut d = self.drawing_state.borrow_mut();
            d.active_tool = tool;
            d.in_progress = None;
            d.start_point = None;
        }
        self.invalidate_all();
    }

    pub fn active_drawing_tool(&self) -> DrawingTool {
        self.drawing_state.borrow().active_tool
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
            alert.status = match alert.status {
                AlertStatus::Active => AlertStatus::Muted,
                AlertStatus::Muted | AlertStatus::Triggered => AlertStatus::Active,
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
        self.invalidate_all();
    }

    pub fn is_replay_active(&self) -> bool {
        self.replay.as_ref().is_some_and(|r| r.active)
    }

    pub fn replay_state(&self) -> Option<&ReplayState> {
        self.replay.as_ref()
    }

    pub fn replay_play_pause(&mut self) {
        if let Some(ref mut rep) = self.replay {
            rep.is_playing = !rep.is_playing;
        }
        self.invalidate_all();
    }

    pub fn replay_step_forward(&mut self) {
        if let Some(ref mut rep) = self.replay {
            let step = match self.chart.basis {
                Basis::Time(tf) => tf.to_milliseconds(),
                Basis::Tick(_) => 10,
            };
            rep.cutoff_time = rep.cutoff_time.saturating_add(step);
        }
        self.invalidate_all();
    }

    pub fn replay_step_backward(&mut self) {
        if let Some(ref mut rep) = self.replay {
            let step = match self.chart.basis {
                Basis::Time(tf) => tf.to_milliseconds(),
                Basis::Tick(_) => 10,
            };
            rep.cutoff_time = rep.cutoff_time.saturating_sub(step);
        }
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
        } else {
            self.replay = Some(ReplayState::new(cutoff));
        }
        self.invalidate_all();
    }

    pub fn scroll_to_timestamp(&mut self, timestamp: u64) {
        if self.chart.scaling > f32::EPSILON && self.chart.bounds.width > f32::EPSILON {
            let base_x = match &self.kind {
                KlineChartKind::Footprint { .. } => {
                    0.5 * (self.chart.bounds.width / self.chart.scaling)
                        - (self.chart.cell_width / self.chart.scaling)
                }
                KlineChartKind::Candles | KlineChartKind::Tpo { .. } => {
                    0.5 * (self.chart.bounds.width / self.chart.scaling)
                        - (8.0 * self.chart.cell_width / self.chart.scaling)
                }
            };
            let x_offset = self.chart.interval_to_x(timestamp);
            self.chart.translation.x = base_x - x_offset;
        }
        self.chart.layout.autoscale = Some(Autoscale::FitToVisible);
        self.invalidate_all();
    }

    pub fn replay_set_cutoff_and_jump(&mut self, cutoff: u64) {
        if let Some(ref mut rep) = self.replay {
            rep.cutoff_time = cutoff;
            rep.is_playing = false;
        } else {
            self.replay = Some(ReplayState::new(cutoff));
        }
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
        let mut triggered = Vec::new();
        for alert in &mut self.alerts {
            if alert.check_trigger(prev_price, current_price) {
                alert.status = AlertStatus::Triggered;
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
        self.chart.last_price = None;
        self.chart.latest_x = 0;
        self.chart.basis = new_basis;

        match new_basis {
            Basis::Time(interval) => {
                let step = self.chart.tick_size;
                let timeseries = TimeSeries::<KlineDataPoint>::new(interval, step, &[])
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
        self.raw_trades.extend_from_slice(trades_buffer);

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
                        (dp.kline.volume.0 + dp.kline.volume.1) == 0.0
                            || (!dp.footprint.is_empty() && dp.trades_fetched)
                    });

                if is_day_complete {
                    let cache_path = data::chart::kline::footprint_cache_path(
                        &base_data_path,
                        &symbol,
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

    pub fn insert_raw_trades(&mut self, raw_trades: Vec<Trade>) {
        if raw_trades.is_empty() {
            return;
        }

        let existing_max_t = self.raw_trades.last().map(|t| t.time).unwrap_or(0);
        let existing_min_t = self.raw_trades.first().map(|t| t.time).unwrap_or(u64::MAX);

        let batch_min_t = raw_trades.first().map(|t| t.time).unwrap_or(0);
        let batch_max_t = raw_trades.last().map(|t| t.time).unwrap_or(0);

        let is_disjoint = if self.raw_trades.is_empty()
            || batch_min_t > existing_max_t
            || batch_max_t < existing_min_t
        {
            true
        } else {
            match self
                .raw_trades
                .binary_search_by_key(&batch_min_t, |t| t.time)
            {
                Err(idx_min) => {
                    idx_min == self.raw_trades.len() || self.raw_trades[idx_min].time > batch_max_t
                }
                Ok(_) => false,
            }
        };

        let new_trades: Vec<Trade> = if is_disjoint {
            raw_trades
        } else {
            raw_trades
                .into_iter()
                .filter(|trade| {
                    if let Ok(idx) = self
                        .raw_trades
                        .binary_search_by_key(&trade.time, |t| t.time)
                    {
                        let mut found = false;
                        let mut i = idx;
                        while i < self.raw_trades.len() && self.raw_trades[i].time == trade.time {
                            if self.raw_trades[i].price == trade.price
                                && self.raw_trades[i].is_sell == trade.is_sell
                                && (self.raw_trades[i].qty - trade.qty).abs() < 1e-5
                            {
                                found = true;
                                break;
                            }
                            i += 1;
                        }
                        if !found && idx > 0 {
                            let mut i = idx - 1;
                            loop {
                                if self.raw_trades[i].time != trade.time {
                                    break;
                                }
                                if self.raw_trades[i].price == trade.price
                                    && self.raw_trades[i].is_sell == trade.is_sell
                                    && (self.raw_trades[i].qty - trade.qty).abs() < 1e-5
                                {
                                    found = true;
                                    break;
                                }
                                if i == 0 {
                                    break;
                                }
                                i -= 1;
                            }
                        }
                        !found
                    } else {
                        true
                    }
                })
                .collect()
        };

        if !new_trades.is_empty() {
            match self.data_source {
                PlotData::TickBased(ref mut tick_aggr) => {
                    tick_aggr.insert_trades(&new_trades);
                }
                PlotData::TimeBased(ref mut timeseries) => {
                    timeseries.insert_trades_existing_buckets(&new_trades);
                }
            }

            self.save_footprint_cache(false);

            if self.raw_trades.is_empty() || batch_min_t >= existing_max_t {
                self.raw_trades.extend(new_trades);
            } else if batch_max_t <= existing_min_t {
                let mut merged = Vec::with_capacity(new_trades.len() + self.raw_trades.len());
                merged.extend(new_trades);
                merged.extend(std::mem::take(&mut self.raw_trades));
                self.raw_trades = merged;
            } else {
                let old = std::mem::take(&mut self.raw_trades);
                let mut merged = Vec::with_capacity(old.len() + new_trades.len());
                let mut i = 0;
                let mut j = 0;
                while i < old.len() && j < new_trades.len() {
                    if old[i].time <= new_trades[j].time {
                        merged.push(old[i]);
                        i += 1;
                    } else {
                        merged.push(new_trades[j]);
                        j += 1;
                    }
                }
                if i < old.len() {
                    merged.extend_from_slice(&old[i..]);
                }
                if j < new_trades.len() {
                    merged.extend_from_slice(&new_trades[j..]);
                }
                self.raw_trades = merged;
            }

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

                    let x_trans = match &self.kind {
                        KlineChartKind::Footprint { .. } => {
                            0.5 * (self.chart.bounds.width / self.chart.scaling)
                                - (self.chart.cell_width / self.chart.scaling)
                        }
                        KlineChartKind::Candles => {
                            0.5 * (self.chart.bounds.width / self.chart.scaling)
                                - (8.0 * self.chart.cell_width / self.chart.scaling)
                        }
                        KlineChartKind::Tpo { .. } => {
                            0.5 * (self.chart.bounds.width / self.chart.scaling)
                                - (8.0 * self.chart.cell_width / self.chart.scaling)
                        }
                    };
                    self.chart.translation.x = x_trans;
                    self.chart.translation.y = -self.chart.bounds.height / 2.0;
                    self.chart.layout.autoscale = Some(Autoscale::FitToVisible);
                } else if let Some(new_latest) = timeseries.latest_timestamp()
                    && new_latest > self.chart.latest_x
                {
                    self.chart.latest_x = new_latest;
                }

                self.indicators
                    .values_mut()
                    .filter_map(Option::as_mut)
                    .for_each(|indi| indi.on_insert_klines(klines_raw));

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

        match &self.data_source {
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
        }
    }

    pub fn last_update(&self) -> Instant {
        self.last_tick
    }

    pub fn invalidate(&mut self, now: Option<Instant>) -> Option<Action> {
        let chart = &mut self.chart;

        if let Some(autoscale) = chart.layout.autoscale {
            match autoscale {
                super::Autoscale::CenterLatest => {
                    let x_translation = match &self.kind {
                        KlineChartKind::Footprint { .. } => {
                            0.5 * (chart.bounds.width / chart.scaling)
                                - (chart.cell_width / chart.scaling)
                        }
                        KlineChartKind::Candles => {
                            0.5 * (chart.bounds.width / chart.scaling)
                                - (8.0 * chart.cell_width / chart.scaling)
                        }
                        KlineChartKind::Tpo { .. } => {
                            0.5 * (chart.bounds.width / chart.scaling)
                                - (8.0 * chart.cell_width / chart.scaling)
                        }
                    };
                    chart.translation.x = x_translation;

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
                    let (start_interval, end_interval) = chart.interval_range(&visible_region);

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
                    rep.cutoff_time = rep.cutoff_time.saturating_add(step * steps);
                    self.last_tick = t;
                }
            } else {
                self.last_tick = t;
            }
            self.missing_data_task()
        } else {
            None
        }
    }

    pub fn active_panel_indicators_count(&self) -> usize {
        let market = self.chart.ticker_info.market_type();
        self.indicators
            .iter()
            .filter(|(kind, val)| {
                kind.is_panel()
                    && KlineIndicator::for_market(market).contains(kind)
                    && val.is_some()
            })
            .count()
    }

    pub fn toggle_indicator(&mut self, indicator: KlineIndicator) {
        let prev_panel_count = self.active_panel_indicators_count();

        if self.indicators[indicator].is_some() {
            self.indicators[indicator] = None;
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
                rv.set_window_hours(self.config.rolling_vwap_window_hours, &self.data_source);
            }
            if indicator == KlineIndicator::LiquidationHeatmap {
                self.liquidation_heatmap.config.enabled = true;
                self.rebuild_liquidation_heatmap();
            }
            box_indi.rebuild_from_source(&self.data_source);
            self.indicators[indicator] = Some(box_indi);
        }

        self.invalidate(None);

        if indicator.is_panel() {
            let current_panel_count = self.active_panel_indicators_count();
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

    pub fn config(&self) -> data::chart::kline::Config {
        self.config
    }

    pub fn set_visual_config(&mut self, config: data::chart::kline::Config) {
        self.config = config;
        data::chart::kline::set_user_default_kline_config(config);

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
            rv.set_window_hours(config.rolling_vwap_window_hours, &self.data_source);
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
        // 1. Dismiss context menu on Escape key
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
            && matches!(
                key.as_ref(),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            )
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
            return Some(canvas::Action::request_redraw().and_capture());
        }

        // 2. Dismiss context menu on wheel scroll or panning movement
        if let Event::Mouse(mouse::Event::WheelScrolled { .. }) = event
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
        }
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event
            && matches!(interaction, Interaction::Panning { .. })
            && self.tpo_cache.borrow().context_menu.is_some()
        {
            self.tpo_cache.borrow_mut().context_menu = None;
        }

        // 3. Handle Right Click: open/toggle context menu on the hit session
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(pos) = cursor.position_in(bounds)
            && matches!(self.kind, KlineChartKind::Tpo { .. })
        {
            let chart = self.state();
            let frame_pos = Point::new(
                (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
            );
            let click_time = chart.x_to_interval(frame_pos.x) as i64;
            let mut cache = self.tpo_cache.borrow_mut();

            let matched = cache
                .session_ranges
                .iter()
                .find(|sr| {
                    let sx1 = chart.interval_to_x(sr.session_start as u64);
                    let sx2 = chart.interval_to_x(sr.session_end as u64);
                    let min_x = sx1.min(sx2);
                    let max_x = sx1.max(sx2);
                    (click_time >= sr.session_start && click_time < sr.session_end)
                        || (frame_pos.x >= min_x && frame_pos.x <= max_x)
                })
                .cloned();

            if let Some(session) = matched {
                let menu_w = 220.0;
                let item_h = 28.0;
                let pad_v = 4.0;
                let menu_h = pad_v * 2.0 + 4.0 * item_h;
                let menu_x = pos.x.min(bounds.width - menu_w - 4.0).max(4.0);
                let menu_y = pos.y.min(bounds.height - menu_h - 4.0).max(4.0);
                let menu_rect =
                    Rectangle::new(Point::new(menu_x, menu_y), Size::new(menu_w, menu_h));

                let mut items = Vec::new();
                let split_label = if session.is_split_brackets {
                    "Collapse".to_string()
                } else {
                    "Split Brackets".to_string()
                };
                items.push(TpoMenuItem {
                    label: split_label,
                    action: Some(TpoMenuAction::ToggleSplitBrackets(session.session_start)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Merge with Next Session".to_string(),
                    action: session
                        .next_start
                        .map(|ns| TpoMenuAction::MergeNext(session.session_start, ns)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Merge with Previous Session".to_string(),
                    action: session
                        .prev_start
                        .map(|ps| TpoMenuAction::MergePrev(ps, session.session_start)),
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h * 2.0),
                        Size::new(menu_w, item_h),
                    ),
                });

                items.push(TpoMenuItem {
                    label: "Reset Cluster".to_string(),
                    action: if session.is_clustered {
                        Some(TpoMenuAction::ResetCluster(session.session_start))
                    } else {
                        None
                    },
                    rect: Rectangle::new(
                        Point::new(menu_x, menu_y + pad_v + item_h * 3.0),
                        Size::new(menu_w, item_h),
                    ),
                });

                cache.context_menu = Some(TpoContextMenu {
                    rect: menu_rect,
                    items,
                });
                return Some(canvas::Action::request_redraw().and_capture());
            } else if cache.context_menu.is_some() {
                cache.context_menu = None;
                return Some(canvas::Action::request_redraw().and_capture());
            }
        }

        // 4. Handle Left Click: execute menu action or dismiss
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(pos) = cursor.position_in(bounds)
        {
            let mut cache = self.tpo_cache.borrow_mut();
            if let Some(menu) = cache.context_menu.take() {
                if menu.rect.contains(pos) {
                    for item in menu.items {
                        if item.rect.contains(pos) {
                            if let Some(action) = item.action {
                                let msg = match action {
                                    TpoMenuAction::ToggleSplitBrackets(s) => {
                                        Message::ToggleSplitBrackets(s)
                                    }
                                    TpoMenuAction::MergeNext(s1, s2) => {
                                        Message::MergeSessions(s1, s2)
                                    }
                                    TpoMenuAction::MergePrev(s1, s2) => {
                                        Message::MergeSessions(s1, s2)
                                    }
                                    TpoMenuAction::ResetCluster(s) => Message::SplitCluster(s),
                                };
                                return Some(canvas::Action::publish(msg).and_capture());
                            } else {
                                return Some(canvas::Action::request_redraw().and_capture());
                            }
                        }
                    }
                }
                return Some(canvas::Action::request_redraw().and_capture());
            }
        }

        // 5. Handle Drawing Tools interaction
        {
            let mut d_state = self.drawing_state.borrow_mut();
            if d_state.active_tool != DrawingTool::None || d_state.in_progress.is_some() {
                // Cancel on Escape or Right click
                if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                    && matches!(
                        key.as_ref(),
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    )
                {
                    d_state.active_tool = DrawingTool::None;
                    d_state.in_progress = None;
                    d_state.start_point = None;
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
                    d_state.active_tool = DrawingTool::None;
                    d_state.in_progress = None;
                    d_state.start_point = None;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();

                    match event {
                        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                            if let Some(ref mut in_prog) = d_state.in_progress {
                                match &mut in_prog.kind {
                                    DrawingKind::Trendline { p2, .. } => {
                                        *p2 = (time, price);
                                    }
                                    DrawingKind::Rectangle { p2, .. } => {
                                        *p2 = (time, price);
                                    }
                                    DrawingKind::HorizontalLine { price: p } => {
                                        *p = price;
                                    }
                                    DrawingKind::Brush { points } => {
                                        points.push((time, price));
                                    }
                                    DrawingKind::Path { points } => {
                                        if let Some(last) = points.last_mut() {
                                            *last = (time, price);
                                        }
                                    }
                                    DrawingKind::Position { stop_price, .. } => {
                                        *stop_price = price;
                                    }
                                }
                                return Some(canvas::Action::request_redraw().and_capture());
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                            match d_state.active_tool {
                                DrawingTool::HorizontalLine => {
                                    let drawing =
                                        Drawing::horizontal(price, [0.95, 0.65, 0.15, 1.0], 1.5);
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    );
                                }
                                DrawingTool::Trendline => {
                                    if let Some(start) = d_state.start_point.take() {
                                        let drawing = Drawing::trendline(
                                            start,
                                            (time, price),
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        );
                                        d_state.active_tool = DrawingTool::None;
                                        d_state.in_progress = None;
                                        return Some(
                                            canvas::Action::publish(Message::AddDrawing(drawing))
                                                .and_capture(),
                                        );
                                    } else {
                                        d_state.start_point = Some((time, price));
                                        d_state.in_progress = Some(Drawing::trendline(
                                            (time, price),
                                            (time, price),
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        ));
                                        return Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        );
                                    }
                                }
                                DrawingTool::Rectangle => {
                                    if let Some(start) = d_state.start_point.take() {
                                        let drawing = Drawing::rectangle(
                                            start,
                                            (time, price),
                                            [0.3, 0.6, 0.95, 0.8],
                                            1.0,
                                        );
                                        d_state.active_tool = DrawingTool::None;
                                        d_state.in_progress = None;
                                        return Some(
                                            canvas::Action::publish(Message::AddDrawing(drawing))
                                                .and_capture(),
                                        );
                                    } else {
                                        d_state.start_point = Some((time, price));
                                        d_state.in_progress = Some(Drawing::rectangle(
                                            (time, price),
                                            (time, price),
                                            [0.3, 0.6, 0.95, 0.8],
                                            1.0,
                                        ));
                                        return Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        );
                                    }
                                }
                                DrawingTool::Brush => {
                                    d_state.start_point = Some((time, price));
                                    d_state.in_progress = Some(Drawing::brush(
                                        vec![(time, price)],
                                        [0.35, 0.7, 1.0, 1.0],
                                        2.0,
                                    ));
                                    return Some(canvas::Action::request_redraw().and_capture());
                                }
                                DrawingTool::Path => {
                                    if let Some(Drawing {
                                        kind: DrawingKind::Path { points },
                                        ..
                                    }) = &mut d_state.in_progress
                                    {
                                        points.push((time, price));
                                        if points.len() >= 4
                                            && let Some(drawing) = d_state.in_progress.take()
                                        {
                                            d_state.active_tool = DrawingTool::None;
                                            return Some(
                                                canvas::Action::publish(Message::AddDrawing(
                                                    drawing,
                                                ))
                                                .and_capture(),
                                            );
                                        }
                                        return Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        );
                                    } else {
                                        d_state.in_progress = Some(Drawing::path(
                                            vec![(time, price), (time, price)],
                                            [0.3, 0.75, 1.0, 1.0],
                                            1.5,
                                        ));
                                        return Some(
                                            canvas::Action::request_redraw().and_capture(),
                                        );
                                    }
                                }
                                DrawingTool::ShortPosition => {
                                    let stop_price = price * 1.015;
                                    let target_price = price * 0.955;
                                    let drawing = Drawing::position(
                                        (time, price),
                                        stop_price,
                                        target_price,
                                        false,
                                        [0.9, 0.3, 0.3, 1.0],
                                        1.0,
                                    );
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    );
                                }
                                DrawingTool::LongPosition => {
                                    let stop_price = price * 0.985;
                                    let target_price = price * 1.045;
                                    let drawing = Drawing::position(
                                        (time, price),
                                        stop_price,
                                        target_price,
                                        true,
                                        [0.2, 0.8, 0.4, 1.0],
                                        1.0,
                                    );
                                    d_state.active_tool = DrawingTool::None;
                                    d_state.in_progress = None;
                                    return Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    );
                                }
                                DrawingTool::None => {}
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                            if let Some(drawing) = d_state.in_progress.take() {
                                if matches!(drawing.kind, DrawingKind::Brush { .. }) {
                                    d_state.active_tool = DrawingTool::None;
                                    return Some(
                                        canvas::Action::publish(Message::AddDrawing(drawing))
                                            .and_capture(),
                                    );
                                } else {
                                    d_state.in_progress = Some(drawing);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 5.5. Handle Drawing Selection, Moving & Modification (TradingView style)
        if self.drawing_state.borrow().active_tool == DrawingTool::None {
            // Cancel / deselect on Escape
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                if matches!(
                    key.as_ref(),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                ) {
                    let mut d_state = self.drawing_state.borrow_mut();
                    if d_state.selected_drawing.take().is_some() {
                        return Some(
                            canvas::Action::publish(Message::SelectDrawing(None)).and_capture(),
                        );
                    }
                }
                // Delete selected drawing on Delete or Backspace
                if matches!(
                    key.as_ref(),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete)
                        | iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace)
                ) && let Some(id) = self.drawing_state.borrow_mut().selected_drawing.take()
                {
                    return Some(canvas::Action::publish(Message::DeleteDrawing(id)).and_capture());
                }
            }

            // Dragging an existing drawing or handle
            let is_drawing_drag = self.drawing_state.borrow().drag.is_some();
            if is_drawing_drag {
                let chart = self.state();
                if let Some(pos) = cursor.position_in(bounds) {
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();

                    match event {
                        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                            let mut d_state = self.drawing_state.borrow_mut();
                            if let Some(DrawingDrag::MovingDrawing {
                                start_time,
                                start_price,
                                initial_drawing,
                                current_drawing,
                                ..
                            }) = &mut d_state.drag
                            {
                                let time_delta = time as i64 - *start_time as i64;
                                let price_delta = price - *start_price;
                                *current_drawing = initial_drawing.clone();
                                current_drawing.translate(time_delta, price_delta);
                                return Some(canvas::Action::request_redraw().and_capture());
                            } else if let Some(DrawingDrag::MovingHandle {
                                handle_idx,
                                initial_drawing,
                                current_drawing,
                                ..
                            }) = &mut d_state.drag
                            {
                                *current_drawing = initial_drawing.clone();
                                match &mut current_drawing.kind {
                                    DrawingKind::HorizontalLine { price: p } => *p = price,
                                    DrawingKind::Trendline { p1, p2 } => {
                                        if *handle_idx == 0 {
                                            *p1 = (time, price);
                                        } else {
                                            *p2 = (time, price);
                                        }
                                    }
                                    DrawingKind::Rectangle { p1, p2 } => {
                                        if *handle_idx == 0 {
                                            *p1 = (time, price);
                                        } else if *handle_idx == 1 {
                                            p1.1 = price;
                                        } else if *handle_idx == 2 {
                                            p2.0 = time;
                                            p1.1 = price;
                                        } else if *handle_idx == 3 {
                                            p2.0 = time;
                                        } else if *handle_idx == 4 {
                                            *p2 = (time, price);
                                        } else if *handle_idx == 5 {
                                            p2.1 = price;
                                        } else if *handle_idx == 6 {
                                            p1.0 = time;
                                            p2.1 = price;
                                        } else if *handle_idx == 7 {
                                            p1.0 = time;
                                        }
                                    }
                                    DrawingKind::Path { points } => {
                                        if let Some(pt) = points.get_mut(*handle_idx) {
                                            *pt = (time, price);
                                        }
                                    }
                                    DrawingKind::Brush { .. } => {}
                                    DrawingKind::Position {
                                        entry,
                                        target_price,
                                        stop_price,
                                        ..
                                    } => {
                                        if *handle_idx == 0 {
                                            *entry = (time, price);
                                        } else if *handle_idx == 1 {
                                            *target_price = price;
                                        } else if *handle_idx == 2 {
                                            *stop_price = price;
                                        }
                                    }
                                }
                                return Some(canvas::Action::request_redraw().and_capture());
                            }
                        }
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                            let mut d_state = self.drawing_state.borrow_mut();
                            if let Some(drag) = d_state.drag.take() {
                                let current_drawing = match drag {
                                    DrawingDrag::MovingDrawing {
                                        current_drawing, ..
                                    }
                                    | DrawingDrag::MovingHandle {
                                        current_drawing, ..
                                    } => current_drawing,
                                };
                                return Some(
                                    canvas::Action::publish(Message::UpdateDrawing(
                                        current_drawing,
                                    ))
                                    .and_capture(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            } else if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                // Check if an alert is near; if so, let alert dragging take precedence
                let is_near_alert = if let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    self.alerts.iter().any(|a| {
                        let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                        let alert_screen_y =
                            (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                        (pos.y - alert_screen_y).abs() <= 8.0
                    })
                } else {
                    false
                };

                if !is_near_alert && let Some(pos) = cursor.position_in(bounds) {
                    let chart = self.state();
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    let time = chart.x_to_interval(frame_pos.x);
                    let price = chart.y_to_price(frame_pos.y).to_f32();
                    let threshold = 8.0 / chart.scaling;

                    // Check handles of currently selected drawing
                    let mut handle_hit = None;
                    if let Some(sel_id) = self.drawing_state.borrow().selected_drawing
                        && let Some(sel_d) = self.drawings.iter().find(|d| d.id == sel_id)
                        && let Some(HitTarget::Handle(idx)) =
                            hit_test_drawing(sel_d, frame_pos, chart, threshold)
                    {
                        handle_hit = Some((sel_d.clone(), idx));
                    }

                    if let Some((sel_d, idx)) = handle_hit {
                        if !sel_d.is_locked {
                            self.drawing_state.borrow_mut().drag =
                                Some(DrawingDrag::MovingHandle {
                                    handle_idx: idx,
                                    initial_drawing: sel_d.clone(),
                                    current_drawing: sel_d,
                                });
                        }
                        return Some(canvas::Action::request_redraw().and_capture());
                    }

                    // Check any drawing
                    let hit = self.drawings.iter().rev().find_map(|d| {
                        hit_test_drawing(d, frame_pos, chart, threshold)
                            .map(|target| (d.clone(), target))
                    });

                    if let Some((drawing, target)) = hit {
                        let mut d_state = self.drawing_state.borrow_mut();
                        d_state.selected_drawing = Some(drawing.id);
                        if !drawing.is_locked {
                            match target {
                                HitTarget::Handle(idx) => {
                                    d_state.drag = Some(DrawingDrag::MovingHandle {
                                        handle_idx: idx,
                                        initial_drawing: drawing.clone(),
                                        current_drawing: drawing.clone(),
                                    });
                                }
                                HitTarget::Body => {
                                    d_state.drag = Some(DrawingDrag::MovingDrawing {
                                        start_time: time,
                                        start_price: price,
                                        initial_drawing: drawing.clone(),
                                        current_drawing: drawing.clone(),
                                    });
                                }
                            }
                        }
                        return Some(
                            canvas::Action::publish(Message::SelectDrawing(Some(drawing.id)))
                                .and_capture(),
                        );
                    } else {
                        // Clicked empty space: deselect
                        let mut d_state = self.drawing_state.borrow_mut();
                        if d_state.selected_drawing.take().is_some() {
                            return Some(
                                canvas::Action::publish(Message::SelectDrawing(None)).and_capture(),
                            );
                        }
                    }
                }
            }
        }

        // 6. Handle Price Alert Dragging (TradingView style)
        if self.drawing_state.borrow().active_tool == DrawingTool::None {
            // Cancel alert drag on Escape
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                && matches!(
                    key.as_ref(),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                )
                && self.alert_drag_state.borrow_mut().take().is_some()
            {
                return Some(canvas::Action::request_redraw().and_capture());
            }

            let is_dragging = self.alert_drag_state.borrow().is_some();
            if is_dragging {
                let chart = self.state();
                let calc_price = |cursor: mouse::Cursor| -> Option<f32> {
                    let pos = cursor.position_from(bounds.position())?;
                    let frame_pos = Point::new(
                        (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                        (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                    );
                    Some(chart.y_to_price(frame_pos.y).to_f32())
                };

                match event {
                    Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if let Some(price) = calc_price(cursor)
                            && let Some(drag) = self.alert_drag_state.borrow_mut().as_mut()
                        {
                            drag.1 = price;
                            return Some(canvas::Action::request_redraw().and_capture());
                        }
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        if let Some((id, drag_price)) = self.alert_drag_state.borrow_mut().take() {
                            let final_price = calc_price(cursor).unwrap_or(drag_price);
                            return Some(
                                canvas::Action::publish(Message::UpdateAlertPrice(id, final_price))
                                    .and_capture(),
                            );
                        }
                    }
                    _ => {}
                }
            } else if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
                && let Some(pos) = cursor.position_in(bounds)
            {
                let chart = self.state();
                let frame_pos = Point::new(
                    (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x,
                    (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y,
                );
                let current_cursor_price = chart.y_to_price(frame_pos.y).to_f32();

                let found_alert = self.alerts.iter().find(|a| {
                    let alert_y = chart.price_to_y(Price::from_f32(a.target_price));
                    let alert_screen_y =
                        (alert_y + chart.translation.y) * chart.scaling + bounds.height / 2.0;
                    (pos.y - alert_screen_y).abs() <= 8.0
                });

                if let Some(alert) = found_alert {
                    *self.alert_drag_state.borrow_mut() = Some((alert.id, current_cursor_price));
                    return Some(canvas::Action::request_redraw().and_capture());
                }
            }
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
            let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);

            frame.translate(center);
            frame.scale(chart.scaling);
            frame.translate(chart.translation);

            let region = chart.visible_region(frame.size());
            let (earliest, raw_latest) = chart.interval_range(&region);
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
                chart.draw_last_price_line(frame, palette, region);
                return;
            }

            let (p_high, p_low) = chart.price_range(&region);
            let visible_max_price = p_high.to_f32_lossy().max(p_low.to_f32_lossy()) as f64;
            let visible_min_price = p_high.to_f32_lossy().min(p_low.to_f32_lossy()) as f64;

            let price_to_y = |price| chart.price_to_y(price);
            let interval_to_x = |interval| chart.interval_to_x(interval);

            if self.indicators[KlineIndicator::LiquidationHeatmap].is_some() {
                let current_price = chart
                    .last_price
                    .as_ref()
                    .map(|lp| lp.price().to_f32())
                    .unwrap_or_else(|| chart.base_price_y.to_f32_lossy());
                draw_liquidation_heatmap(
                    &self.liquidation_heatmap,
                    frame,
                    &region,
                    price_to_y,
                    interval_to_x,
                    earliest,
                    latest,
                    current_price,
                    self.config.liq_show_bands,
                    self.config.liq_show_histogram,
                    palette.is_dark,
                );
            }

            match &self.kind {
                KlineChartKind::Footprint {
                    clusters,
                    scaling,
                    studies,
                    show_bottom_volume,
                } => {
                    let (highest, lowest) = chart.price_range(&region);

                    let max_cluster_qty = self.calc_qty_scales(
                        earliest,
                        latest,
                        highest,
                        lowest,
                        chart.tick_size,
                        *clusters,
                    );

                    let cell_height_unscaled = chart.cell_height * chart.scaling;
                    let cell_width_unscaled = chart.cell_width * chart.scaling;

                    let text_screen_size = {
                        let text_size_from_height = (cell_height_unscaled * 0.72).clamp(8.0, 13.0);
                        let text_size_from_width = (cell_width_unscaled * 0.12).clamp(8.0, 13.0);

                        text_size_from_height.min(text_size_from_width)
                    };
                    let text_size = text_screen_size / chart.scaling;

                    let candle_width = 0.1 * chart.cell_width;
                    let content_spacing = ContentGaps::from_view(candle_width, chart.scaling);

                    let imbalance = studies.iter().find_map(|study| {
                        if let FootprintStudy::Imbalance {
                            threshold,
                            color_scale,
                            ignore_zeros,
                        } = study
                        {
                            Some((*threshold, *color_scale, *ignore_zeros))
                        } else {
                            None
                        }
                    });

                    let min_w = match clusters {
                        ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => 50.0,
                        ClusterKind::BidAsk => 80.0,
                    };
                    let lod = lod_level(cell_width_unscaled, cell_height_unscaled, min_w);
                    let show_text = lod == LodLevel::Full;
                    let has_npoc = studies
                        .iter()
                        .any(|s| matches!(s, FootprintStudy::NPoC { .. }));

                    if lod != LodLevel::Candle {
                        draw_all_npocs(
                            &self.data_source,
                            frame,
                            price_to_y,
                            interval_to_x,
                            candle_width,
                            chart.cell_width,
                            chart.cell_height,
                            palette,
                            studies,
                            earliest,
                            latest,
                            *clusters,
                            content_spacing,
                            imbalance.is_some(),
                        );

                        render_data_source(
                            &self.data_source,
                            frame,
                            earliest,
                            latest,
                            interval_to_x,
                            |frame, x_position, kline, trades| {
                                let cluster_scaling = effective_cluster_qty(
                                    *scaling,
                                    max_cluster_qty,
                                    trades,
                                    *clusters,
                                );

                                draw_clusters(
                                    frame,
                                    price_to_y,
                                    x_position,
                                    chart.cell_width,
                                    chart.cell_height,
                                    chart.scaling,
                                    candle_width,
                                    cluster_scaling,
                                    palette,
                                    text_size,
                                    self.tick_size(),
                                    show_text,
                                    imbalance,
                                    kline,
                                    trades,
                                    *clusters,
                                    content_spacing,
                                    has_npoc,
                                );

                                // ClusterSearch highlight overlay (supports multiple search rules)
                                for study in studies {
                                    if let FootprintStudy::ClusterSearch {
                                        min_volume,
                                        min_delta,
                                        side,
                                        style,
                                        color,
                                        ..
                                    } = study
                                    {
                                        draw_cluster_search_highlights(
                                            frame,
                                            price_to_y,
                                            x_position,
                                            chart.cell_width,
                                            chart.cell_height,
                                            chart.scaling,
                                            trades,
                                            *min_volume,
                                            *min_delta,
                                            *side,
                                            *style,
                                            *color,
                                        );
                                    }
                                }
                            },
                        );
                    } else {
                        // LOD::Candle — render plain thin candles, with optional classic bottom volume
                        let plain_candle_width = (chart.cell_width * 0.8).max(1.0);
                        let show_vol = *show_bottom_volume;
                        let max_vol = if show_vol && earliest <= latest {
                            match &self.data_source {
                                PlotData::TimeBased(ts) => ts
                                    .datapoints
                                    .range(earliest..=latest)
                                    .map(|(_, dp)| dp.kline.volume.0 + dp.kline.volume.1)
                                    .fold(0.0_f32, f32::max),
                                PlotData::TickBased(ta) => {
                                    let e = earliest as usize;
                                    let l = latest as usize;
                                    ta.datapoints
                                        .iter()
                                        .enumerate()
                                        .filter(|(i, _)| *i >= e && *i <= l)
                                        .map(|(_, dp)| dp.kline.volume.0 + dp.kline.volume.1)
                                        .fold(0.0_f32, f32::max)
                                }
                            }
                        } else {
                            0.0
                        };
                        let max_vol_h = region.height * 0.18;
                        let bottom_y = region.y + region.height;

                        render_data_source(
                            &self.data_source,
                            frame,
                            earliest,
                            latest,
                            interval_to_x,
                            |frame, x_position, kline, trades| {
                                draw_candle_dp(
                                    frame,
                                    price_to_y,
                                    plain_candle_width,
                                    palette,
                                    x_position,
                                    kline,
                                );

                                if show_vol && max_vol > 0.0 {
                                    let vol = kline.volume.0 + kline.volume.1;
                                    let h = (vol / max_vol) * max_vol_h;
                                    let vol_color = if kline.close >= kline.open {
                                        palette.success.base.color.scale_alpha(0.50)
                                    } else {
                                        palette.danger.base.color.scale_alpha(0.50)
                                    };
                                    frame.fill_rectangle(
                                        Point::new(
                                            x_position - plain_candle_width / 2.0,
                                            bottom_y - h,
                                        ),
                                        Size::new(plain_candle_width, h),
                                        vol_color,
                                    );
                                }

                                for study in studies {
                                    if let FootprintStudy::ClusterSearch {
                                        min_volume,
                                        min_delta,
                                        side,
                                        style,
                                        color,
                                        ..
                                    } = study
                                    {
                                        draw_cluster_search_highlights(
                                            frame,
                                            price_to_y,
                                            x_position,
                                            chart.cell_width,
                                            chart.cell_height,
                                            chart.scaling,
                                            trades,
                                            *min_volume,
                                            *min_delta,
                                            *side,
                                            *style,
                                            *color,
                                        );
                                    }
                                }
                            },
                        );
                    }
                }
                KlineChartKind::Candles => {
                    let candle_width = chart.cell_width * 0.8;

                    render_data_source(
                        &self.data_source,
                        frame,
                        earliest,
                        latest,
                        interval_to_x,
                        |frame, x_position, kline, _| {
                            draw_candle_dp(
                                frame,
                                price_to_y,
                                candle_width,
                                palette,
                                x_position,
                                kline,
                            );
                        },
                    );
                }
                KlineChartKind::Tpo {
                    show_candles,
                    show_letters,
                    show_ib,
                    show_va,
                    show_poc,
                    show_single_prints,
                    tick_step,
                    period: _,
                    clusters,
                    split_sessions,
                    color_scheme,
                    ib_color,
                    poc_color,
                    single_prints_color,
                } => {
                    let base_price = chart.base_price_y.to_f32_lossy() as f64;
                    let exchange_tick = chart.tick_size.to_f32_lossy() as f64;
                    let tpo_tick = data::chart::tpo::calculate_tpo_tick_size(
                        base_price,
                        exchange_tick,
                        tick_step.multiplier(),
                    );
                    let effective_period = self.kind.effective_tpo_period();

                    draw_tpo_profiles(
                        &self.data_source,
                        &self.tpo_cache,
                        frame,
                        price_to_y,
                        interval_to_x,
                        &region,
                        chart.cell_width,
                        chart.cell_height,
                        chart.scaling,
                        chart.tick_size.to_f32_lossy(),
                        tpo_tick,
                        earliest,
                        latest,
                        visible_min_price,
                        visible_max_price,
                        *show_letters,
                        *show_ib,
                        *show_va,
                        *show_poc,
                        *show_single_prints,
                        effective_period,
                        clusters,
                        split_sessions,
                        *color_scheme,
                        *ib_color,
                        *poc_color,
                        *single_prints_color,
                        palette,
                    );

                    if *show_candles {
                        let candle_width = chart.cell_width * 0.8;
                        render_data_source(
                            &self.data_source,
                            frame,
                            earliest,
                            latest,
                            interval_to_x,
                            |frame, x_position, kline, _| {
                                draw_candle_dp(
                                    frame,
                                    price_to_y,
                                    candle_width,
                                    palette,
                                    x_position,
                                    kline,
                                );
                            },
                        );
                    }
                }
            }

            if self.indicators[KlineIndicator::Vwap].is_some() {
                draw_vwap_overlay(
                    &self.data_source,
                    frame,
                    price_to_y,
                    interval_to_x,
                    earliest,
                    latest,
                );
            }

            if self.indicators[KlineIndicator::RollingVwap].is_some() {
                let rolling_indicator = self.indicators[KlineIndicator::RollingVwap]
                    .as_ref()
                    .and_then(|box_indi| box_indi.as_any())
                    .and_then(|any| any.downcast_ref::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>());

                draw_rolling_vwap_overlay(
                    rolling_indicator,
                    &self.data_source,
                    frame,
                    price_to_y,
                    interval_to_x,
                    earliest,
                    latest,
                    &self.config,
                );
            }

            if self.indicators[KlineIndicator::Tpo].is_some()
                && !matches!(self.kind, KlineChartKind::Tpo { .. })
            {
                let base_price = chart.base_price_y.to_f32_lossy() as f64;
                let exchange_tick = chart.tick_size.to_f32_lossy() as f64;
                let tpo_tick =
                    data::chart::tpo::calculate_tpo_tick_size(base_price, exchange_tick, 0);
                draw_tpo_profiles(
                    &self.data_source,
                    &self.tpo_cache,
                    frame,
                    price_to_y,
                    interval_to_x,
                    &region,
                    chart.cell_width,
                    chart.cell_height,
                    chart.scaling,
                    chart.tick_size.to_f32_lossy(),
                    tpo_tick,
                    earliest,
                    latest,
                    visible_min_price,
                    visible_max_price,
                    true,
                    false,
                    true,
                    true,
                    true,
                    data::chart::tpo::SessionPeriod::Daily,
                    &[],
                    &[],
                    data::chart::kline::TpoColorScheme::default(),
                    data::chart::kline::TpoElementColor::default(),
                    data::chart::kline::TpoElementColor::default(),
                    data::chart::kline::TpoElementColor::default(),
                    palette,
                );
            }


            chart.draw_last_price_line(frame, palette, region);
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
        draw_chart_drawings(
            &mut dynamic_frame,
            &self.drawings,
            &d_state.in_progress,
            drag_drawing,
            chart,
            &region,
            selected_drawing_id,
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
            return mouse::Interaction::Crosshair;
        }
        match interaction {
            Interaction::Panning { .. } => mouse::Interaction::Grabbing,
            Interaction::Zoomin { .. } => mouse::Interaction::ZoomIn,
            Interaction::None | Interaction::Ruler { .. } => {
                if cursor.is_over(bounds) {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            }
        }
    }
}

fn draw_footprint_kline(
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

fn draw_candle_dp(
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

fn render_data_source<F>(
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

fn draw_all_npocs(
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
        ClusterKind::BidAsk => 1.0,
    };

    let start_x_for = |cell_center_x: f32| -> f32 {
        match cluster_kind {
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
            ClusterKind::BidAsk => cell_center_x, // not used for BidAsk clustering
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

fn effective_cluster_qty(
    scaling: ClusterScaling,
    visible_max: f32,
    footprint: &KlineTrades,
    cluster_kind: ClusterKind,
) -> f32 {
    let individual_max = match cluster_kind {
        ClusterKind::BidAsk => footprint
            .trades
            .values()
            .map(|group| group.buy_qty.max(group.sell_qty))
            .fold(0.0_f32, f32::max),
        ClusterKind::DeltaProfile => footprint
            .trades
            .values()
            .map(|group| (group.buy_qty - group.sell_qty).abs())
            .fold(0.0_f32, f32::max),
        ClusterKind::VolumeProfile => footprint
            .trades
            .values()
            .map(|group| group.buy_qty + group.sell_qty)
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
fn draw_clusters(
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
) {
    let text_color = palette.background.weakest.text;

    let bar_width_factor: f32 = 0.9;
    let inset = (cell_width * (1.0 - bar_width_factor)) / 2.0;

    let cell_left = x_position - (cell_width / 2.0);
    let content_left = cell_left + inset;
    let content_right = x_position + (cell_width / 2.0) - inset;

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

            for (price, group) in &footprint.trades {
                let y = price_to_y(*price);

                match cluster_kind {
                    ClusterKind::VolumeProfile => {
                        super::draw_volume_bar(
                            frame,
                            area.bars_left,
                            y,
                            group.buy_qty,
                            group.sell_qty,
                            max_cluster_qty,
                            area.bars_width,
                            cell_height,
                            palette.success.base.color,
                            palette.danger.base.color,
                            bar_alpha,
                            true,
                        );

                        if show_text {
                            let text_str = abbr_large_numbers(group.total_qty());
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
                        let delta = group.delta_qty();
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

                        let bar_width = (delta.abs() / max_cluster_qty) * area.bars_width;
                        if bar_width > 0.0 {
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
                    _ => {}
                }

                if let Some((threshold, color_scale, ignore_zeros)) = imbalance {
                    let step = PriceStep::from_f32(tick_size);
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
                        *price,
                        group.sell_qty,
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

            for (price, group) in &footprint.trades {
                let y = price_to_y(*price);

                if group.buy_qty > 0.0 && right_area_width > 0.0 {
                    if show_text {
                        let text_str = abbr_large_numbers(group.buy_qty);
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

                    let bar_width = (group.buy_qty / max_cluster_qty) * right_area_width;
                    if bar_width > 0.0 {
                        frame.fill_rectangle(
                            Point::new(area.bid_area_left, y - (cell_height / 2.0)),
                            Size::new(bar_width, cell_height),
                            palette.success.base.color.scale_alpha(bar_alpha),
                        );
                    }
                }
                if group.sell_qty > 0.0 && left_area_width > 0.0 {
                    if show_text {
                        let text_str = abbr_large_numbers(group.sell_qty);
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

                    let bar_width = (group.sell_qty / max_cluster_qty) * left_area_width;
                    if bar_width > 0.0 {
                        frame.fill_rectangle(
                            Point::new(area.ask_area_right, y - (cell_height / 2.0)),
                            Size::new(-bar_width, cell_height),
                            palette.danger.base.color.scale_alpha(bar_alpha),
                        );
                    }
                }

                if let Some((threshold, color_scale, ignore_zeros)) = imbalance
                    && area.imb_marker_width > 0.0
                {
                    let step = PriceStep::from_f32(tick_size);
                    let higher_price =
                        Price::from_f32(price.to_f32() + tick_size).round_to_step(step);

                    let rect_width = ((area.imb_marker_width - 1.0) / 2.0).max(1.0);

                    let buyside_x = area.bid_area_right - rect_width - spacing.marker_to_bars;
                    let sellside_x = area.ask_area_left + spacing.marker_to_bars;

                    draw_imbalance_markers(
                        frame,
                        &price_to_y,
                        footprint,
                        *price,
                        group.sell_qty,
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
            ClusterKind::BidAsk => Point::new(x_position, y_poc),
            ClusterKind::VolumeProfile | ClusterKind::DeltaProfile => {
                Point::new(content_right - 2.0, y_poc)
            }
        };
        let poc_align_x = match cluster_kind {
            ClusterKind::BidAsk => Alignment::Center,
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
}

fn draw_cluster_search_highlights(
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
    let border_w = (2.0 / scaling).clamp(1.0, 3.5);
    let is_thin = cell_width < (25.0 / scaling);

    for (price, group) in &footprint.trades {
        let total_vol = group.buy_qty + group.sell_qty;
        let delta = group.buy_qty - group.sell_qty;

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

        // Volumetric scaling factor: markers scale dynamically with anomaly magnitude
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
        let scale_factor = vol_ratio.sqrt().clamp(1.0, 3.0);

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
                            width: (1.0 / scaling).max(0.5),
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.90),
                    ),
                );
            }
            HighlightStyle::Circle => {
                let base_r = (cell_height * 0.85).clamp(8.0 / scaling, 18.0 / scaling);
                let radius = (base_r * scale_factor).clamp(7.0 / scaling, 36.0 / scaling);
                let circle_path = Path::circle(Point::new(x_position, y), radius);
                frame.fill(&circle_path, base_color.scale_alpha(0.35));
                frame.stroke(
                    &circle_path,
                    Stroke::with_color(
                        Stroke {
                            width: border_w,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Ring => {
                let base_r = (cell_height * 0.85).clamp(8.0 / scaling, 18.0 / scaling);
                let radius = (base_r * scale_factor).clamp(7.0 / scaling, 36.0 / scaling);
                let ring_path = Path::circle(Point::new(x_position, y), radius);
                frame.stroke(
                    &ring_path,
                    Stroke::with_color(
                        Stroke {
                            width: border_w * 1.2,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Concentric => {
                let base_r = (cell_height * 0.80).clamp(7.5 / scaling, 17.0 / scaling);
                let inner_radius = (base_r * scale_factor).clamp(6.5 / scaling, 30.0 / scaling);
                let inner_path = Path::circle(Point::new(x_position, y), inner_radius);
                frame.fill(&inner_path, base_color.scale_alpha(0.35));
                frame.stroke(
                    &inner_path,
                    Stroke::with_color(
                        Stroke {
                            width: border_w,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );

                let outer_radius = inner_radius * 1.45;
                let outer_path = Path::circle(Point::new(x_position, y), outer_radius);
                frame.stroke(
                    &outer_path,
                    Stroke::with_color(
                        Stroke {
                            width: border_w * 1.1,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.85),
                    ),
                );
            }
            HighlightStyle::Triangle => {
                let base_size = (cell_height * 0.85).clamp(8.0 / scaling, 18.0 / scaling);
                let half_size = (base_size * scale_factor).clamp(7.0 / scaling, 34.0 / scaling);
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
                            width: border_w,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
            HighlightStyle::Square => {
                let base_size = (cell_height * 0.75).clamp(7.0 / scaling, 16.0 / scaling);
                let half_size = (base_size * scale_factor).clamp(6.0 / scaling, 30.0 / scaling);
                let rect = Path::rectangle(
                    Point::new(x_position - half_size, y - half_size),
                    Size::new(half_size * 2.0, half_size * 2.0),
                );
                frame.fill(&rect, base_color.scale_alpha(0.35));
                frame.stroke(
                    &rect,
                    Stroke::with_color(
                        Stroke {
                            width: border_w,
                            ..Default::default()
                        },
                        base_color.scale_alpha(0.95),
                    ),
                );
            }
        }
    }
}

fn draw_imbalance_markers(
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
        let diagonal_buy_qty = group.buy_qty;

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
    fn from_view(candle_width: f32, scaling: f32) -> Self {
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
struct ContentGaps {
    /// Space between imb. markers candle body
    marker_to_candle: f32,
    /// Space between candle body and clusters
    candle_to_cluster: f32,
    /// Inner space reserved between imb. markers and clusters (used for BidAsk)
    marker_to_bars: f32,
}

fn draw_cluster_text(
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

fn draw_crosshair_tooltip(
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

struct ProfileArea {
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

struct BidAskArea {
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
enum LodLevel {
    /// Full numbers: Bid×Ask / Delta text, POC label
    Full,
    /// Heat blocks only — no text, color-coded by volume/delta
    Heatmap,
    /// Plain thin candles — no cluster content at all, classic bottom volume
    Candle,
}

#[inline]
fn lod_level(cell_width_unscaled: f32, cell_height_unscaled: f32, min_w: f32) -> LodLevel {
    if cell_width_unscaled < 35.0 {
        LodLevel::Candle
    } else if cell_width_unscaled >= min_w && cell_height_unscaled >= 10.0 {
        LodLevel::Full
    } else {
        LodLevel::Heatmap
    }
}

fn bracket_color(
    bracket: char,
    scheme: data::chart::kline::TpoColorScheme,
    palette: &Extended,
) -> iced::Color {
    let idx = if bracket.is_ascii_uppercase() {
        (bracket as u8 - b'A') as usize
    } else if bracket.is_ascii_lowercase() {
        (bracket as u8 - b'a' + 26) as usize
    } else {
        0
    };

    match scheme {
        data::chart::kline::TpoColorScheme::Classic => {
            const PALETTE: [iced::Color; 8] = [
                iced::Color::from_rgb(0.20, 0.65, 0.95), // Sky Blue (0h-2h)
                iced::Color::from_rgb(0.12, 0.76, 0.80), // Teal/Cyan (2h-4h)
                iced::Color::from_rgb(0.22, 0.76, 0.54), // Emerald Mint (4h-6h)
                iced::Color::from_rgb(0.98, 0.74, 0.18), // Golden Amber (6h-8h)
                iced::Color::from_rgb(0.98, 0.48, 0.22), // Sunset Orange (8h-10h)
                iced::Color::from_rgb(0.92, 0.32, 0.45), // Coral Rose (10h-12h)
                iced::Color::from_rgb(0.68, 0.36, 0.82), // Violet Purple (12h-14h)
                iced::Color::from_rgb(0.42, 0.45, 0.86), // Royal Indigo (14h-16h)
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Theme => {
            let colors = [
                palette.primary.base.color,
                palette.success.base.color,
                palette.warning.base.color,
                palette.danger.base.color,
                palette.primary.strong.color,
                palette.success.weak.color,
                palette.warning.weak.color,
                palette.danger.weak.color,
            ];
            colors[(idx / 4) % colors.len()]
        }
        data::chart::kline::TpoColorScheme::Cyan => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.12, 0.85, 0.95),
                iced::Color::from_rgb(0.08, 0.75, 0.88),
                iced::Color::from_rgb(0.20, 0.65, 0.90),
                iced::Color::from_rgb(0.00, 0.90, 0.80),
                iced::Color::from_rgb(0.30, 0.70, 0.98),
                iced::Color::from_rgb(0.15, 0.60, 0.85),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Emerald => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.20, 0.85, 0.55),
                iced::Color::from_rgb(0.15, 0.75, 0.45),
                iced::Color::from_rgb(0.10, 0.90, 0.65),
                iced::Color::from_rgb(0.30, 0.80, 0.40),
                iced::Color::from_rgb(0.25, 0.70, 0.50),
                iced::Color::from_rgb(0.05, 0.65, 0.35),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Amber => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.98, 0.78, 0.18),
                iced::Color::from_rgb(0.95, 0.65, 0.15),
                iced::Color::from_rgb(0.98, 0.55, 0.20),
                iced::Color::from_rgb(0.90, 0.70, 0.25),
                iced::Color::from_rgb(0.85, 0.60, 0.10),
                iced::Color::from_rgb(0.99, 0.82, 0.30),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Purple => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.72, 0.40, 0.92),
                iced::Color::from_rgb(0.60, 0.30, 0.85),
                iced::Color::from_rgb(0.80, 0.45, 0.98),
                iced::Color::from_rgb(0.55, 0.35, 0.80),
                iced::Color::from_rgb(0.85, 0.50, 0.90),
                iced::Color::from_rgb(0.50, 0.25, 0.75),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Red => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.95, 0.30, 0.40),
                iced::Color::from_rgb(0.85, 0.25, 0.35),
                iced::Color::from_rgb(0.98, 0.40, 0.48),
                iced::Color::from_rgb(0.80, 0.20, 0.30),
                iced::Color::from_rgb(0.90, 0.35, 0.42),
                iced::Color::from_rgb(0.75, 0.15, 0.25),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Blue => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.25, 0.60, 0.98),
                iced::Color::from_rgb(0.20, 0.50, 0.90),
                iced::Color::from_rgb(0.35, 0.70, 0.99),
                iced::Color::from_rgb(0.15, 0.45, 0.85),
                iced::Color::from_rgb(0.30, 0.65, 0.95),
                iced::Color::from_rgb(0.10, 0.40, 0.80),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Monochrome => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.85, 0.85, 0.85),
                iced::Color::from_rgb(0.72, 0.72, 0.72),
                iced::Color::from_rgb(0.92, 0.92, 0.92),
                iced::Color::from_rgb(0.65, 0.65, 0.65),
                iced::Color::from_rgb(0.78, 0.78, 0.78),
                iced::Color::from_rgb(0.58, 0.58, 0.58),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Custom([r, g, b]) => {
            let base_r = r as f32 / 255.0;
            let base_g = g as f32 / 255.0;
            let base_b = b as f32 / 255.0;
            let factors = [1.0, 0.85, 1.15, 0.75, 1.05, 0.90];
            let f = factors[(idx / 4) % factors.len()];
            iced::Color::from_rgb(
                (base_r * f).min(1.0),
                (base_g * f).min(1.0),
                (base_b * f).min(1.0),
            )
        }
    }
}

fn draw_context_menu(
    frame: &mut canvas::Frame,
    menu: &TpoContextMenu,
    cursor_pos: Option<Point>,
    palette: &Extended,
) {
    // 1. Soft layered drop shadow
    frame.fill_rectangle(
        Point::new(menu.rect.x - 2.0, menu.rect.y + 2.0),
        Size::new(menu.rect.width + 4.0, menu.rect.height + 4.0),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.12),
    );
    frame.fill_rectangle(
        Point::new(menu.rect.x - 1.0, menu.rect.y + 1.0),
        Size::new(menu.rect.width + 2.0, menu.rect.height + 2.0),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.20),
    );
    frame.fill_rectangle(
        Point::new(menu.rect.x + 1.0, menu.rect.y + 2.0),
        menu.rect.size(),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
    );

    // 2. Menu body & border in theme palette
    let bg_color = palette.background.base.color;
    let border_color = palette.background.weak.color;

    frame.fill_rectangle(menu.rect.position(), menu.rect.size(), bg_color);
    frame.stroke(
        &Path::rectangle(menu.rect.position(), menu.rect.size()),
        Stroke::with_color(
            Stroke {
                width: 1.0,
                ..Default::default()
            },
            border_color,
        ),
    );

    // 3. Typography & item states
    let text_active = palette.background.base.text;
    let text_disabled = palette.background.base.text.scale_alpha(0.35);
    let hover_bg = if palette.is_dark {
        palette.background.weak.color
    } else {
        palette.background.strong.color
    };
    let divider_color = palette.background.weak.color.scale_alpha(0.45);

    for (i, item) in menu.items.iter().enumerate() {
        let is_enabled = item.action.is_some();
        let is_hovered = cursor_pos.is_some_and(|cp| item.rect.contains(cp));

        if is_enabled && is_hovered {
            let hover_rect = Rectangle::new(
                Point::new(item.rect.x + 3.0, item.rect.y + 1.0),
                Size::new(item.rect.width - 6.0, item.rect.height - 2.0),
            );
            frame.fill_rectangle(hover_rect.position(), hover_rect.size(), hover_bg);
        }

        let color = if is_enabled {
            text_active
        } else {
            text_disabled
        };

        frame.fill_text(canvas::Text {
            content: item.label.clone(),
            position: Point::new(item.rect.x + 14.0, item.rect.y + item.rect.height / 2.0),
            size: iced::Pixels(12.0),
            color,
            align_y: Alignment::Center.into(),
            align_x: Alignment::Start.into(),
            ..canvas::Text::default()
        });

        if i + 1 < menu.items.len() {
            let line_y = item.rect.y + item.rect.height;
            frame.fill_rectangle(
                Point::new(menu.rect.x + 8.0, line_y),
                Size::new(menu.rect.width - 16.0, 1.0),
                divider_color,
            );
        }
    }
}

fn resolve_tpo_element_color(
    elem_color: data::chart::kline::TpoElementColor,
    palette: &Extended,
    default_color: iced::Color,
) -> iced::Color {
    match elem_color {
        data::chart::kline::TpoElementColor::Auto => default_color,
        data::chart::kline::TpoElementColor::Theme => palette.warning.base.color,
        data::chart::kline::TpoElementColor::Custom([r, g, b]) => iced::Color::from_rgb8(r, g, b),
        other => {
            if let Some([r, g, b]) = other.to_rgb() {
                iced::Color::from_rgb8(r, g, b)
            } else {
                default_color
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_tpo_profiles(
    data_source: &PlotData<KlineDataPoint>,
    tpo_cache: &RefCell<TpoCache>,
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    region: &Rectangle,
    cell_width: f32,
    cell_height: f32,
    scaling: f32,
    exchange_tick_size: f32,
    tpo_tick_size: f64,
    earliest: u64,
    latest: u64,
    visible_min_price: f64,
    visible_max_price: f64,
    show_letters: bool,
    show_ib: bool,
    show_va: bool,
    show_poc: bool,
    show_single_prints: bool,
    effective_period: data::chart::tpo::SessionPeriod,
    clusters: &[data::chart::tpo::SessionCluster],
    split_sessions: &[i64],
    color_scheme: data::chart::kline::TpoColorScheme,
    ib_color: data::chart::kline::TpoElementColor,
    poc_color: data::chart::kline::TpoElementColor,
    single_prints_color: data::chart::kline::TpoElementColor,
    palette: &Extended,
) {
    let ex_tick = if exchange_tick_size <= 0.0 {
        1.0
    } else {
        exchange_tick_size
    };
    let tpo_tick = if tpo_tick_size <= 0.0 || !tpo_tick_size.is_finite() {
        ex_tick as f64
    } else {
        tpo_tick_size
    };

    // Exact height of one TPO price bin in chart canvas coordinate units
    let tpo_row_height = (tpo_tick as f32 / ex_tick) * cell_height;
    let scaled_row_height = tpo_row_height * scaling;

    let mut all_klines: Vec<&Kline> = Vec::new();
    match data_source {
        PlotData::TimeBased(ts) => {
            for dp in ts.datapoints.values() {
                all_klines.push(&dp.kline);
            }
        }
        PlotData::TickBased(ta) => {
            for dp in &ta.datapoints {
                all_klines.push(&dp.kline);
            }
        }
    }

    if all_klines.is_empty() {
        return;
    }

    let sessions = data::chart::tpo::group_candles_by_period(&all_klines, effective_period);
    if sessions.is_empty() {
        return;
    }

    let letter_col_width = (cell_width * 0.75).clamp(1.0, 16.0);
    let min_tick = ((visible_min_price / tpo_tick).floor() as i64).saturating_sub(2);
    let max_tick = ((visible_max_price / tpo_tick).ceil() as i64).saturating_add(2);

    let mut cache = tpo_cache.borrow_mut();
    if (cache.tick_size - tpo_tick).abs() > 1e-9 || cache.period != Some(effective_period) {
        cache.tick_size = tpo_tick;
        cache.period = Some(effective_period);
        cache.sessions.clear();
    }
    cache.session_ranges.clear();

    let format_session_date = |start: i64| -> String {
        chrono::DateTime::from_timestamp_millis(start)
            .map(|d| {
                use chrono::Timelike;
                let base = d.format("%Y-%m-%d").to_string();
                match effective_period {
                    data::chart::tpo::SessionPeriod::TradingSessions => {
                        let hour = d.hour();
                        let name = if hour < 8 {
                            "Asia"
                        } else if hour < 16 {
                            "London"
                        } else {
                            "NY"
                        };
                        format!("{base} {name}")
                    }
                    data::chart::tpo::SessionPeriod::FourHours => {
                        format!("{base} {:02}:00", d.hour())
                    }
                    _ => base,
                }
            })
            .unwrap_or_else(|| start.to_string())
    };

    // 2. Build or fetch raw profiles from cache
    let mut raw_profiles = Vec::with_capacity(sessions.len());
    for (session_start, session_end, session_candles) in sessions {
        if session_candles.is_empty() {
            continue;
        }

        let candle_count = session_candles.len();
        let last_c = session_candles[candle_count - 1];
        let last_high = last_c.high.to_f32() as f64;
        let last_low = last_c.low.to_f32() as f64;
        let last_time = last_c.time as i64;

        let profile: Arc<data::chart::tpo::TpoProfile> =
            if let Some(entry) = cache.sessions.get(&session_start) {
                if entry.candle_count == candle_count
                    && (entry.last_candle_high - last_high).abs() < 1e-9
                    && (entry.last_candle_low - last_low).abs() < 1e-9
                    && entry.last_candle_time == last_time
                {
                    Arc::clone(&entry.profile)
                } else {
                    let date_str = format_session_date(session_start);
                    let p = Arc::new(data::chart::tpo::build_tpo_profile(
                        &session_candles,
                        session_start,
                        session_end,
                        &date_str,
                        tpo_tick,
                    ));
                    cache.sessions.insert(
                        session_start,
                        TpoCacheEntry {
                            candle_count,
                            last_candle_high: last_high,
                            last_candle_low: last_low,
                            last_candle_time: last_time,
                            profile: Arc::clone(&p),
                        },
                    );
                    p
                }
            } else {
                let date_str = format_session_date(session_start);
                let p = Arc::new(data::chart::tpo::build_tpo_profile(
                    &session_candles,
                    session_start,
                    session_end,
                    &date_str,
                    tpo_tick,
                ));
                cache.sessions.insert(
                    session_start,
                    TpoCacheEntry {
                        candle_count,
                        last_candle_high: last_high,
                        last_candle_low: last_low,
                        last_candle_time: last_time,
                        profile: Arc::clone(&p),
                    },
                );
                p
            };

        raw_profiles.push((*profile).clone());
    }

    // 3. Resolve clusters
    let effective_profiles = if clusters.is_empty() {
        raw_profiles
    } else {
        data::chart::tpo::apply_session_clusters(&raw_profiles, clusters)
    };

    let total_effective = effective_profiles.len();
    for (idx, profile) in effective_profiles.iter().enumerate() {
        let session_start = profile.session_start;
        let session_end = profile.session_end;

        if (session_end as u64) < earliest || (session_start as u64) > latest {
            continue;
        }

        if profile.matrix.is_empty() {
            continue;
        }

        let session_start_x = interval_to_x(session_start as u64);
        let session_end_x = interval_to_x(session_end as u64);
        let session_px_span = (session_end_x - session_start_x).abs().max(cell_width);
        let max_tpo_len = profile.poc.as_ref().map_or(1, |p| p.count.max(1));
        let max_session_draw_width = (session_px_span * 0.95).max(letter_col_width * 2.0);
        let profile_width = ((max_tpo_len as f32) * letter_col_width).min(max_session_draw_width);

        let eff_col_w = (profile_width / (max_tpo_len as f32)).min(letter_col_width);
        let col_gap = (0.8 / scaling).min(eff_col_w * 0.25);
        let draw_w = (eff_col_w - col_gap).max(1.0 / scaling);

        let row_gap = (0.8 / scaling).min(tpo_row_height * 0.25);
        let draw_h = (tpo_row_height - row_gap).max(0.5 / scaling);

        let scaled_eff_col = eff_col_w * scaling;
        let render_text = show_letters && scaled_row_height >= 8.5 && scaled_eff_col >= 7.0;
        let font_size =
            ((scaled_row_height.min(18.0) - 2.0).min(scaled_eff_col - 2.0) / scaling).max(6.0);

        let is_clustered = clusters.iter().any(|c| c.contains(session_start))
            || profile.session_date.ends_with('+');
        let prev_start = if idx > 0 {
            Some(effective_profiles[idx - 1].session_start)
        } else {
            None
        };
        let next_start = if idx + 1 < total_effective {
            Some(effective_profiles[idx + 1].session_start)
        } else {
            None
        };
        let is_session_split = split_sessions.contains(&session_start);

        cache.session_ranges.push(TpoSessionRange {
            session_start,
            session_end,
            prev_start,
            next_start,
            is_clustered,
            is_split_brackets: is_session_split,
        });
        let (va_fill_color, va_line_color, default_poc_color) = match color_scheme {
            data::chart::kline::TpoColorScheme::Theme => (
                palette.primary.base.color.scale_alpha(0.10),
                palette.primary.base.color,
                palette.warning.base.color,
            ),
            data::chart::kline::TpoColorScheme::Custom([r, g, b]) => {
                let base = iced::Color::from_rgb8(r, g, b);
                (
                    base.scale_alpha(0.10),
                    base,
                    iced::Color::from_rgb(0.98, 0.74, 0.18),
                )
            }
            data::chart::kline::TpoColorScheme::Cyan => (
                iced::Color::from_rgba(0.08, 0.80, 0.90, 0.08),
                iced::Color::from_rgb(0.08, 0.80, 0.90),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Emerald => (
                iced::Color::from_rgba(0.15, 0.80, 0.50, 0.08),
                iced::Color::from_rgb(0.15, 0.80, 0.50),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Amber => (
                iced::Color::from_rgba(0.98, 0.70, 0.18, 0.08),
                iced::Color::from_rgb(0.98, 0.70, 0.18),
                iced::Color::from_rgb(0.98, 0.48, 0.22),
            ),
            data::chart::kline::TpoColorScheme::Purple => (
                iced::Color::from_rgba(0.68, 0.36, 0.85, 0.08),
                iced::Color::from_rgb(0.68, 0.36, 0.85),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Red => (
                iced::Color::from_rgba(0.92, 0.30, 0.38, 0.08),
                iced::Color::from_rgb(0.92, 0.30, 0.38),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Blue => (
                iced::Color::from_rgba(0.20, 0.60, 0.95, 0.08),
                iced::Color::from_rgb(0.20, 0.60, 0.95),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Monochrome => (
                iced::Color::from_rgba(0.80, 0.80, 0.80, 0.08),
                iced::Color::from_rgb(0.80, 0.80, 0.80),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Classic => (
                iced::Color::from_rgba(0.20, 0.65, 0.95, 0.08),
                iced::Color::from_rgb(0.20, 0.65, 0.95),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
        };
        let poc_color = resolve_tpo_element_color(poc_color, palette, default_poc_color);
        let ib_color =
            resolve_tpo_element_color(ib_color, palette, iced::Color::from_rgb(0.98, 0.74, 0.18));

        // 1. Shaded Value Area (VAH to VAL)
        if show_va && let Some(ref va) = profile.value_area {
            let vah_y = price_to_y(Price::from_f32(va.vah as f32));
            let val_y = price_to_y(Price::from_f32(va.val as f32));
            let top_y = vah_y.min(val_y);
            let h = (vah_y - val_y).abs() + tpo_row_height;
            let va_width = if is_session_split {
                session_px_span
            } else {
                (profile_width + 16.0).min(max_session_draw_width)
            };

            frame.fill_rectangle(
                Point::new(session_start_x, top_y - tpo_row_height / 2.0),
                Size::new(va_width, h),
                va_fill_color,
            );

            frame.fill_rectangle(
                Point::new(session_start_x, vah_y - 0.5),
                Size::new(va_width, 1.5),
                va_line_color,
            );
            frame.fill_rectangle(
                Point::new(session_start_x, val_y - 0.5),
                Size::new(va_width, 1.5),
                va_line_color,
            );
        }

        // 2. Single Prints Highlights with Vertical Culling
        if show_single_prints {
            for sp in &profile.single_prints {
                if sp.start_price < visible_min_price || sp.start_price > visible_max_price {
                    continue;
                }
                let y = price_to_y(Price::from_f32(sp.start_price as f32));
                let color = match single_prints_color {
                    data::chart::kline::TpoElementColor::Auto => {
                        if sp.is_tail {
                            iced::Color::from_rgba(0.92, 0.32, 0.45, 0.35)
                        } else {
                            iced::Color::from_rgba(0.68, 0.36, 0.82, 0.25)
                        }
                    }
                    custom_color => {
                        let base = resolve_tpo_element_color(
                            custom_color,
                            palette,
                            iced::Color::from_rgb(0.68, 0.36, 0.82),
                        );
                        if sp.is_tail {
                            base.scale_alpha(0.40)
                        } else {
                            base.scale_alpha(0.28)
                        }
                    }
                };
                frame.fill_rectangle(
                    Point::new(session_start_x, y - tpo_row_height / 2.0),
                    Size::new(eff_col_w + 4.0, tpo_row_height),
                    color,
                );
            }
        }

        // 3. Matrix Blocks / Letters (Split Brackets or Collapsed Profile)
        if is_session_split {
            for (&t, row) in profile.matrix.range(min_tick..=max_tick) {
                if row.is_empty() {
                    continue;
                }

                let bin_price = t as f64 * tpo_tick;
                let y = price_to_y(Price::from_f32(bin_price as f32));
                let cell_y = y - tpo_row_height / 2.0 + row_gap / 2.0;

                let row_indices = profile.bracket_indices.get(&t);

                for (idx_in_row, &bracket) in row.iter().enumerate() {
                    let b_idx = if let Some(indices) = row_indices
                        && let Some(&actual_idx) = indices.get(idx_in_row)
                    {
                        actual_idx as i64
                    } else if bracket.is_ascii_uppercase() {
                        (bracket as u8 - b'A') as i64
                    } else if bracket.is_ascii_lowercase() {
                        26 + (bracket as u8 - b'a') as i64
                    } else {
                        0
                    };

                    let bracket_time = session_start + b_idx * 1_800_000;
                    let col_x = interval_to_x(bracket_time as u64);
                    let next_col_x = interval_to_x((bracket_time + 1_800_000) as u64);
                    let col_span = (next_col_x - col_x).abs().max(eff_col_w);
                    let split_draw_w = (col_span - col_gap).clamp(1.0 / scaling, 24.0);

                    if col_x + split_draw_w < region.x || col_x > region.x + region.width {
                        continue;
                    }

                    let color = bracket_color(bracket, color_scheme, palette);
                    if render_text {
                        frame.fill_rectangle(
                            Point::new(col_x, cell_y),
                            Size::new(split_draw_w, draw_h),
                            color.scale_alpha(0.22),
                        );
                        draw_cluster_text(
                            frame,
                            &bracket.to_string(),
                            Point::new(col_x + split_draw_w / 2.0, y),
                            font_size,
                            color,
                            Alignment::Center,
                            Alignment::Center,
                        );
                    } else {
                        frame.fill_rectangle(
                            Point::new(col_x, cell_y),
                            Size::new(split_draw_w, draw_h),
                            color.scale_alpha(0.88),
                        );
                    }
                }
            }
        } else {
            for (&t, row) in profile.matrix.range(min_tick..=max_tick) {
                if row.is_empty() {
                    continue;
                }

                let bin_price = t as f64 * tpo_tick;
                let y = price_to_y(Price::from_f32(bin_price as f32));

                let min_col = if region.x > session_start_x {
                    ((region.x - session_start_x) / eff_col_w).floor().max(0.0) as usize
                } else {
                    0
                };
                let max_col = (((region.x + region.width - session_start_x) / eff_col_w)
                    .ceil()
                    .max(0.0) as usize)
                    .min(row.len());

                if min_col >= max_col {
                    continue;
                }

                let cell_y = y - tpo_row_height / 2.0 + row_gap / 2.0;

                for (col_offset, &bracket) in row[min_col..max_col].iter().enumerate() {
                    let actual_col = min_col + col_offset;
                    let x = session_start_x + (actual_col as f32) * eff_col_w;
                    let color = bracket_color(bracket, color_scheme, palette);

                    if render_text {
                        frame.fill_rectangle(
                            Point::new(x, cell_y),
                            Size::new(draw_w, draw_h),
                            color.scale_alpha(0.22),
                        );
                        draw_cluster_text(
                            frame,
                            &bracket.to_string(),
                            Point::new(x + draw_w / 2.0, y),
                            font_size,
                            color,
                            Alignment::Center,
                            Alignment::Center,
                        );
                    } else {
                        frame.fill_rectangle(
                            Point::new(x, cell_y),
                            Size::new(draw_w, draw_h),
                            color.scale_alpha(0.88),
                        );
                    }
                }
            }
        }

        // 4. Point of Control (POC) Line
        if show_poc
            && let Some(ref poc) = profile.poc
            && poc.price >= visible_min_price
            && poc.price <= visible_max_price
        {
            let poc_y = price_to_y(Price::from_f32(poc.price as f32));
            let poc_line_width = if is_session_split {
                session_px_span
            } else {
                (profile_width + 24.0).min(max_session_draw_width)
            };
            frame.fill_rectangle(
                Point::new(session_start_x, poc_y - 1.0),
                Size::new(poc_line_width, 2.0),
                poc_color,
            );
        }

        // 5. Initial Balance (IB) and Extensions
        if show_ib
            && let Some(ref ib) = profile.ib
            && ib.high >= visible_min_price
            && ib.low <= visible_max_price
        {
            let ib_h_y = price_to_y(Price::from_f32(ib.high as f32));
            let ib_l_y = price_to_y(Price::from_f32(ib.low as f32));
            let top_y = ib_h_y.min(ib_l_y);
            let h = (ib_h_y - ib_l_y).abs() + tpo_row_height;

            let ib_x_offset = (cell_width * 0.5).clamp(4.0, 10.0);
            let bar_w = (ib_x_offset * 0.6).clamp(2.0, 5.0);
            frame.fill_rectangle(
                Point::new(session_start_x - ib_x_offset, top_y - tpo_row_height / 2.0),
                Size::new(bar_w, h),
                ib_color,
            );

            let ext15_y = price_to_y(Price::from_f32(ib.extension_high_1_5 as f32));
            let ext_w = (profile_width * 0.6).min(max_session_draw_width);
            frame.fill_rectangle(
                Point::new(session_start_x, ext15_y - 0.5),
                Size::new(ext_w, 1.0),
                ib_color.scale_alpha(0.60),
            );

            let ext20_y = price_to_y(Price::from_f32(ib.extension_high_2_0 as f32));
            frame.fill_rectangle(
                Point::new(session_start_x, ext20_y - 0.5),
                Size::new(ext_w, 1.0),
                ib_color.scale_alpha(0.60),
            );
        }
    }
}

fn draw_liquidation_heatmap(
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

fn draw_vwap_overlay(
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
            for (time, dp) in &ts.datapoints {
                if *time >= earliest && *time <= latest {
                    let tp = (dp.kline.high.to_f32()
                        + dp.kline.low.to_f32()
                        + dp.kline.close.to_f32()) as f64
                        / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;
                    candles.push((*time as i64, tp, vol));
                }
            }
        }
        PlotData::TickBased(ta) => {
            for dp in &ta.datapoints {
                let time = dp.kline.time;
                if time >= earliest && time <= latest {
                    let tp = (dp.kline.high.to_f32()
                        + dp.kline.low.to_f32()
                        + dp.kline.close.to_f32()) as f64
                        / 3.0;
                    let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;
                    candles.push((dp.kline.time as i64, tp, vol));
                }
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

fn draw_rolling_vwap_overlay(
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

    let stroke_7d = Stroke::default()
        .with_color(iced::Color::from_rgb(0.0, 0.9, 1.0)) // Cyan (7d)
        .with_width(1.5);
    let stroke_30d = Stroke::default()
        .with_color(iced::Color::from_rgb(1.0, 0.84, 0.0)) // Gold (30d)
        .with_width(1.5);
    let stroke_90d = Stroke::default()
        .with_color(iced::Color::from_rgb(1.0, 0.45, 0.1)) // Orange (90d)
        .with_width(1.5);
    let stroke_365d = Stroke::default()
        .with_color(iced::Color::from_rgb(0.85, 0.15, 0.95)) // Magenta (365d)
        .with_width(1.5);

    if let Some(indicator) = rolling_indicator {
        for (&time_u64, pt) in &indicator.data {
            if time_u64 > latest {
                break;
            }
            let x = interval_to_x(time_u64);

            if config.rolling_vwap_show_7d
                && let Some(d7) = pt.d7
            {
                let y = price_to_y(Price::from_f32(d7.vwap as f32));
                if time_u64 >= earliest
                    && let Some((px, py)) = prev_7d
                {
                    frame.stroke(&Path::line(Point::new(px, py), Point::new(x, y)), stroke_7d);
                }
                prev_7d = Some((x, y));
            }

            if config.rolling_vwap_show_30d
                && let Some(d30) = pt.d30
            {
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
            }

            if config.rolling_vwap_show_90d
                && let Some(d90) = pt.d90
            {
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
            }

            if config.rolling_vwap_show_365d
                && let Some(d365) = pt.d365
            {
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
            }
        }
    } else {
        let mut candles: Vec<(i64, f64, f64)> = Vec::new();
        const MAX_WINDOW_MS: i64 = 365 * 86_400_000;
        let fetch_earliest = (earliest as i64).saturating_sub(MAX_WINDOW_MS) as u64;

        match data_source {
            PlotData::TimeBased(ts) => {
                for (time, dp) in &ts.datapoints {
                    if *time >= fetch_earliest && *time <= latest {
                        let tp = (dp.kline.high.to_f32()
                            + dp.kline.low.to_f32()
                            + dp.kline.close.to_f32()) as f64
                            / 3.0;
                        let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;
                        candles.push((*time as i64, tp, vol));
                    }
                }
            }
            PlotData::TickBased(ta) => {
                for dp in &ta.datapoints {
                    let time = dp.kline.time;
                    if time >= fetch_earliest && time <= latest {
                        let tp = (dp.kline.high.to_f32()
                            + dp.kline.low.to_f32()
                            + dp.kline.close.to_f32()) as f64
                            / 3.0;
                        let vol = (dp.kline.volume.0 + dp.kline.volume.1) as f64;
                        candles.push((dp.kline.time as i64, tp, vol));
                    }
                }
            }
        }

        if candles.len() < 2 {
            return;
        }

        let points = data::chart::vwap::calculate_multi_rolling_vwap_series(&candles);
        for (i, pt) in points.iter().enumerate() {
            let time_u64 = candles[i].0 as u64;
            let x = interval_to_x(time_u64);

            if config.rolling_vwap_show_7d
                && let Some(d7) = pt.d7
            {
                let y = price_to_y(Price::from_f32(d7.vwap as f32));
                if time_u64 >= earliest
                    && let Some((px, py)) = prev_7d
                {
                    frame.stroke(&Path::line(Point::new(px, py), Point::new(x, y)), stroke_7d);
                }
                prev_7d = Some((x, y));
            }

            if config.rolling_vwap_show_30d
                && let Some(d30) = pt.d30
            {
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
            }

            if config.rolling_vwap_show_90d
                && let Some(d90) = pt.d90
            {
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
            }

            if config.rolling_vwap_show_365d
                && let Some(d365) = pt.d365
            {
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
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    Handle(usize),
    Body,
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f32 {
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

fn hit_test_drawing(
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
            ..
        } => {
            let x = chart.interval_to_x(entry.0);
            let w = 140.0 / chart.scaling;
            vec![
                Point::new(x, chart.price_to_y(Price::from_f32(entry.1))),
                Point::new(
                    x + w / 2.0,
                    chart.price_to_y(Price::from_f32(*target_price)),
                ),
                Point::new(x + w / 2.0, chart.price_to_y(Price::from_f32(*stop_price))),
            ]
        }
    };

    for (idx, handle) in handles.iter().enumerate() {
        if (frame_pos.x - handle.x).hypot(frame_pos.y - handle.y) <= threshold {
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
            ..
        } => {
            let x = chart.interval_to_x(entry.0);
            let w = 140.0 / chart.scaling;
            let y_entry = chart.price_to_y(Price::from_f32(entry.1));
            let y_tgt = chart.price_to_y(Price::from_f32(*target_price));
            let y_stp = chart.price_to_y(Price::from_f32(*stop_price));
            let min_y = y_tgt.min(y_stp).min(y_entry);
            let max_y = y_tgt.max(y_stp).max(y_entry);
            if frame_pos.x >= x - threshold
                && frame_pos.x <= x + w + threshold
                && frame_pos.y >= min_y - threshold
                && frame_pos.y <= max_y + threshold
            {
                return Some(HitTarget::Body);
            }
        }
    }

    None
}

fn draw_chart_drawings(
    frame: &mut canvas::Frame,
    drawings: &[Drawing],
    in_progress: &Option<Drawing>,
    drag_drawing: Option<&Drawing>,
    chart: &ViewState,
    region: &Rectangle,
    selected_id: Option<uuid::Uuid>,
) {
    let handle_radius = 4.0 / chart.scaling;
    let handle_stroke_w = 1.5 / chart.scaling;
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

    for drawing in drawings_iter.chain(in_progress.iter()) {
        let is_selected = drawing.is_selected || selected_id == Some(drawing.id);
        let color = iced::Color::from_rgba(
            drawing.color[0],
            drawing.color[1],
            drawing.color[2],
            drawing.color[3],
        );
        let stroke = Stroke {
            style: canvas::Style::Solid(color),
            width: drawing.width,
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
                frame.stroke(&Path::line(Point::new(x1, y1), Point::new(x2, y2)), stroke);
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

                let fill_color = iced::Color::from_rgba(
                    drawing.color[0],
                    drawing.color[1],
                    drawing.color[2],
                    (drawing.color[3] * 0.25).max(0.08),
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
                            width: drawing.width + 4.0 / chart.scaling,
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
                ..
            } => {
                let entry_x = chart.interval_to_x(entry.0);
                let entry_y = chart.price_to_y(Price::from_f32(entry.1));
                let target_y = chart.price_to_y(Price::from_f32(*target_price));
                let stop_y = chart.price_to_y(Price::from_f32(*stop_price));
                let box_w = 140.0 / chart.scaling;

                // Profit zone
                let p_min_y = entry_y.min(target_y);
                let p_h = (entry_y - target_y).abs();
                let profit_fill = iced::Color::from_rgba(0.12, 0.78, 0.42, 0.2);
                let profit_stroke = Stroke {
                    style: canvas::Style::Solid(iced::Color::from_rgba(0.12, 0.78, 0.42, 0.8)),
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
                let stop_fill = iced::Color::from_rgba(0.88, 0.24, 0.24, 0.2);
                let stop_stroke = Stroke {
                    style: canvas::Style::Solid(iced::Color::from_rgba(0.88, 0.24, 0.24, 0.8)),
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

                // Entry line
                frame.stroke(
                    &Path::line(
                        Point::new(entry_x, entry_y),
                        Point::new(entry_x + box_w, entry_y),
                    ),
                    Stroke {
                        style: canvas::Style::Solid(iced::Color::from_rgba(0.85, 0.9, 0.95, 0.9)),
                        width: 1.2,
                        ..Default::default()
                    },
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

                if is_selected {
                    draw_round_handle(frame, Point::new(entry_x, entry_y));
                    draw_round_handle(frame, Point::new(entry_x + box_w / 2.0, target_y));
                    draw_round_handle(frame, Point::new(entry_x + box_w / 2.0, stop_y));
                }
            }
        }
    }
}

fn draw_chart_alerts(
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

            // TradingView-style draggable badge on the right edge
            let price_str = format!("{:.*}", chart.decimals, price_val);
            let badge_w = (72.0f32 / chart.scaling)
                .max((price_str.len() as f32 * 8.0 + 16.0) / chart.scaling);
            let badge_h = 18.0 / chart.scaling;
            let badge_x = region.x + region.width - badge_w - 4.0 / chart.scaling;
            let badge_y = y - badge_h / 2.0;

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

            frame.fill_rectangle(
                Point::new(badge_x, badge_y),
                Size::new(badge_w, badge_h),
                if is_being_dragged {
                    iced::Color::from_rgba(0.1, 0.35, 0.55, 0.95)
                } else if alert.status == data::chart::alert::AlertStatus::Active {
                    iced::Color::from_rgba(0.18, 0.15, 0.05, 0.92)
                } else {
                    iced::Color::from_rgba(0.15, 0.15, 0.15, 0.92)
                },
            );
            // Stroke width in iced canvas is always in physical screen pixels, do not divide by scaling
            frame.stroke(
                &Path::rectangle(Point::new(badge_x, badge_y), Size::new(badge_w, badge_h)),
                Stroke {
                    style: canvas::Style::Solid(alert_color),
                    width: if is_being_dragged { 1.5 } else { 1.0 },
                    ..Default::default()
                },
            );

            frame.fill_text(canvas::Text {
                content: price_str,
                position: Point::new(badge_x + badge_w / 2.0, badge_y + badge_h / 2.0),
                color: palette.background.base.text,
                size: iced::Pixels(10.0 / chart.scaling),
                align_x: Alignment::Center.into(),
                align_y: Alignment::Center.into(),
                font: style::AZERET_MONO,
                ..canvas::Text::default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indicator_is_panel_classification() {
        assert!(!KlineIndicator::Tpo.is_panel());
        assert!(!KlineIndicator::LiquidationHeatmap.is_panel());
        assert!(KlineIndicator::Volume.is_panel());
        assert!(KlineIndicator::OpenInterest.is_panel());
        assert!(KlineIndicator::MarketPulse.is_panel());
        assert!(KlineIndicator::NetOi.is_panel());
        assert!(KlineIndicator::Vpin.is_panel());
        assert!(KlineIndicator::Vwap.is_panel());
        assert!(KlineIndicator::RollingVwap.is_panel());
        assert!(KlineIndicator::Cvd.is_panel());
        assert!(KlineIndicator::BidAskRatio.is_panel());
        assert!(KlineIndicator::PositionFlow.is_panel());
    }

    #[test]
    fn test_liquidation_heatmap_indicator_toggle() {
        let mut chart = make_test_chart();
        assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_none());
        assert!(!chart.liquidation_heatmap.config.enabled);

        // Toggle on
        chart.toggle_indicator(KlineIndicator::LiquidationHeatmap);
        assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_some());
        assert!(chart.liquidation_heatmap.config.enabled);

        // Toggle off
        chart.toggle_indicator(KlineIndicator::LiquidationHeatmap);
        assert!(chart.indicators[KlineIndicator::LiquidationHeatmap].is_none());
        assert!(!chart.liquidation_heatmap.config.enabled);
    }

    #[test]
    fn test_insert_raw_trades_deduplication() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let view_cfg = ViewConfig::default();
        let mut chart = KlineChart::new(
            view_cfg,
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Candles,
            None,
        );

        let trade1 = Trade {
            time: 1000,
            price: Price::from_f32(100.0),
            qty: 1.0,
            is_sell: false,
        };
        let trade2 = Trade {
            time: 2000,
            price: Price::from_f32(101.0),
            qty: 2.0,
            is_sell: true,
        };

        // First insertion
        chart.insert_raw_trades(vec![trade1, trade2]);
        assert_eq!(chart.raw_trades().len(), 2);

        // Duplicate insertion (identical trades)
        chart.insert_raw_trades(vec![trade1, trade2]);
        assert_eq!(chart.raw_trades().len(), 2);

        // Insertion with 1 new trade and 1 duplicate
        let trade3 = Trade {
            time: 3000,
            price: Price::from_f32(102.0),
            qty: 0.5,
            is_sell: false,
        };
        chart.insert_raw_trades(vec![trade2, trade3]);
        assert_eq!(chart.raw_trades().len(), 3);
    }

    #[test]
    fn test_lod_level_transitions() {
        let min_w = 80.0;

        // < 35.0 width -> Candle LOD
        assert!(lod_level(34.9, 20.0, min_w) == LodLevel::Candle);
        assert!(lod_level(10.0, 5.0, min_w) == LodLevel::Candle);

        // >= 35.0 and < min_w (or height < 10.0) -> Heatmap LOD
        assert!(lod_level(50.0, 20.0, min_w) == LodLevel::Heatmap);
        assert!(lod_level(90.0, 9.0, min_w) == LodLevel::Heatmap);

        // >= min_w and height >= 10.0 -> Full LOD
        assert!(lod_level(80.0, 10.0, min_w) == LodLevel::Full);
        assert!(lod_level(150.0, 25.0, min_w) == LodLevel::Full);
    }

    #[test]
    fn test_cluster_search_detection_logic() {
        use data::chart::kline::ClusterSearchSide;

        let mut footprint = KlineTrades::new();
        let step = PriceStep::from_f32(1.0);

        // Add trades
        footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 1000,
                price: Price::from_f32(100.0),
                qty: 600.0,
                is_sell: false,
            },
            step,
        );
        footprint.add_trade_to_nearest_bin(
            &Trade {
                time: 1001,
                price: Price::from_f32(100.0),
                qty: 100.0,
                is_sell: true,
            },
            step,
        );

        // At price 100: buy_qty = 600, sell_qty = 100, total = 700, delta = +500
        let group = footprint.trades.get(&Price::from_f32(100.0)).unwrap();
        let total_vol = group.buy_qty + group.sell_qty;
        let delta = group.buy_qty - group.sell_qty;

        assert_eq!(total_vol, 700.0);
        assert_eq!(delta, 500.0);

        // Matches Min Volume = 500
        assert!(total_vol >= 500.0);
        // Matches Min Delta = 400
        assert!(delta.abs() >= 400.0);
        // Matches Side: Both and BuyOnly, but rejects SellOnly
        let side_both = matches!(ClusterSearchSide::Both, ClusterSearchSide::Both) || (delta > 0.0);
        let side_buy = delta > 0.0;
        let side_sell = delta < 0.0;

        assert!(side_both);
        assert!(side_buy);
        assert!(!side_sell);
    }

    #[test]
    fn test_npoc_toggle_visibility_logic() {
        let studies_with_npoc = [
            FootprintStudy::NPoC { lookback: 50 },
            FootprintStudy::ClusterSearch {
                id: 1,
                min_volume: 100.0,
                min_delta: 50.0,
                side: data::chart::kline::ClusterSearchSide::Both,
                style: data::chart::kline::HighlightStyle::Border,
                color: data::chart::kline::HighlightColor::Amber,
            },
        ];
        let has_npoc = studies_with_npoc
            .iter()
            .any(|s| matches!(s, FootprintStudy::NPoC { .. }));
        assert!(has_npoc);

        let studies_without_npoc = [FootprintStudy::ClusterSearch {
            id: 1,
            min_volume: 100.0,
            min_delta: 50.0,
            side: data::chart::kline::ClusterSearchSide::Both,
            style: data::chart::kline::HighlightStyle::Border,
            color: data::chart::kline::HighlightColor::Amber,
        }];
        let has_npoc_off = studies_without_npoc
            .iter()
            .any(|s| matches!(s, FootprintStudy::NPoC { .. }));
        assert!(!has_npoc_off);
    }

    #[test]
    fn test_multi_rule_cluster_search_independence() {
        use data::chart::kline::{
            ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle,
        };

        let rule1 = FootprintStudy::ClusterSearch {
            id: 1,
            min_volume: 100.0,
            min_delta: 50.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Border,
            color: HighlightColor::Amber,
        };
        let rule2 = FootprintStudy::ClusterSearch {
            id: 2,
            min_volume: 300.0,
            min_delta: 150.0,
            side: ClusterSearchSide::BuyOnly,
            style: HighlightStyle::Circle,
            color: HighlightColor::Cyan,
        };
        let rule3 = FootprintStudy::ClusterSearch {
            id: 3,
            min_volume: 500.0,
            min_delta: 250.0,
            side: ClusterSearchSide::SellOnly,
            style: HighlightStyle::Triangle,
            color: HighlightColor::Red,
        };
        let rule4 = FootprintStudy::ClusterSearch {
            id: 4,
            min_volume: 1000.0,
            min_delta: 500.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Square,
            color: HighlightColor::White,
        };

        // Distinct IDs must not be considered the same type
        assert!(!rule1.is_same_type(&rule2));
        assert!(!rule2.is_same_type(&rule3));
        assert!(!rule3.is_same_type(&rule4));
        assert!(rule1.is_same_type(&rule1));

        // Format names
        assert_eq!(format!("{}", rule1), "Cluster Search 1");
        assert_eq!(format!("{}", rule2), "Cluster Search 2");
        assert_eq!(format!("{}", rule3), "Cluster Search 3");
        assert_eq!(format!("{}", rule4), "Cluster Search 4");
    }

    #[test]
    fn test_study_trait_cluster_search_independence() {
        use crate::modal::pane::settings::study::Study;
        use data::chart::kline::{
            ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle,
        };

        let rule1 = FootprintStudy::ClusterSearch {
            id: 1,
            min_volume: 100.0,
            min_delta: 50.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Border,
            color: HighlightColor::Amber,
        };
        let rule2 = FootprintStudy::ClusterSearch {
            id: 2,
            min_volume: 300.0,
            min_delta: 150.0,
            side: ClusterSearchSide::BuyOnly,
            style: HighlightStyle::Circle,
            color: HighlightColor::Cyan,
        };

        // Study trait must NOT use discriminant, must distinguish IDs
        assert!(!<FootprintStudy as Study>::is_same_type(&rule1, &rule2));
        assert!(<FootprintStudy as Study>::is_same_type(&rule1, &rule1));
    }

    #[test]
    fn test_ring_and_concentric_cluster_search_styles() {
        use data::chart::kline::{
            ClusterSearchSide, FootprintStudy, HighlightColor, HighlightStyle,
        };

        let ring_rule = FootprintStudy::ClusterSearch {
            id: 5,
            min_volume: 12000.0,
            min_delta: 6000.0,
            side: ClusterSearchSide::Both,
            style: HighlightStyle::Ring,
            color: HighlightColor::Purple,
        };
        let concentric_rule = FootprintStudy::ClusterSearch {
            id: 6,
            min_volume: 25000.0,
            min_delta: 10000.0,
            side: ClusterSearchSide::BuyOnly,
            style: HighlightStyle::Concentric,
            color: HighlightColor::Orange,
        };

        if let FootprintStudy::ClusterSearch { style, color, .. } = ring_rule {
            assert_eq!(format!("{}", style), "Ring");
            assert_eq!(format!("{}", color), "Purple");
        } else {
            panic!("Expected ClusterSearch variant");
        }

        if let FootprintStudy::ClusterSearch { style, color, .. } = concentric_rule {
            assert_eq!(format!("{}", style), "Concentric");
            assert_eq!(format!("{}", color), "Orange");
        } else {
            panic!("Expected ClusterSearch variant");
        }

        let serialized = serde_json::to_string(&concentric_rule).expect("serialize concentric");
        let deserialized: FootprintStudy =
            serde_json::from_str(&serialized).expect("deserialize concentric");
        assert_eq!(concentric_rule, deserialized);
    }

    #[test]
    fn test_cluster_search_volumetric_scaling() {
        let threshold = 5000.0_f32;
        let total_vol_normal = 5000.0_f32;
        let total_vol_large = 20000.0_f32;
        let total_vol_huge = 45000.0_f32;

        let scale_normal = (total_vol_normal / threshold).sqrt().clamp(1.0, 3.0);
        let scale_large = (total_vol_large / threshold).sqrt().clamp(1.0, 3.0);
        let scale_huge = (total_vol_huge / threshold).sqrt().clamp(1.0, 3.0);

        assert!((scale_normal - 1.0).abs() < 1e-4);
        assert!((scale_large - 2.0).abs() < 1e-4);
        assert!((scale_huge - 3.0).abs() < 1e-4);
    }

    #[test]
    fn test_insert_hist_klines_no_trade_volume_multiplication() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let view_cfg = ViewConfig::default();
        let mut chart = KlineChart::new(
            view_cfg,
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Footprint {
                clusters: data::chart::kline::ClusterKind::DeltaProfile,
                scaling: data::chart::kline::ClusterScaling::VisibleRange,
                studies: Vec::new(),
                show_bottom_volume: false,
            },
            None,
        );

        let kline1 = exchange::Kline {
            time: 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        };
        let trade = Trade {
            time: 300_100,
            price: Price::from_f32(100.0),
            qty: 5.0,
            is_sell: false,
        };

        chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
        chart.insert_raw_trades(vec![trade]);

        let get_vol = |c: &KlineChart| -> f32 {
            if let PlotData::TimeBased(ref ts) = c.data_source
                && let Some(dp) = ts.datapoints.get(&300_000)
                && let Some(g) = dp.footprint.trades.get(&Price::from_f32(100.0))
            {
                g.buy_qty
            } else {
                0.0
            }
        };

        assert_eq!(get_vol(&chart), 5.0);

        // Second kline insert must NOT multiply the trade volume in existing buckets
        let kline2 = exchange::Kline {
            time: 600_000,
            open: Price::from_f32(102.0),
            high: Price::from_f32(106.0),
            low: Price::from_f32(101.0),
            close: Price::from_f32(104.0),
            volume: (5.0, 5.0),
        };
        chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline2]);
        assert_eq!(get_vol(&chart), 5.0);

        // Third kline insert with existing kline update must also NOT multiply
        chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
        assert_eq!(get_vol(&chart), 5.0);
    }

    #[test]
    fn test_set_basis_preserves_trades() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let view_cfg = ViewConfig::default();
        let mut chart = KlineChart::new(
            view_cfg,
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Footprint {
                clusters: data::chart::kline::ClusterKind::DeltaProfile,
                scaling: data::chart::kline::ClusterScaling::VisibleRange,
                studies: Vec::new(),
                show_bottom_volume: false,
            },
            None,
        );

        let kline1 = exchange::Kline {
            time: 300_000,
            open: Price::from_f32(100.0),
            high: Price::from_f32(105.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(102.0),
            volume: (10.0, 10.0),
        };
        let trade = Trade {
            time: 300_100,
            price: Price::from_f32(100.0),
            qty: 7.5,
            is_sell: false,
        };

        chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline1]);
        chart.insert_raw_trades(vec![trade]);

        // Switch to M15 timeframe (15 * 60 * 1000 = 900_000 ms)
        // Trade at 300_100 belongs to interval starting at 0
        chart.set_basis(Basis::Time(exchange::Timeframe::M15));

        let get_vol_at = |c: &KlineChart, bucket: u64| -> f32 {
            if let PlotData::TimeBased(ref ts) = c.data_source
                && let Some(dp) = ts.datapoints.get(&bucket)
                && let Some(g) = dp.footprint.trades.get(&Price::from_f32(100.0))
            {
                g.buy_qty
            } else {
                0.0
            }
        };

        assert_eq!(get_vol_at(&chart, 0), 7.5);

        // When 15m historical klines arrive, trades are retained and not duplicated
        let kline_15m = exchange::Kline {
            time: 0,
            open: Price::from_f32(98.0),
            high: Price::from_f32(106.0),
            low: Price::from_f32(95.0),
            close: Price::from_f32(103.0),
            volume: (20.0, 20.0),
        };
        chart.insert_hist_klines(uuid::Uuid::new_v4(), &[kline_15m]);
        assert_eq!(get_vol_at(&chart, 0), 7.5);
    }

    #[test]
    fn test_footprint_show_bottom_volume_toggle() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let view_cfg = ViewConfig::default();
        let mut chart = KlineChart::new(
            view_cfg,
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Footprint {
                clusters: data::chart::kline::ClusterKind::VolumeProfile,
                scaling: data::chart::kline::ClusterScaling::VisibleRange,
                studies: Vec::new(),
                show_bottom_volume: false,
            },
            None,
        );

        if let KlineChartKind::Footprint {
            show_bottom_volume, ..
        } = chart.kind
        {
            assert!(!show_bottom_volume);
        } else {
            panic!("Expected Footprint chart kind");
        }

        chart.set_footprint_show_bottom_volume(true);
        if let KlineChartKind::Footprint {
            show_bottom_volume, ..
        } = chart.kind
        {
            assert!(show_bottom_volume);
        } else {
            panic!("Expected Footprint chart kind");
        }

        chart.set_footprint_show_bottom_volume(false);
        if let KlineChartKind::Footprint {
            show_bottom_volume, ..
        } = chart.kind
        {
            assert!(!show_bottom_volume);
        } else {
            panic!("Expected Footprint chart kind");
        }
    }

    #[test]
    fn test_insert_raw_trades_prepend_and_append_optimization() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let view_cfg = ViewConfig::default();
        let mut chart = KlineChart::new(
            view_cfg,
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Footprint {
                clusters: data::chart::kline::ClusterKind::VolumeProfile,
                scaling: data::chart::kline::ClusterScaling::VisibleRange,
                studies: Vec::new(),
                show_bottom_volume: false,
            },
            None,
        );

        // Batch 1: middle trades at 200..202
        let t2 = Trade {
            time: 200,
            price: Price::from_f32(100.0),
            qty: 1.0,
            is_sell: false,
        };
        let t3 = Trade {
            time: 202,
            price: Price::from_f32(101.0),
            qty: 1.0,
            is_sell: false,
        };
        chart.insert_raw_trades(vec![t2, t3]);
        assert_eq!(chart.raw_trades().len(), 2);

        // Batch 2: later trades at 300..301 (append)
        let t4 = Trade {
            time: 300,
            price: Price::from_f32(102.0),
            qty: 1.0,
            is_sell: false,
        };
        let t5 = Trade {
            time: 301,
            price: Price::from_f32(103.0),
            qty: 1.0,
            is_sell: false,
        };
        chart.insert_raw_trades(vec![t4, t5]);
        assert_eq!(chart.raw_trades().len(), 4);
        assert_eq!(
            chart
                .raw_trades()
                .iter()
                .map(|t| t.time)
                .collect::<Vec<_>>(),
            vec![200, 202, 300, 301]
        );

        // Batch 3: earlier trades at 100..101 (prepend)
        let t0 = Trade {
            time: 100,
            price: Price::from_f32(98.0),
            qty: 1.0,
            is_sell: false,
        };
        let t1 = Trade {
            time: 101,
            price: Price::from_f32(99.0),
            qty: 1.0,
            is_sell: false,
        };
        chart.insert_raw_trades(vec![t0, t1]);
        assert_eq!(chart.raw_trades().len(), 6);
        assert_eq!(
            chart
                .raw_trades()
                .iter()
                .map(|t| t.time)
                .collect::<Vec<_>>(),
            vec![100, 101, 200, 202, 300, 301]
        );
    }

    fn make_test_chart() -> KlineChart {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        KlineChart::new(
            ViewConfig::default(),
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Candles,
            None,
        )
    }

    #[test]
    fn test_kline_chart_drawings_management() {
        let mut chart = make_test_chart();
        assert!(chart.drawings.is_empty());

        let d1 = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
        chart.add_drawing(d1.clone());
        assert_eq!(chart.drawings.len(), 1);
        assert_eq!(chart.drawings[0], d1);

        let mut d1_updated = d1.clone();
        d1_updated.color = [0.0, 1.0, 0.0, 1.0];
        chart.update_drawing(d1_updated.clone());
        assert_eq!(chart.drawings[0].color, [0.0, 1.0, 0.0, 1.0]);

        chart.set_active_drawing_tool(DrawingTool::Trendline);
        assert_eq!(chart.active_drawing_tool(), DrawingTool::Trendline);

        chart.set_active_drawing_tool(DrawingTool::Brush);
        assert_eq!(chart.active_drawing_tool(), DrawingTool::Brush);

        chart.set_active_drawing_tool(DrawingTool::Path);
        assert_eq!(chart.active_drawing_tool(), DrawingTool::Path);

        chart.set_active_drawing_tool(DrawingTool::ShortPosition);
        assert_eq!(chart.active_drawing_tool(), DrawingTool::ShortPosition);

        chart.set_active_drawing_tool(DrawingTool::LongPosition);
        assert_eq!(chart.active_drawing_tool(), DrawingTool::LongPosition);

        chart.delete_drawing(d1.id);
        assert!(chart.drawings.is_empty());

        chart.add_drawing(d1);
        chart.clear_drawings();
        assert!(chart.drawings.is_empty());
    }

    #[test]
    fn test_kline_chart_drawings_lock_and_clear() {
        let mut chart = make_test_chart();

        let d_unlocked = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
        let mut d_locked = Drawing::horizontal(200.0, [0.0, 1.0, 0.0, 1.0], 2.0);
        d_locked.is_locked = true;

        chart.add_drawing(d_unlocked.clone());
        chart.add_drawing(d_locked.clone());
        assert_eq!(chart.drawings.len(), 2);

        // Select d_locked and test helpers
        chart.set_selected_drawing(Some(d_locked.id));
        assert_eq!(chart.selected_drawing().unwrap().id, d_locked.id);
        assert!(chart.selected_drawing().unwrap().is_locked);

        // Toggle lock
        chart.toggle_selected_drawing_lock();
        assert!(!chart.selected_drawing().unwrap().is_locked);
        chart.toggle_selected_drawing_lock();
        assert!(chart.selected_drawing().unwrap().is_locked);

        // Update color and width
        chart.update_selected_drawing_color([0.5, 0.5, 0.5, 0.8]);
        assert_eq!(
            chart.selected_drawing().unwrap().color,
            [0.5, 0.5, 0.5, 0.8]
        );
        chart.update_selected_drawing_width(3.5);
        assert_eq!(chart.selected_drawing().unwrap().width, 3.5);

        // clear_drawings should retain locked drawings and remove unlocked ones
        chart.clear_drawings();
        assert_eq!(chart.drawings.len(), 1);
        assert_eq!(chart.drawings[0].id, d_locked.id);

        // Deleting the specific locked drawing directly via delete_drawing
        chart.delete_drawing(d_locked.id);
        assert!(chart.drawings.is_empty());
        assert!(chart.selected_drawing().is_none());
    }

    #[test]
    fn test_kline_chart_drawing_drag_and_modification() {
        use iced::widget::canvas::Program;

        let mut chart = make_test_chart();
        let rect = Drawing::rectangle((1000, 100.0), (2000, 110.0), [0.2, 0.5, 0.9, 1.0], 1.0);
        let rect_id = rect.id;
        chart.add_drawing(rect);

        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let mut interaction = Interaction::None;

        // Select drawing manually into drawing_state
        chart.drawing_state.borrow_mut().selected_drawing = Some(rect_id);

        // Delete key press produces DeleteDrawing message
        let delete_key_event = Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            repeat: false,
            text: None,
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Delete),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
        });
        let action = chart.update(
            &mut interaction,
            &delete_key_event,
            bounds,
            mouse::Cursor::Unavailable,
        );
        assert!(action.is_some());
        assert!(chart.drawing_state.borrow().selected_drawing.is_none());

        // Re-add drawing and test dragging
        let rect2 = Drawing::rectangle((1000, 100.0), (2000, 110.0), [0.2, 0.5, 0.9, 1.0], 1.0);
        chart.add_drawing(rect2.clone());

        chart.drawing_state.borrow_mut().drag = Some(DrawingDrag::MovingDrawing {
            start_time: 1000,
            start_price: 100.0,
            initial_drawing: rect2.clone(),
            current_drawing: rect2,
        });

        // Moving cursor updates current_drawing
        let cursor = mouse::Cursor::Available(Point::new(400.0, 300.0));
        let move_event = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(400.0, 300.0),
        });
        let move_action = chart.update(&mut interaction, &move_event, bounds, cursor);
        assert!(move_action.is_some());
        assert!(chart.drawing_state.borrow().drag.is_some());

        // Releasing mouse publishes UpdateDrawing
        let release_event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let release_action = chart.update(&mut interaction, &release_event, bounds, cursor);
        assert!(release_action.is_some());
        assert!(chart.drawing_state.borrow().drag.is_none());
    }

    #[test]
    fn test_kline_chart_brush_fixed_drag_only() {
        let chart = make_test_chart();
        let brush = Drawing::brush(
            vec![(1000, 100.0), (2000, 120.0), (3000, 110.0)],
            [0.35, 0.7, 1.0, 1.0],
            2.0,
        );

        assert!(brush.handles().is_empty());

        let view_state = chart.state();
        let pt_on_stroke = Point::new(
            view_state.interval_to_x(2000),
            view_state.price_to_y(Price::from_f32(120.0)),
        );

        let hit = hit_test_drawing(&brush, pt_on_stroke, view_state, 10.0);
        assert_eq!(hit, Some(HitTarget::Body));

        // Translation shifts whole brush without mutating shape
        let mut translated = brush.clone();
        translated.translate(500, 10.0);
        if let DrawingKind::Brush { points } = &translated.kind {
            assert_eq!(points[0], (1500, 110.0));
            assert_eq!(points[1], (2500, 130.0));
            assert_eq!(points[2], (3500, 120.0));
        } else {
            panic!("Expected Brush");
        }
    }

    #[test]
    fn test_kline_chart_price_alerts() {
        let mut chart = make_test_chart();
        let alert = PriceAlert::new(
            "TEST",
            100.0,
            data::chart::alert::AlertCondition::CrossAbove,
        );
        let alert_id = alert.id;
        chart.add_alert(alert);
        assert_eq!(chart.alerts.len(), 1);

        let triggered = chart.check_price_alerts(99.0, 101.0);
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, alert_id);

        chart.remove_alert(alert_id);
        assert!(chart.alerts.is_empty());
    }

    #[test]
    fn test_kline_chart_replay() {
        let mut chart = make_test_chart();
        assert!(!chart.is_replay_active());

        chart.toggle_replay();
        assert!(chart.is_replay_active());

        chart.replay_set_cutoff(1000);
        assert_eq!(chart.replay_state().unwrap().cutoff_time, 1000);

        chart.replay_step_forward();
        assert!(chart.replay_state().unwrap().cutoff_time > 1000);

        chart.replay_step_backward();
        assert_eq!(chart.replay_state().unwrap().cutoff_time, 1000);

        chart.toggle_replay();
        assert!(!chart.is_replay_active());
    }

    #[test]
    fn test_kline_chart_replay_jump_and_random() {
        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);
        let klines: Vec<Kline> = (0..50)
            .map(|i| Kline {
                time: 1000 + i * 300_000,
                open: Price::from_f32(100.0),
                high: Price::from_f32(105.0),
                low: Price::from_f32(95.0),
                close: Price::from_f32(102.0),
                volume: (5.0, 5.0),
            })
            .collect();
        let mut chart = KlineChart::new(
            ViewConfig::default(),
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &klines,
            Vec::new(),
            &[],
            ticker_info,
            &KlineChartKind::Candles,
            None,
        );

        chart.toggle_replay();
        assert!(chart.is_replay_active());

        // Test find_closest_bar_at_or_before
        let closest = chart.find_closest_bar_at_or_before(1000 + 5 * 300_000 + 100);
        assert_eq!(closest, Some(1000 + 5 * 300_000));

        // Test replay_set_cutoff_and_jump
        chart.replay_set_cutoff_and_jump(1000 + 10 * 300_000);
        assert_eq!(
            chart.replay_state().unwrap().cutoff_time,
            1000 + 10 * 300_000
        );

        // Test replay_pick_random_bar
        let rand_ts = chart.replay_pick_random_bar();
        assert!(rand_ts.is_some());
        let picked = rand_ts.unwrap();
        assert!((1000..=1000 + 49 * 300_000).contains(&picked));
        assert_eq!(chart.replay_state().unwrap().cutoff_time, picked);
    }

    #[test]
    fn test_inverted_range_does_not_panic() {
        let chart = make_test_chart();
        let qty = chart.calc_qty_scales(
            2000,
            1000,
            Price::from_f32(100.0),
            Price::from_f32(90.0),
            PriceStep::from_f32(1.0),
            data::chart::kline::ClusterKind::VolumeProfile,
        );
        assert_eq!(qty, 0.0);
    }

    #[test]
    fn test_kline_chart_alert_dragging() {
        use iced::widget::canvas::Program;

        let mut chart = make_test_chart();
        let alert = PriceAlert::new(
            "BTCUSDT",
            50000.0,
            data::chart::alert::AlertCondition::CrossAbove,
        );
        let alert_id = alert.id;
        chart.add_alert(alert);

        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let mut interaction = Interaction::None;

        // Start drag state
        *chart.alert_drag_state.borrow_mut() = Some((alert_id, 50000.0));

        // Move cursor while dragging - previously caused RefCell BorrowMutError panic (exit code 101)
        let cursor = mouse::Cursor::Available(Point::new(400.0, 300.0));
        let move_event = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(400.0, 300.0),
        });
        let action = chart.update(&mut interaction, &move_event, bounds, cursor);
        assert!(action.is_some());
        assert!(chart.alert_drag_state.borrow().is_some());

        // Release mouse button - finishes drag and publishes Message::UpdateAlertPrice
        let release_event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let release_action = chart.update(&mut interaction, &release_event, bounds, cursor);
        assert!(release_action.is_some());
        assert!(chart.alert_drag_state.borrow().is_none());
    }

    #[test]
    fn test_kline_indicator_settings_persistence() {
        use data::chart::kline::{Config, PositionFlowColors, TpoElementColor};

        let custom_colors = PositionFlowColors {
            new_longs: TpoElementColor::Cyan,
            new_shorts: TpoElementColor::Orange,
            long_forced_close: TpoElementColor::Purple,
            short_forced_close: TpoElementColor::White,
        };

        let mut custom_cfg = Config::factory_default();
        custom_cfg.position_flow_colors = custom_colors;
        custom_cfg.rolling_vwap_window_hours = 48;

        let ticker = exchange::Ticker::new("BTCUSDT", exchange::adapter::Exchange::BinanceLinear);
        let ticker_info = exchange::TickerInfo::new(ticker, 0.1, 0.001, None);

        // 1. Initializing KlineChart::new with explicit config applies settings to indicators
        let chart = KlineChart::new(
            ViewConfig::default(),
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[KlineIndicator::PositionFlow],
            ticker_info,
            &KlineChartKind::Candles,
            Some(custom_cfg),
        );

        assert_eq!(chart.config(), custom_cfg);
        if let Some(indi) = chart.indicators[KlineIndicator::PositionFlow].as_ref() {
            let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
            assert_eq!(pf.colors, custom_colors);
        } else {
            panic!("PositionFlow indicator should be present");
        }

        // 2. set_visual_config updates chart config and sets global user default
        let mut chart2 = make_test_chart();
        let mut updated_cfg = Config::factory_default();
        updated_cfg.position_flow_colors = custom_colors;
        updated_cfg.rolling_vwap_window_hours = 72;
        chart2.set_visual_config(updated_cfg);

        assert_eq!(chart2.config(), updated_cfg);
        assert_eq!(
            data::chart::kline::user_default_kline_config(),
            Some(updated_cfg)
        );

        // 3. New chart created with None inherits user default config
        let chart3 = KlineChart::new(
            ViewConfig::default(),
            Basis::Time(exchange::Timeframe::M5),
            1.0,
            &[],
            Vec::new(),
            &[KlineIndicator::PositionFlow],
            ticker_info,
            &KlineChartKind::Candles,
            None,
        );
        assert_eq!(chart3.config(), updated_cfg);
        if let Some(indi) = chart3.indicators[KlineIndicator::PositionFlow].as_ref() {
            let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
            assert_eq!(pf.colors, custom_colors);
        } else {
            panic!("PositionFlow indicator should be present in chart3");
        }

        // 4. Toggling an indicator on inherits the chart's current config
        let mut chart4 = make_test_chart();
        chart4.set_visual_config(custom_cfg);
        chart4.toggle_indicator(KlineIndicator::PositionFlow);
        if let Some(indi) = chart4.indicators[KlineIndicator::PositionFlow].as_ref() {
            let pf = indi
                .as_any()
                .and_then(|a| a.downcast_ref::<crate::chart::indicator::kline::position_flow::PositionFlowIndicator>())
                .unwrap();
            assert_eq!(pf.colors, custom_colors);
        } else {
            panic!("PositionFlow indicator should be present after toggle");
        }
    }
}
