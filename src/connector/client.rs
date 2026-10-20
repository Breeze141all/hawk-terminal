use arrow_array::{Array, BooleanArray, Float64Array, Int64Array, RecordBatch};
use arrow_ipc::reader::StreamReader;
use exchange::adapter::{AdapterError, MarketKind};
use exchange::util::Price;
use exchange::{SizeUnit, TickerInfo, Trade, volume_size_unit};

#[derive(Clone, Default, Debug)]
pub struct DataSources {
    pub server: Option<ServerClient>,
}

impl DataSources {
    /// Build HTTP client and optionally create a market-data server client
    /// from the persisted network configuration.
    pub fn new(network: &data::Network) -> Self {
        let server_http = Self::build_server_http_client();
        let server = ServerClient::from_mode(&server_http, network);
        Self { server }
    }

    fn build_server_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap_or_else(|e| {
                log::error!("Fatal: failed to build server HTTP client: {e}");
                panic!("Failed to build server HTTP client: {e}");
            })
    }
}

/// A handle to a remote market-data HTTP server.
///
/// The server is expected to expose a `GET /trades.arrow` endpoint that
/// returns Arrow IPC streams of trade data. Query parameters:
/// `venue`, `market`, `symbol`, `from`, `to`, `limit`.
#[derive(Debug, Clone)]
pub struct ServerClient {
    base_url: String,
    client: reqwest::Client,
    auth_token: Option<String>,
}

impl ServerClient {
    /// Create a new client targeting the given base URL (e.g. `http://127.0.0.1:8080`).
    /// Trailing slashes are stripped. Returns `None` if the URL is empty or invalid.
    pub fn new(
        base_url: &str,
        auth_token: Option<String>,
        client: reqwest::Client,
    ) -> Option<Self> {
        let trimmed = base_url.trim().trim_end_matches('/');

        if trimmed.is_empty() {
            return None;
        }

        let _ = trimmed
            .parse::<url::Url>()
            .map_err(|e| log::warn!("Invalid server base URL '{trimmed}': {e}"))
            .ok()?;

        Some(Self {
            base_url: trimmed.to_string(),
            client,
            auth_token,
        })
    }

    /// Create a client with a default TLS-relaxed reqwest client.
    pub fn from_url(base_url: &str, auth_token: Option<String>) -> Option<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .build()
            .ok()?;
        Self::new(base_url, auth_token, client)
    }

    /// Base URL of the market-data server.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    /// Build a [`ServerClient`] from the shared HTTP client and the current
    /// network configuration.
    pub fn from_mode(http_client: &reqwest::Client, network: &data::Network) -> Option<Self> {
        match &network.trade_fetch_mode {
            data::TradeFetchMode::Server => {
                let url = network.server_url.as_deref()?;
                Self::new(url, network.server_auth_token.clone(), http_client.clone())
            }
            _ => None,
        }
    }

    /// Fetch trades from the server's **Arrow IPC** endpoint (`/trades.arrow`).
    pub async fn fetch_trades_arrow(
        &self,
        ticker_info: TickerInfo,
        from: u64,
        to: u64,
        limit: usize,
    ) -> Result<ParsedArrowBatch, AdapterError> {
        let (venue, market) = {
            let exchange = ticker_info.exchange();
            let venue = exchange.venue().to_string().to_lowercase();
            let market = exchange.market_type().to_string().to_lowercase();
            (venue, market)
        };
        let symbol = if let Some(display_symbol) = ticker_info.ticker.display_symbol() {
            display_symbol.to_lowercase()
        } else {
            ticker_info.ticker.as_str().to_lowercase()
        };

        let url = format!("{}/trades.arrow", self.base_url());

        log::debug!(
            "Querying server (arrow): {url} | venue={venue} market={market} symbol={symbol} from={from} to={to} limit={limit}",
        );

        let mut request = self
            .http_client()
            .get(&url)
            .query(&[
                ("venue", venue.as_str()),
                ("market", market.as_str()),
                ("symbol", symbol.as_str()),
            ])
            .query(&[("from", from.to_string())])
            .query(&[("to", to.to_string())])
            .query(&[("limit", limit.to_string())]);

        if let Some(token) = self.auth_token() {
            request = request.header("Authorization", format!("Bearer {token}"));
        }

        let response = request.send().await.map_err(|e| {
            log::warn!("Server arrow request failed: {e}");
            AdapterError::FetchError(e)
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            log::warn!("Server returned {status}: {body}");
            return Err(AdapterError::ParseError(format!(
                "server (arrow) {status}: {body}"
            )));
        }

        if let Some(content_type) = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            && content_type != "application/vnd.apache.arrow.stream"
        {
            log::warn!(
                "Server returned unexpected Content-Type '{content_type}', \
                 expected 'application/vnd.apache.arrow.stream'"
            );
        }

        let bytes = response.bytes().await.map_err(|e| {
            log::warn!("Failed to read arrow response body: {e}");
            AdapterError::ParseError(format!("server (arrow): {e}"))
        })?;

        parse_arrow_trades(bytes, &ticker_info)
    }
}

