use crate::adapter::{AdapterError, Exchange};
use crate::util::Price;
use crate::{TickerInfo, Trade};
use std::path::{Path, PathBuf};

pub const USE_BINARY_CACHE: bool = true;
pub const BINARY_CACHE_MAGIC: &[u8; 4] = b"FST1";
pub const TRADE_RECORD_SIZE: usize = 24;

pub fn find_gap_index(trades: &[Trade], from_idx: usize, max_gap_ms: u64) -> Option<usize> {
    if trades.is_empty() || from_idx >= trades.len() - 1 {
        return None;
    }
    (from_idx..trades.len() - 1)
        .find(|&i| trades[i + 1].time.saturating_sub(trades[i].time) > max_gap_ms)
}

pub fn slice_cached_trades(
    cached: &[Trade],
    start_idx: usize,
    max_gap_ms: u64,
    max_batch: usize,
) -> (Vec<Trade>, u64) {
    if cached.is_empty() || start_idx >= cached.len() {
        return (Vec::new(), 0);
    }

    let effective_end = find_gap_index(cached, start_idx, max_gap_ms)
        .map(|idx| idx + 1)
        .unwrap_or(cached.len());

    if start_idx >= effective_end {
        let fallback_t = cached[start_idx].time.saturating_add(1);
        return (Vec::new(), fallback_t);
    }

    let mut chunk_end = (start_idx + max_batch).min(effective_end);
    let boundary_time = cached[chunk_end - 1].time;
    while chunk_end < effective_end && cached[chunk_end].time == boundary_time {
        chunk_end += 1;
    }
    let trades = cached[start_idx..chunk_end].to_vec();
    let next_from = boundary_time.saturating_add(1);
    (trades, next_from)
}

pub fn encode_trades_binary(trades: &[Trade]) -> Vec<u8> {
    let num_trades = trades.len();
    let mut raw = Vec::with_capacity(8 + num_trades * TRADE_RECORD_SIZE);
    raw.extend_from_slice(BINARY_CACHE_MAGIC);
    raw.extend_from_slice(&(num_trades as u32).to_le_bytes());

    for t in trades {
        raw.extend_from_slice(&t.time.to_le_bytes());
        raw.extend_from_slice(&t.price.units.to_le_bytes());
        raw.extend_from_slice(&t.qty.to_le_bytes());
        raw.push(if t.is_sell { 1 } else { 0 });
        raw.extend_from_slice(&[0u8; 3]);
    }

    lz4_flex::compress_prepend_size(&raw)
}

pub fn decode_trades_binary(compressed_bytes: &[u8]) -> Result<Vec<Trade>, AdapterError> {
    let decompressed = lz4_flex::decompress_size_prepended(compressed_bytes)
        .map_err(|e| AdapterError::ParseError(format!("Failed to decompress binary cache: {e}")))?;

    if decompressed.len() < 8 {
        return Err(AdapterError::ParseError(
            "Binary cache header too short".into(),
        ));
    }

    if &decompressed[0..4] != BINARY_CACHE_MAGIC {
        return Err(AdapterError::ParseError(
            "Binary cache magic mismatch".into(),
        ));
    }

    let count = u32::from_le_bytes(
        decompressed[4..8]
            .try_into()
            .map_err(|_| AdapterError::ParseError("Failed to parse trade count".into()))?,
    ) as usize;

    let payload = &decompressed[8..];
    if payload.len() != count * TRADE_RECORD_SIZE {
        return Err(AdapterError::ParseError(format!(
            "Binary cache payload size mismatch: expected {} bytes for {} trades, got {}",
            count * TRADE_RECORD_SIZE,
            count,
            payload.len()
        )));
    }

    let mut trades = Vec::with_capacity(count);
    let (chunks, _) = payload.as_chunks::<TRADE_RECORD_SIZE>();
    for chunk in chunks {
        let time = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let price_units = i64::from_le_bytes(chunk[8..16].try_into().unwrap());
        let qty = f32::from_le_bytes(chunk[16..20].try_into().unwrap());
        let is_sell = chunk[20] != 0;

        trades.push(Trade {
            time,
            is_sell,
            price: Price { units: price_units },
            qty,
        });
    }

    Ok(trades)
}

