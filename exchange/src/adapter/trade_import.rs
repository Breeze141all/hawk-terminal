use super::AdapterError;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImportTradeSide {
    Long,
    Short,
}

impl std::fmt::Display for ImportTradeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Long => write!(f, "LONG"),
            Self::Short => write!(f, "SHORT"),
        }
    }
}

pub fn compute_pnl(
    side: ImportTradeSide,
    entry_price: f64,
    exit_price: f64,
    size: f64,
    fee: f64,
) -> (f64, f64) {
    if entry_price <= 0.0 || size <= 0.0 {
        return (0.0, 0.0);
    }
    let raw_diff = match side {
        ImportTradeSide::Long => exit_price - entry_price,
        ImportTradeSide::Short => entry_price - exit_price,
    };
    let pnl = (raw_diff * size) - fee;
    let pnl_pct = (raw_diff / entry_price) * 100.0;
    (pnl, pnl_pct)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImportExchangeKind {
    #[default]
    Hyperliquid,
    BinanceFutures,
    BinanceSpot,
    BybitLinear,
    BybitSpot,
    OkexSwap,
    OkexSpot,
}

impl ImportExchangeKind {
    pub const ALL: [Self; 7] = [
        Self::Hyperliquid,
        Self::BinanceFutures,
        Self::BinanceSpot,
        Self::BybitLinear,
        Self::BybitSpot,
        Self::OkexSwap,
        Self::OkexSpot,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Hyperliquid => "Hyperliquid (Perps & Spot)",
            Self::BinanceFutures => "Binance (USDT-M Futures)",
            Self::BinanceSpot => "Binance (Spot)",
            Self::BybitLinear => "Bybit (Linear Perpetual)",
            Self::BybitSpot => "Bybit (Spot)",
            Self::OkexSwap => "OKX (Perpetual Swap)",
            Self::OkexSpot => "OKX (Spot)",
        }
    }

    pub fn requires_secret(&self) -> bool {
        !matches!(self, Self::Hyperliquid)
    }

    pub fn requires_passphrase(&self) -> bool {
        matches!(self, Self::OkexSwap | Self::OkexSpot)
    }

    pub fn exchange_name(&self) -> &'static str {
        match self {
            Self::Hyperliquid => "Hyperliquid",
            Self::BinanceFutures => "Binance Linear",
            Self::BinanceSpot => "Binance Spot",
            Self::BybitLinear => "Bybit Linear",
            Self::BybitSpot => "Bybit Spot",
            Self::OkexSwap => "Okex Linear",
            Self::OkexSpot => "Okex Spot",
        }
    }
}

impl std::fmt::Display for ImportExchangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCredentials {
    pub exchange: ImportExchangeKind,
    pub key_or_address: String,
    pub api_secret: String,
    pub passphrase: String,
    pub symbol: Option<String>,
    pub limit: Option<usize>,
}

