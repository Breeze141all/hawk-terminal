pub mod adapter;
pub mod connect;
pub mod depth;
pub mod fetcher;
mod limiter;
pub mod trades;
pub mod util;

use crate::util::{ContractSize, MinQtySize, MinTicksize, Price};
pub use adapter::Event;
use adapter::{Exchange, MarketKind, StreamKind};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use std::sync::atomic::{AtomicU8, Ordering};
use std::{fmt, hash::Hash};

/// Unit for displaying volume/quantity size values.
///
/// - `Base`: Display in base asset units (e.g., BTC for BTCUSDT)
/// - `Quote`: Display in quote currency value (e.g., USD/USDT equivalent)
///
/// Note: Only applies to linear perpetuals and spot markets.
/// Inverse perpetuals always display in USD regardless of this setting.
#[repr(u8)]
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub enum SizeUnit {
    Base = 0,
    #[default]
    Quote = 1,
}

static SIZE_CALC_UNIT: AtomicU8 = AtomicU8::new(SizeUnit::Base as u8);

pub fn set_preferred_currency(v: SizeUnit) {
    SIZE_CALC_UNIT.store(v as u8, Ordering::Relaxed);
}

pub fn volume_size_unit() -> SizeUnit {
    match SIZE_CALC_UNIT.load(Ordering::Relaxed) {
        0 => SizeUnit::Base,
        1 => SizeUnit::Quote,
        _ => SizeUnit::Base,
    }
}

/// Desired frequency for orderbook depth updates.
///
/// Maps user-selected update intervals to exchange-specific depth levels.
/// Used for some exchanges that determine push frequency based on subscribed depth level
/// (e.g., Bybit pushes every 300ms for 1000-level depth, 100ms for 200-level).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum PushFrequency {
    #[default]
    ServerDefault,
    Custom(Timeframe),
}

impl std::fmt::Display for PushFrequency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushFrequency::ServerDefault => write!(f, "Server Default"),
            PushFrequency::Custom(tf) => write!(f, "{}", tf),
        }
    }
}

impl std::fmt::Display for Timeframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Timeframe::MS100 => "100ms",
                Timeframe::MS200 => "200ms",
                Timeframe::MS300 => "300ms",
                Timeframe::MS500 => "500ms",
                Timeframe::MS1000 => "1s",
                Timeframe::M1 => "1m",
                Timeframe::M3 => "3m",
                Timeframe::M5 => "5m",
                Timeframe::M15 => "15m",
                Timeframe::M30 => "30m",
                Timeframe::H1 => "1h",
                Timeframe::H2 => "2h",
                Timeframe::H4 => "4h",
                Timeframe::H12 => "12h",
                Timeframe::D1 => "1d",
            }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, PartialOrd, Ord)]
pub enum Timeframe {
    MS100,
    MS200,
    MS300,
    MS500,
    MS1000,
    M1,
    M3,
    M5,
    M15,
    M30,
    H1,
    H2,
    H4,
    H12,
    D1,
}

impl Timeframe {
    pub const KLINE: [Timeframe; 10] = [
        Timeframe::M1,
        Timeframe::M3,
        Timeframe::M5,
        Timeframe::M15,
        Timeframe::M30,
        Timeframe::H1,
        Timeframe::H2,
        Timeframe::H4,
        Timeframe::H12,
        Timeframe::D1,
    ];

    pub const HEATMAP: [Timeframe; 5] = [
        Timeframe::MS100,
        Timeframe::MS200,
        Timeframe::MS300,
        Timeframe::MS500,
        Timeframe::MS1000,
    ];

    /// Returns duration in minutes. For subminute timeframes (`MS100`..`MS1000`), returns 0.
    pub fn to_minutes(self) -> u16 {
        match self {
            Timeframe::M1 => 1,
            Timeframe::M3 => 3,
            Timeframe::M5 => 5,
            Timeframe::M15 => 15,
            Timeframe::M30 => 30,
            Timeframe::H1 => 60,
            Timeframe::H2 => 120,
            Timeframe::H4 => 240,
            Timeframe::H12 => 720,
            Timeframe::D1 => 1440,
            Timeframe::MS100
            | Timeframe::MS200
            | Timeframe::MS300
            | Timeframe::MS500
            | Timeframe::MS1000 => 0,
        }
    }

    /// Returns duration in minutes if the timeframe is at least 1 minute, or `None` for subminute timeframes.
    pub fn try_to_minutes(self) -> Option<u16> {
        match self {
            Timeframe::MS100
            | Timeframe::MS200
            | Timeframe::MS300
            | Timeframe::MS500
            | Timeframe::MS1000 => None,
            _ => Some(self.to_minutes()),
        }
    }

