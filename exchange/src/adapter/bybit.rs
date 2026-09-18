use super::{
    super::{
        Exchange, Kline, MarketKind, OpenInterest, Price, PushFrequency, SizeUnit, StreamKind,
        Ticker, TickerInfo, TickerStats, Timeframe, Trade,
        adapter::StreamTicksize,
        connect::{State, connect_ws},
        de_string_to_f32, de_string_to_u64,
        depth::{DeOrder, DepthPayload, DepthUpdate, LocalDepthCache},
        is_symbol_supported,
        limiter::{self, http_request_with_limiter},
        volume_size_unit,
    },
    AdapterError, Event,
};

use fastwebsockets::{Frame, OpCode};
use iced_futures::{
    futures::{SinkExt, Stream, channel::mpsc},
    stream,
};
use serde_json::{Value, json};
use sonic_rs::{Deserialize, JsonValueTrait};
use tokio::sync::Mutex;

use std::{
    collections::HashMap,
    io::BufReader,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

use crate::trades::cache::{
    USE_BINARY_CACHE, find_gap_index, load_intraday_trades_from_cache, load_raw_trades_from_cache,
    save_intraday_trades_to_cache, save_raw_trades_to_cache,
};

const WS_DOMAIN: &str = "stream.bybit.com";
const FETCH_DOMAIN: &str = "https://api.bybit.com";

static BYBIT_LIMITER: LazyLock<Mutex<BybitLimiter>> =
    LazyLock::new(|| Mutex::new(BybitLimiter::new(LIMIT, REFILL_RATE)));

const LIMIT: usize = 600;

const REFILL_RATE: Duration = Duration::from_secs(5);
const LIMITER_BUFFER_PCT: f32 = 0.05;

pub struct BybitLimiter {
    bucket: limiter::FixedWindowBucket,
}

impl BybitLimiter {
    pub fn new(limit: usize, refill_rate: Duration) -> Self {
        let effective_limit = (limit as f32 * (1.0 - LIMITER_BUFFER_PCT)) as usize;
        Self {
            bucket: limiter::FixedWindowBucket::new(effective_limit, refill_rate),
        }
    }
}

impl limiter::RateLimiter for BybitLimiter {
    fn prepare_request(&mut self, weight: usize) -> Option<Duration> {
        self.bucket.calculate_wait_time(weight)
    }

    fn update_from_response(&mut self, _response: &reqwest::Response, weight: usize) {
        self.bucket.consume_tokens(weight);
    }

    fn should_exit_on_response(&self, response: &reqwest::Response) -> bool {
        response.status() == 403
    }
}

fn exchange_from_market_type(market: MarketKind) -> Exchange {
    match market {
        MarketKind::Spot => Exchange::BybitSpot,
        MarketKind::LinearPerps => Exchange::BybitLinear,
        MarketKind::InversePerps => Exchange::BybitInverse,
    }
}

#[derive(Deserialize)]
struct SonicDepth {
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "b")]
    pub bids: Vec<DeOrder>,
    #[serde(rename = "a")]
    pub asks: Vec<DeOrder>,
}

