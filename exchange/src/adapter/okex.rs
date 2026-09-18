use crate::{
    OpenInterest, Price, PushFrequency, SizeUnit,
    adapter::{StreamKind, StreamTicksize},
    limiter::{self, RateLimiter},
    volume_size_unit,
};

use super::{
    super::{
        Exchange, Kline, MarketKind, Ticker, TickerInfo, TickerStats, Timeframe, Trade,
        connect::{State, connect_ws},
        de_string_to_f32, de_string_to_u64, is_symbol_supported,
        limiter::HTTP_CLIENT,
    },
    AdapterError, Event,
};

use super::super::depth::{DeOrder, DepthPayload, DepthUpdate, LocalDepthCache};

use fastwebsockets::{Frame, OpCode};
use iced_futures::{
    futures::{SinkExt, Stream, channel::mpsc},
    stream,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::BufReader,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};
use tokio::sync::Mutex;

use crate::trades::cache::{
    USE_BINARY_CACHE, find_gap_index, load_intraday_trades_from_cache, load_raw_trades_from_cache,
    save_intraday_trades_to_cache, save_raw_trades_to_cache,
};

const WS_DOMAIN: &str = "ws.okx.com";

const LIMIT: usize = 20;

const REFILL_RATE: Duration = Duration::from_secs(2);
const LIMITER_BUFFER_PCT: f32 = 0.05;

static OKEX_LIMITER: LazyLock<Mutex<OkexLimiter>> =
    LazyLock::new(|| Mutex::new(OkexLimiter::new(LIMIT, REFILL_RATE)));

pub struct OkexLimiter {
    bucket: limiter::FixedWindowBucket,
}

impl OkexLimiter {
    pub fn new(limit: usize, refill_rate: Duration) -> Self {
        let effective_limit = (limit as f32 * (1.0 - LIMITER_BUFFER_PCT)) as usize;
        Self {
            bucket: limiter::FixedWindowBucket::new(effective_limit, refill_rate),
        }
    }
}

impl RateLimiter for OkexLimiter {
    fn prepare_request(&mut self, weight: usize) -> Option<Duration> {
        self.bucket.calculate_wait_time(weight)
    }

    fn update_from_response(&mut self, _response: &reqwest::Response, weight: usize) {
        self.bucket.consume_tokens(weight);
    }

    fn should_exit_on_response(&self, response: &reqwest::Response) -> bool {
        response.status() == 429
    }
}

#[derive(Deserialize, Debug)]
struct SonicTrade {
    #[serde(rename = "ts", deserialize_with = "de_string_to_u64")]
    pub time: u64,
    #[serde(rename = "px", deserialize_with = "de_string_to_f32")]
    pub price: f32,
    #[serde(rename = "sz", deserialize_with = "de_string_to_f32")]
    pub qty: f32,
    #[serde(rename = "side")]
    pub is_sell: String,
}

struct SonicDepth {
    pub update_id: u64,
    pub bids: Vec<DeOrder>,
    pub asks: Vec<DeOrder>,
}

enum StreamData {
    Trade(Vec<SonicTrade>),
    Depth(SonicDepth, String, u64),
}