/// Expected Arrow column names in the IPC stream.
pub const ARROW_TS_COL: &str = "ts";
pub const ARROW_PRICE_COL: &str = "price";
pub const ARROW_QTY_COL: &str = "qty";
pub const ARROW_IS_SELL_COL: &str = "is_sell";

/// Resolved column indices for the four required Arrow fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeColumns {
    pub ts: usize,
    pub price: usize,
    pub qty: usize,
    pub is_sell: usize,
}

/// Validate the Arrow stream schema up-front: resolve column indices by
/// name, verify the expected Arrow data types, and fail fast on any
/// mismatch before a single row is parsed.
pub fn validate_arrow_schema(schema: &arrow_schema::Schema) -> Result<TradeColumns, AdapterError> {
    use arrow_schema::DataType;

    let index_of = |name: &str| {
        schema.index_of(name).map_err(|_| {
            AdapterError::ParseError(format!(
                "Arrow stream schema missing required column '{name}'"
            ))
        })
    };

    let cols = TradeColumns {
        ts: index_of(ARROW_TS_COL)?,
        price: index_of(ARROW_PRICE_COL)?,
        qty: index_of(ARROW_QTY_COL)?,
        is_sell: index_of(ARROW_IS_SELL_COL)?,
    };

    let check_type = |idx: usize, name: &str, expected: &DataType| -> Result<(), AdapterError> {
        let field = schema.field(idx);
        if field.data_type() != expected {
            return Err(AdapterError::ParseError(format!(
                "Arrow stream schema column '{name}': expected {expected:?}, got {:?}",
                field.data_type()
            )));
        }
        Ok(())
    };

    check_type(cols.ts, ARROW_TS_COL, &DataType::Int64)?;
    check_type(cols.price, ARROW_PRICE_COL, &DataType::Float64)?;
    check_type(cols.qty, ARROW_QTY_COL, &DataType::Float64)?;
    check_type(cols.is_sell, ARROW_IS_SELL_COL, &DataType::Boolean)?;

    Ok(cols)
}

/// Unit of raw quantity values returned by an exchange or data server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawQtyUnit {
    Base,
    Quote,
    Contracts,
}

#[derive(Debug, Clone, Copy)]
pub struct QtyNormalization {
    pub size_in_quote_ccy: bool,
    pub contract_size: Option<f64>,
    pub market_kind: MarketKind,
    pub raw_qty_unit: RawQtyUnit,
}

impl QtyNormalization {
    pub fn new(size_in_quote_ccy: bool, ticker: TickerInfo, raw_qty_unit: RawQtyUnit) -> Self {
        let market_kind = ticker.exchange().market_type();
        let contract_size = ticker.effective_contract_size().map(|cs| cs as f64).or({
            if matches!(market_kind, MarketKind::InversePerps) {
                Some(1.0)
            } else {
                None
            }
        });

        Self {
            size_in_quote_ccy,
            contract_size,
            market_kind,
            raw_qty_unit,
        }
    }

    pub fn normalize(&self, qty: f64, price: f64) -> f32 {
        let safe_price = if price <= f64::EPSILON { 1.0 } else { price };
        let (base_qty, quote_qty) = match self.raw_qty_unit {
            RawQtyUnit::Base => (qty, qty * price),
            RawQtyUnit::Quote => (qty / safe_price, qty),
            RawQtyUnit::Contracts => {
                let contract_size = self.contract_size.unwrap_or(1.0);
                if matches!(self.market_kind, MarketKind::InversePerps) {
                    let quote_qty = qty * contract_size;
                    (quote_qty / safe_price, quote_qty)
                } else {
                    let base_qty = qty * contract_size;
                    (base_qty, base_qty * price)
                }
            }
        };

        let result =
            if matches!(self.market_kind, MarketKind::InversePerps) || self.size_in_quote_ccy {
                quote_qty
            } else {
                base_qty
            };
        result as f32
    }
}

