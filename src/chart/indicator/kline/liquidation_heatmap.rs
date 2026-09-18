use crate::chart::{Caches, Message, ViewState, indicator::kline::KlineIndicatorImpl};
use data::chart::PlotData;
use data::chart::kline::KlineDataPoint;
use iced::widget::Space;
use std::ops::RangeInclusive;

pub struct LiquidationHeatmapIndicator {
    cache: Caches,
}

impl LiquidationHeatmapIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
        }
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
}