#[derive(Deserialize, Debug)]
struct SonicTrade {
    #[serde(rename = "T")]
    pub time: u64,
    #[serde(rename = "p", deserialize_with = "de_string_to_f32")]
    pub price: f32,
    #[serde(rename = "v", deserialize_with = "de_string_to_f32")]
    pub qty: f32,
    #[serde(rename = "S")]
    pub is_sell: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SonicKline {
    #[serde(rename = "start")]
    pub time: u64,
    #[serde(rename = "open", deserialize_with = "de_string_to_f32")]
    pub open: f32,
    #[serde(rename = "high", deserialize_with = "de_string_to_f32")]
    pub high: f32,
    #[serde(rename = "low", deserialize_with = "de_string_to_f32")]
    pub low: f32,
    #[serde(rename = "close", deserialize_with = "de_string_to_f32")]
    pub close: f32,
    #[serde(rename = "volume", deserialize_with = "de_string_to_f32")]
    pub volume: f32,
    #[serde(rename = "interval")]
    pub interval: String,
}

enum StreamData {
    Trade(Vec<SonicTrade>),
    Depth(SonicDepth, String, u64),
    Kline(Ticker, Vec<SonicKline>),
}

#[derive(Debug)]
enum StreamName {
    Depth(Ticker),
    Trade(Ticker),
    Kline(Ticker),
    Unknown,
}

impl StreamName {
    fn from_topic(topic: &str, is_ticker: Option<Ticker>, market_type: MarketKind) -> Self {
        let parts: Vec<&str> = topic.split('.').collect();

        if let Some(ticker_str) = parts.last() {
            let exchange = exchange_from_market_type(market_type);
            let ticker = is_ticker.unwrap_or_else(|| Ticker::new(ticker_str, exchange));

            match parts.first() {
                Some(&"publicTrade") => StreamName::Trade(ticker),
                Some(&"orderbook") => StreamName::Depth(ticker),
                Some(&"kline") => StreamName::Kline(ticker),
                _ => StreamName::Unknown,
            }
        } else {
            StreamName::Unknown
        }
    }
}

#[derive(Debug)]
enum StreamWrapper {
    Trade,
    Depth,
    Kline,
}

#[allow(unused_assignments)]
fn feed_de(
    slice: &[u8],
    ticker: Option<Ticker>,
    market_type: MarketKind,
) -> Result<StreamData, AdapterError> {
    let mut stream_type: Option<StreamWrapper> = None;
    let mut depth_wrap: Option<SonicDepth> = None;

    let mut data_type = String::new();
    let mut topic_ticker: Option<Ticker> = ticker;

    let iter: sonic_rs::ObjectJsonIter = sonic_rs::to_object_iter(slice);

    for elem in iter {
        let (k, v) = elem.map_err(|e| AdapterError::ParseError(e.to_string()))?;

        if k == "topic" {
            if let Some(val) = v.as_str() {
                let mut is_ticker = None;

                if let Some(t) = ticker {
                    is_ticker = Some(t);
                }

                match StreamName::from_topic(val, is_ticker, market_type) {
                    StreamName::Depth(t) => {
                        stream_type = Some(StreamWrapper::Depth);
                        topic_ticker = Some(t);
                    }
                    StreamName::Trade(t) => {
                        stream_type = Some(StreamWrapper::Trade);
                        topic_ticker = Some(t);
                    }
                    StreamName::Kline(t) => {
                        stream_type = Some(StreamWrapper::Kline);
                        topic_ticker = Some(t);
                    }
                    _ => {
                        log::error!("Unknown stream name");
                    }
                }
            }
        } else if k == "type" {
            v.as_str().unwrap().clone_into(&mut data_type);
        } else if k == "data" {
            match stream_type {
                Some(StreamWrapper::Trade) => {
                    let trade_wrap: Vec<SonicTrade> = sonic_rs::from_str(&v.as_raw_faststr())
                        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

                    return Ok(StreamData::Trade(trade_wrap));
                }
                Some(StreamWrapper::Depth) => {
                    if depth_wrap.is_none() {
                        depth_wrap = Some(SonicDepth {
                            update_id: 0,
                            bids: Vec::new(),
                            asks: Vec::new(),
                        });
                    }
                    depth_wrap = Some(
                        sonic_rs::from_str(&v.as_raw_faststr())
                            .map_err(|e| AdapterError::ParseError(e.to_string()))?,
                    );
                }
                Some(StreamWrapper::Kline) => {
                    let kline_wrap: Vec<SonicKline> = sonic_rs::from_str(&v.as_raw_faststr())
                        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

                    if let Some(t) = topic_ticker {
                        return Ok(StreamData::Kline(t, kline_wrap));
                    } else {
                        return Err(AdapterError::ParseError(
                            "Missing ticker for kline data".to_string(),
                        ));
                    }
                }
                _ => {
                    log::error!("Unknown stream type");
                }
            }
        } else if k == "cts"
            && let Some(dw) = depth_wrap
        {
            let time: u64 = v
                .as_u64()
                .ok_or_else(|| AdapterError::ParseError("Failed to parse u64".to_string()))?;

            return Ok(StreamData::Depth(dw, data_type.to_string(), time));
        }
    }

    Err(AdapterError::ParseError("Unknown data".to_string()))
}

async fn try_connect(
    streams: &Value,
    market_type: MarketKind,
    output: &mut mpsc::Sender<Event>,
) -> State {
    let exchange = match market_type {
        MarketKind::Spot => Exchange::BybitSpot,
        MarketKind::LinearPerps => Exchange::BybitLinear,
        MarketKind::InversePerps => Exchange::BybitInverse,
    };
    let url = format!(
        "wss://{}/v5/public/{}",
        WS_DOMAIN,
        match market_type {
            MarketKind::Spot => "spot",
            MarketKind::LinearPerps => "linear",
            MarketKind::InversePerps => "inverse",
        }
    );

    match connect_ws(WS_DOMAIN, &url).await {
        Ok(mut websocket) => {
            if let Err(e) = websocket
                .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                    streams.to_string().as_bytes(),
                )))
                .await
            {
                let _ = output
                    .send(Event::Disconnected(
                        exchange,
                        format!("Failed subscribing: {e}"),
                    ))
                    .await;
                return State::Disconnected;
            }

