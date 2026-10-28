//! Real forced-liquidation feed (ROADMAP #118).
//!
//! Binance futures publish each forced order on `<symbol>@forceOrder` (USD-M on `fstream`,
//! COIN-M on `dstream`); Bybit publishes them on the `allLiquidation.<symbol>` topic of its
//! linear and inverse public streams. Other venues are not wired up yet.
//!
//! Unlike the liquidation heatmap, which is a model, these are the liquidations the exchange
//! actually executed. Binance throttles the stream to the largest liquidation per symbol per
//! second, so small ones can be missing there.

use crate::adapter::{Exchange, MarketKind};
use crate::connect::connect_ws;
use crate::{Ticker, TickerInfo};

use fastwebsockets::{Frame, OpCode};
use iced_futures::{
    futures::{SinkExt, Stream},
    stream,
};
use serde_json::Value;

const BYBIT_WS_DOMAIN: &str = "stream.bybit.com";
/// Bybit drops idle public connections after 10 minutes; its docs ask for a ping every 20 s.
const BYBIT_PING_SECS: u64 = 20;

/// One forced liquidation as reported by the exchange.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Liquidation {
    pub time: u64,
    pub price: f32,
    /// Executed size in the exchange's own unit (base coin, or contracts on inverse markets).
    pub qty: f32,
    /// Executed size in USD.
    pub notional_usd: f64,
    /// A long position was force-closed (the exchange sold into the book).
    pub long_liquidated: bool,
}

#[derive(Debug, Clone)]
pub enum LiquidationEvent {
    Connected,
    Disconnected(String),
    Received(Vec<Liquidation>),
}

/// Venues whose liquidation stream is wired up.
pub fn is_supported(exchange: Exchange) -> bool {
    matches!(
        exchange,
        Exchange::BinanceLinear
            | Exchange::BinanceInverse
            | Exchange::BybitLinear
            | Exchange::BybitInverse
    )
}

fn str_f64(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// USD value of one inverse contract on Binance COIN-M (100 for BTCUSD, 10 otherwise).
fn binance_inverse_contract_usd(ticker: &Ticker) -> f64 {
    if ticker.to_full_symbol_and_type().0 == "BTCUSD_PERP" {
        100.0
    } else {
        10.0
    }
}

/// Parses one Binance `forceOrder` message (raw or wrapped in a combined-stream envelope).
pub fn parse_binance_force_order(
    payload: &[u8],
    market: MarketKind,
    ticker: &Ticker,
) -> Option<Liquidation> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    let v = v.get("data").unwrap_or(&v);
    if v.get("e")?.as_str()? != "forceOrder" {
        return None;
    }
    let o = v.get("o")?;
    // Average fill price and filled quantity; fall back to the order's own price/qty.
    let price = o
        .get("ap")
        .and_then(str_f64)
        .filter(|p| *p > 0.0)
        .or_else(|| o.get("p").and_then(str_f64))?;
    let qty = o
        .get("z")
        .and_then(str_f64)
        .filter(|q| *q > 0.0)
        .or_else(|| o.get("q").and_then(str_f64))?;
    let notional_usd = match market {
        MarketKind::InversePerps => qty * binance_inverse_contract_usd(ticker),
        _ => qty * price,
    };
    Some(Liquidation {
        time: o.get("T")?.as_u64()?,
        price: price as f32,
        qty: qty as f32,
        notional_usd,
        long_liquidated: o.get("S")?.as_str()? == "SELL",
    })
}

/// Parses one Bybit `allLiquidation` message into its liquidations.
pub fn parse_bybit_all_liquidation(payload: &[u8], market: MarketKind) -> Vec<Liquidation> {
    let Ok(v) = serde_json::from_slice::<Value>(payload) else {
        return Vec::new();
    };
    let is_topic = v
        .get("topic")
        .and_then(Value::as_str)
        .is_some_and(|t| t.starts_with("allLiquidation."));
    let Some(items) = v.get("data").and_then(Value::as_array).filter(|_| is_topic) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let price = item.get("p").and_then(str_f64)?;
            let qty = item.get("v").and_then(str_f64)?;
            // Inverse contracts are worth 1 USD each, so the size already is the notional.
            let notional_usd = match market {
                MarketKind::InversePerps => qty,
                _ => qty * price,
            };
            Some(Liquidation {
                time: item.get("T")?.as_u64()?,
                price: price as f32,
                qty: qty as f32,
                notional_usd,
                // Bybit reports the side of the liquidated position: "Buy" is a long.
                long_liquidated: item.get("S")?.as_str()? == "Buy",
            })
        })
        .collect()
}

