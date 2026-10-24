use crate::{
    Kline, Price, PushFrequency, SizeUnit, StreamKind, Ticker, TickerInfo, TickerStats, Timeframe,
    Trade,
    adapter::{AdapterError, Event, Exchange, MarketKind, StreamTicksize},
    connect::{State, connect_ws},
    depth::{DeOrder, DepthPayload, DepthUpdate, LocalDepthCache},
    is_symbol_supported,
    limiter::{FixedWindowBucket, RateLimiter, http_request_with_limiter},
    volume_size_unit,
};

use fastwebsockets::{FragmentCollector, Frame, OpCode};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use iced_futures::{
    futures::{SinkExt, Stream},
    stream,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use sonic_rs::{JsonValueTrait, to_object_iter_unchecked};

use std::{collections::HashMap, sync::LazyLock, time::Duration};
use tokio::sync::Mutex;

const FETCH_DOMAIN: &str = "https://api.mexc.com/api";
const MEXC_FUTURES_WS_DOMAIN: &str = "contract.mexc.com";
const MEXC_FUTURES_WS_PATH: &str = "/edge";

const PING_INTERVAL_SECS: u64 = 15;

const LIMIT: usize = 20;
const REFILL_RATE: Duration = Duration::from_secs(2);

pub struct MexcLimiter {
    bucket: FixedWindowBucket,
}

impl MexcLimiter {
    pub fn new(limit: usize, refill_rate: Duration) -> Self {
        Self {
            bucket: FixedWindowBucket::new(limit, refill_rate),
        }
    }
}

impl RateLimiter for MexcLimiter {
    fn prepare_request(&mut self, weight: usize) -> Option<Duration> {
        self.bucket.calculate_wait_time(weight)
    }

    fn update_from_response(&mut self, _response: &reqwest::Response, _weight: usize) {}

    fn should_exit_on_response(&self, response: &reqwest::Response) -> bool {
        response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
    }
}

static MEXC_LIMITER: LazyLock<Mutex<MexcLimiter>> =
    LazyLock::new(|| Mutex::new(MexcLimiter::new(LIMIT, REFILL_RATE)));

#[derive(Deserialize)]
struct DepthSnapshotResponse {
    #[serde(rename = "data")]
    data: DepthData,
}

#[derive(Deserialize)]
struct DepthData {
    #[serde(rename = "asks")]
    asks: Vec<DeOrder>,
    #[serde(rename = "bids")]
    bids: Vec<DeOrder>,
    #[serde(rename = "version")]
    version: u64,
    #[serde(rename = "timestamp")]
    timestamp: u64,
}

#[derive(Deserialize, Debug)]
struct SonicTrade {
    #[serde(rename = "p")]
    pub price: f32,
    #[serde(rename = "v")]
    pub qty: f32,
    #[serde(rename = "T")]
    pub direction: u8, // 1 = buy, 2 = sell
    #[serde(rename = "t")]
    pub time: u64,
}

#[derive(Deserialize)]
struct FuturesDepthItem {
    pub price: f32,
    pub qty: f32,
    #[allow(dead_code)]
    pub order_count: Option<f32>,
}

#[derive(Deserialize)]
struct SonicDepth {
    #[serde(rename = "asks")]
    pub asks: Vec<FuturesDepthItem>,
    #[serde(rename = "bids")]
    pub bids: Vec<FuturesDepthItem>,
    #[serde(rename = "version")]
    pub version: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SonicKline {
    #[serde(rename = "t")]
    pub time: u64,
    #[serde(rename = "o")]
    pub open: f32,
    #[serde(rename = "h")]
    pub high: f32,
    #[serde(rename = "l")]
    pub low: f32,
    #[serde(rename = "c")]
    pub close: f32,
    #[serde(rename = "q")]
    pub quote_volume: Option<f32>,
    #[serde(rename = "a")]
    pub amount: Option<f32>,
    #[serde(rename = "interval")]
    pub interval: Option<String>,
    #[serde(rename = "symbol")]
    pub symbol: Option<String>,
}

enum StreamData {
    Trade(Ticker, Vec<SonicTrade>, u64),
    Depth(SonicDepth, u64),
    Kline(Ticker, Vec<SonicKline>),
    Pong(#[allow(dead_code)] u64),
    Subscription(String),
}

enum StreamName {
    Depth,
    Trade,
    Kline,
    Subscription(String),
    Error,
    Pong,
    Unknown,
}

impl StreamName {
    fn from_topic(topic: &str) -> Self {
        let parts: Vec<&str> = topic.split('.').collect();

        if parts.first() == Some(&"pong") {
            return StreamName::Pong;
        }

        match parts.get(1) {
            Some(&"sub") => {
                StreamName::Subscription(parts.get(2).map(|s| s.to_string()).unwrap_or_default())
            }
            Some(&"deal") => StreamName::Trade,
            Some(&"depth") => StreamName::Depth,
            Some(&"kline") => StreamName::Kline,
            Some(&"error") => StreamName::Error,
            _ => StreamName::Unknown,
        }
    }
}

pub fn exchange_from_market_type(market: MarketKind) -> Exchange {
    match market {
        MarketKind::Spot => Exchange::MexcSpot,
        MarketKind::LinearPerps => Exchange::MexcLinear,
        MarketKind::InversePerps => Exchange::MexcInverse,
    }
}

pub fn convert_to_mexc_timeframe(timeframe: Timeframe, market: MarketKind) -> Option<&'static str> {
    if market == MarketKind::Spot {
        match timeframe {
            Timeframe::M1 => Some("1m"),
            Timeframe::M5 => Some("5m"),
            Timeframe::M15 => Some("15m"),
            Timeframe::M30 => Some("30m"),
            Timeframe::H1 => Some("60m"),
            Timeframe::H4 => Some("4h"),
            Timeframe::D1 => Some("1d"),
            Timeframe::W1 => Some("1W"),
            Timeframe::MN1 => Some("1M"),
            _ => None,
        }
    } else {
        match timeframe {
            Timeframe::M1 => Some("Min1"),
            Timeframe::M5 => Some("Min5"),
            Timeframe::M15 => Some("Min15"),
            Timeframe::M30 => Some("Min30"),
            Timeframe::H1 => Some("Min60"),
            Timeframe::H4 => Some("Hour4"),
            Timeframe::D1 => Some("Day1"),
            Timeframe::W1 => Some("Week1"),
            Timeframe::MN1 => Some("Month1"),
            _ => None,
        }
    }
}

pub async fn fetch_ticksize(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    let mut ticker_info_map = HashMap::new();
    let url = match market_type {
        MarketKind::Spot => format!("{FETCH_DOMAIN}/v3/exchangeInfo"),
        MarketKind::InversePerps | MarketKind::LinearPerps => {
            format!("{FETCH_DOMAIN}/v1/contract/detail")
        }
    };

    let response_text =
        http_request_with_limiter(&url, &MEXC_LIMITER, 1, Some(Method::GET), None).await?;

    let exchange_info: Value = sonic_rs::from_str(&response_text).map_err(|e| {
        AdapterError::ParseError(format!("Failed to parse MEXC exchange info: {e}"))
    })?;

    match market_type {
        MarketKind::Spot => {
            let exchange = exchange_from_market_type(market_type);

            let symbols = exchange_info["symbols"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Missing symbols array".to_string()))?;

            for item in symbols {
                // status: 1 - online, 2 - Pause, 3 - offline
                if let Some(status) = item["status"].as_str()
                    && status != "1"
                    && status != "2"
                {
                    continue;
                }

                let symbol_str = item["symbol"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Missing symbol".to_string()))?;

                if !is_symbol_supported(symbol_str, exchange, true) {
                    continue;
                }

                if let Some(quote_asset) = item["quoteAsset"].as_str()
                    && quote_asset != "USDT"
                    && quote_asset != "USDC"
                    && quote_asset != "USD"
                {
                    continue;
                }

                let min_qty = item["baseSizePrecision"]
                    .as_str()
                    .ok_or_else(|| {
                        AdapterError::ParseError("Missing baseSizePrecision".to_string())
                    })?
                    .parse::<f32>()
                    .map_err(|_| {
                        AdapterError::ParseError("Failed to parse baseSizePrecision".to_string())
                    })?;

                let quote_asset_precision =
                    item["quoteAssetPrecision"].as_i64().ok_or_else(|| {
                        AdapterError::ParseError("Missing quoteAssetPrecision".to_string())
                    })?;

                let min_ticksize = 10f32.powi(-quote_asset_precision as i32);

                let ticker = Ticker::new(symbol_str, exchange);
                let info = TickerInfo::new(ticker, min_ticksize, min_qty, None);
                ticker_info_map.insert(ticker, Some(info));
            }
        }
        MarketKind::LinearPerps | MarketKind::InversePerps => {
            let result_list: &Vec<Value> = exchange_info["data"].as_array().ok_or_else(|| {
                AdapterError::ParseError("Result list is not an array".to_string())
            })?;

            for item in result_list {
                // Status: 0 enabled, 1 delivery, 2 delivered, 3 offline, 4 paused
                if let Some(state) = item["state"].as_i64()
                    && state != 0
                {
                    continue;
                }

                let symbol = item["symbol"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Missing symbol".to_string()))?;

                let Some(quote_asset) = item["quoteCoin"].as_str() else {
                    return Err(AdapterError::ParseError("Missing quoteCoin".to_string()));
                };

                if quote_asset != "USDT" && quote_asset != "USD" && quote_asset != "USDC" {
                    continue;
                }

                let settle_asset = item["settleCoin"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Missing settleCoin".to_string()))?;

                let base_asset = item["baseCoin"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Missing baseCoin".to_string()))?;

                let item_market_kind = if settle_asset == base_asset {
                    MarketKind::InversePerps
                } else if settle_asset == quote_asset {
                    MarketKind::LinearPerps
                } else {
                    return Err(AdapterError::ParseError(
                        "Unknown contract type".to_string(),
                    ));
                };

                if item_market_kind != market_type {
                    continue;
                }

                let exchange = exchange_from_market_type(item_market_kind);

                if !is_symbol_supported(symbol, exchange, true) {
                    continue;
                }

                let min_qty_contracts = item["minVol"].as_f64().ok_or_else(|| {
                    AdapterError::ParseError("Missing minVol (min_qty)".to_string())
                })? as f32;

                let min_ticksize = item["priceUnit"].as_f64().ok_or_else(|| {
                    AdapterError::ParseError("Missing priceUnit (ticksize)".to_string())
                })? as f32;

                let contract_size = item["contractSize"]
                    .as_f64()
                    .ok_or_else(|| AdapterError::ParseError("Missing contractSize".to_string()))?
                    as f32;

                let min_qty = min_qty_contracts * contract_size;

                let ticker = Ticker::new(symbol, exchange);
                let info = TickerInfo::new(ticker, min_ticksize, min_qty, Some(contract_size));
                ticker_info_map.insert(ticker, Some(info));
            }
        }
    }

    Ok(ticker_info_map)
}

pub async fn fetch_ticker_prices(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, TickerStats>, AdapterError> {
    let exchange = exchange_from_market_type(market_type);
    let mut ticker_prices_map = HashMap::new();

    let url = match market_type {
        MarketKind::Spot => format!("{FETCH_DOMAIN}/v3/ticker/24hr"),
        MarketKind::InversePerps | MarketKind::LinearPerps => {
            format!("{FETCH_DOMAIN}/v1/contract/ticker")
        }
    };

    let response_text =
        http_request_with_limiter(&url, &MEXC_LIMITER, 1, Some(Method::GET), None).await?;

    let parsed_response: Value =
        sonic_rs::from_str(&response_text).map_err(|e| AdapterError::ParseError(e.to_string()))?;

    match market_type {
        MarketKind::Spot => {
            let result_list: &Vec<Value> = parsed_response
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Data is not an array".to_string()))?;

            for item in result_list {
                let symbol = item["symbol"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Symbol not found".to_string()))?;

                if !is_symbol_supported(symbol, exchange, false) {
                    continue;
                }

                if !symbol.ends_with("USDT")
                    && !symbol.ends_with("USD")
                    && !symbol.ends_with("USDC")
                {
                    continue;
                }

                let last_price = item["lastPrice"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Last price not found".to_string()))?
                    .parse::<f32>()
                    .map_err(|e| {
                        AdapterError::ParseError(format!("Failed to parse last price: {e}"))
                    })?;

                let price_change_percent = item["priceChangePercent"]
                    .as_str()
                    .ok_or_else(|| {
                        AdapterError::ParseError("Price change percent not found".to_string())
                    })?
                    .parse::<f32>()
                    .map_err(|e| {
                        AdapterError::ParseError(format!(
                            "Failed to parse price change percent: {e}"
                        ))
                    })?;

                let volume = item["volume"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Volume not found".to_string()))?
                    .parse::<f32>()
                    .map_err(|e| {
                        AdapterError::ParseError(format!("Failed to parse volume: {e}"))
                    })?;

                let volume_in_usd = if let Some(qv) = item["quoteVolume"].as_str() {
                    qv.parse::<f32>().unwrap_or(volume * last_price)
                } else {
                    volume * last_price
                };

                let daily_price_chg = price_change_percent * 100.0;

                let ticker_stats = TickerStats {
                    mark_price: last_price,
                    daily_price_chg,
                    daily_volume: volume_in_usd,
                };

                ticker_prices_map.insert(Ticker::new(symbol, exchange), ticker_stats);
            }
        }
        MarketKind::LinearPerps | MarketKind::InversePerps => {
            let result_list: &Vec<Value> = parsed_response["data"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Data is not an array".to_string()))?;

            for item in result_list {
                let symbol = item["symbol"]
                    .as_str()
                    .ok_or_else(|| AdapterError::ParseError("Symbol not found".to_string()))?;

                if !is_symbol_supported(symbol, exchange, false) {
                    continue;
                }

                let ticker = Ticker::new(symbol, exchange);

                let last_price = item["lastPrice"]
                    .as_f64()
                    .ok_or_else(|| AdapterError::ParseError("Last price not found".to_string()))?
                    as f32;

                let rise_fall_rate = item["riseFallRate"]
                    .as_f64()
                    .ok_or_else(|| AdapterError::ParseError("Missing riseFallRate".to_string()))?
                    as f32;

                let volume_24 = item["volume24"]
                    .as_f64()
                    .ok_or_else(|| AdapterError::ParseError("Missing volume24".to_string()))?
                    as f32;

                let volume_in_usd = volume_24 * last_price;
                let daily_price_chg = rise_fall_rate * 100.0;

                let ticker_stats = TickerStats {
                    mark_price: last_price,
                    daily_price_chg,
                    daily_volume: volume_in_usd,
                };

                ticker_prices_map.insert(ticker, ticker_stats);
            }
        }
    }

    Ok(ticker_prices_map)
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct FuturesApiResponse {
    success: bool,
    code: u8,
    data: Value,
}

#[derive(Deserialize, Debug)]
struct KlineSpot {
    #[allow(dead_code)]
    open_ts: u64,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
    vol: f32,
    close_ts: u64,
    #[allow(dead_code)]
    asset_vol: f32,
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let ticker = ticker_info.ticker;

    let (symbol_str, market_type) = ticker.to_full_symbol_and_type();
    let timeframe_str = convert_to_mexc_timeframe(timeframe, market_type).ok_or_else(|| {
        AdapterError::InvalidRequest(format!(
            "Unsupported MEXC kline timeframe {timeframe} for {market_type}"
        ))
    })?;

    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
    let contract_size = ticker_info.effective_contract_size().unwrap_or(1.0);

    let mut url = match market_type {
        MarketKind::Spot => format!(
            "{FETCH_DOMAIN}/v3/klines?symbol={}&interval={}",
            symbol_str.to_uppercase(),
            timeframe_str
        ),
        MarketKind::LinearPerps | MarketKind::InversePerps => format!(
            "{FETCH_DOMAIN}/v1/contract/kline/{}?interval={}",
            symbol_str.to_uppercase(),
            timeframe_str
        ),
    };

    if let Some((start_ms, end_ms)) = range {
        if start_ms >= end_ms {
            return Ok(Vec::new());
        }
        match market_type {
            MarketKind::Spot => {
                url.push_str(&format!("&startTime={}&endTime={}", start_ms, end_ms));
            }
            MarketKind::LinearPerps | MarketKind::InversePerps => {
                let start_sec = start_ms / 1000;
                let end_sec = end_ms / 1000;
                url.push_str(&format!("&start={}&end={}", start_sec, end_sec));
            }
        }
    }

    let response_text =
        http_request_with_limiter(&url, &MEXC_LIMITER, 1, Some(Method::GET), None).await?;

    let klines_result: Result<Vec<Kline>, AdapterError> = match market_type {
        MarketKind::Spot => {
            let parsed_response: Vec<KlineSpot> = sonic_rs::from_str(&response_text)
                .map_err(|e| AdapterError::ParseError(e.to_string()))?;

            let klines: Vec<Kline> = parsed_response
                .into_iter()
                .map(|kline| {
                    let volume = if size_in_quote_ccy {
                        kline.vol * kline.close
                    } else {
                        kline.vol
                    };
                    Kline::new(
                        kline.close_ts,
                        kline.open,
                        kline.high,
                        kline.low,
                        kline.close,
                        (-1.0, volume),
                        ticker_info.min_ticksize,
                    )
                })
                .collect();
            Ok(klines)
        }
        MarketKind::LinearPerps | MarketKind::InversePerps => {
            let parsed_response: FuturesApiResponse = sonic_rs::from_str(&response_text)
                .map_err(|e| AdapterError::ParseError(e.to_string()))?;

            let data = &parsed_response.data;
            let times = data["time"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Time array not found".to_string()))?;
            let opens = data["open"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Open array not found".to_string()))?;
            let highs = data["high"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("High array not found".to_string()))?;
            let lows = data["low"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Low array not found".to_string()))?;
            let closes = data["close"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Close array not found".to_string()))?;
            let volumes = data["vol"]
                .as_array()
                .ok_or_else(|| AdapterError::ParseError("Vol array not found".to_string()))?;

            (0..times.len())
                .map(|i| {
                    let timestamp = times[i].as_u64().ok_or_else(|| {
                        AdapterError::ParseError("Time value not found".to_string())
                    })? * 1000;

                    let open = opens[i].as_f64().ok_or_else(|| {
                        AdapterError::ParseError("Open value not found".to_string())
                    })? as f32;
                    let high = highs[i].as_f64().ok_or_else(|| {
                        AdapterError::ParseError("High value not found".to_string())
                    })? as f32;
                    let low = lows[i].as_f64().ok_or_else(|| {
                        AdapterError::ParseError("Low value not found".to_string())
                    })? as f32;
                    let close = closes[i].as_f64().ok_or_else(|| {
                        AdapterError::ParseError("Close value not found".to_string())
                    })? as f32;
                    let vol = volumes[i].as_f64().ok_or_else(|| {
                        AdapterError::ParseError("Vol value not found".to_string())
                    })? as f32;

                    let normalized_vol = if size_in_quote_ccy {
                        vol * contract_size * close
                    } else {
                        vol * contract_size
                    };

                    Ok(Kline::new(
                        timestamp,
                        open,
                        high,
                        low,
                        close,
                        (-1.0, normalized_vol),
                        ticker_info.min_ticksize,
                    ))
                })
                .collect()
        }
    };

    klines_result
}

pub async fn fetch_depth_snapshot(symbol: &str) -> Result<DepthPayload, AdapterError> {
    let url = format!("{FETCH_DOMAIN}/v1/contract/depth/{symbol}?limit=1000");
    let response_text =
        http_request_with_limiter(&url, &MEXC_LIMITER, 1, Some(Method::GET), None).await?;

    let snapshot: DepthSnapshotResponse = sonic_rs::from_str(&response_text).map_err(|e| {
        log::error!("Failed to parse MEXC depth snapshot: {}", e);
        AdapterError::ParseError(e.to_string())
    })?;

    Ok(DepthPayload {
        last_update_id: snapshot.data.version,
        time: snapshot.data.timestamp,
        bids: snapshot.data.bids,
        asks: snapshot.data.asks,
    })
}

async fn send_ping(
    websocket: &mut FragmentCollector<TokioIo<Upgraded>>,
) -> Result<(), &'static str> {
    let ping_msg = json!({"method": "ping"});
    if websocket
        .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
            ping_msg.to_string().as_bytes(),
        )))
        .await
        .is_err()
    {
        log::error!("Failed to send MEXC ping");
        return Err("Failed to send ping");
    }
    Ok(())
}

fn feed_de(
    slice: &[u8],
    ticker: Option<Ticker>,
    market_type: MarketKind,
) -> Result<StreamData, AdapterError> {
    let mut stream_type: Option<StreamName> = None;
    let mut ts: Option<u64> = None;
    let mut data_faststr: Option<sonic_rs::FastStr> = None;

    let iter: sonic_rs::ObjectJsonIter = unsafe { to_object_iter_unchecked(slice) };
    let mut topic_ticker: Option<Ticker> = ticker;

    for elem in iter {
        let (k, v) = elem.map_err(|e| AdapterError::ParseError(e.to_string()))?;

        if k == "channel" {
            if let Some(val) = v.as_str() {
                stream_type = Some(StreamName::from_topic(val));
            }
        } else if k == "data" {
            data_faststr = Some(v.as_raw_faststr().clone());
        } else if k == "ts" {
            ts = v.as_u64();
        } else if k == "symbol"
            && let Some(ticker_str) = v.as_str()
            && topic_ticker.is_none()
        {
            topic_ticker = Some(Ticker::new(
                ticker_str,
                exchange_from_market_type(market_type),
            ));
        }
    }

    if let Some(data) = data_faststr {
        match stream_type {
            Some(StreamName::Kline) => {
                let mut kline_data: SonicKline = sonic_rs::from_str(&data)
                    .map_err(|e| AdapterError::ParseError(e.to_string()))?;
                kline_data.time *= 1000;
                let sym = kline_data.symbol.as_deref().unwrap_or("");
                let ticker = topic_ticker
                    .unwrap_or_else(|| Ticker::new(sym, exchange_from_market_type(market_type)));
                return Ok(StreamData::Kline(ticker, vec![kline_data]));
            }
            Some(StreamName::Trade) => {
                let deals_data: Vec<SonicTrade> = sonic_rs::from_str(&data)
                    .map_err(|e| AdapterError::ParseError(e.to_string()))?;
                let trade_ticker = topic_ticker.ok_or_else(|| {
                    AdapterError::ParseError("Missing ticker for trade data".to_string())
                })?;
                return Ok(StreamData::Trade(
                    trade_ticker,
                    deals_data,
                    ts.unwrap_or_default(),
                ));
            }
            Some(StreamName::Depth) => {
                let depth: SonicDepth = sonic_rs::from_str(&data)
                    .map_err(|e| AdapterError::ParseError(e.to_string()))?;
                return Ok(StreamData::Depth(depth, ts.unwrap_or_default()));
            }
            Some(StreamName::Pong) => {
                return Ok(StreamData::Pong(ts.unwrap_or_default()));
            }
            Some(StreamName::Subscription(name)) => {
                return Ok(StreamData::Subscription(name));
            }
            Some(StreamName::Error) => {
                log::error!("MEXC WS Error: {data}");
            }
            _ => {}
        }
    }

    Err(AdapterError::ParseError("Unknown MEXC WS data".to_string()))
}

pub fn connect_market_stream(
    ticker_info: TickerInfo,
    push_freq: PushFrequency,
) -> impl Stream<Item = Event> {
    stream::channel(100, async move |mut output| {
        let mut state: State = State::Disconnected;

        let ticker = ticker_info.ticker;
        let (symbol_str, market_type) = ticker.to_full_symbol_and_type();
        let exchange = exchange_from_market_type(market_type);

        let mut orderbook = LocalDepthCache::default();
        let mut trades_buffer: Vec<Trade> = Vec::new();

        let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
        let contract_size = ticker_info.effective_contract_size().unwrap_or(1.0);

        let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
        let mut snapshot_time: u64 = 0;

        loop {
            match &mut state {
                State::Disconnected => {
                    let url = format!("wss://{MEXC_FUTURES_WS_DOMAIN}{MEXC_FUTURES_WS_PATH}");
                    match connect_ws(MEXC_FUTURES_WS_DOMAIN, &url).await {
                        Ok(mut websocket) => {
                            let depth_sub = json!({
                                "method": "sub.depth",
                                "param": { "symbol": symbol_str }
                            });
                            let deal_sub = json!({
                                "method": "sub.deal",
                                "param": { "symbol": symbol_str }
                            });

                            let res1 = websocket
                                .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                    depth_sub.to_string().as_bytes(),
                                )))
                                .await;

                            let res2 = websocket
                                .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                    deal_sub.to_string().as_bytes(),
                                )))
                                .await;

                            if res1.is_err() || res2.is_err() {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                continue;
                            }

                            // Pull initial depth snapshot via REST
                            if let Ok(snapshot) = fetch_depth_snapshot(&symbol_str).await {
                                snapshot_time = snapshot.time;
                                orderbook.update(
                                    DepthUpdate::Snapshot(snapshot),
                                    ticker_info.min_ticksize,
                                );
                            }

                            let _ = output.send(Event::Connected(exchange)).await;
                            state = State::Connected(websocket);
                        }
                        Err(e) => {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            let _ = output
                                .send(Event::Disconnected(exchange, e.to_string()))
                                .await;
                        }
                    }
                }
                State::Connected(websocket) => {
                    tokio::select! {
                        _ = ping_interval.tick() => {
                            if send_ping(websocket).await.is_err() {
                                state = State::Disconnected;
                            }
                        }
                        msg = websocket.read_frame() => {
                            match msg {
                                Ok(msg) => match msg.opcode {
                                    OpCode::Text => {
                                        if let Ok(data) = feed_de(&msg.payload[..], Some(ticker), market_type) {
                                            match data {
                                                StreamData::Pong(_) => {}
                                                StreamData::Subscription(stream_name) => {
                                                    if stream_name == "depth"
                                                        && snapshot_time == 0
                                                        && let Ok(snapshot) = fetch_depth_snapshot(&symbol_str).await
                                                    {
                                                        snapshot_time = snapshot.time;
                                                        orderbook.update(
                                                            DepthUpdate::Snapshot(snapshot),
                                                            ticker_info.min_ticksize,
                                                        );
                                                    }
                                                }
                                                StreamData::Trade(_trade_ticker, deals, _ts) => {
                                                    for deal in &deals {
                                                        let price = Price::from_f32(deal.price)
                                                            .round_to_min_tick(ticker_info.min_ticksize);
                                                        let qty = if size_in_quote_ccy {
                                                            deal.qty * contract_size * deal.price
                                                        } else {
                                                            deal.qty * contract_size
                                                        };

                                                        trades_buffer.push(Trade {
                                                            time: deal.time,
                                                            is_sell: deal.direction == 2,
                                                            price,
                                                            qty,
                                                        });
                                                    }
                                                }
                                                StreamData::Depth(de_depth, time) => {
                                                    if time < snapshot_time {
                                                        continue;
                                                    }
                                                    let depth = DepthPayload {
                                                        last_update_id: de_depth.version,
                                                        time,
                                                        bids: de_depth
                                                            .bids
                                                            .iter()
                                                            .map(|x| DeOrder {
                                                                price: x.price,
                                                                qty: if size_in_quote_ccy {
                                                                    x.qty * contract_size * x.price
                                                                } else {
                                                                    x.qty * contract_size
                                                                },
                                                            })
                                                            .collect(),
                                                        asks: de_depth
                                                            .asks
                                                            .iter()
                                                            .map(|x| DeOrder {
                                                                price: x.price,
                                                                qty: if size_in_quote_ccy {
                                                                    x.qty * contract_size * x.price
                                                                } else {
                                                                    x.qty * contract_size
                                                                },
                                                            })
                                                            .collect(),
                                                    };

                                                    orderbook.update(
                                                        DepthUpdate::Diff(depth),
                                                        ticker_info.min_ticksize,
                                                    );

                                                    let _ = output
                                                        .send(Event::DepthReceived(
                                                            StreamKind::DepthAndTrades {
                                                                ticker_info,
                                                                depth_aggr: StreamTicksize::Client,
                                                                push_freq,
                                                            },
                                                            time,
                                                            orderbook.depth.clone(),
                                                            std::mem::take(&mut trades_buffer)
                                                                .into_boxed_slice(),
                                                        ))
                                                        .await;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    OpCode::Close => {
                                        let _ = output
                                            .send(Event::Disconnected(
                                                exchange,
                                                "Connection closed".to_string(),
                                            ))
                                            .await;
                                        state = State::Disconnected;
                                    }
                                    _ => {}
                                },
                                Err(e) => {
                                    let _ = output
                                        .send(Event::Disconnected(
                                            exchange,
                                            format!("Websocket error: {e:?}"),
                                        ))
                                        .await;
                                    state = State::Disconnected;
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

pub fn connect_kline_stream(
    streams: Vec<(TickerInfo, Timeframe)>,
    market_type: MarketKind,
) -> impl Stream<Item = Event> {
    stream::channel(100, async move |mut output| {
        let mut state = State::Disconnected;
        let exchange = exchange_from_market_type(market_type);
        let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;

        let ticker_info_map: HashMap<Ticker, TickerInfo> = streams
            .iter()
            .map(|(info, _)| (info.ticker, *info))
            .collect();

        let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));

        loop {
            match &mut state {
                State::Disconnected => {
                    let url = format!("wss://{MEXC_FUTURES_WS_DOMAIN}{MEXC_FUTURES_WS_PATH}");
                    match connect_ws(MEXC_FUTURES_WS_DOMAIN, &url).await {
                        Ok(mut websocket) => {
                            for (ticker_info, timeframe) in &streams {
                                let symbol = ticker_info.ticker.to_full_symbol_and_type().0;
                                let Some(interval) =
                                    convert_to_mexc_timeframe(*timeframe, market_type)
                                else {
                                    continue;
                                };

                                let sub_msg = json!({
                                    "method": "sub.kline",
                                    "param": {
                                        "symbol": symbol.to_uppercase(),
                                        "interval": interval,
                                    }
                                });

                                let _ = websocket
                                    .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                        sub_msg.to_string().as_bytes(),
                                    )))
                                    .await;
                            }

                            let _ = output.send(Event::Connected(exchange)).await;
                            state = State::Connected(websocket);
                        }
                        Err(e) => {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            let _ = output
                                .send(Event::Disconnected(exchange, e.to_string()))
                                .await;
                        }
                    }
                }
                State::Connected(websocket) => {
                    tokio::select! {
                        _ = ping_interval.tick() => {
                            if send_ping(websocket).await.is_err() {
                                state = State::Disconnected;
                            }
                        }
                        msg = websocket.read_frame() => {
                            match msg {
                                Ok(msg) => match msg.opcode {
                                    OpCode::Text => {
                                        if let Ok(StreamData::Kline(ticker, sonic_klines)) =
                                            feed_de(&msg.payload[..], None, market_type)
                                            && let Some(&ticker_info) = ticker_info_map.get(&ticker)
                                        {
                                            let contract_size = ticker_info
                                                .effective_contract_size()
                                                .unwrap_or(1.0);

                                            for k in sonic_klines {
                                                let vol = k.amount.unwrap_or(0.0);
                                                let normalized_vol = if size_in_quote_ccy {
                                                    k.quote_volume.unwrap_or(vol * contract_size * k.close)
                                                } else {
                                                    vol * contract_size
                                                };

                                                let timeframe = streams
                                                    .iter()
                                                    .find(|(info, _)| info.ticker == ticker)
                                                    .map(|(_, tf)| *tf)
                                                    .unwrap_or(Timeframe::M1);

                                                let kline = Kline::new(
                                                    k.time,
                                                    k.open,
                                                    k.high,
                                                    k.low,
                                                    k.close,
                                                    (-1.0, normalized_vol),
                                                    ticker_info.min_ticksize,
                                                );

                                                let _ = output
                                                    .send(Event::KlineReceived(
                                                        StreamKind::Kline {
                                                            ticker_info,
                                                            timeframe,
                                                        },
                                                        kline,
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }
                                    OpCode::Close => {
                                        let _ = output
                                            .send(Event::Disconnected(
                                                exchange,
                                                "Connection closed".to_string(),
                                            ))
                                            .await;
                                        state = State::Disconnected;
                                    }
                                    _ => {}
                                },
                                Err(e) => {
                                    let _ = output
                                        .send(Event::Disconnected(
                                            exchange,
                                            format!("Websocket error: {e:?}"),
                                        ))
                                        .await;
                                    state = State::Disconnected;
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

pub async fn fetch_trades(
    ticker_info: TickerInfo,
    from_time: u64,
    _data_path: std::path::PathBuf,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    let from_date = chrono::DateTime::from_timestamp_millis(from_time as i64)
        .ok_or_else(|| AdapterError::ParseError("Invalid timestamp".into()))?
        .date_naive();

    let next_day_start = from_date
        .succ_opt()
        .unwrap_or(from_date)
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    let _ = ticker_info;
    Ok((Vec::new(), next_day_start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mexc_topic_stream_name() {
        assert!(matches!(
            StreamName::from_topic("sub.deal"),
            StreamName::Trade
        ));
        assert!(matches!(
            StreamName::from_topic("push.deal"),
            StreamName::Trade
        ));
        assert!(matches!(
            StreamName::from_topic("sub.depth"),
            StreamName::Depth
        ));
        assert!(matches!(
            StreamName::from_topic("push.depth"),
            StreamName::Depth
        ));
        assert!(matches!(
            StreamName::from_topic("sub.kline"),
            StreamName::Kline
        ));
        assert!(matches!(
            StreamName::from_topic("push.kline"),
            StreamName::Kline
        ));
        assert!(matches!(StreamName::from_topic("pong"), StreamName::Pong));
        assert!(
            matches!(StreamName::from_topic("rs.sub.deal"), StreamName::Subscription(s) if s == "deal")
        );
    }

    #[test]
    fn test_mexc_timeframe_conversion() {
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M1, MarketKind::LinearPerps),
            Some("Min1")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M5, MarketKind::LinearPerps),
            Some("Min5")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M15, MarketKind::LinearPerps),
            Some("Min15")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M30, MarketKind::LinearPerps),
            Some("Min30")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H1, MarketKind::LinearPerps),
            Some("Min60")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H4, MarketKind::LinearPerps),
            Some("Hour4")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::D1, MarketKind::LinearPerps),
            Some("Day1")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::W1, MarketKind::LinearPerps),
            Some("Week1")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::MN1, MarketKind::LinearPerps),
            Some("Month1")
        );

        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M1, MarketKind::Spot),
            Some("1m")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M5, MarketKind::Spot),
            Some("5m")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H1, MarketKind::Spot),
            Some("60m")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H4, MarketKind::Spot),
            Some("4h")
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::D1, MarketKind::Spot),
            Some("1d")
        );

        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::M3, MarketKind::LinearPerps),
            None
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H2, MarketKind::LinearPerps),
            None
        );
        assert_eq!(
            convert_to_mexc_timeframe(Timeframe::H12, MarketKind::LinearPerps),
            None
        );
    }

    #[test]
    fn test_mexc_supports_kline_timeframe() {
        let linear = Exchange::MexcLinear;
        assert!(linear.supports_kline_timeframe(Timeframe::M1));
        assert!(linear.supports_kline_timeframe(Timeframe::M5));
        assert!(linear.supports_kline_timeframe(Timeframe::M15));
        assert!(linear.supports_kline_timeframe(Timeframe::M30));
        assert!(linear.supports_kline_timeframe(Timeframe::H1));
        assert!(linear.supports_kline_timeframe(Timeframe::H4));
        assert!(linear.supports_kline_timeframe(Timeframe::D1));
        assert!(linear.supports_kline_timeframe(Timeframe::W1));
        assert!(linear.supports_kline_timeframe(Timeframe::MN1));

        assert!(!linear.supports_kline_timeframe(Timeframe::M3));
        assert!(!linear.supports_kline_timeframe(Timeframe::H2));
        assert!(!linear.supports_kline_timeframe(Timeframe::H12));
    }

    #[test]
    fn test_mexc_feed_de_pong() {
        let pong_payload = br#"{"channel":"pong","data":1710000000000}"#;
        let res = feed_de(pong_payload, None, MarketKind::LinearPerps);
        assert!(res.is_ok());
        assert!(matches!(res.unwrap(), StreamData::Pong(_)));
    }

    #[test]
    fn test_mexc_feed_de_trade() {
        let trade_payload = br#"{"channel":"push.deal","symbol":"BTC_USDT","data":[{"p":65000.5,"v":1.2,"T":1,"t":1710000000000}],"ts":1710000000000}"#;
        let res = feed_de(trade_payload, None, MarketKind::LinearPerps);
        assert!(res.is_ok());
        if let StreamData::Trade(ticker, deals, ts) = res.unwrap() {
            assert_eq!(ticker.to_string(), "BTC_USDT");
            assert_eq!(deals.len(), 1);
            assert_eq!(deals[0].price, 65000.5);
            assert_eq!(deals[0].qty, 1.2);
            assert_eq!(deals[0].direction, 1);
            assert_eq!(deals[0].time, 1710000000000);
            assert_eq!(ts, 1710000000000);
        } else {
            panic!("Expected StreamData::Trade");
        }
    }

    #[test]
    fn test_mexc_feed_de_kline() {
        let kline_payload = br#"{"channel":"push.kline","symbol":"BTC_USDT","data":{"t":1710000,"o":65000.0,"h":65100.0,"l":64900.0,"c":65050.0,"a":120.0,"interval":"Min1"},"ts":1710000000}"#;
        let res = feed_de(kline_payload, None, MarketKind::LinearPerps);
        assert!(res.is_ok());
        if let StreamData::Kline(ticker, klines) = res.unwrap() {
            assert_eq!(ticker.to_string(), "BTC_USDT");
            assert_eq!(klines.len(), 1);
            assert_eq!(klines[0].time, 1710000 * 1000);
            assert_eq!(klines[0].open, 65000.0);
            assert_eq!(klines[0].high, 65100.0);
            assert_eq!(klines[0].low, 64900.0);
            assert_eq!(klines[0].close, 65050.0);
        } else {
            panic!("Expected StreamData::Kline");
        }
    }

    #[test]
    fn test_mexc_depth_snapshot_deserialization() {
        let depth_json = r#"{
            "data": {
                "asks": [[65000.0, 1.5], [65001.0, 2.0]],
                "bids": [[64999.0, 3.0], [64998.0, 1.0]],
                "version": 123456,
                "timestamp": 1710000000000
            }
        }"#;

        let snapshot: Result<DepthSnapshotResponse, _> = sonic_rs::from_str(depth_json);
        assert!(snapshot.is_ok());
        let snapshot = snapshot.unwrap();
        assert_eq!(snapshot.data.version, 123456);
        assert_eq!(snapshot.data.timestamp, 1710000000000);
        assert_eq!(snapshot.data.asks.len(), 2);
        assert_eq!(snapshot.data.bids.len(), 2);
        assert_eq!(snapshot.data.asks[0].price, 65000.0);
        assert_eq!(snapshot.data.asks[0].qty, 1.5);
        assert_eq!(snapshot.data.bids[0].price, 64999.0);
        assert_eq!(snapshot.data.bids[0].qty, 3.0);
    }
}
