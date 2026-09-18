use std::fmt::{self, Debug, Display};

use enum_map::Enum;
use exchange::adapter::MarketKind;
use serde::{Deserialize, Serialize};

pub trait Indicator: PartialEq + Display + 'static {
    fn for_market(market: MarketKind) -> &'static [Self]
    where
        Self: Sized;
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Eq, Enum)]
pub enum KlineIndicator {
    Volume,
    OpenInterest,
    MarketPulse,
    NetOi,
    Vpin,
    Vwap,
    Tpo,
    RollingVwap,
    Cvd,
    BidAskRatio,
    PositionFlow,
    LiquidationHeatmap,
}

impl Indicator for KlineIndicator {
    fn for_market(market: MarketKind) -> &'static [Self] {
        match market {
            MarketKind::Spot => &Self::FOR_SPOT,
            MarketKind::LinearPerps | MarketKind::InversePerps => &Self::FOR_PERPS,
        }
    }
}

impl KlineIndicator {
    // Indicator togglers on UI menus depend on these arrays.
    // Every variant needs to be in either SPOT, PERPS or both.
    /// Indicators that can be used with spot market tickers
    const FOR_SPOT: [KlineIndicator; 8] = [
        KlineIndicator::Volume,
        KlineIndicator::Vpin,
        KlineIndicator::Vwap,
        KlineIndicator::Tpo,
        KlineIndicator::RollingVwap,
        KlineIndicator::Cvd,
        KlineIndicator::BidAskRatio,
        KlineIndicator::PositionFlow,
    ];
    /// Indicators that can be used with perpetual swap market tickers
    const FOR_PERPS: [KlineIndicator; 12] = [
        KlineIndicator::Volume,
        KlineIndicator::OpenInterest,
        KlineIndicator::MarketPulse,
        KlineIndicator::NetOi,
        KlineIndicator::Vpin,
        KlineIndicator::Vwap,
        KlineIndicator::Tpo,
        KlineIndicator::RollingVwap,
        KlineIndicator::Cvd,
        KlineIndicator::BidAskRatio,
        KlineIndicator::PositionFlow,
        KlineIndicator::LiquidationHeatmap,
    ];

    /// Returns true if this indicator requires an independent sub-panel in the layout.
    /// Overlays like TPO and Liquidation Heatmap return false because they render directly on the price chart canvas.
    pub const fn is_panel(&self) -> bool {
        !matches!(
            self,
            KlineIndicator::Tpo | KlineIndicator::LiquidationHeatmap
        )
    }
}

impl Display for KlineIndicator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            KlineIndicator::Volume => write!(f, "Volume"),
            KlineIndicator::OpenInterest => write!(f, "Open Interest"),
            KlineIndicator::MarketPulse => write!(f, "Market Pulse"),
            KlineIndicator::NetOi => write!(f, "Net OI"),
            KlineIndicator::Vpin => write!(f, "VPIN"),
            KlineIndicator::Vwap => write!(f, "VWAP"),
            KlineIndicator::Tpo => write!(f, "TPO Profile"),
            KlineIndicator::RollingVwap => write!(f, "Rolling VWAP"),
            KlineIndicator::Cvd => write!(f, "Cumulative Delta"),
            KlineIndicator::BidAskRatio => write!(f, "Bid/Ask Ratio"),
            KlineIndicator::PositionFlow => write!(f, "Position Flow"),
            KlineIndicator::LiquidationHeatmap => write!(f, "Liquidation Heatmap"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Eq, Enum)]
pub enum HeatmapIndicator {
    Volume,
}

impl Indicator for HeatmapIndicator {
    fn for_market(market: MarketKind) -> &'static [Self] {
        match market {
            MarketKind::Spot => &Self::FOR_SPOT,
            MarketKind::LinearPerps | MarketKind::InversePerps => &Self::FOR_PERPS,
        }
    }
}

impl HeatmapIndicator {
    // Indicator togglers on UI menus depend on these arrays.
    // Every variant needs to be in either SPOT, PERPS or both.
    /// Indicators that can be used with spot market tickers
    const FOR_SPOT: [HeatmapIndicator; 1] = [HeatmapIndicator::Volume];
    /// Indicators that can be used with perpetual swap market tickers
    const FOR_PERPS: [HeatmapIndicator; 1] = [HeatmapIndicator::Volume];
}

impl Display for HeatmapIndicator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HeatmapIndicator::Volume => write!(f, "Volume"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Temporary workaround,
/// represents any indicator type in the UI
pub enum UiIndicator {
    Heatmap(HeatmapIndicator),
    Kline(KlineIndicator),
}

impl From<KlineIndicator> for UiIndicator {
    fn from(k: KlineIndicator) -> Self {
        UiIndicator::Kline(k)
    }
}

impl From<HeatmapIndicator> for UiIndicator {
    fn from(h: HeatmapIndicator) -> Self {
        UiIndicator::Heatmap(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liquidation_heatmap_indicator_properties() {
        assert!(!KlineIndicator::LiquidationHeatmap.is_panel());
        assert!(
            KlineIndicator::for_market(MarketKind::LinearPerps)
                .contains(&KlineIndicator::LiquidationHeatmap)
        );
        assert!(
            KlineIndicator::for_market(MarketKind::InversePerps)
                .contains(&KlineIndicator::LiquidationHeatmap)
        );
        assert!(
            !KlineIndicator::for_market(MarketKind::Spot)
                .contains(&KlineIndicator::LiquidationHeatmap)
        );
        assert_eq!(
            KlineIndicator::LiquidationHeatmap.to_string(),
            "Liquidation Heatmap"
        );
    }
}