/// Connects to the liquidation stream of `ticker_info` and reconnects after errors.
/// Yields nothing but a `Disconnected` notice for unsupported venues.
pub fn connect_liquidation_stream(ticker_info: TickerInfo) -> impl Stream<Item = LiquidationEvent> {
    stream::channel(100, async move |mut output| {
        let ticker = ticker_info.ticker;
        let exchange = ticker.exchange;
        if !is_supported(exchange) {
            let _ = output
                .send(LiquidationEvent::Disconnected(format!(
                    "No liquidation stream for {exchange}"
                )))
                .await;
            return;
        }
        let (symbol, market) = ticker.to_full_symbol_and_type();
        let is_binance = matches!(exchange, Exchange::BinanceLinear | Exchange::BinanceInverse);

        loop {
            let (domain, url, subscribe) = if is_binance {
                let domain = if market == MarketKind::InversePerps {
                    "dstream.binance.com"
                } else {
                    "fstream.binance.com"
                };
                let url = format!("wss://{domain}/ws/{}@forceOrder", symbol.to_lowercase());
                (domain, url, None)
            } else {
                let category = if market == MarketKind::InversePerps {
                    "inverse"
                } else {
                    "linear"
                };
                let url = format!("wss://{BYBIT_WS_DOMAIN}/v5/public/{category}");
                let sub = serde_json::json!({
                    "op": "subscribe",
                    "args": [format!("allLiquidation.{symbol}")],
                })
                .to_string();
                (BYBIT_WS_DOMAIN, url, Some(sub))
            };

            let mut ws = match connect_ws(domain, &url).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = output
                        .send(LiquidationEvent::Disconnected(format!(
                            "Failed to connect: {e}"
                        )))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    continue;
                }
            };
            if let Some(sub) = &subscribe
                && let Err(e) = ws
                    .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                        sub.as_bytes(),
                    )))
                    .await
            {
                let _ = output
                    .send(LiquidationEvent::Disconnected(format!(
                        "Failed subscribing: {e}"
                    )))
                    .await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
            let _ = output.send(LiquidationEvent::Connected).await;

            let mut ping = tokio::time::interval(tokio::time::Duration::from_secs(BYBIT_PING_SECS));
            ping.tick().await;
            let reason = loop {
                tokio::select! {
                    frame = ws.read_frame() => match frame {
                        Ok(msg) => match msg.opcode {
                            OpCode::Text => {
                                let found = if is_binance {
                                    parse_binance_force_order(&msg.payload, market, &ticker)
                                        .into_iter()
                                        .collect()
                                } else {
                                    parse_bybit_all_liquidation(&msg.payload, market)
                                };
                                if !found.is_empty()
                                    && output.send(LiquidationEvent::Received(found)).await.is_err()
                                {
                                    return;
                                }
                            }
                            OpCode::Ping => {
                                let _ = ws.write_frame(Frame::pong(msg.payload)).await;
                            }
                            OpCode::Close => break "Connection closed".to_string(),
                            _ => {}
                        },
                        Err(e) => break format!("Read error: {e}"),
                    },
                    _ = ping.tick(), if !is_binance => {
                        let msg = r#"{"op":"ping"}"#;
                        if let Err(e) = ws
                            .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(msg.as_bytes())))
                            .await
                        {
                            break format!("Ping failed: {e}");
                        }
                    }
                }
            };
            let _ = output.send(LiquidationEvent::Disconnected(reason)).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticker(symbol: &str, exchange: Exchange) -> Ticker {
        Ticker::new(symbol, exchange)
    }

    #[test]
    fn test_binance_force_order_long_liquidation() {
        // Shape from the Binance USD-M "Liquidation Order Streams" docs.
        let payload = br#"{"e":"forceOrder","E":1568014460893,"o":{"s":"BTCUSDT","S":"SELL","o":"LIMIT","f":"IOC","q":"0.014","p":"9910","ap":"9910","X":"FILLED","l":"0.014","z":"0.014","T":1568014460893}}"#;
        let t = ticker("BTCUSDT", Exchange::BinanceLinear);
        let liq = parse_binance_force_order(payload, MarketKind::LinearPerps, &t).unwrap();
        assert_eq!(liq.time, 1568014460893);
        assert!(liq.long_liquidated);
        assert!((liq.notional_usd - 0.014 * 9910.0).abs() < 1e-9);
    }

    #[test]
    fn test_binance_force_order_combined_envelope_and_inverse_contracts() {
        let payload = br#"{"stream":"btcusd_perp@forceOrder","data":{"e":"forceOrder","E":1,"o":{"s":"BTCUSD_PERP","S":"BUY","q":"3","p":"60000","ap":"60010","z":"3","T":42}}}"#;
        let t = ticker("BTCUSD_PERP", Exchange::BinanceInverse);
        let liq = parse_binance_force_order(payload, MarketKind::InversePerps, &t).unwrap();
        assert!(!liq.long_liquidated);
        assert_eq!(liq.price, 60010.0);
        // 3 contracts x 100 USD each.
        assert_eq!(liq.notional_usd, 300.0);
    }

    #[test]
    fn test_binance_ignores_other_events() {
        let t = ticker("BTCUSDT", Exchange::BinanceLinear);
        let payload = br#"{"e":"aggTrade","E":1,"s":"BTCUSDT"}"#;
        assert!(parse_binance_force_order(payload, MarketKind::LinearPerps, &t).is_none());
        assert!(parse_binance_force_order(b"not json", MarketKind::LinearPerps, &t).is_none());
    }

    #[test]
    fn test_bybit_all_liquidation() {
        // Shape from the Bybit v5 "All Liquidation" docs.
        let payload = br#"{"topic":"allLiquidation.ROSEUSDT","type":"snapshot","ts":1739502303204,"data":[{"T":1739502302929,"s":"ROSEUSDT","S":"Sell","v":"20000","p":"0.04499"},{"T":1739502302930,"s":"ROSEUSDT","S":"Buy","v":"100","p":"0.045"}]}"#;
        let liqs = parse_bybit_all_liquidation(payload, MarketKind::LinearPerps);
        assert_eq!(liqs.len(), 2);
        assert!(!liqs[0].long_liquidated);
        assert!(liqs[1].long_liquidated);
        assert!((liqs[0].notional_usd - 20000.0 * 0.04499).abs() < 1e-9);
    }

    #[test]
    fn test_bybit_inverse_size_is_usd_and_other_topics_are_ignored() {
        let payload = br#"{"topic":"allLiquidation.BTCUSD","data":[{"T":1,"s":"BTCUSD","S":"Buy","v":"2500","p":"60000"}]}"#;
        let liqs = parse_bybit_all_liquidation(payload, MarketKind::InversePerps);
        assert_eq!(liqs[0].notional_usd, 2500.0);

        let pong = br#"{"success":true,"ret_msg":"pong","op":"ping"}"#;
        assert!(parse_bybit_all_liquidation(pong, MarketKind::LinearPerps).is_empty());
        let trade =
            br#"{"topic":"publicTrade.BTCUSDT","data":[{"T":1,"S":"Buy","v":"1","p":"2"}]}"#;
        assert!(parse_bybit_all_liquidation(trade, MarketKind::LinearPerps).is_empty());
    }

    #[tokio::test]
    #[ignore = "network: connects to the live Binance and Bybit liquidation streams"]
    async fn test_live_streams_connect() {
        use iced_futures::futures::StreamExt;
        for (symbol, exchange) in [
            ("BTCUSDT", Exchange::BinanceLinear),
            ("BTCUSDT", Exchange::BybitLinear),
        ] {
            let ti = TickerInfo::new(Ticker::new(symbol, exchange), 0.1, 0.001, None);
            let mut stream = Box::pin(connect_liquidation_stream(ti));
            let first = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
                .await
                .expect("no event within 10 s");
            assert!(
                matches!(first, Some(LiquidationEvent::Connected)),
                "{exchange}: {first:?}"
            );
        }
    }

    #[test]
    fn test_supported_venues() {
        assert!(is_supported(Exchange::BinanceLinear));
        assert!(is_supported(Exchange::BybitInverse));
        assert!(!is_supported(Exchange::BinanceSpot));
        assert!(!is_supported(Exchange::OkexLinear));
    }
}