    pub fn to_milliseconds(self) -> u64 {
        match self {
            Timeframe::MS100 => 100,
            Timeframe::MS200 => 200,
            Timeframe::MS300 => 300,
            Timeframe::MS500 => 500,
            Timeframe::MS1000 => 1_000,
            _ => {
                let minutes = self.to_minutes();
                u64::from(minutes) * 60_000
            }
        }
    }
}

impl From<Timeframe> for f32 {
    fn from(timeframe: Timeframe) -> f32 {
        timeframe.to_milliseconds() as f32
    }
}

impl From<Timeframe> for u64 {
    fn from(timeframe: Timeframe) -> u64 {
        timeframe.to_milliseconds()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTimeframe(pub u64);

impl fmt::Display for InvalidTimeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid milliseconds value for Timeframe: {}", self.0)
    }
}

impl std::error::Error for InvalidTimeframe {}

/// Serializable version of `(Exchange, Ticker)` tuples that is used for keys in maps
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SerTicker {
    pub exchange: Exchange,
    pub ticker: Ticker,
}

impl SerTicker {
    pub fn new(exchange: Exchange, ticker_str: &str) -> Self {
        let ticker = Ticker::new(ticker_str, exchange);
        Self { exchange, ticker }
    }

    pub fn from_parts(ticker: Ticker) -> Self {
        Self {
            exchange: ticker.exchange,
            ticker,
        }
    }

    fn exchange_to_string(exchange: Exchange) -> &'static str {
        match exchange {
            Exchange::BinanceLinear => "BinanceLinear",
            Exchange::BinanceInverse => "BinanceInverse",
            Exchange::BinanceSpot => "BinanceSpot",
            Exchange::BybitLinear => "BybitLinear",
            Exchange::BybitInverse => "BybitInverse",
            Exchange::BybitSpot => "BybitSpot",
            Exchange::HyperliquidLinear => "HyperliquidLinear",
            Exchange::HyperliquidSpot => "HyperliquidSpot",
            Exchange::OkexLinear => "OkexLinear",
            Exchange::OkexInverse => "OkexInverse",
            Exchange::OkexSpot => "OkexSpot",
        }
    }

    fn string_to_exchange(s: &str) -> Result<Exchange, String> {
        match s {
            "BinanceLinear" => Ok(Exchange::BinanceLinear),
            "BinanceInverse" => Ok(Exchange::BinanceInverse),
            "BinanceSpot" => Ok(Exchange::BinanceSpot),
            "BybitLinear" => Ok(Exchange::BybitLinear),
            "BybitInverse" => Ok(Exchange::BybitInverse),
            "BybitSpot" => Ok(Exchange::BybitSpot),
            "HyperliquidLinear" => Ok(Exchange::HyperliquidLinear),
            "HyperliquidSpot" => Ok(Exchange::HyperliquidSpot),
            "OkexLinear" => Ok(Exchange::OkexLinear),
            "OkexInverse" => Ok(Exchange::OkexInverse),
            "OkexSpot" => Ok(Exchange::OkexSpot),
            _ => Err(format!("Unknown exchange: {}", s)),
        }
    }
}

impl Serialize for SerTicker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (ticker_str, _) = self.ticker.to_full_symbol_and_type();
        let exchange_str = Self::exchange_to_string(self.exchange);
        let combined = format!("{}:{}", exchange_str, ticker_str);
        serializer.serialize_str(&combined)
    }
}

impl<'de> Deserialize<'de> for SerTicker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split(':').collect();

        if parts.len() != 2 {
            return Err(serde::de::Error::custom(format!(
                "Invalid ExchangeTicker format: expected 'Exchange:Ticker', got '{}'",
                s
            )));
        }

        let exchange_str = parts[0];
        let exchange = Self::string_to_exchange(exchange_str).map_err(serde::de::Error::custom)?;

        let ticker_str = parts[1];
        let ticker = Ticker::new(ticker_str, exchange);

        Ok(SerTicker { exchange, ticker })
    }
}

impl fmt::Display for SerTicker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (ticker_str, _) = self.ticker.to_full_symbol_and_type();
        let exchange_str = Self::exchange_to_string(self.exchange);
        write!(f, "{}:{}", exchange_str, ticker_str)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticker {
    bytes: [u8; Ticker::MAX_LEN as usize],
    pub exchange: Exchange,
    // Optional display symbol for UI, mainly used for Hyperliquid spot markets
    // to show "HYPEUSDC" instead of "@107"
    display_bytes: [u8; Ticker::MAX_LEN as usize],
    has_display_symbol: bool,
}

