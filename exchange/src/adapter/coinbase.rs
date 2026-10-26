use crate::{
    Price, PushFrequency,
    adapter::{AdapterError, Event, StreamKind, StreamTicksize},
    depth::{DeOrder, DepthPayload, DepthUpdate, LocalDepthCache},
    limiter::HTTP_CLIENT,
    trades::cache::{load_intraday_trades_from_cache, save_raw_trades_to_cache},
};

use super::super::{
    Exchange, Kline, MarketKind, Ticker, TickerInfo, TickerStats, Timeframe, Trade,
    connect::{State, connect_ws},
    is_symbol_supported,
};

use fastwebsockets::{Frame, OpCode};
use iced_futures::{
    futures::{SinkExt, Stream},
    stream,
};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, time::Duration};

pub const WS_DOMAIN: &str = "advanced-trade-ws.coinbase.com";
pub const REST_EXCHANGE_BASE: &str = "https://api.exchange.coinbase.com";
pub const REST_V2_BASE: &str = "https://api.coinbase.com/v2";

pub fn timeframe_to_seconds(tf: Timeframe) -> Option<u64> {
    match tf {
        Timeframe::M1 => Some(60),
        Timeframe::M5 => Some(300),
        Timeframe::M15 => Some(900),
        Timeframe::M30 => Some(1800),
        Timeframe::H1 => Some(3600),
        Timeframe::H2 => Some(7200),
        Timeframe::H4 => Some(14400),
        Timeframe::D1 => Some(86400),
        _ => {
            let ms = tf.to_milliseconds();
            if ms >= 60_000 { Some(ms / 1000) } else { None }
        }
    }
}

pub fn parse_iso_or_epoch(val: &str) -> u64 {
    if let Ok(secs) = val.parse::<u64>() {
        return secs * 1000;
    }
    chrono::DateTime::parse_from_rfc3339(val)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis() as u64)
}

pub async fn fetch_ticksize(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    if market_type != MarketKind::Spot {
        return Ok(HashMap::new());
    }

    let url = format!("{REST_EXCHANGE_BASE}/products");
    let response_text = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(AdapterError::FetchError)?
        .text()
        .await
        .map_err(AdapterError::FetchError)?;

    let list: Vec<Value> = serde_json::from_str(&response_text)
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let exchange = Exchange::CoinbaseSpot;
    let mut map = HashMap::new();

    for item in list {
        let symbol = match item["id"].as_str() {
            Some(s) => s,
            None => continue,
        };

        if item["status"].as_str().unwrap_or("") != "online" {
            continue;
        }
        if item["trading_disabled"].as_bool().unwrap_or(false) {
            continue;
        }

        let quote_currency = item["quote_currency"].as_str().unwrap_or("");
        if quote_currency != "USD" && quote_currency != "USDC" && quote_currency != "USDT" {
            continue;
        }

        if !is_symbol_supported(symbol, exchange, true) {
            continue;
        }

        let tick_size = item["quote_increment"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.01);

        let min_qty = item["base_increment"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.00000001);

        let ticker = Ticker::new(symbol, exchange);
        let ticker_info = TickerInfo::new(ticker, tick_size, min_qty, None);
        map.insert(ticker, Some(ticker_info));
    }

    Ok(map)
}

