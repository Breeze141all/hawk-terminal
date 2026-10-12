use crate::Exchange;
use crate::adapter::AdapterError;
use enum_map::EnumMap;
use serde_json::Value;
use std::sync::atomic::{AtomicI64, Ordering};

static CLOCK_SKEWS: std::sync::LazyLock<EnumMap<Exchange, AtomicI64>> =
    std::sync::LazyLock::new(EnumMap::default);
static LAST_SYNC_MS: std::sync::LazyLock<EnumMap<Exchange, AtomicI64>> =
    std::sync::LazyLock::new(EnumMap::default);
static GLOBAL_CLOCK_SKEW: AtomicI64 = AtomicI64::new(0);

/// Sets the clock skew delta in milliseconds: `server_time - local_time`.
///
/// Positive delta: local clock is behind exchange server.
/// Negative delta: local clock is ahead of exchange server.
pub fn set_clock_skew(exchange: Exchange, delta_ms: i64) {
    CLOCK_SKEWS[exchange].store(delta_ms, Ordering::Relaxed);
    LAST_SYNC_MS[exchange].store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
    GLOBAL_CLOCK_SKEW.store(delta_ms, Ordering::Relaxed);
}

/// Retrieves the current clock skew delta for a specific exchange (or global fallback).
pub fn get_clock_skew(exchange: Option<Exchange>) -> i64 {
    if let Some(ex) = exchange {
        let skew = CLOCK_SKEWS[ex].load(Ordering::Relaxed);
        if skew != 0 {
            return skew;
        }
    }
    GLOBAL_CLOCK_SKEW.load(Ordering::Relaxed)
}

/// Resets all stored clock skews to zero (primarily for unit testing).
pub fn reset_clock_skews() {
    for ex in Exchange::ALL {
        CLOCK_SKEWS[ex].store(0, Ordering::Relaxed);
        LAST_SYNC_MS[ex].store(0, Ordering::Relaxed);
    }
    GLOBAL_CLOCK_SKEW.store(0, Ordering::Relaxed);
}

/// Returns true if the exchange clock has never been synced or if the sync is older than `max_age_ms`.
pub fn is_sync_needed(exchange: Exchange, max_age_ms: u64) -> bool {
    let last = LAST_SYNC_MS[exchange].load(Ordering::Relaxed);
    if last <= 0 {
        return true;
    }
    let now = chrono::Utc::now().timestamp_millis();
    now.saturating_sub(last) as u64 > max_age_ms
}

/// Returns the estimated exchange server time in milliseconds epoch.
///
/// Applies `server_time = (local_time + delta)`.
pub fn server_time_ms(exchange: Option<Exchange>) -> u64 {
    let local_now = chrono::Utc::now().timestamp_millis();
    let delta = get_clock_skew(exchange);
    local_now.saturating_add(delta).max(0) as u64
}

/// Returns midnight (00:00:00 UTC) of today in milliseconds epoch, based on exchange server time.
pub fn today_midnight_ms(exchange: Option<Exchange>) -> u64 {
    let server_now = server_time_ms(exchange);
    chrono::DateTime::from_timestamp_millis(server_now as i64)
        .map(|dt| {
            dt.date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis() as u64
        })
        .unwrap_or_else(|| (server_now / 86_400_000) * 86_400_000)
}

/// Extracts exchange server time in milliseconds from a JSON payload or HTTP Date header.
pub fn parse_server_time_from_response(
    exchange: Exchange,
    body: &str,
    date_header: Option<&str>,
) -> Option<u64> {
    // 1. Try exchange-specific JSON parsing
    if let Ok(val) = serde_json::from_str::<Value>(body) {
        match exchange {
            Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => {
                if let Some(ts) = val.get("serverTime").and_then(Value::as_u64) {
                    return Some(ts);
                }
            }
            Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => {
                if let Some(ts) = val.get("time").and_then(Value::as_u64) {
                    return Some(ts);
                }
                if let Some(res) = val.get("result") {
                    if let Some(nano_str) = res.get("timeNano").and_then(Value::as_str)
                        && let Ok(nano) = nano_str.parse::<u64>()
                    {
                        return Some(nano / 1_000_000);
                    }
                    if let Some(sec_str) = res.get("timeSecond").and_then(Value::as_str)
                        && let Ok(sec) = sec_str.parse::<u64>()
                    {
                        return Some(sec.saturating_mul(1000));
                    }
                }
            }
            Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => {
                if let Some(data) = val.get("data").and_then(Value::as_array)
                    && let Some(first) = data.first()
                    && let Some(ts_str) = first.get("ts").and_then(Value::as_str)
                    && let Ok(ts) = ts_str.parse::<u64>()
                {
                    return Some(ts);
                }
            }
            Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
                if let Some(ts) = val.get("serverTime").and_then(Value::as_u64) {
                    return Some(ts);
                }
            }
        }
    }

    // 2. HTTP Date header fallback
    if let Some(date_str) = date_header
        && let Ok(dt) = chrono::DateTime::parse_from_rfc2822(date_str)
    {
        return Some(dt.timestamp_millis() as u64);
    }

    None
}

