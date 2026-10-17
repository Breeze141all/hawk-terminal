use exchange::adapter::{AdapterError, StreamKind};
use exchange::{TickerInfo, Trade};
use iced::task::{Straw, sipper};
use std::path::PathBuf;
use std::sync::RwLock;
use uuid::Uuid;

use crate::connector::client::ServerClient;
pub use data::TradeFetchMode;
pub use exchange::fetcher::{
    FetchRange, FetchRequests, FetchSpec, FetchedData, InfoKind, ReqError, RequestHandler,
};

static TRADE_FETCH_MODE: RwLock<TradeFetchMode> = RwLock::new(TradeFetchMode::Off);
static SERVER_CLIENT: RwLock<Option<ServerClient>> = RwLock::new(None);

/// Maximum trades to request per Arrow IPC call to server.
pub const ARROW_LIMIT: usize = 400_000;

/// Override the global trade-fetch mode at runtime.
pub fn set_trade_fetch_mode(mode: TradeFetchMode) {
    if let Ok(mut guard) = TRADE_FETCH_MODE.write() {
        *guard = mode;
    } else {
        log::error!("Trade fetch mode lock poisoned — resetting to Off");
    }
}

/// Return the current global trade-fetch mode.
pub fn trade_fetch_mode() -> TradeFetchMode {
    TRADE_FETCH_MODE
        .read()
        .map(|g| g.clone())
        .unwrap_or(TradeFetchMode::Off)
}

/// Returns `true` when a trade-fetch source (server or exchange API) is active.
pub fn is_trade_fetch_enabled() -> bool {
    trade_fetch_mode() != TradeFetchMode::Off
}

/// Set active server client for Arrow IPC trade streaming.
pub fn set_server_client(client: Option<ServerClient>) {
    if let Ok(mut guard) = SERVER_CLIENT.write() {
        *guard = client;
    } else {
        log::error!("Server client lock poisoned");
    }
}

/// Retrieve active server client.
pub fn active_server_client() -> Option<ServerClient> {
    SERVER_CLIENT.read().ok().and_then(|g| g.clone())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FetchTaskStatus {
    Loading(InfoKind),
    Completed,
}

#[derive(Debug, Clone)]
pub enum FetchUpdate {
    Status {
        pane_id: Uuid,
        status: FetchTaskStatus,
    },
    Data {
        layout_id: Uuid,
        pane_id: Uuid,
        stream: StreamKind,
        data: FetchedData,
    },
    Error {
        pane_id: Uuid,
        error: String,
        req_id: Option<Uuid>,
    },
}

/// Fetch trades from the configured source using a single forward-paging
/// loop (oldest → newest).
///
/// When `server` is `Some`, historical trades are streamed over the Apache
/// Arrow IPC endpoint (`GET /trades.arrow`) in batches up to `ARROW_LIMIT`.
/// When `server` is `None`, falls back to direct exchange adapters.
///
/// Returns `Ok(u64)` containing the latest trade timestamp seen.
pub fn fetch_trades_paged(
    server: Option<ServerClient>,
    ticker_info: TickerInfo,
    from_time: u64,
    to_time: u64,
    data_path: PathBuf,
) -> impl Straw<u64, Vec<Trade>, AdapterError> {
    sipper(async move |mut progress| {
        let mut cursor = from_time;
        let mut actual_last_trade_t = from_time;

        if server.is_none() && exchange::clock::is_sync_needed(ticker_info.exchange(), 60_000) {
            let _ = exchange::clock::ping_server_time(ticker_info.exchange()).await;
        }

        while cursor < to_time {
            let prev_cursor = cursor;

            if let Some(ref client) = server {
                let parsed = client
                    .fetch_trades_arrow(ticker_info, cursor, to_time, ARROW_LIMIT)
                    .await?;

                let is_empty = parsed.raw_row_count == 0;
                if !is_empty
                    && let Some(last_ts) = parsed.last_ts
                {
                    actual_last_trade_t = actual_last_trade_t.max(last_ts);
                    cursor = last_ts.saturating_add(1);
                }
                if !parsed.trades.is_empty() {
                    progress.send(parsed.trades).await;
                }
                if is_empty {
                    break;
                }
            } else {
                let (mut batch, next_trade_t) = match exchange::trades::fetch_trades(
                    ticker_info,
                    cursor,
                    data_path.clone(),
                )
                .await
                {
                    Ok(res) => res,
                    Err(err) => return Err(err),
                };

                let batch_len = batch.len();
                let now_ms = exchange::clock::server_time_ms(Some(ticker_info.exchange()));
                let is_at_market_tip = cursor >= now_ms.saturating_sub(120_000)
                    || next_trade_t >= now_ms.saturating_sub(120_000);
                if batch_len == 0 && is_at_market_tip {
                    break;
                }

                let had_trades_beyond_to_time = batch.iter().any(|t| t.time >= to_time);
                batch.retain(|t| t.time < to_time);

                if let Some(last_trade) = batch.last() {
                    actual_last_trade_t = actual_last_trade_t.max(last_trade.time);
                }
                if !batch.is_empty() {
                    progress.send(batch).await;
                }

                if had_trades_beyond_to_time || next_trade_t >= to_time || next_trade_t <= cursor {
                    break;
                }

                let sleep_ms = match ticker_info.exchange() {
                    exchange::Exchange::OkexSpot
                    | exchange::Exchange::OkexLinear
                    | exchange::Exchange::OkexInverse => 200,
                    exchange::Exchange::BybitSpot
                    | exchange::Exchange::BybitLinear
                    | exchange::Exchange::BybitInverse => 100,
                    _ => 50,
                };

                if (100..=2000).contains(&batch_len) {
                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                } else if batch_len == 0 {
                    tokio::task::yield_now().await;
                }

                cursor = next_trade_t;
            }

            if cursor <= prev_cursor {
                log::error!(
                    "paging cursor did not advance past {prev_cursor:?} despite a non-empty response; \
                     aborting to avoid an infinite loop (source may be malformed)"
                );
                return Err(AdapterError::ParseError(
                    "Source may be malformed. Check logs for details.".to_string(),
                ));
            }
        }

        Ok(actual_last_trade_t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ex17_trade_fetch_mode_runtime_toggle() {
        set_trade_fetch_mode(TradeFetchMode::Off);
        assert!(!is_trade_fetch_enabled());
        assert_eq!(trade_fetch_mode(), TradeFetchMode::Off);

        set_trade_fetch_mode(TradeFetchMode::Server);
        assert!(is_trade_fetch_enabled());
        assert_eq!(trade_fetch_mode(), TradeFetchMode::Server);

        set_trade_fetch_mode(TradeFetchMode::Exchange);
        assert!(is_trade_fetch_enabled());
        assert_eq!(trade_fetch_mode(), TradeFetchMode::Exchange);

        // Reset to Off
        set_trade_fetch_mode(TradeFetchMode::Off);
    }
}
