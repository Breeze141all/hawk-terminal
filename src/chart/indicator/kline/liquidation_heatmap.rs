use crate::chart::indicator::kline::open_interest::OpenInterestIndicator;
use crate::chart::{
    Caches, Message, ViewState,
    indicator::kline::{FetchCtx, KlineIndicatorImpl},
};
use data::chart::PlotData;
use data::chart::kline::KlineDataPoint;
use exchange::fetcher::FetchRange;
use iced::widget::Space;
use std::ops::RangeInclusive;

/// The heatmap itself lives in `KlineChart::liquidation_heatmap`; this shell only marks the
/// overlay as enabled and, with OI weighting on, keeps the open interest history loaded.
pub struct LiquidationHeatmapIndicator {
    cache: Caches,
    /// Some while OI weighting is on: tracks which OI range is loaded and asks for the rest.
    oi_loader: Option<OpenInterestIndicator>,
}

impl LiquidationHeatmapIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            oi_loader: None,
        }
    }

    pub fn set_oi_weighting(&mut self, enabled: bool) {
        match (enabled, self.oi_loader.is_some()) {
            (true, false) => self.oi_loader = Some(OpenInterestIndicator::new()),
            (false, true) => self.oi_loader = None,
            _ => {}
        }
    }
}

/// Applies the chart config's OI weighting switch if `indi` is the heatmap shell.
pub fn apply_chart_config(indi: &mut dyn KlineIndicatorImpl, config: &data::chart::kline::Config) {
    if let Some(any) = indi.as_any_mut()
        && let Some(heatmap) = any.downcast_mut::<LiquidationHeatmapIndicator>()
    {
        heatmap.set_oi_weighting(config.liq_weight_by_oi);
    }
}

impl Default for LiquidationHeatmapIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for LiquidationHeatmapIndicator {
    fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }

    fn clear_crosshair_caches(&mut self) {
        self.cache.clear_crosshair();
    }

    fn element<'a>(
        &'a self,
        _chart: &'a ViewState,
        _visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        Space::new().into()
    }

    fn rebuild_from_source(&mut self, _source: &PlotData<KlineDataPoint>) {
        self.clear_all_caches();
    }

    fn fetch_range(&mut self, ctx: &FetchCtx) -> Option<FetchRange> {
        self.oi_loader.as_mut()?.fetch_range(ctx)
    }

    fn on_open_interest(&mut self, data: &[exchange::OpenInterest]) {
        if let Some(loader) = self.oi_loader.as_mut() {
            loader.on_open_interest(data);
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