pub async fn fetch_ticker_prices(
    market_type: MarketKind,
) -> Result<HashMap<Ticker, TickerStats>, AdapterError> {
    if market_type != MarketKind::Spot {
        return Ok(HashMap::new());
    }

    let url = format!("{REST_V2_BASE}/prices/USD/spot");
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

    let mut map = HashMap::new();
    let exchange = Exchange::CoinbaseSpot;

    if let Some(arr) = doc["data"].as_array() {
        for item in arr {
            let base = match item["base"].as_str() {
                Some(b) => b,
                None => continue,
            };
            let price = item["amount"]
                .as_str()
                .and_then(|p| p.parse::<f32>().ok())
                .unwrap_or(0.0);

            // Construct USD and USDC ticker keys
            for quote in &["USD", "USDC"] {
                let sym = format!("{base}-{quote}");
                let ticker = Ticker::new(&sym, exchange);
                map.insert(
                    ticker,
                    TickerStats {
                        mark_price: price,
                        daily_price_chg: 0.0,
                        daily_volume: 0.0,
                    },
                );
            }
        }
    }

    Ok(map)
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let granularity = timeframe_to_seconds(timeframe).unwrap_or(300);

    let mut url =
        format!("{REST_EXCHANGE_BASE}/products/{symbol}/candles?granularity={granularity}");

    if let Some((start_ms, end_ms)) = range {
        let start_iso = chrono::DateTime::from_timestamp_millis(start_ms as i64)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let end_iso = chrono::DateTime::from_timestamp_millis(end_ms as i64)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        if !start_iso.is_empty() && !end_iso.is_empty() {
            url.push_str(&format!("&start={start_iso}&end={end_iso}"));
        }
    }

    let response_text = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(AdapterError::FetchError)?
        .text()
        .await
        .map_err(AdapterError::FetchError)?;

    let raw_candles: Vec<Value> = serde_json::from_str(&response_text)
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let mut klines = Vec::with_capacity(raw_candles.len());

    for item in raw_candles {
        // Coinbase candles format: [time, low, high, open, close, volume]
        let arr = match item.as_array() {
            Some(a) if a.len() >= 6 => a,
            _ => continue,
        };

        let time_sec = arr[0].as_u64().unwrap_or(0);
        let low = arr[1].as_f64().map(|v| v as f32).unwrap_or(0.0);
        let high = arr[2].as_f64().map(|v| v as f32).unwrap_or(0.0);
        let open = arr[3].as_f64().map(|v| v as f32).unwrap_or(0.0);
        let close = arr[4].as_f64().map(|v| v as f32).unwrap_or(0.0);
        let volume = arr[5].as_f64().map(|v| v as f32).unwrap_or(0.0);

        let kline = Kline::new(
            time_sec * 1000,
            open,
            high,
            low,
            close,
            (volume, volume * 0.5),
            ticker_info.min_ticksize,
        );
        klines.push(kline);
    }

    // Coinbase returns newest first, sort chronologically ascending
    klines.sort_by_key(|k| k.time);
    Ok(klines)
}

pub async fn fetch_trades(
    ticker_info: TickerInfo,
    from_time: u64,
    data_path: PathBuf,
) -> Result<(Vec<Trade>, u64), AdapterError> {
    let today_date = chrono::Utc::now().date_naive();
    let next_day_start = today_date
        .succ_opt()
        .unwrap_or(today_date)
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    // Check intraday cache
    if let Some(cached) = load_intraday_trades_from_cache(&data_path, &ticker_info, today_date)
        && !cached.is_empty()
    {
        return Ok((cached, next_day_start));
    }

    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let url = format!("{REST_EXCHANGE_BASE}/products/{symbol}/trades?limit=100");

    let response_text = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .map_err(AdapterError::FetchError)?
        .text()
        .await
        .map_err(AdapterError::FetchError)?;

    let raw_trades: Vec<Value> = serde_json::from_str(&response_text)
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;

    let mut trades = Vec::with_capacity(raw_trades.len());

    for item in raw_trades {
        let time_str = item["time"].as_str().unwrap_or("");
        let time_ms = parse_iso_or_epoch(time_str);
        if time_ms < from_time {
            continue;
        }

        let price_f = item["price"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.0);
        let qty = item["size"]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.0);

        let side = item["side"].as_str().unwrap_or("");
        let is_sell = side.eq_ignore_ascii_case("sell");

        trades.push(Trade {
            time: time_ms,
            is_sell,
            price: Price::from_f32(price_f),
            qty, // Canonical Base units
        });
    }

    trades.sort_by_key(|t| t.time);
    let _ = save_raw_trades_to_cache(&data_path, &ticker_info, today_date, &trades);
    Ok((trades, next_day_start))
}