pub fn raw_trade_subpath(ticker_info: &TickerInfo) -> String {
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let symbol_upper = symbol.to_uppercase();
    match ticker_info.exchange() {
        Exchange::BinanceSpot => format!("data/spot/daily/aggTrades/{symbol_upper}"),
        Exchange::BinanceLinear => format!("data/futures/um/daily/aggTrades/{symbol_upper}"),
        Exchange::BinanceInverse => format!("data/futures/cm/daily/aggTrades/{symbol_upper}"),
        Exchange::BybitSpot => format!("data/spot/daily/trades/{symbol_upper}"),
        Exchange::BybitLinear => format!("data/linear/daily/trades/{symbol_upper}"),
        Exchange::BybitInverse => format!("data/inverse/daily/trades/{symbol_upper}"),
        Exchange::OkexSpot => format!("data/spot/daily/trades/{symbol_upper}"),
        Exchange::OkexLinear => format!("data/linear/daily/trades/{symbol_upper}"),
        Exchange::OkexInverse => format!("data/inverse/daily/trades/{symbol_upper}"),
        Exchange::HyperliquidSpot => format!("data/spot/daily/trades/{symbol_upper}"),
        Exchange::HyperliquidLinear => format!("data/linear/daily/trades/{symbol_upper}"),
    }
}

pub fn raw_trade_bin_path(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
) -> PathBuf {
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let symbol_upper = symbol.to_uppercase();
    let market_subpath = raw_trade_subpath(ticker_info);
    let tag = match ticker_info.exchange() {
        Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse => "aggTrades",
        _ => "trades",
    };
    let bin_file_name = format!("{symbol_upper}-{tag}-{}.bin", date.format("%Y-%m-%d"));
    base_data_path.join(market_subpath).join(bin_file_name)
}

pub fn intraday_raw_trade_bin_path(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
) -> PathBuf {
    let (symbol, _) = ticker_info.ticker.to_full_symbol_and_type();
    let symbol_upper = symbol.to_uppercase();
    let market_subpath = raw_trade_subpath(ticker_info);
    let bin_file_name = format!("{symbol_upper}-intraday-{}.bin", date.format("%Y-%m-%d"));
    base_data_path.join(market_subpath).join(bin_file_name)
}

pub fn load_intraday_trades_from_cache(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
) -> Option<Vec<Trade>> {
    let key = (base_data_path.to_path_buf(), *ticker_info, date);
    let pending = {
        let buffer = INTRADAY_BUFFER.lock().ok()?;
        buffer.get(&key).cloned()
    };

    let bin_path = intraday_raw_trade_bin_path(base_data_path, ticker_info, date);
    let disk_trades = if bin_path.exists()
        && let Ok(bytes) = std::fs::read(&bin_path)
        && let Ok(trades) = decode_trades_binary(&bytes)
    {
        Some(trades)
    } else {
        None
    };

    match (disk_trades, pending) {
        (Some(disk), Some(mem)) if !mem.is_empty() => Some(merge_intraday_trades(&disk, &mem)),
        (Some(disk), _) => Some(disk),
        (None, Some(mem)) if !mem.is_empty() => Some(merge_intraday_trades(&[], &mem)),
        _ => None,
    }
}

use rustc_hash::FxHashMap;
use std::sync::{LazyLock, Mutex};

type IntradayKey = (PathBuf, TickerInfo, chrono::NaiveDate);

static INTRADAY_BUFFER: LazyLock<Mutex<FxHashMap<IntradayKey, Vec<Trade>>>> =
    LazyLock::new(|| Mutex::new(FxHashMap::default()));