impl Ticker {
    const MAX_LEN: u8 = 28;

    pub fn new(ticker: &str, exchange: Exchange) -> Self {
        Self::new_with_display(ticker, exchange, None)
    }

    pub fn new_with_display(
        ticker: &str,
        exchange: Exchange,
        display_symbol: Option<&str>,
    ) -> Self {
        assert!(ticker.len() <= Self::MAX_LEN as usize, "Ticker too long");
        assert!(ticker.is_ascii(), "Ticker must be ASCII");
        assert!(!ticker.contains('|'), "Ticker cannot contain '|'");

        let mut bytes = [0u8; Self::MAX_LEN as usize];
        bytes[..ticker.len()].copy_from_slice(ticker.as_bytes());

        let mut display_bytes = [0u8; Self::MAX_LEN as usize];
        let has_display_symbol = if let Some(display) = display_symbol {
            assert!(
                display.len() <= Self::MAX_LEN as usize,
                "Display symbol too long"
            );
            assert!(display.is_ascii(), "Display symbol must be ASCII");
            // Display symbol cannot contain '|' as it's used as delimiter
            assert!(!display.contains('|'), "Display symbol cannot contain '|'");
            display_bytes[..display.len()].copy_from_slice(display.as_bytes());
            true
        } else {
            false
        };

        Ticker {
            bytes,
            exchange,
            display_bytes,
            has_display_symbol,
        }
    }

    #[inline]
    fn as_str(&self) -> &str {
        let end = self
            .bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(Self::MAX_LEN as usize);
        std::str::from_utf8(&self.bytes[..end]).unwrap()
    }

    #[inline]
    fn display_as_str(&self) -> &str {
        if self.has_display_symbol {
            let end = self
                .display_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(Self::MAX_LEN as usize);
            std::str::from_utf8(&self.display_bytes[..end]).unwrap()
        } else {
            self.as_str()
        }
    }

    /// Get the display symbol if it exists, otherwise None
    pub fn display_symbol(&self) -> Option<&str> {
        if self.has_display_symbol {
            Some(self.display_as_str())
        } else {
            None
        }
    }

    pub fn to_full_symbol_and_type(&self) -> (String, MarketKind) {
        (self.as_str().to_owned(), self.market_type())
    }

    pub fn display_symbol_and_type(&self) -> (String, MarketKind) {
        let market_kind = self.market_type();

        let result = if self.has_display_symbol {
            // Use the custom display symbol (e.g., "HYPEUSDC" for Hyperliquid spot)
            self.display_as_str().to_owned()
        } else {
            let mut result = self.as_str().to_owned();
            // Transform Hyperliquid symbols to standardized display format
            if matches!(self.exchange, Exchange::HyperliquidLinear)
                && market_kind == MarketKind::LinearPerps
            {
                // For Hyperliquid Linear Perps, append USDT to match other exchanges' format
                // The "P" suffix will be added later in compute_display_data for all perpetual contracts
                result.push_str("USDT");
            }
            result
        };

        (result, market_kind)
    }

    pub fn market_type(&self) -> MarketKind {
        self.exchange.market_type()
    }

    pub fn symbol_and_exchange_string(&self) -> String {
        format!(
            "{}:{}",
            SerTicker::exchange_to_string(self.exchange),
            self.as_str()
        )
    }
}

impl fmt::Display for Ticker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Ticker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sym, kind) = self.display_symbol_and_type();
        let internal_sym = self.as_str();
        if self.has_display_symbol && internal_sym != sym {
            write!(
                f,
                "Ticker({}:{}[{}], {:?})",
                SerTicker::exchange_to_string(self.exchange),
                sym,
                internal_sym,
                kind
            )
        } else {
            write!(
                f,
                "Ticker({}:{}, {:?})",
                SerTicker::exchange_to_string(self.exchange),
                sym,
                kind
            )
        }
    }
}

impl Serialize for Ticker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let internal = self.as_str();
        let exchange = SerTicker::exchange_to_string(self.exchange);
        let s = if self.has_display_symbol {
            let display = self.display_as_str();
            format!("{exchange}:{internal}|{display}")
        } else {
            format!("{exchange}:{internal}")
        };
        serializer.serialize_str(&s)
    }
}

