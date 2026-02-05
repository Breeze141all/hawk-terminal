use crate::adapter::StreamKind;
use crate::{FundingRate, Kline, NetOiDataPoint, OpenInterest, SpotKline, Trade};

use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

static TRADE_FETCH_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn toggle_trade_fetch(value: bool) {
    TRADE_FETCH_ENABLED.store(value, Ordering::Relaxed);
}

pub fn is_trade_fetch_enabled() -> bool {
    TRADE_FETCH_ENABLED.load(Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub enum FetchedData {
    Trades {
        batch: Vec<Trade>,
        until_time: u64,
    },
    Klines {
        data: Vec<Kline>,
        req_id: Option<uuid::Uuid>,
    },
    OI {
        data: Vec<OpenInterest>,
        req_id: Option<uuid::Uuid>,
    },
    FundingRates {
        data: Vec<FundingRate>,
        req_id: Option<uuid::Uuid>,
    },
    SpotKlines {
        data: Vec<SpotKline>,
        req_id: Option<uuid::Uuid>,
    },
    NetOiData {
        data: Vec<NetOiDataPoint>,
        req_id: Option<uuid::Uuid>,
    },
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ReqError {
    #[error("Request is already completed")]
    Completed,
    #[error("Request is already failed: {0}")]
    Failed(String),
    #[error("Request overlaps with an existing request")]
    Overlaps,
}

#[derive(PartialEq, Debug)]
enum RequestStatus {
    Pending,
    Completed(u64),
    Failed(String),
}

pub struct RequestHandler {
    requests: HashMap<Uuid, FetchRequest>,
}

impl RequestHandler {
    pub fn new() -> Self {
        RequestHandler {
            requests: HashMap::new(),
        }
    }

    pub fn add_request(&mut self, fetch: FetchRange) -> Result<Option<Uuid>, ReqError> {
        let request = FetchRequest::new(fetch);
        let id = Uuid::new_v4();

        if let Some((existing_id, existing_req)) = self.requests.iter().find_map(|(k, v)| {
            if v.same_with(&request) {
                Some((*k, v))
            } else {
                None
            }
        }) {
            return match &existing_req.status {
                RequestStatus::Failed(error_msg) => Err(ReqError::Failed(error_msg.clone())),
                RequestStatus::Completed(ts) => {
                    // retry completed requests after a cooldown
                    // to handle data source failures or outdated results gracefully
                    if chrono::Utc::now().timestamp_millis() as u64 - ts > 30_000 {
                        Ok(Some(existing_id))
                    } else {
                        Ok(None)
                    }
                }
                RequestStatus::Pending => Err(ReqError::Overlaps),
            };
        }

        self.requests.insert(id, request);
        Ok(Some(id))
    }

    pub fn mark_completed(&mut self, id: Uuid) {
        if let Some(request) = self.requests.get_mut(&id) {
            let timestamp = chrono::Utc::now().timestamp_millis() as u64;
            request.status = RequestStatus::Completed(timestamp);
        } else {
            log::warn!("Request not found: {:?}", id);
        }
    }

    pub fn mark_failed(&mut self, id: Uuid, error: String) {
        if let Some(request) = self.requests.get_mut(&id) {
            request.status = RequestStatus::Failed(error);
        } else {
            log::warn!("Request not found: {:?}", id);
        }
    }
}

impl Default for RequestHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum FetchRange {
    Kline(u64, u64),
    OpenInterest(u64, u64),
    Trades(u64, u64),
    FundingRate(u64, u64),
    SpotKline(u64, u64),
    /// Net OI data fetch with days and interval parameters
    NetOiData {
        days: u16,
        interval: NetOiInterval,
    },
}

/// Interval for Net OI data API
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum NetOiInterval {
    M15,
    M30,
    H1,
    H2,
    H4,
    D1,
}

impl NetOiInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            NetOiInterval::M15 => "15m",
            NetOiInterval::M30 => "30m",
            NetOiInterval::H1 => "1h",
            NetOiInterval::H2 => "2h",
            NetOiInterval::H4 => "4h",
            NetOiInterval::D1 => "1d",
        }
    }

    pub fn from_timeframe(tf: crate::Timeframe) -> Option<Self> {
        match tf {
            crate::Timeframe::M15 => Some(NetOiInterval::M15),
            crate::Timeframe::M30 => Some(NetOiInterval::M30),
            crate::Timeframe::H1 => Some(NetOiInterval::H1),
            crate::Timeframe::H2 => Some(NetOiInterval::H2),
            crate::Timeframe::H4 => Some(NetOiInterval::H4),
            crate::Timeframe::D1 => Some(NetOiInterval::D1),
            _ => None,
        }
    }
}

#[derive(PartialEq, Debug)]
struct FetchRequest {
    fetch_type: FetchRange,
    status: RequestStatus,
}

impl FetchRequest {
    fn new(fetch_type: FetchRange) -> Self {
        FetchRequest {
            fetch_type,
            status: RequestStatus::Pending,
        }
    }

    fn same_with(&self, other: &FetchRequest) -> bool {
        match (&self.fetch_type, &other.fetch_type) {
            (FetchRange::Kline(s1, e1), FetchRange::Kline(s2, e2)) => e1 == e2 && s1 == s2,
            (FetchRange::OpenInterest(s1, e1), FetchRange::OpenInterest(s2, e2)) => {
                e1 == e2 && s1 == s2
            }
            (FetchRange::FundingRate(s1, e1), FetchRange::FundingRate(s2, e2)) => {
                e1 == e2 && s1 == s2
            }
            (FetchRange::SpotKline(s1, e1), FetchRange::SpotKline(s2, e2)) => e1 == e2 && s1 == s2,
            (
                FetchRange::NetOiData {
                    days: d1,
                    interval: i1,
                },
                FetchRange::NetOiData {
                    days: d2,
                    interval: i2,
                },
            ) => d1 == d2 && i1 == i2,
            _ => false,
        }
    }
}

pub struct FetchSpec {
    pub req_id: uuid::Uuid,
    pub fetch: FetchRange,
    pub stream: Option<StreamKind>,
}

impl From<(uuid::Uuid, FetchRange, Option<StreamKind>)> for FetchSpec {
    fn from(t: (uuid::Uuid, FetchRange, Option<StreamKind>)) -> Self {
        FetchSpec {
            req_id: t.0,
            fetch: t.1,
            stream: t.2,
        }
    }
}

pub type FetchRequests = SmallVec<[FetchSpec; 1]>;

impl std::fmt::Debug for FetchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchSpec")
            .field("req_id", &self.req_id)
            .field("fetch", &self.fetch)
            .field("stream", &self.stream)
            .finish()
    }
}

impl Clone for FetchSpec {
    fn clone(&self) -> Self {
        FetchSpec {
            req_id: self.req_id,
            fetch: self.fetch,
            stream: self.stream,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InfoKind {
    FetchingKlines,
    FetchingTrades(usize),
    FetchingOI,
    FetchingMarketPulse,
    FetchingNetOi,
}