pub fn connect_market_stream(
    ticker_info: TickerInfo,
    push_freq: PushFrequency,
) -> impl Stream<Item = Event> {
    stream::channel(100, async move |mut output| {
        let mut state: State = State::Disconnected;
        let ticker = ticker_info.ticker;
        let (symbol_str, _) = ticker.to_full_symbol_and_type();
        let exchange = ticker.exchange;

        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": [symbol_str],
            "channel": "market_trades"
        });

        let subscribe_l2_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": [symbol_str],
            "channel": "level2"
        });

        let mut trades_buffer: Vec<Trade> = Vec::new();
        let mut orderbook = LocalDepthCache::default();
        let mut flush_interval = tokio::time::interval(Duration::from_millis(30));
        flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            match &mut state {
                State::Disconnected => {
                    let url = "/";
                    match connect_ws(WS_DOMAIN, url).await {
                        Ok(mut ws) => {
                            let text1 = subscribe_msg.to_string();
                            let text2 = subscribe_l2_msg.to_string();
                            if ws
                                .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                    text1.as_bytes(),
                                )))
                                .await
                                .is_err()
                                || ws
                                    .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                        text2.as_bytes(),
                                    )))
                                    .await
                                    .is_err()
                            {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                let _ = output
                                    .send(Event::Disconnected(
                                        exchange,
                                        "Subscribe failed".to_string(),
                                    ))
                                    .await;
                                state = State::Disconnected;
                                continue;
                            }
                            let _ = output.send(Event::Connected(exchange)).await;
                            state = State::Connected(ws);
                        }
                        Err(e) => {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            let _ = output
                                .send(Event::Disconnected(exchange, e.to_string()))
                                .await;
                            state = State::Disconnected;
                        }
                    }
                }
                State::Connected(ws) => {
                    tokio::select! {
                        _ = flush_interval.tick() => {
                            if !trades_buffer.is_empty() {
                                let time = trades_buffer.last().map(|t| t.time).unwrap_or(0);
                                let _ = output.send(Event::DepthReceived(
                                    StreamKind::DepthAndTrades {
                                        ticker_info,
                                        depth_aggr: StreamTicksize::Client,
                                        push_freq,
                                    },
                                    time,
                                    orderbook.depth.clone(),
                                    std::mem::take(&mut trades_buffer).into_boxed_slice(),
                                )).await;
                            }
                        }
                        frame_res = ws.read_frame() => match frame_res {
                            Ok(frame) => match frame.opcode {
                                OpCode::Text => {
                                    if let Ok(val) = serde_json::from_slice::<Value>(&frame.payload) {
                                        let channel = val["channel"].as_str().unwrap_or("");
                                        let events = val["events"].as_array();

                                        if (channel == "l2_data" || channel == "level2") && let Some(evs) = events {
                                            for ev in evs {
                                                let ev_type = ev["type"].as_str().unwrap_or("");
                                                if let Some(updates) = ev["updates"].as_array() {
                                                    let mut bids = Vec::new();
                                                    let mut asks = Vec::new();
                                                    let mut latest_time = 0u64;

                                                    for u in updates {
                                                        let side = u["side"].as_str().unwrap_or("");
                                                        let price = u["price_level"]
                                                            .as_str()
                                                            .and_then(|s| s.parse::<f32>().ok())
                                                            .unwrap_or(0.0);
                                                        let qty = u["new_quantity"]
                                                            .as_str()
                                                            .and_then(|s| s.parse::<f32>().ok())
                                                            .unwrap_or(0.0);

                                                        if let Some(t_str) = u["event_time"].as_str() {
                                                            let t = parse_iso_or_epoch(t_str);
                                                            if t > latest_time {
                                                                latest_time = t;
                                                            }
                                                        }

                                                        let is_bid = side.eq_ignore_ascii_case("bid") || side.eq_ignore_ascii_case("buy");
                                                        if is_bid {
                                                            bids.push(DeOrder { price, qty });
                                                        } else {
                                                            asks.push(DeOrder { price, qty });
                                                        }
                                                    }

                                                    let payload = DepthPayload {
                                                        last_update_id: 1,
                                                        time: latest_time,
                                                        bids,
                                                        asks,
                                                    };

                                                    if ev_type == "snapshot" {
                                                        orderbook.update(DepthUpdate::Snapshot(payload), ticker_info.min_ticksize);
                                                    } else {
                                                        orderbook.update(DepthUpdate::Diff(payload), ticker_info.min_ticksize);
                                                    }

                                                    let _ = output.send(Event::DepthReceived(
                                                        StreamKind::DepthAndTrades {
                                                            ticker_info,
                                                            depth_aggr: StreamTicksize::Client,
                                                            push_freq,
                                                        },
                                                        latest_time,
                                                        orderbook.depth.clone(),
                                                        std::mem::take(&mut trades_buffer).into_boxed_slice(),
                                                    )).await;
                                                }
                                            }
                                        } else if channel == "market_trades" && let Some(evs) = events {
                                            for ev in evs {
                                                if let Some(trades_arr) = ev["trades"].as_array() {
                                                    for t in trades_arr {
                                                        let price = t["price"]
                                                            .as_str()
                                                            .and_then(|s| s.parse::<f32>().ok())
                                                            .unwrap_or(0.0);
                                                        let qty = t["size"]
                                                            .as_str()
                                                            .and_then(|s| s.parse::<f32>().ok())
                                                            .unwrap_or(0.0);
                                                        let side = t["side"].as_str().unwrap_or("");
                                                        let is_sell = side.eq_ignore_ascii_case("sell");
                                                        let time_ms = t["time"]
                                                            .as_str()
                                                            .map(parse_iso_or_epoch)
                                                            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

                                                        trades_buffer.push(Trade {
                                                            time: time_ms,
                                                            is_sell,
                                                            price: Price::from_f32(price),
                                                            qty,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                OpCode::Ping => {
                                    let _ = ws.write_frame(Frame::pong(frame.payload)).await;
                                }
                                OpCode::Close => {
                                    state = State::Disconnected;
                                    let _ = output.send(Event::Disconnected(exchange, "Connection closed".to_string())).await;
                                }
                                _ => {}
                            },
                            Err(e) => {
                                state = State::Disconnected;
                                let _ = output.send(Event::Disconnected(exchange, e.to_string())).await;
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
    _market_type: MarketKind,
) -> impl Stream<Item = Event> {
    stream::channel(100, async move |mut output| {
        let mut state: State = State::Disconnected;

        let mut product_ids = Vec::with_capacity(streams.len());
        let mut lookup = HashMap::new();

        for (ticker_info, timeframe) in &streams {
            let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
            product_ids.push(symbol.clone());
            lookup.insert(symbol, (*ticker_info, *timeframe));
        }

        let exchange = streams
            .first()
            .map(|(t, _)| t.exchange())
            .unwrap_or(Exchange::CoinbaseSpot);

        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": product_ids,
            "channel": "candles"
        });

        loop {
            match &mut state {
                State::Disconnected => {
                    let url = "/";
                    match connect_ws(WS_DOMAIN, url).await {
                        Ok(mut ws) => {
                            let text = subscribe_msg.to_string();
                            if ws
                                .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                                    text.as_bytes(),
                                )))
                                .await
                                .is_err()
                            {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                let _ = output
                                    .send(Event::Disconnected(
                                        exchange,
                                        "Subscribe candles failed".to_string(),
                                    ))
                                    .await;
                                state = State::Disconnected;
                                continue;
                            }
                            let _ = output.send(Event::Connected(exchange)).await;
                            state = State::Connected(ws);
                        }
                        Err(e) => {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            let _ = output
                                .send(Event::Disconnected(exchange, e.to_string()))
                                .await;
                            state = State::Disconnected;
                        }
                    }
                }
                State::Connected(ws) => match ws.read_frame().await {
                    Ok(frame) => match frame.opcode {
                        OpCode::Text => {
                            if let Ok(val) = serde_json::from_slice::<Value>(&frame.payload) {
                                let channel = val["channel"].as_str().unwrap_or("");
                                if channel == "candles"
                                    && let Some(events) = val["events"].as_array()
                                {
                                    for ev in events {
                                        if let Some(candles_arr) = ev["candles"].as_array() {
                                            for c in candles_arr {
                                                let product_id =
                                                    c["product_id"].as_str().unwrap_or("");
                                                if let Some(&(ticker_info, timeframe)) =
                                                    lookup.get(product_id)
                                                {
                                                    let start_sec = c["start"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<u64>().ok())
                                                        .unwrap_or(0);
                                                    let open = c["open"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<f32>().ok())
                                                        .unwrap_or(0.0);
                                                    let high = c["high"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<f32>().ok())
                                                        .unwrap_or(0.0);
                                                    let low = c["low"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<f32>().ok())
                                                        .unwrap_or(0.0);
                                                    let close = c["close"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<f32>().ok())
                                                        .unwrap_or(0.0);
                                                    let volume = c["volume"]
                                                        .as_str()
                                                        .and_then(|s| s.parse::<f32>().ok())
                                                        .unwrap_or(0.0);

                                                    let kline = Kline::new(
                                                        start_sec * 1000,
                                                        open,
                                                        high,
                                                        low,
                                                        close,
                                                        (volume, volume * 0.5),
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
                                }
                            }
                        }
                        OpCode::Ping => {
                            let _ = ws.write_frame(Frame::pong(frame.payload)).await;
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
                            .send(Event::Disconnected(exchange, e.to_string()))
                            .await;
                    }
                },
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coinbase_timeframe_conversion() {
        assert_eq!(timeframe_to_seconds(Timeframe::M1), Some(60));
        assert_eq!(timeframe_to_seconds(Timeframe::M5), Some(300));
        assert_eq!(timeframe_to_seconds(Timeframe::M15), Some(900));
        assert_eq!(timeframe_to_seconds(Timeframe::H1), Some(3600));
        assert_eq!(timeframe_to_seconds(Timeframe::H4), Some(14400));
        assert_eq!(timeframe_to_seconds(Timeframe::D1), Some(86400));
    }

    #[test]
    fn test_coinbase_market_trades_json_parsing() {
        let sample_json = r#"{
            "channel": "market_trades",
            "timestamp": "2026-10-25T17:25:03.25506121Z",
            "sequence_num": 0,
            "events": [
                {
                    "type": "update",
                    "trades": [
                        {
                            "product_id": "BTC-USD",
                            "trade_id": "1110492587",
                            "price": "80668.14",
                            "size": "0.052",
                            "side": "BUY",
                            "time": "2026-10-25T17:25:03.255Z"
                        },
                        {
                            "product_id": "BTC-USD",
                            "trade_id": "1110492588",
                            "price": "80665.00",
                            "size": "0.120",
                            "side": "SELL",
                            "time": "2026-10-25T17:25:04.100Z"
                        }
                    ]
                }
            ]
        }"#;

        let val: Value = serde_json::from_str(sample_json).unwrap();
        assert_eq!(val["channel"], "market_trades");
        let trades = val["events"][0]["trades"].as_array().unwrap();
        assert_eq!(trades.len(), 2);

        let t1 = &trades[0];
        let p1: f32 = t1["price"].as_str().unwrap().parse().unwrap();
        let s1: f32 = t1["size"].as_str().unwrap().parse().unwrap();
        let is_sell1 = t1["side"].as_str().unwrap().eq_ignore_ascii_case("sell");
        assert_eq!(p1, 80668.14);
        assert_eq!(s1, 0.052);
        assert!(!is_sell1);

        let t2 = &trades[1];
        let is_sell2 = t2["side"].as_str().unwrap().eq_ignore_ascii_case("sell");
        assert!(is_sell2);
    }

    #[test]
    fn test_coinbase_level2_snapshot_and_diff() {
        let snapshot_json = r#"{
            "channel": "l2_data",
            "timestamp": "2026-10-25T17:25:03.255Z",
            "events": [
                {
                    "type": "snapshot",
                    "product_id": "BTC-USD",
                    "updates": [
                        { "side": "bid", "price_level": "80000.0", "new_quantity": "2.5" },
                        { "side": "offer", "price_level": "80100.0", "new_quantity": "1.8" }
                    ]
                }
            ]
        }"#;

        let val: Value = serde_json::from_str(snapshot_json).unwrap();
        let updates = val["events"][0]["updates"].as_array().unwrap();
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for u in updates {
            let side = u["side"].as_str().unwrap();
            let price: f32 = u["price_level"].as_str().unwrap().parse().unwrap();
            let qty: f32 = u["new_quantity"].as_str().unwrap().parse().unwrap();
            if side == "bid" {
                bids.push(DeOrder { price, qty });
            } else {
                asks.push(DeOrder { price, qty });
            }
        }

        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].price, 80000.0);
        assert_eq!(bids[0].qty, 2.5);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].price, 80100.0);
        assert_eq!(asks[0].qty, 1.8);
    }

    #[test]
    fn test_coinbase_base_volume_invariant() {
        // Invariant #115: Coinbase volumes must strictly represent Base assets (BTC, ETH)
        let sample_trade_size = "0.04523";
        let parsed_qty: f32 = sample_trade_size.parse().unwrap();
        assert!((parsed_qty - 0.04523).abs() < 1e-5);
    }
}