            let _ = output.send(Event::Connected(exchange)).await;
            State::Connected(websocket)
        }
        Err(err) => {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            let _ = output
                .send(Event::Disconnected(
                    exchange,
                    format!("Failed to connect: {err}"),
                ))
                .await;
            State::Disconnected
        }
    }
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

        let mut trades_buffer: Vec<Trade> = Vec::new();
        let mut orderbook = LocalDepthCache::default();

        let size_in_quote_ccy =
            volume_size_unit() == SizeUnit::Quote && market_type != MarketKind::InversePerps;

        loop {
            match &mut state {
                State::Disconnected => {
                    let depth_level = if let PushFrequency::Custom(tf) = push_freq {
                        match market_type {
                            MarketKind::Spot => match tf {
                                Timeframe::MS200 => "200",
                                Timeframe::MS300 => "1000",
                                _ => "200",
                            },
                            MarketKind::LinearPerps | MarketKind::InversePerps => match tf {
                                Timeframe::MS100 => "200",
                                Timeframe::MS300 => "1000",
                                _ => "200",
                            },
                        }
                    } else {
                        "200"
                    };

                    let stream_1 = format!("publicTrade.{symbol_str}");
                    let stream_2 = format!("orderbook.{depth_level}.{symbol_str}");

                    let subscribe_message = serde_json::json!({
                        "op": "subscribe",
                        "args": [stream_1, stream_2]
                    });
                    state = try_connect(&subscribe_message, market_type, &mut output).await;
                }
                State::Connected(websocket) => match websocket.read_frame().await {
                    Ok(msg) => match msg.opcode {
                        OpCode::Text => {
                            if let Ok(data) = feed_de(&msg.payload[..], Some(ticker), market_type) {
                                match data {
                                    StreamData::Trade(de_trade_vec) => {
                                        for de_trade in &de_trade_vec {
                                            let price = Price::from_f32(de_trade.price)
                                                .round_to_min_tick(ticker_info.min_ticksize);
                                            let qty = if size_in_quote_ccy {
                                                (de_trade.qty * de_trade.price).round()
                                            } else {
                                                de_trade.qty
                                            };

                                            let trade = Trade {
                                                time: de_trade.time,
                                                is_sell: de_trade.is_sell == "Sell",
                                                price,
                                                qty,
                                            };

                                            trades_buffer.push(trade);
                                        }
                                    }
                                    StreamData::Depth(de_depth, data_type, time) => {
                                        let depth = DepthPayload {
                                            last_update_id: de_depth.update_id,
                                            time,
                                            bids: de_depth
                                                .bids
                                                .iter()
                                                .map(|x| DeOrder {
                                                    price: x.price,
                                                    qty: if size_in_quote_ccy {
                                                        (x.qty * x.price).round()
                                                    } else {
                                                        x.qty
                                                    },
                                                })
                                                .collect(),
                                            asks: de_depth
                                                .asks
                                                .iter()
                                                .map(|x| DeOrder {
                                                    price: x.price,
                                                    qty: if size_in_quote_ccy {
                                                        (x.qty * x.price).round()
                                                    } else {
                                                        x.qty
                                                    },
                                                })
                                                .collect(),
                                        };

                                        if (data_type == "snapshot") || (depth.last_update_id == 1)
                                        {
                                            orderbook.update(
                                                DepthUpdate::Snapshot(depth),
                                                ticker_info.min_ticksize,
                                            );
                                        } else if data_type == "delta" {
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
                                    }
                                    _ => {
                                        log::warn!("Unknown data received");
                                    }
                                }
                            }
                        }
                        OpCode::Close => {
                            state = State::Disconnected;
                            let _ = output
                                .send(Event::Disconnected(
                                    exchange,
                                    "Connection closed".to_string(),
                                ))
                                .await;
                        }
                        _ => {}
                    },
                    Err(e) => {
                        state = State::Disconnected;
                        let _ = output
                            .send(Event::Disconnected(
                                exchange,
                                "Error reading frame: ".to_string() + &e.to_string(),
                            ))
                            .await;
                    }
                },
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
        let size_in_quote_ccy =
            volume_size_unit() == SizeUnit::Quote && market_type != MarketKind::InversePerps;

        let ticker_info_map = streams
            .iter()
            .map(|(ticker_info, _)| (ticker_info.ticker, *ticker_info))
            .collect::<HashMap<Ticker, TickerInfo>>();

        loop {
            match &mut state {
                State::Disconnected => {
                    let stream_str = streams
                        .iter()
                        .map(|(ticker_info, timeframe)| {
                            let ticker = ticker_info.ticker;
                            let timeframe_str = {
                                if Timeframe::D1 == *timeframe {
                                    "D".to_string()
                                } else {
                                    timeframe.to_minutes().to_string()
                                }
                            };
                            format!(
                                "kline.{timeframe_str}.{}",
                                ticker.to_full_symbol_and_type().0
                            )
                        })
                        .collect::<Vec<String>>();
                    let subscribe_message = serde_json::json!({
                        "op": "subscribe",
                        "args": stream_str
                    });

                    state = try_connect(&subscribe_message, market_type, &mut output).await;
                }
                State::Connected(websocket) => match websocket.read_frame().await {
                    Ok(msg) => match msg.opcode {
                        OpCode::Text => {
                            if let Ok(StreamData::Kline(ticker, de_kline_vec)) =
                                feed_de(&msg.payload[..], None, market_type)
                            {
                                for de_kline in &de_kline_vec {
                                    let volume = if size_in_quote_ccy {
                                        (de_kline.volume * de_kline.close).round()
                                    } else {
                                        de_kline.volume
                                    };

                                    if let Some(timeframe) = string_to_timeframe(&de_kline.interval)
                                    {
                                        if let Some(info) = ticker_info_map.get(&ticker) {
                                            let ticker_info = *info;

                                            let kline = Kline::new(
                                                de_kline.time,
                                                de_kline.open,
                                                de_kline.high,
                                                de_kline.low,
                                                de_kline.close,
                                                (-1.0, volume),
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
                                        } else {
                                            log::error!(
                                                "Ticker info not found for ticker: {}",
                                                ticker
                                            );
                                        }
                                    } else {
                                        log::error!(
                                            "Failed to find timeframe: {}, {:?}",
                                            &de_kline.interval,
                                            streams
                                        );
                                    }
                                }
                            }
                        }
                        OpCode::Close => {
                            state = State::Disconnected;
                            let _ = output
                                .send(Event::Disconnected(
                                    exchange,
                                    "Connection closed".to_string(),
                                ))
                                .await;
                        }
                        _ => {}
                    },
                    Err(e) => {
                        state = State::Disconnected;
                        let _ = output
                            .send(Event::Disconnected(
                                exchange,
                                "Error reading frame: ".to_string() + &e.to_string(),
                            ))
                            .await;
                    }
                },
            }
        }
    })
}

fn string_to_timeframe(interval: &str) -> Option<Timeframe> {
    Timeframe::KLINE
        .iter()
        .find(|&tf| {
            tf.to_minutes().to_string() == interval || {
                if tf == &Timeframe::D1 {
                    interval == "D"
                } else {
                    false
                }
            }
        })
        .copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeOpenInterest {
    #[serde(rename = "openInterest", deserialize_with = "de_string_to_f32")]
    pub value: f32,
    #[serde(deserialize_with = "de_string_to_u64")]
    pub timestamp: u64,
}

/// # Panics
///
/// Will panic if the `period` is not one of the supported timeframes for open interest
pub async fn fetch_historical_oi(
    ticker: Ticker,
    range: Option<(u64, u64)>,
    period: Timeframe,
) -> Result<Vec<OpenInterest>, AdapterError> {
    let ticker_str = ticker.to_full_symbol_and_type().0.to_uppercase();
    let period_str = match period {
        Timeframe::M5 => "5min",
        Timeframe::M15 => "15min",
        Timeframe::M30 => "30min",
        Timeframe::H1 => "1h",
        Timeframe::H4 => "4h",
        Timeframe::D1 => "1d",
        _ => panic!("Unsupported timeframe for open interest: {period}"),
    };

    let mut url = format!(
        "{FETCH_DOMAIN}/v5/market/open-interest?category=linear&symbol={ticker_str}&intervalTime={period_str}",
    );

    if let Some((start, end)) = range {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Could not get system time")
            .as_millis() as u64;
        let end = end.min(now_ms);
        if start >= end {
            return Ok(Vec::new());
        }

        let interval_ms = period.to_milliseconds().max(1);
        let num_intervals = ((end - start) / interval_ms).clamp(1, 200);

        url.push_str(&format!(
            "&startTime={start}&endTime={end}&limit={num_intervals}"
        ));
    } else {
        url.push_str("&limit=200");
    }

    let response_text = http_request_with_limiter(&url, &BYBIT_LIMITER, 1, None, None).await?;

    let content: Value = sonic_rs::from_str(&response_text).map_err(|e| {
        log::error!(
            "Failed to parse JSON from {}: {}\nResponse: {}",
            url,
            e,
            response_text
        );
        AdapterError::ParseError(e.to_string())
    })?;

    let result_list = content["result"]["list"].as_array().ok_or_else(|| {
        log::error!("Result list is not an array in response: {}", response_text);
        AdapterError::ParseError("Result list is not an array".to_string())
    })?;

    let bybit_oi: Vec<DeOpenInterest> =
        serde_json::from_value(json!(result_list)).map_err(|e| {
            log::error!(
                "Failed to parse open interest array: {}\nResponse: {}",
                e,
                response_text
            );
            AdapterError::ParseError(format!("Failed to parse open interest: {e}"))
        })?;

    let open_interest: Vec<OpenInterest> = bybit_oi
        .into_iter()
        .map(|x| OpenInterest {
            time: x.timestamp,
            value: x.value,
        })
        .collect();

    if open_interest.is_empty() {
        log::warn!(
            "No open interest data found for {}, from url: {}",
            ticker_str,
            url
        );
    }

    Ok(open_interest)
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ApiResponse {
    #[serde(rename = "retCode")]
    ret_code: u32,
    #[serde(rename = "retMsg")]
    ret_msg: String,
    result: ApiResult,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ApiResult {
    symbol: String,
    category: String,
    list: Vec<Vec<Value>>,
}

fn parse_kline_field<T: std::str::FromStr>(field: Option<&str>) -> Result<T, AdapterError> {
    field
        .ok_or_else(|| AdapterError::ParseError("Failed to parse kline".to_string()))
        .and_then(|s| {
            s.parse::<T>()
                .map_err(|_| AdapterError::ParseError("Failed to parse kline".to_string()))
        })
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let ticker = ticker_info.ticker;

    let (symbol_str, market_type) = &ticker.to_full_symbol_and_type();
    let timeframe_str = {
        if Timeframe::D1 == timeframe {
            "D".to_string()
        } else {
            timeframe.to_minutes().to_string()
        }
    };

    let market = match market_type {
        MarketKind::Spot => "spot",
        MarketKind::LinearPerps => "linear",
        MarketKind::InversePerps => "inverse",
    };

    let mut url = format!(
        "{FETCH_DOMAIN}/v5/market/kline?category={}&symbol={}&interval={}",
        market,
        symbol_str.to_uppercase(),
        timeframe_str
    );

    if let Some((start, end)) = range {
        let interval_ms = timeframe.to_milliseconds();
        let num_intervals = ((end - start) / interval_ms).min(1000);

        url.push_str(&format!("&start={start}&end={end}&limit={num_intervals}"));
    } else {
        url.push_str("&limit=1000");
    }

    let response: ApiResponse =
        limiter::http_parse_with_limiter(&url, &BYBIT_LIMITER, 1, None, None).await?;

    let size_in_quote_ccy =
        volume_size_unit() == SizeUnit::Quote && *market_type != MarketKind::InversePerps;

    let klines: Result<Vec<Kline>, AdapterError> = response
        .result
        .list
        .iter()
        .map(|kline| {
            let time = parse_kline_field::<u64>(kline[0].as_str())?;

            let open = parse_kline_field::<f32>(kline[1].as_str())?;
            let high = parse_kline_field::<f32>(kline[2].as_str())?;
            let low = parse_kline_field::<f32>(kline[3].as_str())?;
            let close = parse_kline_field::<f32>(kline[4].as_str())?;

            let mut volume = parse_kline_field::<f32>(kline[5].as_str())?;
            volume = if size_in_quote_ccy {
                (volume * close).round()
            } else {
                volume
            };

            let kline = Kline::new(
                time,
                open,
                high,
                low,
                close,
                (-1.0, volume),
                ticker_info.min_ticksize,
            );

            Ok(kline)
        })
        .collect();

    klines
}

pub async fn fetch_ticksize(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    let exchange = exchange_from_market_type(market_type);

    let market = match market_type {
        MarketKind::Spot => "spot",
        MarketKind::LinearPerps => "linear",
        MarketKind::InversePerps => "inverse",
    };

    let url = format!("{FETCH_DOMAIN}/v5/market/instruments-info?category={market}&limit=1000",);

    let response_text = crate::limiter::HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(AdapterError::FetchError)?
        .text()
        .await
        .map_err(AdapterError::FetchError)?;

    let exchange_info: Value =
        sonic_rs::from_str(&response_text).map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let result_list: &Vec<Value> = exchange_info["result"]["list"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Result list is not an array".to_string()))?;

    let mut ticker_info_map = HashMap::new();

    for item in result_list {
        let symbol = item["symbol"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Symbol not found".to_string()))?;

        if !is_symbol_supported(symbol, exchange, true) {
            continue;
        }

        if let Some(contract_type) = item["contractType"].as_str()
            && contract_type != "LinearPerpetual"
            && contract_type != "InversePerpetual"
        {
            continue;
        }

        if let Some(quote_asset) = item["quoteCoin"].as_str()
            && quote_asset != "USDT"
            && quote_asset != "USD"
        {
            continue;
        }

        let lot_size_filter = item["lotSizeFilter"]
            .as_object()
            .ok_or_else(|| AdapterError::ParseError("Lot size filter not found".to_string()))?;

        let min_qty = lot_size_filter["minOrderQty"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Min order qty not found".to_string()))?
            .parse::<f32>()
            .map_err(|_| AdapterError::ParseError("Failed to parse min order qty".to_string()))?;

        let price_filter = item["priceFilter"]
            .as_object()
            .ok_or_else(|| AdapterError::ParseError("Price filter not found".to_string()))?;

        let min_ticksize = price_filter["tickSize"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Tick size not found".to_string()))?
            .parse::<f32>()
            .map_err(|_| AdapterError::ParseError("Failed to parse tick size".to_string()))?;

        let ticker = Ticker::new(symbol, exchange);
        let info = TickerInfo::new(ticker, min_ticksize, min_qty, None);

        ticker_info_map.insert(ticker, Some(info));
    }

    Ok(ticker_info_map)
}

pub async fn fetch_ticker_prices(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, TickerStats>, AdapterError> {
    let exchange = exchange_from_market_type(market_type);

    let market = match market_type {
        MarketKind::Spot => "spot",
        MarketKind::LinearPerps => "linear",
        MarketKind::InversePerps => "inverse",
    };

    let url = format!("{FETCH_DOMAIN}/v5/market/tickers?category={market}");

    let parsed_response: Value =
        limiter::http_parse_with_limiter(&url, &BYBIT_LIMITER, 1, None, None).await?;

    let result_list: &Vec<Value> = parsed_response["result"]["list"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Result list is not an array".to_string()))?;

    let mut ticker_prices_map = HashMap::new();

    for item in result_list {
        let symbol = item["symbol"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Symbol not found".to_string()))?;

        if !is_symbol_supported(symbol, exchange, false) {
            continue;
        }

        let mark_price = item["lastPrice"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Mark price not found".to_string()))?
            .parse::<f32>()
            .map_err(|_| AdapterError::ParseError("Failed to parse mark price".to_string()))?;

        let daily_price_chg = item["price24hPcnt"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Daily price change not found".to_string()))?
            .parse::<f32>()
            .map_err(|_| {
                AdapterError::ParseError("Failed to parse daily price change".to_string())
            })?;

        let daily_volume = item["volume24h"]
            .as_str()
            .ok_or_else(|| AdapterError::ParseError("Daily volume not found".to_string()))?
            .parse::<f32>()
            .map_err(|_| AdapterError::ParseError("Failed to parse daily volume".to_string()))?;

        let volume_in_usd = if market_type == MarketKind::InversePerps {
            daily_volume
        } else {
            daily_volume * mark_price
        };

        let ticker_stats = TickerStats {
            mark_price,
            daily_price_chg: daily_price_chg * 100.0,
            daily_volume: volume_in_usd,
        };

        ticker_prices_map.insert(Ticker::new(symbol, exchange), ticker_stats);
    }

    Ok(ticker_prices_map)
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct BybitRecentTradeItem {
    pub price: String,
    pub size: String,
    pub side: String,
    pub time: String,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct BybitRecentTradeResult {
    pub list: Vec<BybitRecentTradeItem>,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct BybitRecentTradeResponse {
    pub result: BybitRecentTradeResult,
}

pub async fn fetch_intraday_trades(
    ticker_info: TickerInfo,
    from: u64,
) -> Result<Vec<Trade>, AdapterError> {
    let ticker = ticker_info.ticker;
    let (symbol_str, market_type) = ticker.to_full_symbol_and_type();

    let category = match market_type {
        MarketKind::Spot => "spot",
        MarketKind::LinearPerps => "linear",
        MarketKind::InversePerps => "inverse",
    };

    let url = format!(
        "{FETCH_DOMAIN}/v5/market/recent-trade?category={category}&symbol={}&limit=1000",
        symbol_str.to_uppercase()
    );

    let parsed: BybitRecentTradeResponse =
        limiter::http_parse_with_limiter(&url, &BYBIT_LIMITER, 1, None, None).await?;

    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;

    let mut trades: Vec<Trade> = parsed
        .result
        .list
        .into_iter()
        .filter_map(|item| {
            let time = item.time.parse::<u64>().ok()?;
            if time < from {
                return None;
            }
            let price_f32 = item.price.parse::<f32>().ok()?;
            let mut qty = item.size.parse::<f32>().ok()?;
            if size_in_quote_ccy {
                qty = (qty * price_f32).round();
            }
            let is_sell = item.side.eq_ignore_ascii_case("sell");
            let price = Price::from_f32(price_f32).round_to_min_tick(ticker_info.min_ticksize);

            Some(Trade {
                time,
                is_sell,
                price,
                qty,
            })
        })
        .collect();

    trades.sort_by_key(|t| t.time);
    Ok(trades)
}

pub async fn get_hist_trades(
    ticker_info: TickerInfo,
    date: chrono::NaiveDate,
    base_path: PathBuf,
) -> Result<Vec<Trade>, AdapterError> {
    if USE_BINARY_CACHE
        && let Some(trades) = load_raw_trades_from_cache(&base_path, &ticker_info, date)
    {
        log::info!(
            "Using binary cached Bybit trades for {date} ({} trades)",
            trades.len()
        );
        return Ok(trades);
    }

    let ticker = ticker_info.ticker;
    let (symbol, market_type) = ticker.to_full_symbol_and_type();
    let symbol_upper = symbol.to_uppercase();

    let category = match market_type {
        MarketKind::Spot => "spot",
        MarketKind::LinearPerps | MarketKind::InversePerps => "trading",
    };
    let date_str = date.format("%Y-%m-%d");
    let file_name = format!("{symbol_upper}_{date_str}.csv.gz");
    let url = format!("https://public.bybit.com/{category}/{symbol_upper}/{file_name}");

    log::info!("Downloading Bybit historical trades from {url}");
    let resp = reqwest::get(&url).await.map_err(AdapterError::FetchError)?;
    if !resp.status().is_success() {
        return Err(AdapterError::InvalidRequest(format!(
            "Failed to fetch Bybit trades from {url}: status {}",
            resp.status()
        )));
    }

    let body = resp.bytes().await.map_err(AdapterError::FetchError)?;
    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
    let min_ticksize = ticker_info.min_ticksize;

    let trades = tokio::task::spawn_blocking(move || -> Result<Vec<Trade>, AdapterError> {
        let gz_decoder = flate2::read::GzDecoder::new(&body[..]);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(BufReader::new(gz_decoder));

        let headers = rdr
            .headers()
            .map_err(|e| {
                AdapterError::ParseError(format!("Failed to read Bybit CSV headers: {e}"))
            })?
            .clone();

        let time_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("timestamp"))
            .unwrap_or(0);
        let side_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("side"))
            .unwrap_or(2);
        let size_col = headers
            .iter()
            .position(|h| {
                h.eq_ignore_ascii_case("size")
                    || h.eq_ignore_ascii_case("volume")
                    || h.eq_ignore_ascii_case("qty")
            })
            .unwrap_or(3);
        let price_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("price"))
            .unwrap_or(4);

        let mut trades = Vec::new();
        for result in rdr.records() {
            let record = match result {
                Ok(r) => r,
                Err(_) => continue,
            };

            let raw_time = match record.get(time_col) {
                Some(t) => t,
                None => continue,
            };

            let time = if raw_time.contains('.') {
                (raw_time.parse::<f64>().unwrap_or(0.0) * 1000.0) as u64
            } else {
                let t = raw_time.parse::<u64>().unwrap_or(0);
                if t < 100_000_000_000 {
                    t * 1000
                } else if t > 100_000_000_000_000 {
                    t / 1000
                } else {
                    t
                }
            };

            let is_sell = record
                .get(side_col)
                .map(|s| s.eq_ignore_ascii_case("sell"))
                .unwrap_or(false);

            let price_f32 = match record.get(price_col).and_then(|p| p.parse::<f32>().ok()) {
                Some(p) => p,
                None => continue,
            };

            let mut qty = match record.get(size_col).and_then(|q| q.parse::<f32>().ok()) {
                Some(q) => q,
                None => continue,
            };

            if size_in_quote_ccy {
                qty = (qty * price_f32).round();
            }

            let price = Price::from_f32(price_f32).round_to_min_tick(min_ticksize);

            trades.push(Trade {
                time,
                is_sell,
                price,
                qty,
            });
        }

        trades.sort_by_key(|t| t.time);
        trades.dedup_by(|a, b| a.time == b.time && a.price == b.price && a.qty == b.qty);
        Ok(trades)
    })
    .await
    .map_err(|e| {
        AdapterError::ParseError(format!("Join error during Bybit trades parsing: {e}"))
    })??;

    if USE_BINARY_CACHE
        && !trades.is_empty()
        && let Err(e) = save_raw_trades_to_cache(&base_path, &ticker_info, date, &trades)
    {
        log::warn!("Failed to save Bybit binary cache: {e}");
    }

    Ok(trades)
}

async fn fetch_trades_from_intraday_cache_or_rest(
    ticker_info: TickerInfo,
    from_time: u64,
    target_date: chrono::NaiveDate,
    data_path: &Path,
    day_end_fallback: u64,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    let cached =
        load_intraday_trades_from_cache(data_path, &ticker_info, target_date).unwrap_or_default();
    if !cached.is_empty() {
        let t_first = cached.first().unwrap().time;
        let t_last = cached.last().unwrap().time;

        if from_time < t_first {
            let new_trades = fetch_intraday_trades(ticker_info, from_time).await?;
            if !new_trades.is_empty() {
                let next_from = new_trades
                    .last()
                    .map(|t| t.time.saturating_add(1))
                    .unwrap_or(t_first);
                let _ = save_intraday_trades_to_cache(
                    data_path,
                    &ticker_info,
                    target_date,
                    &new_trades,
                );
                return Ok((new_trades, next_from));
            } else {
                let end_idx = find_gap_index(&cached, 0, 60_000).unwrap_or(cached.len() - 1);
                let trades = cached[0..=end_idx].to_vec();
                let next_from = cached[end_idx].time.saturating_add(1);
                return Ok((trades, next_from));
            }
        }

        if from_time <= t_last {
            let start_idx = cached.partition_point(|t| t.time < from_time);
            let in_gap = start_idx < cached.len()
                && cached[start_idx].time.saturating_sub(from_time) > 60_000;

            if in_gap {
                let new_trades = fetch_intraday_trades(ticker_info, from_time).await?;
                if !new_trades.is_empty() {
                    let next_from = new_trades
                        .last()
                        .map(|t| t.time.saturating_add(1))
                        .unwrap_or_else(|| cached[start_idx].time);
                    let _ = save_intraday_trades_to_cache(
                        data_path,
                        &ticker_info,
                        target_date,
                        &new_trades,
                    );
                    return Ok((new_trades, next_from));
                } else {
                    let end_idx =
                        find_gap_index(&cached, start_idx, 60_000).unwrap_or(cached.len() - 1);
                    let trades = cached[start_idx..=end_idx].to_vec();
                    let next_from = cached[end_idx].time.saturating_add(1);
                    return Ok((trades, next_from));
                }
            }

            if let Some(end_idx) = find_gap_index(&cached, start_idx, 60_000) {
                let trades = cached[start_idx..=end_idx].to_vec();
                let next_from = cached[end_idx].time.saturating_add(1);
                return Ok((trades, next_from));
            }

            let trades = cached[start_idx..].to_vec();
            let next_from = t_last.saturating_add(1);
            return Ok((trades, next_from));
        }
    }

    let new_trades = fetch_intraday_trades(ticker_info, from_time).await?;
    let next_from = new_trades
        .last()
        .map(|t| t.time.saturating_add(1))
        .unwrap_or(day_end_fallback);

    if !new_trades.is_empty() {
        let _ = save_intraday_trades_to_cache(data_path, &ticker_info, target_date, &new_trades);
    }
    Ok((new_trades, next_from))
}

pub async fn fetch_trades(
    ticker_info: TickerInfo,
    from_time: u64,
    data_path: PathBuf,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    let today_date = chrono::Utc::now().date_naive();
    let today_midnight = today_date.and_hms_opt(0, 0, 0).unwrap().and_utc();

    if from_time as i64 >= today_midnight.timestamp_millis() {
        return fetch_trades_from_intraday_cache_or_rest(
            ticker_info,
            from_time,
            today_date,
            &data_path,
            from_time.saturating_add(60_000),
        )
        .await;
    }

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

    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let max_days: i64 = if symbol.to_uppercase().starts_with("BTC") {
        130
    } else {
        30
    };
    let cutoff_date = chrono::Utc::now().date_naive() - chrono::Duration::days(max_days);
    if from_date < cutoff_date {
        return Ok((Vec::new(), next_day_start));
    }

    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let is_recent = from_time >= now_ms.saturating_sub(48 * 3600 * 1000);

    // Check if recent past day is already completely cached in intraday cache
    if is_recent
        && let Some(cached) = load_intraday_trades_from_cache(&data_path, &ticker_info, from_date)
        && let Some(day_start_dt) = from_date.and_hms_opt(0, 0, 0)
    {
        let day_start = day_start_dt.and_utc().timestamp_millis() as u64;
        let day_end = day_start + 86_400_000 - 1;
        let is_full = !cached.is_empty()
            && cached.first().map(|t| t.time).unwrap_or(0) <= day_start + 600_000
            && cached.last().map(|t| t.time).unwrap_or(0) >= day_end - 600_000;
        if is_full {
            let _ = save_raw_trades_to_cache(&data_path, &ticker_info, from_date, &cached);
            return Ok((cached, next_day_start));
        }
    }

    match get_hist_trades(ticker_info, from_date, data_path.clone()).await {
        Ok(trades) => Ok((trades, next_day_start)),
        Err(e) => {
            log::warn!(
                "Bybit historical trades fetch failed for {}: {}, falling back to intraday fetch if recent",
                from_date,
                e
            );
            if is_recent {
                fetch_trades_from_intraday_cache_or_rest(
                    ticker_info,
                    from_time,
                    from_date,
                    &data_path,
                    next_day_start,
                )
                .await
            } else {
                Err(e)
            }
        }
    }
}