const INTRADAY_FLUSH_THRESHOLD: usize = 20_000;

pub fn merge_intraday_trades_with_diff(
    existing: &[Trade],
    new_trades: &[Trade],
) -> (Vec<Trade>, Vec<Trade>) {
    let trade_cmp = |a: &Trade, b: &Trade| {
        a.time
            .cmp(&b.time)
            .then_with(|| a.price.cmp(&b.price))
            .then_with(|| a.qty.total_cmp(&b.qty))
            .then_with(|| a.is_sell.cmp(&b.is_sell))
    };

    let trade_eq = |a: &mut Trade, b: &mut Trade| {
        a.time == b.time
            && a.price == b.price
            && (a.qty - b.qty).abs() < 1e-5
            && a.is_sell == b.is_sell
    };

    if existing.is_empty() {
        let mut res = new_trades.to_vec();
        res.sort_by(trade_cmp);
        res.dedup_by(trade_eq);
        let diff = res.clone();
        return (res, diff);
    }
    if new_trades.is_empty() {
        return (existing.to_vec(), Vec::new());
    }

    let mut sorted_new = new_trades.to_vec();
    sorted_new.sort_by(trade_cmp);
    sorted_new.dedup_by(trade_eq);

    let last_existing = &existing[existing.len() - 1];
    let first_new = &sorted_new[0];

    let mut merged = Vec::with_capacity(existing.len() + sorted_new.len());
    let mut diff = Vec::with_capacity(sorted_new.len());

    if first_new.time > last_existing.time {
        merged.extend_from_slice(existing);
        merged.extend(sorted_new.clone());
        return (merged, sorted_new);
    }

    let mut i = 0;
    let mut j = 0;
    while i < existing.len() && j < sorted_new.len() {
        match trade_cmp(&existing[i], &sorted_new[j]) {
            std::cmp::Ordering::Less => {
                merged.push(existing[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                merged.push(sorted_new[j]);
                diff.push(sorted_new[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                merged.push(existing[i]);
                i += 1;
                j += 1;
            }
        }
    }
    if i < existing.len() {
        merged.extend_from_slice(&existing[i..]);
    }
    if j < sorted_new.len() {
        merged.extend_from_slice(&sorted_new[j..]);
        diff.extend_from_slice(&sorted_new[j..]);
    }
    (merged, diff)
}

pub fn merge_intraday_trades(existing: &[Trade], new_trades: &[Trade]) -> Vec<Trade> {
    merge_intraday_trades_with_diff(existing, new_trades).0
}

fn write_intraday_trades_to_disk(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
    trades_to_persist: &[Trade],
) -> Result<(), AdapterError> {
    if trades_to_persist.is_empty() {
        return Ok(());
    }
    let base_bin_path = intraday_raw_trade_bin_path(base_data_path, ticker_info, date);
    if let Some(parent) = base_bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AdapterError::ParseError(format!("Failed to create dir: {e}")))?;
    }

    let disk_existing = if base_bin_path.exists()
        && let Ok(bytes) = std::fs::read(&base_bin_path)
        && let Ok(trades) = decode_trades_binary(&bytes)
    {
        trades
    } else {
        Vec::new()
    };

    let merged = merge_intraday_trades(&disk_existing, trades_to_persist);

    let compressed = encode_trades_binary(&merged);
    let temp_bin_path = base_bin_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_bin_path, &compressed) {
        return Err(AdapterError::ParseError(format!(
            "Failed to write temp binary cache {:?}: {e}",
            temp_bin_path
        )));
    }
    if base_bin_path.exists() {
        let _ = std::fs::remove_file(&base_bin_path);
    }
    if let Err(e) = std::fs::rename(&temp_bin_path, &base_bin_path) {
        let _ = std::fs::remove_file(&temp_bin_path);
        return Err(AdapterError::ParseError(format!(
            "Failed to rename {temp_bin_path:?} to {base_bin_path:?}: {e}"
        )));
    }
    log::info!(
        "Saved {} intraday trades to binary cache {:?}",
        merged.len(),
        base_bin_path
    );
    Ok(())
}

pub fn save_intraday_trades_to_cache_buffered(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
    trades: &[Trade],
) -> Result<(), AdapterError> {
    if trades.is_empty() {
        return Ok(());
    }
    let key = (base_data_path.to_path_buf(), *ticker_info, date);
    let mut buffer = INTRADAY_BUFFER
        .lock()
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;
    let entry: &mut Vec<Trade> = buffer.entry(key).or_default();
    entry.extend_from_slice(trades);
    if entry.len() >= INTRADAY_FLUSH_THRESHOLD {
        let to_flush: Vec<Trade> = std::mem::take(entry);
        drop(buffer);
        write_intraday_trades_to_disk(base_data_path, ticker_info, date, &to_flush)?;
    }
    Ok(())
}

pub fn flush_intraday_trades_cache(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
) -> Result<(), AdapterError> {
    let key = (base_data_path.to_path_buf(), *ticker_info, date);
    let mut buffer = INTRADAY_BUFFER
        .lock()
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;
    if let Some(trades) = buffer.remove(&key)
        && !trades.is_empty()
    {
        drop(buffer);
        write_intraday_trades_to_disk(base_data_path, ticker_info, date, &trades)?;
    }
    Ok(())
}

pub fn flush_all_intraday_trades_cache() -> Result<(), AdapterError> {
    let mut buffer = INTRADAY_BUFFER
        .lock()
        .map_err(|e| AdapterError::ParseError(e.to_string()))?;
    let all: Vec<(IntradayKey, Vec<Trade>)> = buffer.drain().collect();
    drop(buffer);
    for ((base_path, ticker_info, date), trades) in all {
        if !trades.is_empty() {
            write_intraday_trades_to_disk(&base_path, &ticker_info, date, &trades)?;
        }
    }
    Ok(())
}

pub fn save_intraday_trades_to_cache(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
    trades: &[Trade],
) -> Result<(), AdapterError> {
    write_intraday_trades_to_disk(base_data_path, ticker_info, date, trades)
}

pub fn load_raw_trades_from_cache(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
) -> Option<Vec<Trade>> {
    let bin_path = raw_trade_bin_path(base_data_path, ticker_info, date);
    if bin_path.exists()
        && let Ok(bytes) = std::fs::read(&bin_path)
        && let Ok(trades) = decode_trades_binary(&bytes)
        && let Some(day_start_dt) = date.and_hms_opt(0, 0, 0)
    {
        let day_start = day_start_dt.and_utc().timestamp_millis() as u64;
        let day_end = day_start + 86_400_000 - 1;
        let is_full = !trades.is_empty()
            && trades.first().map(|t| t.time).unwrap_or(0) <= day_start + 600_000
            && trades.last().map(|t| t.time).unwrap_or(0) >= day_end - 600_000;
        if is_full {
            return Some(trades);
        } else {
            log::warn!(
                "Cached binary archive {:?} is incomplete ({} trades), removing",
                bin_path,
                trades.len()
            );
            let _ = std::fs::remove_file(&bin_path);
        }
    }
    None
}

pub fn save_raw_trades_to_cache(
    base_data_path: &Path,
    ticker_info: &TickerInfo,
    date: chrono::NaiveDate,
    trades: &[Trade],
) -> Result<(), AdapterError> {
    if trades.is_empty() {
        return Ok(());
    }
    let base_bin_path = raw_trade_bin_path(base_data_path, ticker_info, date);
    if let Some(parent) = base_bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AdapterError::ParseError(format!("Failed to create dir: {e}")))?;
    }
    let compressed = encode_trades_binary(trades);
    let temp_bin_path = base_bin_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_bin_path, &compressed) {
        return Err(AdapterError::ParseError(format!(
            "Failed to write temp binary cache {:?}: {e}",
            temp_bin_path
        )));
    }
    if base_bin_path.exists() {
        let _ = std::fs::remove_file(&base_bin_path);
    }
    if let Err(e) = std::fs::rename(&temp_bin_path, &base_bin_path) {
        let _ = std::fs::remove_file(&temp_bin_path);
        return Err(AdapterError::ParseError(format!(
            "Failed to rename {temp_bin_path:?} to {base_bin_path:?}: {e}"
        )));
    }
    log::info!(
        "Saved {} trades to binary cache {:?}",
        trades.len(),
        base_bin_path
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ticker;

    #[test]
    fn test_save_intraday_trades_merge_behavior() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_intraday_merge_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        let ticker_info = TickerInfo::new(ticker, 0.1, 0.001, None);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let base_t = date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64;

        // Batch 1: 10:00 - 12:00 (10:00, 11:00, 12:00)
        let batch1 = vec![
            Trade {
                time: base_t + 10 * 3600 * 1000,
                price: Price::from_f32(60000.0),
                qty: 1.0,
                is_sell: false,
            },
            Trade {
                time: base_t + 11 * 3600 * 1000,
                price: Price::from_f32(60010.0),
                qty: 2.0,
                is_sell: true,
            },
            Trade {
                time: base_t + 12 * 3600 * 1000,
                price: Price::from_f32(60020.0),
                qty: 1.5,
                is_sell: false,
            },
        ];
        save_intraday_trades_to_cache(&temp_dir, &ticker_info, date, &batch1).unwrap();

        // Batch 2: 00:00 - 02:00 (00:00, 01:00, 02:00)
        let batch2 = vec![
            Trade {
                time: base_t,
                price: Price::from_f32(59900.0),
                qty: 0.5,
                is_sell: false,
            },
            Trade {
                time: base_t + 3600 * 1000,
                price: Price::from_f32(59950.0),
                qty: 1.2,
                is_sell: true,
            },
            Trade {
                time: base_t + 2 * 3600 * 1000,
                price: Price::from_f32(59980.0),
                qty: 3.0,
                is_sell: false,
            },
        ];
        save_intraday_trades_to_cache(&temp_dir, &ticker_info, date, &batch2).unwrap();

        // Batch 3: 11:00 - 13:00 (11:00 duplicate, 12:00 duplicate, 13:00 new)
        let batch3 = vec![
            Trade {
                time: base_t + 11 * 3600 * 1000,
                price: Price::from_f32(60010.0),
                qty: 2.0,
                is_sell: true,
            },
            Trade {
                time: base_t + 12 * 3600 * 1000,
                price: Price::from_f32(60020.0),
                qty: 1.5,
                is_sell: false,
            },
            Trade {
                time: base_t + 13 * 3600 * 1000,
                price: Price::from_f32(60050.0),
                qty: 0.8,
                is_sell: true,
            },
        ];
        save_intraday_trades_to_cache(&temp_dir, &ticker_info, date, &batch3).unwrap();

        let loaded = load_intraday_trades_from_cache(&temp_dir, &ticker_info, date).unwrap();
        // Total unique trades: 00:00, 01:00, 02:00, 10:00, 11:00, 12:00, 13:00 -> 7 trades
        assert_eq!(loaded.len(), 7);
        assert_eq!(loaded[0].time, base_t);
        assert_eq!(loaded[1].time, base_t + 3600 * 1000);
        assert_eq!(loaded[2].time, base_t + 2 * 3600 * 1000);
        assert_eq!(loaded[3].time, base_t + 10 * 3600 * 1000);
        assert_eq!(loaded[4].time, base_t + 11 * 3600 * 1000);
        assert_eq!(loaded[5].time, base_t + 12 * 3600 * 1000);
        assert_eq!(loaded[6].time, base_t + 13 * 3600 * 1000);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