fn feed_de(slice: &[u8], _ticker: Ticker) -> Result<StreamData, AdapterError> {
    let v: Value =
        serde_json::from_slice(slice).map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let mut channel = String::new();
    if let Some(arg) = v.get("arg")
        && let Some(ch) = arg.get("channel").and_then(|c| c.as_str())
    {
        channel = ch.to_string();
    }

    if let Some(action) = v.get("action").and_then(|a| a.as_str())
        && let Some(data_arr) = v.get("data")
        && let Some(first) = data_arr.get(0)
    {
        let bids: Vec<DeOrder> = if let Some(b) = first.get("bids") {
            serde_json::from_value(b.clone())
                .map_err(|e| AdapterError::ParseError(e.to_string()))?
        } else {
            Vec::new()
        };
        let asks: Vec<DeOrder> = if let Some(a) = first.get("asks") {
            serde_json::from_value(a.clone())
                .map_err(|e| AdapterError::ParseError(e.to_string()))?
        } else {
            Vec::new()
        };

        let seq_id = first.get("seqId").and_then(|s| s.as_u64()).unwrap_or(0);

        let time = first
            .get("ts")
            .and_then(|t| t.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let depth = SonicDepth {
            update_id: seq_id,
            bids,
            asks,
        };

        match channel.as_str() {
            "books" => {
                let dtype = if action == "update" {
                    "delta"
                } else {
                    "snapshot"
                };
                return Ok(StreamData::Depth(depth, dtype.to_string(), time));
            }
            _ => {
                return Err(AdapterError::ParseError(
                    "Depth message for non-depth subscription".to_string(),
                ));
            }
        }
    }

    if let Some(data_arr) = v.get("data") {
        let trades: Vec<SonicTrade> = serde_json::from_value(data_arr.clone())
            .map_err(|e| AdapterError::ParseError(e.to_string()))?;

        if matches!(channel.as_str(), "trades" | "trade") {
            return Ok(StreamData::Trade(trades));
        }
    }

    Err(AdapterError::ParseError("Unknown data".to_string()))
}

async fn try_connect(
    streams: &Value,
    exchange: Exchange,
    output: &mut mpsc::Sender<Event>,
    topic: &str,
) -> State {
    let url = format!("wss://{WS_DOMAIN}/ws/v5/{topic}");

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
        let exchange = ticker.exchange;

        let subscribe_message = serde_json::json!({
            "op": "subscribe",
            "args": [
                { "channel": "trades", "instId": symbol_str },
                { "channel": "books",  "instId": symbol_str },
            ],
        });

        let mut trades_buffer: Vec<Trade> = vec![];
        let mut orderbook = LocalDepthCache::default();

        let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
        let contract_size = ticker_info.contract_size.map(f32::from);

        loop {
            match &mut state {
                State::Disconnected => {
                    state = try_connect(&subscribe_message, exchange, &mut output, "public").await;
                }
                State::Connected(ws) => match ws.read_frame().await {
                    Ok(msg) => match msg.opcode {
                        OpCode::Text => {
                            if let Ok(data) = feed_de(&msg.payload[..], ticker) {
                                match data {
                                    StreamData::Trade(de_trade_vec) => {
                                        for de_trade in &de_trade_vec {
                                            let price = Price::from_f32(de_trade.price)
                                                .round_to_min_tick(ticker_info.min_ticksize);
                                            let qty = calc_qty(
                                                de_trade.qty,
                                                de_trade.price,
                                                size_in_quote_ccy,
                                                contract_size,
                                                market_type,
                                            );

                                            let trade = Trade {
                                                time: de_trade.time,
                                                is_sell: de_trade.is_sell == "sell"
                                                    || de_trade.is_sell == "SELL",
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
                                                    qty: calc_qty(
                                                        x.qty,
                                                        x.price,
                                                        size_in_quote_ccy,
                                                        contract_size,
                                                        market_type,
                                                    ),
                                                })
                                                .collect(),
                                            asks: de_depth
                                                .asks
                                                .iter()
                                                .map(|x| DeOrder {
                                                    price: x.price,
                                                    qty: calc_qty(
                                                        x.qty,
                                                        x.price,
                                                        size_in_quote_ccy,
                                                        contract_size,
                                                        market_type,
                                                    ),
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

        let mut args = Vec::with_capacity(streams.len());
        let mut lookup = HashMap::new();
        for (ticker_info, timeframe) in &streams {
            let ticker = ticker_info.ticker;

            if let Some(bar) = timeframe_to_okx_bar(*timeframe) {
                let (symbol, _mt) = ticker.to_full_symbol_and_type();
                let channel = format!("candle{bar}");
                args.push(serde_json::json!({
                    "channel": channel,
                    "instId": symbol,
                }));
                lookup.insert((channel, symbol), (*ticker_info, *timeframe));
            }
        }

        let exchange = streams
            .first()
            .map(|(t, _)| t.exchange())
            .unwrap_or_else(|| Exchange::OkexSpot);

        let subscribe_message = serde_json::json!({
            "op": "subscribe",
            "args": args,
        });

        let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;

        loop {
            match &mut state {
                State::Disconnected => {
                    state =
                        try_connect(&subscribe_message, exchange, &mut output, "business").await;
                }
                State::Connected(ws) => match ws.read_frame().await {
                    Ok(msg) => match msg.opcode {
                        OpCode::Text => {
                            if let Ok(v) = serde_json::from_slice::<Value>(&msg.payload[..]) {
                                let channel = v["arg"]["channel"].as_str().unwrap_or("");
                                if !channel.starts_with("candle") {
                                    continue;
                                }

                                let inst = match v["arg"]["instId"].as_str() {
                                    Some(s) => s,
                                    None => continue,
                                };
                                let (ticker_info, timeframe) =
                                    match lookup.get(&(channel.to_string(), inst.to_string())) {
                                        Some(t) => *t,
                                        None => continue,
                                    };

                                let contract_size = ticker_info.contract_size.map(f32::from);

                                if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
                                    for row in data {
                                        let time = row
                                            .get(0)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<u64>().ok());
                                        let open = row
                                            .get(1)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<f32>().ok());
                                        let high = row
                                            .get(2)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<f32>().ok());
                                        let low = row
                                            .get(3)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<f32>().ok());
                                        let close = row
                                            .get(4)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<f32>().ok());
                                        let volume = row
                                            .get(5)
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| s.parse::<f32>().ok());

                                        let (ts, open, high, low, close) =
                                            match (time, open, high, low, close) {
                                                (
                                                    Some(ts),
                                                    Some(open),
                                                    Some(high),
                                                    Some(low),
                                                    Some(close),
                                                ) => (ts, open, high, low, close),
                                                _ => continue,
                                            };

                                        let volume_in_display = if let Some(vq) = volume {
                                            calc_qty(
                                                vq,
                                                close,
                                                size_in_quote_ccy,
                                                contract_size,
                                                market_type,
                                            )
                                        } else {
                                            0.0
                                        };

                                        let kline = Kline::new(
                                            ts,
                                            open,
                                            high,
                                            low,
                                            close,
                                            (-1.0, volume_in_display),
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

fn calc_qty(
    qty: f32,
    price: f32,
    size_in_quote_ccy: bool,
    contract_size: Option<f32>,
    market: MarketKind,
) -> f32 {
    let is_inverse = matches!(market, MarketKind::InversePerps);

    match contract_size {
        Some(cs) => {
            if is_inverse {
                if size_in_quote_ccy { qty * cs } else { qty }
            } else if size_in_quote_ccy {
                qty * cs * price
            } else {
                qty * cs
            }
        }
        None => {
            if size_in_quote_ccy {
                qty * price
            } else {
                qty
            }
        }
    }
}

fn okx_inst_type(m: MarketKind) -> &'static str {
    match m {
        MarketKind::Spot => "SPOT",
        MarketKind::LinearPerps | MarketKind::InversePerps => "SWAP",
    }
}

fn timeframe_to_okx_bar(tf: Timeframe) -> Option<&'static str> {
    Some(match tf {
        Timeframe::M1 => "1m",
        Timeframe::M3 => "3m",
        Timeframe::M5 => "5m",
        Timeframe::M15 => "15m",
        Timeframe::M30 => "30m",
        Timeframe::H1 => "1H",
        Timeframe::H2 => "2H",
        Timeframe::H4 => "4H",
        Timeframe::H12 => "12Hutc",
        Timeframe::D1 => "1Dutc",
        _ => return None,
    })
}

pub async fn fetch_ticksize(
    market_type: MarketKind,
) -> Result<std::collections::HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    let inst_type = okx_inst_type(market_type);
    let url = format!(
        "https://www.okx.com/api/v5/public/instruments?instType={}",
        inst_type
    );

    let response_text = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(AdapterError::FetchError)?
        .text()
        .await
        .map_err(AdapterError::FetchError)?;

    let doc: Value = serde_json::from_str(&response_text)
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let list = doc["data"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Result list is not an array".to_string()))?;

    let exchange = match market_type {
        MarketKind::Spot => Exchange::OkexSpot,
        MarketKind::LinearPerps => Exchange::OkexLinear,
        MarketKind::InversePerps => Exchange::OkexInverse,
    };

    let mut map = std::collections::HashMap::new();

    for item in list {
        let symbol = match item["instId"].as_str() {
            Some(s) => s,
            None => continue,
        };

        if item["state"].as_str().unwrap_or("") != "live" {
            continue;
        }

        let accept = match market_type {
            MarketKind::Spot => item["quoteCcy"].as_str() == Some("USDT"),
            MarketKind::LinearPerps => {
                item["ctType"].as_str() == Some("linear")
                    && (item["settleCcy"].as_str() == Some("USDT"))
            }
            MarketKind::InversePerps => item["ctType"].as_str() == Some("inverse"),
        };
        if !accept {
            continue;
        }

        if !is_symbol_supported(symbol, exchange, true) {
            continue;
        }

        let min_ticksize = item["tickSz"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .ok_or_else(|| AdapterError::ParseError("Tick size not found".to_string()))?;
        let min_qty = item["lotSz"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .ok_or_else(|| AdapterError::ParseError("Lot size not found".to_string()))?;
        let contract_size = if market_type == MarketKind::Spot {
            None
        } else {
            item["ctVal"].as_str().and_then(|s| s.parse::<f32>().ok())
        };

        let ticker = Ticker::new(symbol, exchange);
        let info = TickerInfo::new(ticker, min_ticksize, min_qty, contract_size);

        map.insert(ticker, Some(info));
    }

    Ok(map)
}

pub async fn fetch_ticker_prices(
    market_type: MarketKind,
) -> Result<std::collections::HashMap<Ticker, TickerStats>, AdapterError> {
    let inst_type = okx_inst_type(market_type);
    let url = format!(
        "https://www.okx.com/api/v5/market/tickers?instType={}",
        inst_type
    );

    let parsed_response: Value =
        limiter::http_parse_with_limiter(&url, &OKEX_LIMITER, 1, None, None).await?;

    let list = parsed_response["data"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Result list is not an array".to_string()))?;

    let exchange = match market_type {
        MarketKind::Spot => Exchange::OkexSpot,
        MarketKind::LinearPerps => Exchange::OkexLinear,
        MarketKind::InversePerps => Exchange::OkexInverse,
    };

    let mut map = std::collections::HashMap::new();

    for item in list {
        let symbol = match item["instId"].as_str() {
            Some(s) => s,
            None => continue,
        };

        if !is_symbol_supported(symbol, exchange, false) {
            continue;
        }

        let last_trade_price = item["last"].as_str().and_then(|s| s.parse::<f32>().ok());
        let open24h = item["open24h"].as_str().and_then(|s| s.parse::<f32>().ok());

        let Some(vol24h) = item["volCcy24h"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
        else {
            continue;
        };

        let (last_price, previous_daily_open) =
            if let (Some(last), Some(previous_daily_open)) = (last_trade_price, open24h) {
                (last, previous_daily_open)
            } else {
                continue;
            };
        let daily_price_chg = if previous_daily_open > 0.0 {
            (last_price - previous_daily_open) / previous_daily_open * 100.0
        } else {
            0.0
        };

        let volume_usd =
            if market_type == MarketKind::LinearPerps || market_type == MarketKind::InversePerps {
                vol24h * last_price
            } else {
                vol24h
            };

        map.insert(
            Ticker::new(symbol, exchange),
            TickerStats {
                mark_price: last_price,
                daily_price_chg,
                daily_volume: volume_usd,
            },
        );
    }

    Ok(map)
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let ticker = ticker_info.ticker;

    let (symbol_str, market) = ticker.to_full_symbol_and_type();
    let contract_size = ticker_info.contract_size.map(f32::from);

    let bar = timeframe_to_okx_bar(timeframe).ok_or_else(|| {
        AdapterError::InvalidRequest(format!("Unsupported timeframe: {timeframe}"))
    })?;

    let mut url = format!(
        "https://www.okx.com/api/v5/market/history-candles?instId={}&bar={}&limit={}",
        symbol_str,
        bar,
        match range {
            Some((start, end)) => {
                ((end - start) / timeframe.to_milliseconds()).clamp(1, 300)
            }
            None => 300,
        }
    );

    if let Some((start, end)) = range {
        url.push_str(&format!("&before={start}&after={end}"));
    }

    let doc: Value = limiter::http_parse_with_limiter(&url, &OKEX_LIMITER, 1, None, None).await?;

    let list = doc["data"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Kline result is not an array".to_string()))?;

    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;

    let mut klines: Vec<Kline> = Vec::with_capacity(list.len());

    for row in list {
        let time = row
            .get(0)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok());
        let open = row
            .get(1)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f32>().ok());
        let high = row
            .get(2)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f32>().ok());
        let low = row
            .get(3)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f32>().ok());
        let close = row
            .get(4)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f32>().ok());
        let volume = row
            .get(5)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f32>().ok());

        let (ts, open, high, low, close) = match (time, open, high, low, close) {
            (Some(ts), Some(o), Some(h), Some(l), Some(c)) => (ts, o, h, l, c),
            _ => continue,
        };
        let volume_in_display = if let Some(vq) = volume {
            calc_qty(vq, close, size_in_quote_ccy, contract_size, market)
        } else {
            0.0
        };

        let kline = Kline::new(
            ts,
            open,
            high,
            low,
            close,
            (-1.0, volume_in_display),
            ticker_info.min_ticksize,
        );

        klines.push(kline);
    }

    klines.sort_by_key(|k| k.time);
    Ok(klines)
}

const TRADING_STATS_DOMAIN: &str = "https://www.okx.com/api/v5/rubik/stat";

pub async fn fetch_historical_oi(
    ticker: Ticker,
    range: Option<(u64, u64)>,
    period: Timeframe,
) -> Result<Vec<OpenInterest>, AdapterError> {
    let (ticker_str, _market) = ticker.to_full_symbol_and_type();

    let bar = timeframe_to_okx_bar(period)
        .ok_or_else(|| AdapterError::InvalidRequest(format!("Unsupported timeframe: {period}")))?;

    let mut url = TRADING_STATS_DOMAIN.to_string()
        + format!("/contracts/open-interest-history?instId={ticker_str}&period={bar}").as_str();

    if let Some((start, end)) = range {
        url.push_str(&format!("&begin={start}&end={end}"));
    }

    let response_text =
        limiter::http_request_with_limiter(&url, &OKEX_LIMITER, 1, None, None).await?;

    let doc: Value = serde_json::from_str(&response_text)
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let list = doc["data"]
        .as_array()
        .ok_or_else(|| AdapterError::ParseError("Fetch result is not an array".to_string()))?;

    // data = [ [ts, oi, oiCcy, oiUsd], ... ]
    let open_interest: Vec<OpenInterest> = list
        .iter()
        .filter_map(|row| {
            let arr = row.as_array()?;
            let ts = arr.first()?.as_str()?.parse::<u64>().ok()?;
            let oi_ccy = arr.get(2)?.as_str()?.parse::<f32>().ok()?;
            Some(OpenInterest {
                time: ts,
                value: oi_ccy,
            })
        })
        .collect();

    Ok(open_interest)
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct OkxRestTradeItem {
    #[serde(rename = "tradeId")]
    pub trade_id: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub ts: String,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct OkxRestTradeResponse {
    pub data: Vec<OkxRestTradeItem>,
}

pub async fn fetch_intraday_trades(
    ticker_info: TickerInfo,
    from: u64,
) -> Result<Vec<Trade>, AdapterError> {
    let ticker = ticker_info.ticker;
    let (symbol_str, _) = ticker.to_full_symbol_and_type();
    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;

    let mut all_trades = Vec::new();
    let mut cursor: Option<String> = None;

    // Fetch up to 10 pages (1000 trades max) backwards to prevent blocking Tokio with rate limits
    for _ in 0..10 {
        let mut url = format!(
            "https://www.okx.com/api/v5/market/history-trades?instId={}&limit=100",
            symbol_str
        );
        if let Some(ref c) = cursor {
            url.push_str(&format!("&after={c}"));
        }

        let resp: OkxRestTradeResponse =
            limiter::http_parse_with_limiter(&url, &OKEX_LIMITER, 1, None, None).await?;

        if resp.data.is_empty() {
            break;
        }

        let last_trade_id = resp.data.last().map(|t| t.trade_id.clone());
        let mut reached_from = false;

        for item in resp.data {
            let time = match item.ts.parse::<u64>() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if time < from {
                reached_from = true;
            }

            let price_f32 = match item.px.parse::<f32>() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let mut qty = match item.sz.parse::<f32>() {
                Ok(q) => q,
                Err(_) => continue,
            };

            if size_in_quote_ccy {
                qty = (qty * price_f32).round();
            }

            let is_sell = item.side.eq_ignore_ascii_case("sell") || item.side == "2";
            let price = Price::from_f32(price_f32).round_to_min_tick(ticker_info.min_ticksize);

            all_trades.push(Trade {
                time,
                is_sell,
                price,
                qty,
            });
        }

        if reached_from || last_trade_id.is_none() {
            break;
        }
        cursor = last_trade_id;
    }

    all_trades.retain(|t| t.time >= from);
    all_trades.sort_by_key(|t| t.time);
    all_trades.dedup_by(|a, b| a.time == b.time && a.price == b.price && a.qty == b.qty);
    Ok(all_trades)
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
            "Using binary cached OKX trades for {date} ({} trades)",
            trades.len()
        );
        return Ok(trades);
    }

    let ticker = ticker_info.ticker;
    let (symbol_str, _) = ticker.to_full_symbol_and_type();

    let date_str = date.format("%Y%m%d");
    let file_name = format!("{symbol_str}-trades-{date_str}.zip");
    let url =
        format!("https://static.okx.com/cdn/okex/traderecords/trades/daily/{date_str}/{file_name}");

    log::info!("Downloading OKX historical trades from {url}");
    let resp = reqwest::get(&url).await.map_err(AdapterError::FetchError)?;
    if !resp.status().is_success() {
        return Err(AdapterError::InvalidRequest(format!(
            "Failed to fetch OKX trades from {url}: status {}",
            resp.status()
        )));
    }

    let body = resp.bytes().await.map_err(AdapterError::FetchError)?;
    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
    let min_ticksize = ticker_info.min_ticksize;

    let trades = tokio::task::spawn_blocking(move || -> Result<Vec<Trade>, AdapterError> {
        let cursor = std::io::Cursor::new(body);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| AdapterError::ParseError(format!("Failed to open OKX zip: {e}")))?;

        let mut trades = Vec::new();

        for i in 0..archive.len() {
            let zip_file = archive
                .by_index(i)
                .map_err(|e| AdapterError::ParseError(format!("Failed to read zip entry: {e}")))?;

            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(true)
                .from_reader(BufReader::new(zip_file));

            let headers = rdr
                .headers()
                .map_err(|e| {
                    AdapterError::ParseError(format!("Failed to read OKX CSV headers: {e}"))
                })?
                .clone();

            let time_col = headers
                .iter()
                .position(|h| {
                    h.eq_ignore_ascii_case("ts")
                        || h.eq_ignore_ascii_case("time")
                        || h.eq_ignore_ascii_case("timestamp")
                })
                .unwrap_or(0);
            let side_col = headers
                .iter()
                .position(|h| h.eq_ignore_ascii_case("side") || h.eq_ignore_ascii_case("type"))
                .unwrap_or(3);
            let size_col = headers
                .iter()
                .position(|h| {
                    h.eq_ignore_ascii_case("sz")
                        || h.eq_ignore_ascii_case("qty")
                        || h.eq_ignore_ascii_case("size")
                        || h.eq_ignore_ascii_case("volume")
                })
                .unwrap_or(2);
            let price_col = headers
                .iter()
                .position(|h| h.eq_ignore_ascii_case("px") || h.eq_ignore_ascii_case("price"))
                .unwrap_or(1);

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
                    .map(|s| s.eq_ignore_ascii_case("sell") || s == "2")
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
        }

        trades.sort_by_key(|t| t.time);
        trades.dedup_by(|a, b| a.time == b.time && a.price == b.price && a.qty == b.qty);
        Ok(trades)
    })
    .await
    .map_err(|e| {
        AdapterError::ParseError(format!("Join error during OKX trades parsing: {e}"))
    })??;

    if USE_BINARY_CACHE
        && !trades.is_empty()
        && let Err(e) = save_raw_trades_to_cache(&base_path, &ticker_info, date, &trades)
    {
        log::warn!("Failed to save OKX binary cache: {e}");
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
        90
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
                "OKX historical trades fetch failed for {}: {}, falling back to intraday fetch if recent",
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
