use std::time::Duration;

use exchange::Trade;
use serde::{Deserialize, Serialize};

use crate::util::ok_or_default;

const TRADE_RETENTION_MS: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub trade_size_filter: f32,
    #[serde(default = "default_buffer_filter")]
    pub trade_retention: Duration,
    #[serde(deserialize_with = "ok_or_default", default)]
    pub stacked_bar: Option<StackedBar>,
    #[serde(default)]
    pub additional_tickers: Vec<exchange::TickerInfo>,
    #[serde(default)]
    pub disabled_tickers: Vec<exchange::Ticker>,
    /// Show the exchange's real forced liquidations above the tape (Binance and Bybit perps).
    #[serde(default)]
    pub show_liquidations: bool,
    /// Hide liquidations smaller than this many USD.
    #[serde(default = "default_liquidation_min_usd")]
    pub liquidation_min_usd: f32,
}

/// Minimum liquidation sizes offered in the Time & Sales settings.
pub const LIQUIDATION_MIN_USD_CHOICES: [f32; 5] = [0.0, 10_000.0, 50_000.0, 250_000.0, 1_000_000.0];

fn default_liquidation_min_usd() -> f32 {
    10_000.0
}

impl Default for Config {
    fn default() -> Self {
        Config {
            trade_size_filter: 0.0,
            trade_retention: Duration::from_millis(TRADE_RETENTION_MS),
            stacked_bar: StackedBar::Compact(StackedBarRatio::default()).into(),
            additional_tickers: Vec::new(),
            disabled_tickers: Vec::new(),
            show_liquidations: false,
            liquidation_min_usd: default_liquidation_min_usd(),
        }
    }
}

fn default_buffer_filter() -> Duration {
    Duration::from_millis(TRADE_RETENTION_MS)
}

/// Zero-allocation tape trade tagged with ticker index for multi-ticker aggregation
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TapeTrade {
    pub trade: Trade,
    #[serde(default)]
    pub ticker_index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Copy)]
pub enum StackedBar {
    Compact(StackedBarRatio),
    Full(StackedBarRatio),
}

impl StackedBar {
    pub fn ratio(self) -> StackedBarRatio {
        match self {
            StackedBar::Compact(r) | StackedBar::Full(r) => r,
        }
    }

    pub fn with_ratio(self, r: StackedBarRatio) -> Self {
        match self {
            StackedBar::Compact(_) => StackedBar::Compact(r),
            StackedBar::Full(_) => StackedBar::Full(r),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default, Copy)]
pub enum StackedBarRatio {
    Count,
    #[default]
    Volume,
    AverageSize,
}

impl std::fmt::Display for StackedBarRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StackedBarRatio::Count => write!(f, "Count"),
            StackedBarRatio::AverageSize => write!(f, "Average trade size"),
            StackedBarRatio::Volume => write!(f, "Volume"),
        }
    }
}

impl StackedBarRatio {
    pub const ALL: [StackedBarRatio; 3] = [
        StackedBarRatio::Count,
        StackedBarRatio::Volume,
        StackedBarRatio::AverageSize,
    ];
}

#[derive(Default)]
pub struct HistAgg {
    buy_count: u64,
    sell_count: u64,
    buy_sum: f64,
    sell_sum: f64,
}

impl HistAgg {
    pub fn add(&mut self, trade: &Trade) {
        let qty = trade.qty as f64;

        if trade.is_sell {
            self.sell_count += 1;
            self.sell_sum += qty;
        } else {
            self.buy_count += 1;
            self.buy_sum += qty;
        }
    }

    pub fn remove(&mut self, trade: &Trade) {
        let qty = trade.qty as f64;

        if trade.is_sell {
            self.sell_count = self.sell_count.saturating_sub(1);
            self.sell_sum -= qty;
        } else {
            self.buy_count = self.buy_count.saturating_sub(1);
            self.buy_sum -= qty;
        }
    }

    pub fn values_for(&self, ratio_kind: StackedBarRatio) -> Option<(f64, f64, f32)> {
        match ratio_kind {
            StackedBarRatio::Count => {
                let buy = self.buy_count as f64;
                let sell = self.sell_count as f64;
                let total = buy + sell;

                if total <= 0.0 {
                    return None;
                }
                let buy_ratio = (buy / total) as f32;

                Some((buy, sell, buy_ratio))
            }
            StackedBarRatio::Volume => {
                let buy = self.buy_sum;
                let sell = self.sell_sum;
                let total = buy + sell;

                if total <= 0.0 {
                    return None;
                }
                let buy_ratio = (buy / total) as f32;

                Some((buy, sell, buy_ratio))
            }
            StackedBarRatio::AverageSize => {
                let buy_avg = if self.buy_count > 0 {
                    self.buy_sum / self.buy_count as f64
                } else {
                    0.0
                };
                let sell_avg = if self.sell_count > 0 {
                    self.sell_sum / self.sell_count as f64
                } else {
                    0.0
                };

                let denom = buy_avg + sell_avg;
                if denom <= 0.0 {
                    return None;
                }
                let buy_ratio = (buy_avg / denom) as f32;

                Some((buy_avg, sell_avg, buy_ratio))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::util::Price;

    #[test]
    fn test_config_saved_before_liquidations_still_loads() {
        let mut old = serde_json::to_value(Config::default()).unwrap();
        let map = old.as_object_mut().unwrap();
        map.remove("show_liquidations");
        map.remove("liquidation_min_usd");

        let cfg: Config = serde_json::from_value(old).unwrap();
        assert!(!cfg.show_liquidations);
        assert_eq!(cfg.liquidation_min_usd, 10_000.0);
    }

    #[test]
    fn test_ter16_zero_alloc_tape_trade_and_hist_agg() {
        let mut agg = HistAgg::default();
        let t1 = Trade {
            time: 1000,
            is_sell: false,
            price: Price::from_f64(100.0),
            qty: 2.5,
        };
        let t2 = Trade {
            time: 1050,
            is_sell: true,
            price: Price::from_f64(100.5),
            qty: 1.5,
        };

        agg.add(&t1);
        agg.add(&t2);

        let (buy_cnt, sell_cnt, count_ratio) =
            agg.values_for(StackedBarRatio::Count).expect("count ratio");
        assert_eq!(buy_cnt, 1.0);
        assert_eq!(sell_cnt, 1.0);
        assert!((count_ratio - 0.5).abs() < 1e-4);

        let (buy_vol, sell_vol, vol_ratio) =
            agg.values_for(StackedBarRatio::Volume).expect("vol ratio");
        assert_eq!(buy_vol, 2.5);
        assert_eq!(sell_vol, 1.5);
        assert!((vol_ratio - (2.5 / 4.0)).abs() < 1e-4);

        agg.remove(&t1);
        let (buy_cnt_after, _, _) = agg
            .values_for(StackedBarRatio::Count)
            .expect("after remove");
        assert_eq!(buy_cnt_after, 0.0);
    }

    #[test]
    fn test_ter20_multi_ticker_config_defaults() {
        let cfg = Config::default();
        assert!(cfg.additional_tickers.is_empty());
        assert!(cfg.disabled_tickers.is_empty());
        assert_eq!(cfg.trade_size_filter, 0.0);
    }
}