/// Executes a ping request to the exchange server, measures RTT via Cristian's algorithm,
/// updates the stored clock skew delta (`server_time - local_time`), and returns the delta in ms.
pub async fn ping_server_time(exchange: Exchange) -> Result<i64, AdapterError> {
    let (url, is_post, body) = match exchange {
        Exchange::BinanceSpot => ("https://api.binance.com/api/v3/time", false, None),
        Exchange::BinanceLinear => ("https://fapi.binance.com/fapi/v1/time", false, None),
        Exchange::BinanceInverse => ("https://dapi.binance.com/dapi/v1/time", false, None),
        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => {
            ("https://api.bybit.com/v5/market/time", false, None)
        }
        Exchange::OkexSpot | Exchange::OkexLinear | Exchange::OkexInverse => {
            ("https://www.okx.com/api/v5/public/time", false, None)
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => (
            "https://api.hyperliquid.xyz/info",
            true,
            Some("{\"type\":\"meta\"}"),
        ),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(AdapterError::FetchError)?;

    let t0 = chrono::Utc::now().timestamp_millis();
    let resp = if is_post {
        client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.unwrap_or(""))
            .send()
            .await
    } else {
        client.get(url).send().await
    }
    .map_err(AdapterError::FetchError)?;

    let date_hdr = resp
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let body_text = resp.text().await.map_err(AdapterError::FetchError)?;
    let t1 = chrono::Utc::now().timestamp_millis();

    let server_time = parse_server_time_from_response(exchange, &body_text, date_hdr.as_deref())
        .ok_or_else(|| {
            AdapterError::ParseError(format!(
                "Failed to parse server time for {:?} from response: {}",
                exchange, body_text
            ))
        })?;

    // Cristian's algorithm: local midpoint when server processed the request
    let local_midpoint = t0 + (t1.saturating_sub(t0)) / 2;
    let delta = (server_time as i64) - local_midpoint;

    set_clock_skew(exchange, delta);
    log::info!(
        "Clock skew for {:?}: {} ms (local_midpoint: {}, server_time: {})",
        exchange,
        delta,
        local_midpoint,
        server_time
    );

    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_binance_time() {
        let json = r#"{"serverTime":1791393025070}"#;
        let ts = parse_server_time_from_response(Exchange::BinanceLinear, json, None);
        assert_eq!(ts, Some(1791393025070));
    }

    #[test]
    fn test_parse_bybit_time() {
        let json = r#"{"retCode":0,"retMsg":"OK","result":{"timeSecond":"1791393029","timeNano":"1791393029095979339"},"time":1791393029095}"#;
        let ts = parse_server_time_from_response(Exchange::BybitLinear, json, None);
        assert_eq!(ts, Some(1791393029095));

        // Fallback without "time" field using "timeNano"
        let json_nano_only = r#"{"result":{"timeNano":"1791393029095979339"}}"#;
        let ts_nano = parse_server_time_from_response(Exchange::BybitLinear, json_nano_only, None);
        assert_eq!(ts_nano, Some(1791393029095));
    }

    #[test]
    fn test_parse_okx_time() {
        let json = r#"{"code":"0","data":[{"ts":"1791393034497"}],"msg":""}"#;
        let ts = parse_server_time_from_response(Exchange::OkexLinear, json, None);
        assert_eq!(ts, Some(1791393034497));
    }

    #[test]
    fn test_parse_http_date_fallback() {
        let date_hdr = "Wed, 07 Oct 2026 17:10:55 GMT";
        let expected = chrono::DateTime::parse_from_rfc2822(date_hdr)
            .unwrap()
            .timestamp_millis() as u64;
        let ts = parse_server_time_from_response(Exchange::HyperliquidLinear, "{}", Some(date_hdr));
        assert_eq!(ts, Some(expected));
    }

    #[test]
    fn test_clock_skew_delta_calculation_and_server_time() {
        reset_clock_skews();
        assert_eq!(get_clock_skew(Some(Exchange::BinanceLinear)), 0);

        // Client is 150 seconds ahead of server -> delta = -150_000 ms
        let delta = -150_000i64;
        set_clock_skew(Exchange::BinanceLinear, delta);

        assert_eq!(get_clock_skew(Some(Exchange::BinanceLinear)), -150_000);
        // Other exchange with no specific skew falls back to global skew
        assert_eq!(get_clock_skew(Some(Exchange::BybitLinear)), -150_000);

        let local_now = chrono::Utc::now().timestamp_millis() as u64;
        let server_now = server_time_ms(Some(Exchange::BinanceLinear));
        assert!(server_now <= local_now.saturating_sub(149_000));
        assert!(server_now >= local_now.saturating_sub(151_000));

        reset_clock_skews();
    }
}
