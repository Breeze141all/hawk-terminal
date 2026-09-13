use crate::chart::{Caches, Message, ViewState, indicator::kline::KlineIndicatorImpl};
use data::chart::PlotData;
use data::chart::kline::KlineDataPoint;
use iced::widget::Space;
use std::ops::RangeInclusive;

pub struct TpoIndicator {
    cache: Caches,
}

impl TpoIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
        }
    }
}

impl Default for TpoIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for TpoIndicator {
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