impl Default for ImportCredentials {
    fn default() -> Self {
        Self {
            exchange: ImportExchangeKind::Hyperliquid,
            key_or_address: String::new(),
            api_secret: String::new(),
            passphrase: String::new(),
            symbol: None,
            limit: Some(100),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawFill {
    pub symbol: String,
    pub timestamp: u64,
    pub is_buy: bool,
    pub price: f64,
    pub size: f64,
    pub fee: f64,
    pub order_id: Option<String>,
    pub trade_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImportedTrade {
    pub exchange: String,
    pub ticker: String,
    pub side: ImportTradeSide,
    pub entry_price: f64,
    pub exit_price: f64,
    pub size: f64,
    pub fee: f64,
    pub pnl: f64,
    pub pnl_percent: f64,
    pub timestamp_open: u64,
    pub timestamp_close: u64,
    pub notes: String,
}

#[derive(Debug, Clone)]
struct OpenLot {
    timestamp: u64,
    price: f64,
    size: f64,
    initial_size: f64,
    fee: f64,
}

/// Reconstructs closed trades from raw exchange fills using FIFO matching.
/// Fills are automatically sorted chronologically (oldest to newest) before processing.
/// Asymptotic complexity: O(N log N) sorting + O(N) single-pass FIFO matching = O(N log N) time, O(N) space.
pub fn reconstruct_trades_fifo(exchange_name: &str, mut fills: Vec<RawFill>) -> Vec<ImportedTrade> {
    fills.sort_by_key(|f| f.timestamp);

    let mut closed_trades = Vec::new();
    // Maps ticker -> (current_position_is_buy, deque of lots)
    let mut inventory: HashMap<String, (bool, VecDeque<OpenLot>)> = HashMap::new();

    for fill in fills {
        if fill.price <= 0.0 || fill.size <= 0.0 {
            continue;
        }

        let symbol = fill.symbol.clone();
        let (is_buy_inv, lots) = inventory
            .entry(symbol.clone())
            .or_insert_with(|| (fill.is_buy, VecDeque::new()));

        if lots.is_empty() {
            *is_buy_inv = fill.is_buy;
            lots.push_back(OpenLot {
                timestamp: fill.timestamp,
                price: fill.price,
                size: fill.size,
                initial_size: fill.size,
                fee: fill.fee,
            });
            continue;
        }

        if *is_buy_inv == fill.is_buy {
            lots.push_back(OpenLot {
                timestamp: fill.timestamp,
                price: fill.price,
                size: fill.size,
                initial_size: fill.size,
                fee: fill.fee,
            });
            continue;
        }

        // Opposing fill: close lots FIFO
        let side = if *is_buy_inv {
            ImportTradeSide::Long
        } else {
            ImportTradeSide::Short
        };

        let mut remaining_to_close = fill.size;
        let mut matched_entry_value = 0.0;
        let mut matched_size_total = 0.0;
        let mut matched_entry_fees = 0.0;
        let mut earliest_open_ts = fill.timestamp;

        while remaining_to_close > 1e-9 && !lots.is_empty() {
            let front = lots.front_mut().unwrap();
            let take_size = remaining_to_close.min(front.size);

            if earliest_open_ts == fill.timestamp || front.timestamp < earliest_open_ts {
                earliest_open_ts = front.timestamp;
            }

            let entry_fee_portion = if front.initial_size > 0.0 {
                front.fee * (take_size / front.initial_size)
            } else {
                0.0
            };

            matched_entry_value += front.price * take_size;
            matched_size_total += take_size;
            matched_entry_fees += entry_fee_portion;

            front.size -= take_size;
            remaining_to_close -= take_size;

            if front.size <= 1e-9 {
                lots.pop_front();
            }
        }

        if matched_size_total > 1e-9 {
            let exit_fee_portion = if fill.size > 0.0 {
                fill.fee * (matched_size_total / fill.size)
            } else {
                0.0
            };
            let total_fee = matched_entry_fees + exit_fee_portion;
            let avg_entry_price = matched_entry_value / matched_size_total;
            let exit_price = fill.price;

            let (pnl, pnl_pct) = compute_pnl(
                side,
                avg_entry_price,
                exit_price,
                matched_size_total,
                total_fee,
            );

            closed_trades.push(ImportedTrade {
                exchange: exchange_name.to_string(),
                ticker: symbol.clone(),
                side,
                entry_price: avg_entry_price,
                exit_price,
                size: matched_size_total,
                fee: total_fee,
                pnl,
                pnl_percent: pnl_pct,
                timestamp_open: earliest_open_ts,
                timestamp_close: fill.timestamp,
                notes: format!(
                    "Imported from {} | Entry: {:.4}, Exit: {:.4}, Size: {:.4}",
                    exchange_name, avg_entry_price, exit_price, matched_size_total
                ),
            });
        }

        // If the opposing fill had greater size than existing inventory, reverse position
        if remaining_to_close > 1e-9 {
            *is_buy_inv = fill.is_buy;
            let remaining_fee = if fill.size > 0.0 {
                fill.fee * (remaining_to_close / fill.size)
            } else {
                0.0
            };
            lots.push_back(OpenLot {
                timestamp: fill.timestamp,
                price: fill.price,
                size: remaining_to_close,
                initial_size: remaining_to_close,
                fee: remaining_fee,
            });
        }
    }

    closed_trades
}

/// Deduplicates imported trades against existing imported trades in O(M + N) time.
pub fn deduplicate_trades(
    existing: &[ImportedTrade],
    incoming: Vec<ImportedTrade>,
) -> (Vec<ImportedTrade>, usize) {
    let mut seen = std::collections::HashSet::new();

    for entry in existing {
        let key = (
            entry.exchange.to_lowercase(),
            entry.ticker.to_uppercase(),
            entry.side,
            entry.timestamp_open,
            entry.timestamp_close,
            (entry.entry_price * 100.0).round() as i64,
            (entry.size * 10000.0).round() as i64,
        );
        seen.insert(key);
    }

    let mut deduplicated = Vec::with_capacity(incoming.len());
    let mut skipped = 0;

    for trade in incoming {
        let key = (
            trade.exchange.to_lowercase(),
            trade.ticker.to_uppercase(),
            trade.side,
            trade.timestamp_open,
            trade.timestamp_close,
            (trade.entry_price * 100.0).round() as i64,
            (trade.size * 10000.0).round() as i64,
        );
        if seen.insert(key) {
            deduplicated.push(trade);
        } else {
            skipped += 1;
        }
    }

    (deduplicated, skipped)
}

fn sign_hmac_sha256_hex(secret: &str, message: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, message.as_bytes());
    hex::encode(signature.as_ref())
}

fn sign_hmac_sha256_base64(secret: &str, message: &str) -> String {
    use base64::Engine;
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.as_ref())
}

/// Fetches trade history from the configured exchange and reconstructs closed trades.
pub async fn fetch_exchange_trades(
    creds: &ImportCredentials,
) -> Result<Vec<ImportedTrade>, AdapterError> {
    let client = &*crate::limiter::HTTP_CLIENT;

    match creds.exchange {
        ImportExchangeKind::Hyperliquid => {
            let address = creds.key_or_address.trim();
            if address.is_empty() {
                return Err(AdapterError::InvalidRequest(
                    "Wallet address is required for Hyperliquid import".to_string(),
                ));
            }

            let body = serde_json::json!({
                "type": "userFills",
                "user": address
            });

            let resp = client
                .post("https://api.hyperliquid.xyz/info")
                .json(&body)
                .send()
                .await
                .map_err(AdapterError::FetchError)?;

            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(AdapterError::InvalidRequest(format!(
                    "Hyperliquid API error {status}: {err_text}"
                )));
            }

            let fills_json: serde_json::Value =
                resp.json().await.map_err(AdapterError::FetchError)?;
            let fills_array = fills_json.as_array().ok_or_else(|| {
                AdapterError::ParseError(
                    "Expected JSON array of userFills from Hyperliquid".to_string(),
                )
            })?;

            let mut raw_fills = Vec::new();
            for item in fills_array {
                let coin = item["coin"].as_str().unwrap_or("UNKNOWN").to_string();
                if let Some(ref sym) = creds.symbol
                    && !sym.trim().is_empty()
                    && !coin.eq_ignore_ascii_case(sym.trim())
                {
                    continue;
                }

                let px = item["px"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let sz = item["sz"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let side_str = item["side"].as_str().unwrap_or("B");
                let is_buy = side_str == "B";
                let time = item["time"].as_u64().unwrap_or(0);
                let fee = item["fee"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let tid = item["tid"].as_u64().map(|id| id.to_string());

                if px > 0.0 && sz > 0.0 {
                    raw_fills.push(RawFill {
                        symbol: coin,
                        timestamp: time,
                        is_buy,
                        price: px,
                        size: sz,
                        fee,
                        order_id: None,
                        trade_id: tid,
                    });
                }
            }

            Ok(reconstruct_trades_fifo(
                creds.exchange.exchange_name(),
                raw_fills,
            ))
        }

        ImportExchangeKind::BinanceFutures | ImportExchangeKind::BinanceSpot => {
            let api_key = creds.key_or_address.trim();
            let secret = creds.api_secret.trim();
            if api_key.is_empty() || secret.is_empty() {
                return Err(AdapterError::InvalidRequest(
                    "API Key and Secret are required for Binance import".to_string(),
                ));
            }

            let symbol = creds
                .symbol
                .as_deref()
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "BTCUSDT".to_string());

            let now_ms = chrono::Utc::now().timestamp_millis();
            let limit = creds.limit.unwrap_or(100).min(1000);

            let is_futures = creds.exchange == ImportExchangeKind::BinanceFutures;
            let (url_base, endpoint) = if is_futures {
                ("https://fapi.binance.com", "/fapi/v1/userTrades")
            } else {
                ("https://api.binance.com", "/api/v3/myTrades")
            };

            let query = format!("symbol={symbol}&limit={limit}&timestamp={now_ms}&recvWindow=5000");
            let signature = sign_hmac_sha256_hex(secret, &query);
            let full_url = format!("{url_base}{endpoint}?{query}&signature={signature}");

            let resp = client
                .get(&full_url)
                .header("X-MBX-APIKEY", api_key)
                .send()
                .await
                .map_err(AdapterError::FetchError)?;

            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(AdapterError::InvalidRequest(format!(
                    "Binance API error {status}: {err_text}"
                )));
            }

            let fills_json: serde_json::Value =
                resp.json().await.map_err(AdapterError::FetchError)?;
            let fills_array = fills_json.as_array().ok_or_else(|| {
                AdapterError::ParseError("Expected JSON array of trades from Binance".to_string())
            })?;

            let mut raw_fills = Vec::new();
            for item in fills_array {
                let sym = item["symbol"].as_str().unwrap_or(&symbol).to_string();
                let px = item["price"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let qty = item["qty"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let is_buy = if is_futures {
                    item["side"]
                        .as_str()
                        .map(|s| s.eq_ignore_ascii_case("BUY"))
                        .unwrap_or(true)
                } else {
                    item["isBuyer"].as_bool().unwrap_or(true)
                };
                let time = item["time"].as_u64().unwrap_or(0);
                let fee = item["commission"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let tid = item["id"].as_i64().map(|id| id.to_string());

                if px > 0.0 && qty > 0.0 {
                    raw_fills.push(RawFill {
                        symbol: sym,
                        timestamp: time,
                        is_buy,
                        price: px,
                        size: qty,
                        fee,
                        order_id: None,
                        trade_id: tid,
                    });
                }
            }

            Ok(reconstruct_trades_fifo(
                creds.exchange.exchange_name(),
                raw_fills,
            ))
        }

        ImportExchangeKind::BybitLinear | ImportExchangeKind::BybitSpot => {
            let api_key = creds.key_or_address.trim();
            let secret = creds.api_secret.trim();
            if api_key.is_empty() || secret.is_empty() {
                return Err(AdapterError::InvalidRequest(
                    "API Key and Secret are required for Bybit import".to_string(),
                ));
            }

            let category = if creds.exchange == ImportExchangeKind::BybitLinear {
                "linear"
            } else {
                "spot"
            };
            let limit = creds.limit.unwrap_or(100).min(100);
            let mut query = format!("category={category}&limit={limit}");
            if let Some(ref sym) = creds.symbol
                && !sym.trim().is_empty()
            {
                query.push_str(&format!("&symbol={}", sym.trim().to_uppercase()));
            }

            let now_ms = chrono::Utc::now().timestamp_millis();
            let recv_window = "5000";
            let sign_payload = format!("{now_ms}{api_key}{recv_window}{query}");
            let signature = sign_hmac_sha256_hex(secret, &sign_payload);

            let full_url = format!("https://api.bybit.com/v5/execution/list?{query}");
            let resp = client
                .get(&full_url)
                .header("X-BAPI-API-KEY", api_key)
                .header("X-BAPI-TIMESTAMP", now_ms.to_string())
                .header("X-BAPI-RECV-WINDOW", recv_window)
                .header("X-BAPI-SIGN", signature)
                .send()
                .await
                .map_err(AdapterError::FetchError)?;

            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(AdapterError::InvalidRequest(format!(
                    "Bybit API error {status}: {err_text}"
                )));
            }

            let data_json: serde_json::Value =
                resp.json().await.map_err(AdapterError::FetchError)?;
            let list = data_json["result"]["list"].as_array().ok_or_else(|| {
                AdapterError::ParseError("Expected result.list array from Bybit".to_string())
            })?;

            let mut raw_fills = Vec::new();
            for item in list {
                let sym = item["symbol"].as_str().unwrap_or("UNKNOWN").to_string();
                let px = item["execPrice"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let sz = item["execQty"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let side_str = item["side"].as_str().unwrap_or("Buy");
                let is_buy = side_str.eq_ignore_ascii_case("Buy");
                let time = item["execTime"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let fee = item["execFee"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let tid = item["execId"].as_str().map(|s| s.to_string());

                if px > 0.0 && sz > 0.0 {
                    raw_fills.push(RawFill {
                        symbol: sym,
                        timestamp: time,
                        is_buy,
                        price: px,
                        size: sz,
                        fee,
                        order_id: None,
                        trade_id: tid,
                    });
                }
            }

            Ok(reconstruct_trades_fifo(
                creds.exchange.exchange_name(),
                raw_fills,
            ))
        }

        ImportExchangeKind::OkexSwap | ImportExchangeKind::OkexSpot => {
            let api_key = creds.key_or_address.trim();
            let secret = creds.api_secret.trim();
            let passphrase = creds.passphrase.trim();
            if api_key.is_empty() || secret.is_empty() || passphrase.is_empty() {
                return Err(AdapterError::InvalidRequest(
                    "API Key, Secret, and Passphrase are required for OKX import".to_string(),
                ));
            }

            let inst_type = if creds.exchange == ImportExchangeKind::OkexSwap {
                "SWAP"
            } else {
                "SPOT"
            };
            let mut query = format!(
                "instType={inst_type}&limit={}",
                creds.limit.unwrap_or(100).min(100)
            );
            if let Some(ref sym) = creds.symbol
                && !sym.trim().is_empty()
            {
                query.push_str(&format!("&instId={}", sym.trim().to_uppercase()));
            }

            let now = chrono::Utc::now();
            let timestamp = now.format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
            let path_and_query = format!("/api/v5/trade/fills-history?{query}");
            let sign_payload = format!("{timestamp}GET{path_and_query}");
            let signature = sign_hmac_sha256_base64(secret, &sign_payload);

            let full_url = format!("https://www.okx.com{path_and_query}");
            let resp = client
                .get(&full_url)
                .header("OK-ACCESS-KEY", api_key)
                .header("OK-ACCESS-SIGN", signature)
                .header("OK-ACCESS-TIMESTAMP", timestamp)
                .header("OK-ACCESS-PASSPHRASE", passphrase)
                .send()
                .await
                .map_err(AdapterError::FetchError)?;

            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(AdapterError::InvalidRequest(format!(
                    "OKX API error {status}: {err_text}"
                )));
            }

            let data_json: serde_json::Value =
                resp.json().await.map_err(AdapterError::FetchError)?;
            let list = data_json["data"].as_array().ok_or_else(|| {
                AdapterError::ParseError("Expected data array from OKX".to_string())
            })?;

            let mut raw_fills = Vec::new();
            for item in list {
                let sym = item["instId"].as_str().unwrap_or("UNKNOWN").to_string();
                let px = item["fillPx"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let sz = item["fillSz"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let side_str = item["side"].as_str().unwrap_or("buy");
                let is_buy = side_str.eq_ignore_ascii_case("buy");
                let time = item["ts"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let fee = item["fee"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|f| f.abs())
                    .unwrap_or(0.0);
                let tid = item["fillId"].as_str().map(|s| s.to_string());

                if px > 0.0 && sz > 0.0 {
                    raw_fills.push(RawFill {
                        symbol: sym,
                        timestamp: time,
                        is_buy,
                        price: px,
                        size: sz,
                        fee,
                        order_id: None,
                        trade_id: tid,
                    });
                }
            }

            Ok(reconstruct_trades_fifo(
                creds.exchange.exchange_name(),
                raw_fills,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_single_long_trade() {
        let fills = vec![
            RawFill {
                symbol: "BTCUSDT".to_string(),
                timestamp: 1000,
                is_buy: true,
                price: 50000.0,
                size: 1.0,
                fee: 10.0,
                order_id: None,
                trade_id: None,
            },
            RawFill {
                symbol: "BTCUSDT".to_string(),
                timestamp: 2000,
                is_buy: false,
                price: 55000.0,
                size: 1.0,
                fee: 11.0,
                order_id: None,
                trade_id: None,
            },
        ];

        let trades = reconstruct_trades_fifo("Binance Linear", fills);
        assert_eq!(trades.len(), 1);
        let t = &trades[0];
        assert_eq!(t.side, ImportTradeSide::Long);
        assert_eq!(t.entry_price, 50000.0);
        assert_eq!(t.exit_price, 55000.0);
        assert_eq!(t.size, 1.0);
        assert_eq!(t.fee, 21.0);
        assert_eq!(t.pnl, 4979.0); // (55000 - 50000)*1.0 - 21.0
        assert_eq!(t.timestamp_open, 1000);
        assert_eq!(t.timestamp_close, 2000);
    }

    #[test]
    fn test_fifo_partial_fills_and_multiple_entries() {
        let fills = vec![
            RawFill {
                symbol: "ETHUSDT".to_string(),
                timestamp: 100,
                is_buy: true,
                price: 2000.0,
                size: 2.0,
                fee: 2.0,
                order_id: None,
                trade_id: None,
            },
            RawFill {
                symbol: "ETHUSDT".to_string(),
                timestamp: 200,
                is_buy: true,
                price: 3000.0,
                size: 2.0,
                fee: 3.0,
                order_id: None,
                trade_id: None,
            },
            RawFill {
                symbol: "ETHUSDT".to_string(),
                timestamp: 300,
                is_buy: false,
                price: 2500.0,
                size: 3.0,
                fee: 4.5,
                order_id: None,
                trade_id: None,
            },
        ];

        let trades = reconstruct_trades_fifo("Hyperliquid", fills);
        assert_eq!(trades.len(), 1);
        let t = &trades[0];
        assert_eq!(t.side, ImportTradeSide::Long);
        assert_eq!(t.size, 3.0);
        assert!((t.entry_price - 2333.3333333333335).abs() < 1e-6);
        assert_eq!(t.exit_price, 2500.0);
        assert!((t.fee - 8.0).abs() < 1e-6);
        assert!((t.pnl - 492.0).abs() < 1e-6);
        assert_eq!(t.timestamp_open, 100);
        assert_eq!(t.timestamp_close, 300);
    }

    #[test]
    fn test_fifo_position_reversal() {
        let fills = vec![
            RawFill {
                symbol: "BTCUSDT".to_string(),
                timestamp: 10,
                is_buy: true,
                price: 50000.0,
                size: 1.0,
                fee: 1.0,
                order_id: None,
                trade_id: None,
            },
            RawFill {
                symbol: "BTCUSDT".to_string(),
                timestamp: 20,
                is_buy: false,
                price: 60000.0,
                size: 2.0,
                fee: 2.0,
                order_id: None,
                trade_id: None,
            },
            RawFill {
                symbol: "BTCUSDT".to_string(),
                timestamp: 30,
                is_buy: true,
                price: 55000.0,
                size: 1.0,
                fee: 1.0,
                order_id: None,
                trade_id: None,
            },
        ];

        let trades = reconstruct_trades_fifo("Bybit Linear", fills);
        assert_eq!(trades.len(), 2);

        // Trade 1: Long
        assert_eq!(trades[0].side, ImportTradeSide::Long);
        assert_eq!(trades[0].size, 1.0);
        assert_eq!(trades[0].entry_price, 50000.0);
        assert_eq!(trades[0].exit_price, 60000.0);
        assert_eq!(trades[0].fee, 2.0);
        assert_eq!(trades[0].pnl, 9998.0);

        // Trade 2: Short
        assert_eq!(trades[1].side, ImportTradeSide::Short);
        assert_eq!(trades[1].size, 1.0);
        assert_eq!(trades[1].entry_price, 60000.0);
        assert_eq!(trades[1].exit_price, 55000.0);
        assert_eq!(trades[1].fee, 2.0);
        assert_eq!(trades[1].pnl, 4998.0);
    }

    #[test]
    fn test_deduplicate_trades() {
        let incoming = vec![
            ImportedTrade {
                exchange: "Binance Linear".to_string(),
                ticker: "BTCUSDT".to_string(),
                side: ImportTradeSide::Long,
                entry_price: 50000.0,
                exit_price: 55000.0,
                size: 1.0,
                fee: 10.0,
                pnl: 4990.0,
                pnl_percent: 10.0,
                timestamp_open: 1000,
                timestamp_close: 2000,
                notes: "".to_string(),
            },
            ImportedTrade {
                exchange: "Binance Linear".to_string(),
                ticker: "ETHUSDT".to_string(),
                side: ImportTradeSide::Short,
                entry_price: 3000.0,
                exit_price: 2800.0,
                size: 5.0,
                fee: 5.0,
                pnl: 995.0,
                pnl_percent: 6.6,
                timestamp_open: 1500,
                timestamp_close: 2500,
                notes: "".to_string(),
            },
        ];

        let existing = vec![incoming[0].clone()];

        let (deduped, skipped) = deduplicate_trades(&existing, incoming);
        assert_eq!(skipped, 1);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].ticker, "ETHUSDT");
    }
}
