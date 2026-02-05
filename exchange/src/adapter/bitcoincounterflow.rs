//! Bitcoin Counterflow API adapter for Net OI data
//!
//! This module provides access to the bitcoincounterflow.com API for fetching
//! Open Interest data with price, used for calculating Net Longs/Shorts indicators.
//!
//! API Endpoint: GET https://api.bitcoincounterflow.com/api/open-interest
//! Parameters:
//!   - days: 7, 21, 90, 365, 730
//!   - interval: 15m, 30m, 1h, 2h, 4h, 1d

use super::AdapterError;
use crate::{NetOiDataPoint, fetcher::NetOiInterval};

use serde::Deserialize;

const API_BASE: &str = "https://api.bitcoincounterflow.com/api/open-interest";

/// Raw API response structure
#[derive(Debug, Deserialize)]
struct RawNetOiData {
    timestamp: String,
    price: f64,
    #[serde(rename = "openInterest")]
    open_interest: f64,
}

/// Fetch Net OI data from bitcoincounterflow.com API
///
/// # Arguments
/// * `days` - History depth in days (7, 21, 90, 365, 730)
/// * `interval` - Candle timeframe (15m, 30m, 1h, 2h, 4h, 1d)
///
/// # Returns
/// Vector of NetOiDataPoint with timestamp, price, and open interest
pub async fn fetch_net_oi_data(
    days: u16,
    interval: NetOiInterval,
) -> Result<Vec<NetOiDataPoint>, AdapterError> {
    // Validate days parameter
    let valid_days = [7, 21, 90, 365, 730];
    if !valid_days.contains(&days) {
        return Err(AdapterError::InvalidRequest(format!(
            "Invalid days parameter: {}. Must be one of: {:?}",
            days, valid_days
        )));
    }

    let url = format!("{}?days={}&interval={}", API_BASE, days, interval.as_str());

    // Make request with browser-like headers (required by this API)
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36",
        )
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://bitcoincounterflow.com")
        .header("Referer", "https://bitcoincounterflow.com/dashboards/")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(AdapterError::InvalidRequest(format!(
            "Net OI API returned status: {}",
            response.status()
        )));
    }

    let text = response.text().await?;

    let raw_data: Vec<RawNetOiData> = serde_json::from_str(&text).map_err(|e| {
        log::error!("Failed to parse Net OI data: {}", e);
        AdapterError::ParseError(format!("Failed to parse Net OI data: {}", e))
    })?;

    // Convert to our data structure
    let data_points: Vec<NetOiDataPoint> = raw_data
        .into_iter()
        .filter_map(|raw| {
            // Parse ISO timestamp to milliseconds
            let time = parse_iso_timestamp(&raw.timestamp)?;
            Some(NetOiDataPoint {
                time,
                price: raw.price as f32,
                open_interest: raw.open_interest,
            })
        })
        .collect();

    Ok(data_points)
}

/// Parse ISO 8601 timestamp string to milliseconds since epoch
fn parse_iso_timestamp(s: &str) -> Option<u64> {
    // Expected format: "2024-01-15T12:00:00Z" or similar
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .or_else(|| {
            // Try without timezone (assume UTC)
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|dt| dt.and_utc().fixed_offset())
        })
        .map(|dt| dt.timestamp_millis() as u64)
}

/// Check if a symbol is BTC (this API only supports Bitcoin)
pub fn is_btc_symbol(symbol: &str) -> bool {
    let upper = symbol.to_uppercase();
    upper.starts_with("BTC") || upper.starts_with("XBTUSD")
}
