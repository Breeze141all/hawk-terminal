pub mod cache;
pub mod coordinator;

use crate::adapter::{AdapterError, Exchange};
use crate::{TickerInfo, Trade};
use std::path::PathBuf;

pub async fn dispatch_fetch_trades(
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
        Exchange::MexcSpot | Exchange::MexcLinear | Exchange::MexcInverse => {
            crate::adapter::mexc::fetch_trades(ticker_info, from_time, data_path).await
        }
    }
}

pub async fn fetch_trades(
    ticker_info: TickerInfo,
    from_time: u64,
    data_path: PathBuf,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    let today_midnight = crate::clock::today_midnight_ms(Some(ticker_info.exchange()));
    if from_time >= today_midnight {
        coordinator::COORDINATOR
            .fetch_intraday(
                ticker_info,
                from_time,
                data_path,
                today_midnight,
                dispatch_fetch_trades,
            )
            .await
    } else {
        dispatch_fetch_trades(ticker_info, from_time, data_path).await
    }
}