/// Result of parsing an Arrow IPC trade stream.
#[derive(Debug, Clone, Default)]
pub struct ParsedArrowBatch {
    /// Valid, non-null trades ready for display.
    pub trades: Vec<Trade>,
    /// Total rows received across all batches in the stream.
    pub raw_row_count: usize,
    /// Latest non-null `ts` seen across *all* rows (including rows
    /// filtered out due to nulls in other columns). Used for cursor
    /// advancement in the paging loop.
    pub last_ts: Option<u64>,
}

/// Parse raw Arrow IPC stream bytes into a [`ParsedArrowBatch`].
///
/// Expects the Arrow IPC streaming format. The four required columns
/// (`ts`, `price`, `qty`, `is_sell`) are looked up by name. Column order
/// is not significant and extra columns are silently ignored.
pub fn parse_arrow_trades(
    data: bytes::Bytes,
    ticker_info: &TickerInfo,
) -> Result<ParsedArrowBatch, AdapterError> {
    let reader = StreamReader::try_new(std::io::Cursor::new(data), None)
        .map_err(|e| AdapterError::ParseError(format!("arrow stream open: {e}")))?;

    // Resolve column positions by name and validate types once up-front.
    let cols = validate_arrow_schema(&reader.schema())?;

    let market_kind = ticker_info.exchange().market_type();
    let raw_qty_unit = match market_kind {
        MarketKind::InversePerps => RawQtyUnit::Quote,
        MarketKind::Spot | MarketKind::LinearPerps => RawQtyUnit::Base,
    };

    let size_in_quote_ccy = volume_size_unit() == SizeUnit::Quote;
    let qty_norm = QtyNormalization::new(size_in_quote_ccy, *ticker_info, raw_qty_unit);

    let mut trades = Vec::new();
    let mut raw_row_count: usize = 0;
    let mut last_ts: Option<u64> = None;
    let mut skipped_total: u64 = 0;

    for result in reader {
        let batch: RecordBatch =
            result.map_err(|e| AdapterError::ParseError(format!("arrow batch: {e}")))?;

        raw_row_count += batch.num_rows();

        let ts_col = batch
            .column(cols.ts)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AdapterError::ParseError(format!("column '{ARROW_TS_COL}' is not Int64"))
            })?;
        let price_col = batch
            .column(cols.price)
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| {
                AdapterError::ParseError(format!("column '{ARROW_PRICE_COL}' is not Float64"))
            })?;
        let qty_col = batch
            .column(cols.qty)
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| {
                AdapterError::ParseError(format!("column '{ARROW_QTY_COL}' is not Float64"))
            })?;
        let is_sell_col = batch
            .column(cols.is_sell)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .ok_or_else(|| {
                AdapterError::ParseError(format!("column '{ARROW_IS_SELL_COL}' is not Boolean"))
            })?;

        let has_nulls = ts_col.null_count() > 0
            || price_col.null_count() > 0
            || qty_col.null_count() > 0
            || is_sell_col.null_count() > 0;

        if has_nulls {
            let mut skipped_in_batch: u64 = 0;

            for i in 0..batch.num_rows() {
                // Track the latest non-null ts across *all* rows for
                // cursor advancement, even if the row is filtered out.
                if !ts_col.is_null(i) {
                    let ts = ts_col.value(i).max(0) as u64;
                    last_ts = Some(last_ts.map_or(ts, |prev| prev.max(ts)));
                }

                if ts_col.is_null(i)
                    || price_col.is_null(i)
                    || qty_col.is_null(i)
                    || is_sell_col.is_null(i)
                {
                    skipped_in_batch += 1;
                    continue;
                }
                let ts = ts_col.value(i).max(0) as u64;
                let price_val = price_col.value(i);
                let qty_val = qty_col.value(i);
                trades.push(Trade {
                    time: ts,
                    is_sell: is_sell_col.value(i),
                    price: Price::from_f64(price_val).round_to_min_tick(ticker_info.min_ticksize),
                    qty: qty_norm.normalize(qty_val, price_val),
                });
            }

            skipped_total += skipped_in_batch;
        } else {
            for i in 0..batch.num_rows() {
                let ts = ts_col.value(i).max(0) as u64;
                let price_val = price_col.value(i);
                let qty_val = qty_col.value(i);
                trades.push(Trade {
                    time: ts,
                    is_sell: is_sell_col.value(i),
                    price: Price::from_f64(price_val).round_to_min_tick(ticker_info.min_ticksize),
                    qty: qty_norm.normalize(qty_val, price_val),
                });
            }
        }
    }

    if last_ts.is_none() {
        last_ts = trades.last().map(|t| t.time);
    }

    if skipped_total > 0 {
        log::warn!(
            "Arrow stream for {} ({}): skipped {skipped_total} of {raw_row_count} row(s) with nulls in required fields",
            ticker_info.ticker,
            ticker_info.exchange(),
        );
    }

    if trades.is_empty() {
        log::info!(
            "Arrow stream contained no valid trades for {} ({}) (raw rows: {raw_row_count})",
            ticker_info.ticker,
            ticker_info.exchange(),
        );
    }

    Ok(ParsedArrowBatch {
        trades,
        raw_row_count,
        last_ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch};
    use arrow_ipc::writer::StreamWriter;
    use arrow_schema::{DataType, Field, Schema};
    use exchange::{Exchange, Ticker};
    use std::sync::Arc;

    fn make_test_ticker_info() -> TickerInfo {
        let ticker = Ticker::new("BTCUSDT", Exchange::BinanceLinear);
        TickerInfo::new(ticker, 0.1, 0.001, None)
    }

    #[test]
    fn test_ex17_validate_arrow_schema_valid() {
        let schema = Schema::new(vec![
            Field::new(ARROW_TS_COL, DataType::Int64, false),
            Field::new(ARROW_PRICE_COL, DataType::Float64, false),
            Field::new(ARROW_QTY_COL, DataType::Float64, false),
            Field::new(ARROW_IS_SELL_COL, DataType::Boolean, false),
        ]);
        let cols = validate_arrow_schema(&schema).expect("schema must be valid");
        assert_eq!(cols.ts, 0);
        assert_eq!(cols.price, 1);
        assert_eq!(cols.qty, 2);
        assert_eq!(cols.is_sell, 3);
    }

    #[test]
    fn test_ex17_validate_arrow_schema_reordered_with_extra() {
        let schema = Schema::new(vec![
            Field::new("extra_col", DataType::Utf8, true),
            Field::new(ARROW_IS_SELL_COL, DataType::Boolean, false),
            Field::new(ARROW_QTY_COL, DataType::Float64, false),
            Field::new(ARROW_PRICE_COL, DataType::Float64, false),
            Field::new(ARROW_TS_COL, DataType::Int64, false),
        ]);
        let cols =
            validate_arrow_schema(&schema).expect("schema must allow reordering and extra columns");
        assert_eq!(cols.ts, 4);
        assert_eq!(cols.price, 3);
        assert_eq!(cols.qty, 2);
        assert_eq!(cols.is_sell, 1);
    }

    #[test]
    fn test_ex17_validate_arrow_schema_missing_column() {
        let schema = Schema::new(vec![
            Field::new(ARROW_TS_COL, DataType::Int64, false),
            Field::new(ARROW_PRICE_COL, DataType::Float64, false),
            Field::new(ARROW_QTY_COL, DataType::Float64, false),
        ]);
        let err = validate_arrow_schema(&schema).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing required column 'is_sell'")
        );
    }

    #[test]
    fn test_ex17_validate_arrow_schema_wrong_type() {
        let schema = Schema::new(vec![
            Field::new(ARROW_TS_COL, DataType::Float64, false), // Wrong type!
            Field::new(ARROW_PRICE_COL, DataType::Float64, false),
            Field::new(ARROW_QTY_COL, DataType::Float64, false),
            Field::new(ARROW_IS_SELL_COL, DataType::Boolean, false),
        ]);
        let err = validate_arrow_schema(&schema).unwrap_err();
        assert!(err.to_string().contains("expected Int64, got Float64"));
    }

    #[test]
    fn test_ex17_parse_arrow_trades_roundtrip() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(ARROW_TS_COL, DataType::Int64, false),
            Field::new(ARROW_PRICE_COL, DataType::Float64, false),
            Field::new(ARROW_QTY_COL, DataType::Float64, false),
            Field::new(ARROW_IS_SELL_COL, DataType::Boolean, false),
        ]));

        let ts_array: ArrayRef = Arc::new(Int64Array::from(vec![
            1_700_000_000,
            1_700_000_100,
            1_700_000_200,
        ]));
        let price_array: ArrayRef =
            Arc::new(Float64Array::from(vec![50_000.0, 50_100.0, 49_950.0]));
        let qty_array: ArrayRef = Arc::new(Float64Array::from(vec![1.5, 2.0, 0.5]));
        let is_sell_array: ArrayRef = Arc::new(BooleanArray::from(vec![false, true, false]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![ts_array, price_array, qty_array, is_sell_array],
        )
        .expect("RecordBatch creation must succeed");

        let mut buffer = Vec::new();
        {
            let mut writer = StreamWriter::try_new(&mut buffer, &schema)
                .expect("StreamWriter init must succeed");
            writer.write(&batch).expect("write batch must succeed");
            writer.finish().expect("finish must succeed");
        }

        let ticker_info = make_test_ticker_info();
        let parsed = parse_arrow_trades(bytes::Bytes::from(buffer), &ticker_info)
            .expect("parse_arrow_trades must succeed");

        assert_eq!(parsed.raw_row_count, 3);
        assert_eq!(parsed.trades.len(), 3);
        assert_eq!(parsed.last_ts, Some(1_700_000_200));

        assert_eq!(parsed.trades[0].time, 1_700_000_000);
        assert!(!parsed.trades[0].is_sell);
        assert_eq!(parsed.trades[0].price.to_f64(), 50_000.0);

        assert_eq!(parsed.trades[1].time, 1_700_000_100);
        assert!(parsed.trades[1].is_sell);
        assert_eq!(parsed.trades[1].price.to_f64(), 50_100.0);
    }

    #[test]
    fn test_ex17_parse_arrow_trades_null_handling() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(ARROW_TS_COL, DataType::Int64, true),
            Field::new(ARROW_PRICE_COL, DataType::Float64, true),
            Field::new(ARROW_QTY_COL, DataType::Float64, true),
            Field::new(ARROW_IS_SELL_COL, DataType::Boolean, true),
        ]));

        // Row 0: Valid
        // Row 1: Null price
        // Row 2: Null is_sell
        // Row 3: Valid
        let ts_array: ArrayRef = Arc::new(Int64Array::from(vec![
            Some(1_000),
            Some(2_000),
            Some(3_000),
            Some(4_000),
        ]));
        let price_array: ArrayRef = Arc::new(Float64Array::from(vec![
            Some(100.0),
            None,
            Some(102.0),
            Some(103.0),
        ]));
        let qty_array: ArrayRef = Arc::new(Float64Array::from(vec![
            Some(1.0),
            Some(2.0),
            Some(3.0),
            Some(4.0),
        ]));
        let is_sell_array: ArrayRef = Arc::new(BooleanArray::from(vec![
            Some(false),
            Some(true),
            None,
            Some(true),
        ]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![ts_array, price_array, qty_array, is_sell_array],
        )
        .expect("batch creation");

        let mut buffer = Vec::new();
        {
            let mut writer = StreamWriter::try_new(&mut buffer, &schema).expect("writer");
            writer.write(&batch).expect("write");
            writer.finish().expect("finish");
        }

        let ticker_info = make_test_ticker_info();
        let parsed = parse_arrow_trades(bytes::Bytes::from(buffer), &ticker_info)
            .expect("parse_arrow_trades");

        assert_eq!(parsed.raw_row_count, 4);
        assert_eq!(parsed.trades.len(), 2); // Rows 0 and 3
        assert_eq!(parsed.trades[0].time, 1_000);
        assert_eq!(parsed.trades[1].time, 4_000);
        assert_eq!(parsed.last_ts, Some(4_000));
    }

    #[test]
    fn test_ex17_server_client_url_validation() {
        assert!(ServerClient::from_url("", None).is_none());
        assert!(ServerClient::from_url("   ", None).is_none());
        assert!(ServerClient::from_url("not_a_valid_url", None).is_none());

        let client =
            ServerClient::from_url("http://127.0.0.1:8080///", Some("token123".to_string()))
                .expect("Valid URL must succeed");
        assert_eq!(client.base_url(), "http://127.0.0.1:8080");
        assert_eq!(client.auth_token(), Some("token123"));
    }
}
