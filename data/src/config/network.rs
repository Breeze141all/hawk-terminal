use serde::{Deserialize, Serialize};

/// Network configuration for market-data fetching.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Network {
    pub server_url: Option<String>,
    /// Bearer token for the market-data server.
    /// Not persisted in plaintext JSON state.
    #[serde(skip)]
    pub server_auth_token: Option<String>,
    pub trade_fetch_mode: TradeFetchMode,
}

impl Network {
    pub fn for_persistence(&self) -> Self {
        Self {
            server_url: self.server_url.clone(),
            server_auth_token: None,
            trade_fetch_mode: self.trade_fetch_mode.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TradeFetchMode {
    #[default]
    Off,
    /// Direct exchange API only
    Exchange,
    /// Remote Arrow IPC market-data server.
    Server,
}

impl std::fmt::Display for TradeFetchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Exchange => write!(f, "Exchange"),
            Self::Server => write!(f, "Server"),
        }
    }
}