/// Backwards compatible deserializer for Ticker so it won't break old persistent states
#[derive(Deserialize)]
#[serde(untagged)]
enum TickerDe {
    Str(String),
    // Old packed format
    Old {
        data: [u64; 2],
        len: u8,
        exchange: String,
    },
}

impl<'de> Deserialize<'de> for Ticker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match TickerDe::deserialize(deserializer)? {
            TickerDe::Str(s) => {
                let (exchange_str, rest) = s
                    .split_once(':')
                    .ok_or_else(|| serde::de::Error::custom("expected \"Exchange:Symbol\""))?;
                let exchange = SerTicker::string_to_exchange(exchange_str)
                    .map_err(serde::de::Error::custom)?;

                let (symbol, display) = if let Some((sym, disp)) = rest.split_once('|') {
                    (sym, Some(disp))
                } else {
                    (rest, None)
                };
                Ok(Ticker::new_with_display(symbol, exchange, display))
            }
            TickerDe::Old {
                data,
                len,
                exchange,
            } => {
                // Decode old 6-bit packed symbol
                if len as usize > 20 {
                    return Err(serde::de::Error::custom("old Ticker.len > 20"));
                }

                let mut symbol = String::with_capacity(len as usize);
                for i in 0..(len as usize) {
                    let shift = (i % 10) * 6;
                    let v = ((data[i / 10] >> shift) & 0x3F) as u8;
                    let ch = match v {
                        0..=9 => (b'0' + v) as char,
                        10..=35 => (b'A' + (v - 10)) as char,
                        36 => '_',
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "invalid old char code {}",
                                v
                            )));
                        }
                    };
                    symbol.push(ch);
                }

                let exchange_enum =
                    SerTicker::string_to_exchange(&exchange).map_err(serde::de::Error::custom)?;

                Ok(Ticker::new(&symbol, exchange_enum))
            }
        }
    }
}

pub enum StreamPairKind {
    SingleSource(TickerInfo),
    MultiSource(Vec<TickerInfo>),
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Hash, Eq)]
pub struct TickerInfo {
    pub ticker: Ticker,
    #[serde(rename = "tickSize")]
    pub min_ticksize: MinTicksize,
    pub min_qty: MinQtySize,
    pub contract_size: Option<ContractSize>,
}

impl TickerInfo {
    pub fn new(
        ticker: Ticker,
        min_ticksize: f32,
        min_qty: f32,
        contract_size: Option<f32>,
    ) -> Self {
        Self {
            ticker,
            min_ticksize: MinTicksize::from(min_ticksize),
            min_qty: MinQtySize::from(min_qty),
            contract_size: contract_size.map(ContractSize::from),
        }
    }

    pub fn market_type(&self) -> MarketKind {
        self.ticker.market_type()
    }

    pub fn is_perps(&self) -> bool {
        let market_type = self.ticker.market_type();
        market_type == MarketKind::LinearPerps || market_type == MarketKind::InversePerps
    }

