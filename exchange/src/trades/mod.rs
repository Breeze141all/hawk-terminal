pub mod cache;

use crate::adapter::{AdapterError, Exchange};
use crate::{TickerInfo, Trade};
use std::path::PathBuf;

pub async fn fetch_trades(
    ticker_info: TickerInfo,
    from_time: u64,
    data_path: PathBuf,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    match ticker_info.exchange() {
        Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => {
            crate::adapter::binance::fetch_trades(ticker_info, from_time, data_path).await
        }
        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => {
            crate::adapter::bybit::fetch_trades(ticker_info, from_time, data_path).await
        }
        Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => {
            crate::adapter::okex::fetch_trades(ticker_info, from_time, data_path).await
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
            crate::adapter::hyperliquid::fetch_trades(ticker_info, from_time, data_path).await
        }
    }
}
