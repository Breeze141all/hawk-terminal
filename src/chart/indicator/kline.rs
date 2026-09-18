use crate::chart::{Message, ViewState};

use data::chart::PlotData;
use data::chart::indicator::KlineIndicator;
use data::chart::kline::KlineDataPoint;
use exchange::fetcher::FetchRange;
use exchange::{Kline, Timeframe, Trade};

pub mod bid_ask_ratio;
pub mod cvd;
pub mod liquidation_heatmap;
pub mod market_pulse;
pub mod net_oi;
pub mod open_interest;
pub mod position_flow;
pub mod rolling_vwap;
pub mod tpo;
pub mod volume;
pub mod vpin;
pub mod vwap;

pub trait KlineIndicatorImpl {
    /// Clear all caches for a full redraw
    fn clear_all_caches(&mut self);

    /// Clear caches related to crosshair only
    /// e.g. tooltips and scale labels for a partial redraw
    fn clear_crosshair_caches(&mut self);

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        visible_range: std::ops::RangeInclusive<u64>,
    ) -> iced::Element<'a, Message>;

    /// If the indicator needs data fetching, return the required range
    fn fetch_range(&mut self, _ctx: &FetchCtx) -> Option<FetchRange> {
        None
    }

    /// Rebuild data using kline(OHLCV) source
    fn rebuild_from_source(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_insert_klines(&mut self, _klines: &[Kline]) {}

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        _source: &PlotData<KlineDataPoint>,
    ) {
    }

    fn on_ticksize_change(&mut self, _source: &PlotData<KlineDataPoint>) {}

    /// Timeframe/tick interval has changed
    fn on_basis_change(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_open_interest(&mut self, _pairs: &[exchange::OpenInterest]) {}

    fn on_funding_rates(&mut self, _rates: &[exchange::FundingRate]) {}

    fn on_spot_klines(&mut self, _klines: &[exchange::SpotKline]) {}

    fn on_net_oi_data(&mut self, _data: &[exchange::NetOiDataPoint]) {}

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

pub struct FetchCtx<'a> {
    pub main_chart: &'a ViewState,
    pub timeframe: Timeframe,
    pub visible_earliest: u64,
    pub kline_latest: u64,
    pub prefetch_earliest: u64,
}

pub fn make_empty(which: KlineIndicator) -> Box<dyn KlineIndicatorImpl> {
    match which {
        KlineIndicator::Volume => Box::new(super::kline::volume::VolumeIndicator::new()),
        KlineIndicator::OpenInterest => {
            Box::new(super::kline::open_interest::OpenInterestIndicator::new())
        }
        KlineIndicator::MarketPulse => {
            Box::new(super::kline::market_pulse::MarketPulseIndicator::new())
        }
        KlineIndicator::NetOi => Box::new(super::kline::net_oi::NetOiIndicator::new()),
        KlineIndicator::Vpin => Box::new(super::kline::vpin::VpinIndicator::new()),
        KlineIndicator::Vwap => Box::new(super::kline::vwap::VwapIndicator::new()),
        KlineIndicator::Tpo => Box::new(super::kline::tpo::TpoIndicator::new()),
        KlineIndicator::RollingVwap => {
            Box::new(super::kline::rolling_vwap::RollingVwapIndicator::new())
        }
        KlineIndicator::Cvd => Box::new(super::kline::cvd::CvdIndicator::new()),
        KlineIndicator::BidAskRatio => {
            Box::new(super::kline::bid_ask_ratio::BidAskRatioIndicator::new())
        }
        KlineIndicator::PositionFlow => {
            Box::new(super::kline::position_flow::PositionFlowIndicator::new())
        }
        KlineIndicator::LiquidationHeatmap => {
            Box::new(super::kline::liquidation_heatmap::LiquidationHeatmapIndicator::new())
        }
    }
}
