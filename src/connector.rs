pub mod client;
pub mod fetcher;

pub use client::{DataSources, ParsedArrowBatch, ServerClient};
pub use fetcher::{
    FetchRange, FetchTaskStatus, FetchUpdate, FetchedData, RequestHandler, TradeFetchMode,
    fetch_trades_paged,
};