    pub fn exchange(&self) -> Exchange {
        self.ticker.exchange
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Trade {
    pub time: u64,
    #[serde(deserialize_with = "bool_from_int")]
    pub is_sell: bool,
    pub price: Price,
    pub qty: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Kline {
    pub time: u64,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: (f32, f32),
}

impl Kline {
    pub fn new(
        time: u64,
        open: f32,
        high: f32,
        low: f32,
        close: f32,
        volume: (f32, f32),
        min_ticksize: MinTicksize,
    ) -> Self {
        Self {
            time,
            open: Price::from_f32(open).round_to_min_tick(MinTicksize::from(min_ticksize)),
            high: Price::from_f32(high).round_to_min_tick(MinTicksize::from(min_ticksize)),
            low: Price::from_f32(low).round_to_min_tick(MinTicksize::from(min_ticksize)),
            close: Price::from_f32(close).round_to_min_tick(MinTicksize::from(min_ticksize)),
            volume,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TickerStats {
    pub mark_price: f32,
    pub daily_price_chg: f32,
    pub daily_volume: f32,
}

pub fn is_symbol_supported(symbol: &str, exchange: Exchange, log: bool) -> bool {
    let valid_symbol = symbol
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid_symbol {
        return true;
    } else if log {
        log::warn!("Unsupported ticker: '{}': {:?}", exchange, symbol,);
    }
    false
}

fn bool_from_int<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if let Some(b) = value.as_bool() {
        return Ok(b);
    }
    match value.as_i64() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(serde::de::Error::custom("expected bool or 0/1")),
    }
}

fn de_string_to_f32<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    s.parse::<f32>().map_err(serde::de::Error::custom)
}

fn de_string_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    s.parse::<u64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenInterest {
    pub time: u64,
    pub value: f32,
}

/// Funding rate data point for perpetual contracts
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FundingRate {
    pub time: u64,
    pub rate: f32,
}

/// Spot kline data for basis calculation (futures - spot spread)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotKline {
    pub time: u64,
    pub close: f32,
}

/// Market tension data combining multiple metrics for MTM Tension Index
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarketTensionData {
    pub time: u64,
    pub volatility: f32, // High - Low (True Range)
    pub volume: f32,     // Raw volume
    pub funding: f32,    // Absolute funding rate
    pub basis: f32,      // Absolute basis (futures - spot)
}

/// Net OI data point from external API (bitcoincounterflow.com)
/// Contains price and open interest for Net Longs/Shorts calculation
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetOiDataPoint {
    pub time: u64,
    pub price: f32,
    pub open_interest: f64,
}

fn str_f32_parse(s: &str) -> f32 {
    s.parse::<f32>().unwrap_or_else(|e| {
        log::error!("Failed to parse float: {}, error: {}", s, e);
        0.0
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Hash)]
pub struct TickMultiplier(pub u16);

impl std::fmt::Display for TickMultiplier {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}x", self.0)
    }
}

impl TickMultiplier {
    pub const ALL: [TickMultiplier; 9] = [
        TickMultiplier(1),
        TickMultiplier(2),
        TickMultiplier(5),
        TickMultiplier(10),
        TickMultiplier(25),
        TickMultiplier(50),
        TickMultiplier(100),
        TickMultiplier(200),
        TickMultiplier(500),
    ];

    pub fn is_custom(&self) -> bool {
        !Self::ALL.contains(self)
    }

    pub fn base(&self, scaled_value: f32) -> f32 {
        let decimals = (-scaled_value.log10()).ceil() as i32 + 2;
        let multiplier = 10f32.powi(decimals);

        ((scaled_value * multiplier) / f32::from(self.0)).round() / multiplier
    }

    /// Returns the final tick size after applying the user selected multiplier
    ///
    /// Usually used for price steps in chart scales
    pub fn multiply_with_min_tick_size(&self, ticker_info: TickerInfo) -> f32 {
        // MinTicksize is 10^p with p in [-8, 2]
        let power = ticker_info.min_ticksize.power as i32;
        let multiply = self.0 as f32;

        let decimal_places: u32 = if power < 0 { (-power) as u32 } else { 0 };

        let raw = if power >= 0 {
            multiply * 10f32.powi(power)
        } else {
            multiply / 10f32.powi(-power)
        };

        round_to_decimal_places(raw, decimal_places)
    }
}

fn round_to_decimal_places(value: f32, places: u32) -> f32 {
    let factor = 10.0f32.powi(places as i32);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_serde_roundtrip() {
        let trade = Trade {
            time: 1726210800000,
            is_sell: true,
            price: util::Price {
                units: 5800000000000,
            },
            qty: 1.25,
        };

        let json = serde_json::to_string(&trade).expect("serialize trade");
        let de: Trade = serde_json::from_str(&json).expect("deserialize trade");
        assert_eq!(de.time, trade.time);
        assert_eq!(de.is_sell, trade.is_sell);
        assert_eq!(de.price.units, trade.price.units);
        assert_eq!(de.qty, trade.qty);

        let sonic_bytes = sonic_rs::to_vec(&trade).expect("sonic serialize");
        let sonic_de: Trade = sonic_rs::from_slice(&sonic_bytes).expect("sonic deserialize");
        assert_eq!(sonic_de.time, trade.time);
        assert_eq!(sonic_de.is_sell, trade.is_sell);
        assert_eq!(sonic_de.price.units, trade.price.units);
        assert_eq!(sonic_de.qty, trade.qty);
    }

    #[tokio::test]
    async fn test_get_hist_trades_sep12() {
        let ticker = Ticker::new("btcusdt", Exchange::BinanceLinear);
        let ticker_info = TickerInfo {
            ticker,
            min_ticksize: util::MinTicksize { power: -1 },
            min_qty: util::MinQtySize { power: -3 },
            contract_size: None,
        };
        let mut data_path = std::path::PathBuf::from(
            r"C:\Users\Breeze\AppData\Roaming\hawk-terminal\market_data\binance",
        );
        if !data_path.exists() {
            data_path = std::path::PathBuf::from(
                r"C:\Users\Breeze\AppData\Roaming\flowsurface\market_data\binance",
            );
        }
        if !data_path.exists() {
            return;
        }
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let trades = adapter::binance::get_hist_trades(ticker_info, date, data_path.clone()).await;
        match &trades {
            Ok(t) => println!(
                "Parsed {} trades, first: {:?}, last: {:?}",
                t.len(),
                t.first(),
                t.last()
            ),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(trades.is_ok());

        // Now test fetch_trades at 00:00:00 on Sep 12
        let t0 = 1789171200000u64; // 2026-09-12 00:00:00
        let res = adapter::binance::fetch_trades(ticker_info, t0, data_path).await;
        match &res {
            Ok((t, next_t)) => println!(
                "fetch_trades Sep 12: {} trades, next_t: {}, first: {:?}, last: {:?}",
                t.len(),
                next_t,
                t.first(),
                t.last()
            ),
            Err(e) => println!("fetch_trades error: {:?}", e),
        }
        assert!(res.is_ok());
    }

    #[test]
    fn test_binary_cache_roundtrip() {
        let original_trades = vec![
            Trade {
                time: 1726210800000,
                is_sell: true,
                price: util::Price {
                    units: 5800000000000,
                },
                qty: 1.25,
            },
            Trade {
                time: 1726210800123,
                is_sell: false,
                price: util::Price {
                    units: 5800010000000,
                },
                qty: 0.005,
            },
            Trade {
                time: 1726210801000,
                is_sell: true,
                price: util::Price { units: -100 },
                qty: 100000.0,
            },
        ];

        let encoded = adapter::binance::encode_trades_binary(&original_trades);
        let decoded = adapter::binance::decode_trades_binary(&encoded).expect("decode failed");

        assert_eq!(decoded.len(), original_trades.len());
        for (dec, orig) in decoded.iter().zip(original_trades.iter()) {
            assert_eq!(dec.time, orig.time);
            assert_eq!(dec.is_sell, orig.is_sell);
            assert_eq!(dec.price.units, orig.price.units);
            assert_eq!(dec.qty, orig.qty);
        }
    }

    #[test]
    fn test_binary_cache_corruption_detection() {
        let original_trades = vec![Trade {
            time: 1000,
            is_sell: false,
            price: util::Price { units: 50000 },
            qty: 1.0,
        }];

        let encoded = adapter::binance::encode_trades_binary(&original_trades);

        // 1. Corrupt magic bytes
        let mut corrupted_magic = encoded.clone();
        // Decompress, change magic, recompress
        let mut raw = lz4_flex::decompress_size_prepended(&corrupted_magic).unwrap();
        raw[0] = b'X';
        corrupted_magic = lz4_flex::compress_prepend_size(&raw);
        assert!(adapter::binance::decode_trades_binary(&corrupted_magic).is_err());

        // 2. Corrupt trade count
        let mut corrupted_count = encoded.clone();
        let mut raw = lz4_flex::decompress_size_prepended(&corrupted_count).unwrap();
        raw[4] = 99; // change count
        corrupted_count = lz4_flex::compress_prepend_size(&raw);
        assert!(adapter::binance::decode_trades_binary(&corrupted_count).is_err());

        // 3. Truncated data
        assert!(adapter::binance::decode_trades_binary(&encoded[..4]).is_err());
    }

    #[tokio::test]
    async fn test_benchmark_bin_vs_zip() {
        let ticker = Ticker::new("btcusdt", Exchange::BinanceLinear);
        let ticker_info = TickerInfo {
            ticker,
            min_ticksize: util::MinTicksize { power: -1 },
            min_qty: util::MinQtySize { power: -3 },
            contract_size: None,
        };
        let mut data_path = std::path::PathBuf::from(
            r"C:\Users\Breeze\AppData\Roaming\hawk-terminal\market_data\binance",
        );
        if !data_path.exists() {
            data_path = std::path::PathBuf::from(
                r"C:\Users\Breeze\AppData\Roaming\flowsurface\market_data\binance",
            );
        }
        if !data_path.exists() {
            return;
        }
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();

        let t_start_bin = std::time::Instant::now();
        let bin_trades = adapter::binance::get_hist_trades(ticker_info, date, data_path)
            .await
            .expect("read bin cache");
        let bin_duration = t_start_bin.elapsed();

        println!(
            "Binary cache read: {} trades in {:?} ({:.2} ms)",
            bin_trades.len(),
            bin_duration,
            bin_duration.as_secs_f64() * 1000.0
        );

        assert!(!bin_trades.is_empty());
    }

    #[tokio::test]
    async fn test_get_hist_trades_aug21() {
        let ticker = Ticker::new("btcusdt", Exchange::BinanceLinear);
        let ticker_info = TickerInfo {
            ticker,
            min_ticksize: util::MinTicksize { power: -1 },
            min_qty: util::MinQtySize { power: -3 },
            contract_size: None,
        };
        let mut data_path = std::path::PathBuf::from(
            r"C:\Users\Breeze\AppData\Roaming\hawk-terminal\market_data\binance",
        );
        if !data_path.exists() {
            data_path = std::path::PathBuf::from(
                r"C:\Users\Breeze\AppData\Roaming\flowsurface\market_data\binance",
            );
        }
        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
        let trades = adapter::binance::get_hist_trades(ticker_info, date, data_path).await;
        match &trades {
            Ok(t) => println!("Aug 21 SUCCESS: {} trades", t.len()),
            Err(e) => println!("Aug 21 ERROR: {:?}", e),
        }
        assert!(trades.is_ok());
    }

    #[test]
    fn test_find_gap_index() {
        let trades = vec![
            Trade {
                time: 1000,
                is_sell: false,
                price: util::Price { units: 100 },
                qty: 1.0,
            },
            Trade {
                time: 2000,
                is_sell: true,
                price: util::Price { units: 101 },
                qty: 2.0,
            },
            // Gap > 60_000 ms
            Trade {
                time: 100_000,
                is_sell: false,
                price: util::Price { units: 102 },
                qty: 1.5,
            },
            Trade {
                time: 101_000,
                is_sell: true,
                price: util::Price { units: 103 },
                qty: 0.5,
            },
        ];

        // Searching from index 0 should find the gap between index 1 (2000) and index 2 (100_000)
        assert_eq!(
            adapter::binance::find_gap_index(&trades, 0, 60_000),
            Some(1)
        );
        // Searching from index 1 should still find the gap at index 1
        assert_eq!(
            adapter::binance::find_gap_index(&trades, 1, 60_000),
            Some(1)
        );
        // Searching from index 2 should find no gap
        assert_eq!(adapter::binance::find_gap_index(&trades, 2, 60_000), None);
        // Searching past end
        assert_eq!(adapter::binance::find_gap_index(&trades, 3, 60_000), None);
    }

    #[test]
    fn test_bybit_gz_csv_parsing() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let csv_content = b"timestamp,symbol,side,size,price,tickDirection,trdMatchID,grossValue,homeNotional,foreignNotional\n1672531200.123,BTCUSDT,Buy,0.5,60000.5,PlusTick,1,30000,0.5,30000\n1672531201.456,BTCUSDT,Sell,1.25,60001.0,MinusTick,2,75000,1.25,75000\n";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(csv_content).unwrap();
        let gz_bytes = encoder.finish().unwrap();

        let gz_decoder = flate2::read::GzDecoder::new(&gz_bytes[..]);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(std::io::BufReader::new(gz_decoder));

        let headers = rdr.headers().unwrap().clone();
        let time_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("timestamp"))
            .unwrap();
        let side_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("side"))
            .unwrap();
        let size_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("size"))
            .unwrap();
        let price_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("price"))
            .unwrap();

        let mut trades = Vec::new();
        for res in rdr.records() {
            let record = res.unwrap();
            let raw_time = &record[time_col];
            let time = (raw_time.parse::<f64>().unwrap() * 1000.0) as u64;
            let is_sell = record[side_col].eq_ignore_ascii_case("sell");
            let price_f32 = record[price_col].parse::<f32>().unwrap();
            let qty = record[size_col].parse::<f32>().unwrap();
            trades.push(Trade {
                time,
                is_sell,
                price: Price::from_f32(price_f32),
                qty,
            });
        }

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].time, 1672531200123);
        assert!(!trades[0].is_sell);
        assert_eq!(trades[0].qty, 0.5);
        assert_eq!(trades[1].time, 1672531201456);
        assert!(trades[1].is_sell);
        assert_eq!(trades[1].qty, 1.25);
    }

    #[test]
    fn test_okx_zip_csv_parsing() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let csv_content = b"tradeId,px,sz,side,ts\n1001,65000.25,1.5,sell,1672531200000\n1002,65001.0,0.8,buy,1672531201000\n";
        let mut zip_buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip_writer = zip::ZipWriter::new(&mut zip_buffer);
            zip_writer
                .start_file("BTC-USDT-trades.csv", SimpleFileOptions::default())
                .unwrap();
            zip_writer.write_all(csv_content).unwrap();
            zip_writer.finish().unwrap();
        }

        let zip_bytes = zip_buffer.into_inner();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
        let file = archive.by_index(0).unwrap();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(std::io::BufReader::new(file));

        let headers = rdr.headers().unwrap().clone();
        let time_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("ts"))
            .unwrap();
        let side_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("side"))
            .unwrap();
        let size_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("sz"))
            .unwrap();
        let price_col = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("px"))
            .unwrap();

        let mut trades = Vec::new();
        for res in rdr.records() {
            let record = res.unwrap();
            let time = record[time_col].parse::<u64>().unwrap();
            let is_sell = record[side_col].eq_ignore_ascii_case("sell");
            let price_f32 = record[price_col].parse::<f32>().unwrap();
            let qty = record[size_col].parse::<f32>().unwrap();
            trades.push(Trade {
                time,
                is_sell,
                price: Price::from_f32(price_f32),
                qty,
            });
        }

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].time, 1672531200000);
        assert!(trades[0].is_sell);
        assert_eq!(trades[0].qty, 1.5);
        assert_eq!(trades[1].time, 1672531201000);
        assert!(!trades[1].is_sell);
        assert_eq!(trades[1].qty, 0.8);
    }

    #[test]
    fn test_hyperliquid_recent_trades_json() {
        let json_data = r#"[
            {"coin":"BTC","side":"A","sz":"0.015","px":"98450.5","time":1725648785150,"hash":"0xabc","tid":123},
            {"coin":"BTC","side":"B","sz":"0.1","px":"98445.0","time":1725648784900,"hash":"0xdef","tid":122}
        ]"#;

        #[derive(serde::Deserialize)]
        struct Item {
            side: String,
            sz: String,
            px: String,
            time: u64,
        }

        let items: Vec<Item> = serde_json::from_str(json_data).unwrap();
        let trades: Vec<Trade> = items
            .into_iter()
            .map(|it| Trade {
                time: it.time,
                is_sell: it.side == "A",
                price: Price::from_f32(it.px.parse::<f32>().unwrap()),
                qty: it.sz.parse::<f32>().unwrap(),
            })
            .collect();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].time, 1725648785150);
        assert!(trades[0].is_sell); // Ask == Sell
        assert_eq!(trades[0].qty, 0.015);
        assert_eq!(trades[1].time, 1725648784900);
        assert!(!trades[1].is_sell); // Bid == Buy
        assert_eq!(trades[1].qty, 0.1);
    }

    #[test]
    fn test_unified_binary_cache_paths() {
        let base_path = std::path::PathBuf::from("C:/data");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();

        let binance_ticker = TickerInfo {
            ticker: Ticker::new("btcusdt", Exchange::BinanceLinear),
            min_ticksize: util::MinTicksize { power: -1 },
            min_qty: util::MinQtySize { power: -3 },
            contract_size: None,
        };
        let bybit_ticker = TickerInfo {
            ticker: Ticker::new("BTCUSDT", Exchange::BybitLinear),
            min_ticksize: util::MinTicksize { power: -1 },
            min_qty: util::MinQtySize { power: -3 },
            contract_size: None,
        };

        let binance_path = trades::cache::raw_trade_bin_path(&base_path, &binance_ticker, date);
        assert!(
            binance_path
                .to_str()
                .unwrap()
                .contains("aggTrades-2026-09-18.bin")
        );

        let bybit_path = trades::cache::raw_trade_bin_path(&base_path, &bybit_ticker, date);
        assert!(
            bybit_path
                .to_str()
                .unwrap()
                .contains("trades-2026-09-18.bin")
        );
    }

    #[test]
    fn test_timeframe_subminute_and_minutes() {
        let subminute = [
            Timeframe::MS100,
            Timeframe::MS200,
            Timeframe::MS300,
            Timeframe::MS500,
            Timeframe::MS1000,
        ];
        for tf in subminute {
            assert_eq!(tf.to_minutes(), 0);
            assert_eq!(tf.try_to_minutes(), None);
        }

        assert_eq!(Timeframe::M1.to_minutes(), 1);
        assert_eq!(Timeframe::M1.try_to_minutes(), Some(1));
        assert_eq!(Timeframe::H1.to_minutes(), 60);
        assert_eq!(Timeframe::H1.try_to_minutes(), Some(60));
        assert_eq!(Timeframe::D1.to_minutes(), 1440);
        assert_eq!(Timeframe::D1.try_to_minutes(), Some(1440));
    }
}
