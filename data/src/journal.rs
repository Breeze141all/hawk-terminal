use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use uuid::Uuid;

pub const JOURNAL_PATH: &str = "journal.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum JournalMode {
    Disabled,
    #[default]
    Basic,
    Extended,
}

impl JournalMode {
    pub const ALL: [Self; 3] = [Self::Disabled, Self::Basic, Self::Extended];
}

impl std::fmt::Display for JournalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalMode::Disabled => write!(f, "Disabled"),
            JournalMode::Basic => write!(f, "Basic (Sidebar)"),
            JournalMode::Extended => write!(f, "Extended (Window)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Long,
    Short,
}

impl std::fmt::Display for TradeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeSide::Long => write!(f, "LONG"),
            TradeSide::Short => write!(f, "SHORT"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeStatus {
    Open,
    Closed,
    Cancelled,
}

impl std::fmt::Display for TradeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeStatus::Open => write!(f, "Open"),
            TradeStatus::Closed => write!(f, "Closed"),
            TradeStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JournalEntry {
    pub id: Uuid,
    pub timestamp_open: u64,
    pub timestamp_close: Option<u64>,
    pub exchange: String,
    pub ticker: String,
    pub side: TradeSide,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub size: f64,
    pub fee: f64,
    pub pnl: Option<f64>,
    pub pnl_percent: Option<f64>,
    pub status: TradeStatus,
    pub setup_tag: Option<String>,
    pub notes: String,
}

impl JournalEntry {
    pub fn new(
        exchange: String,
        ticker: String,
        side: TradeSide,
        entry_price: f64,
        size: f64,
        fee: f64,
        setup_tag: Option<String>,
        notes: String,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: Uuid::new_v4(),
            timestamp_open: now,
            timestamp_close: None,
            exchange,
            ticker,
            side,
            entry_price,
            exit_price: None,
            size,
            fee,
            pnl: None,
            pnl_percent: None,
            status: TradeStatus::Open,
            setup_tag,
            notes,
        }
    }

    pub fn close(&mut self, exit_price: f64, close_fee: f64) {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        self.timestamp_close = Some(now);
        self.exit_price = Some(exit_price);
        self.fee += close_fee;
        self.status = TradeStatus::Closed;

        let (pnl, pnl_pct) =
            compute_pnl(self.side, self.entry_price, exit_price, self.size, self.fee);
        self.pnl = Some(pnl);
        self.pnl_percent = Some(pnl_pct);
    }

    pub fn recompute_pnl(&mut self) {
        if let Some(exit_price) = self.exit_price {
            let (pnl, pnl_pct) =
                compute_pnl(self.side, self.entry_price, exit_price, self.size, self.fee);
            self.pnl = Some(pnl);
            self.pnl_percent = Some(pnl_pct);
        } else {
            self.pnl = None;
            self.pnl_percent = None;
        }
    }
}

pub fn compute_pnl(
    side: TradeSide,
    entry_price: f64,
    exit_price: f64,
    size: f64,
    fee: f64,
) -> (f64, f64) {
    if entry_price <= 0.0 || size <= 0.0 {
        return (0.0, 0.0);
    }

    let raw_diff = match side {
        TradeSide::Long => exit_price - entry_price,
        TradeSide::Short => entry_price - exit_price,
    };

    let pnl = (raw_diff * size) - fee;
    let pnl_pct = (raw_diff / entry_price) * 100.0;

    (pnl, pnl_pct)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JournalStats {
    pub total_trades: usize,
    pub open_trades: usize,
    pub closed_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub breakeven_trades: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub total_profit: f64,
    pub total_loss: f64,
    pub profit_factor: f64,
    pub total_fees: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
}

impl JournalStats {
    pub fn compute(entries: &[JournalEntry]) -> Self {
        let mut stats = JournalStats {
            total_trades: entries.len(),
            ..Default::default()
        };

        for entry in entries {
            stats.total_fees += entry.fee;

            match entry.status {
                TradeStatus::Open => stats.open_trades += 1,
                TradeStatus::Cancelled => {}
                TradeStatus::Closed => {
                    stats.closed_trades += 1;
                    if let Some(pnl) = entry.pnl {
                        stats.net_pnl += pnl;
                        if pnl > 0.0001 {
                            stats.winning_trades += 1;
                            stats.total_profit += pnl;
                        } else if pnl < -0.0001 {
                            stats.losing_trades += 1;
                            stats.total_loss += pnl.abs();
                        } else {
                            stats.breakeven_trades += 1;
                        }
                    }
                }
            }
        }

        if stats.closed_trades > 0 {
            stats.win_rate = (stats.winning_trades as f64 / stats.closed_trades as f64) * 100.0;
        }

        if stats.winning_trades > 0 {
            stats.avg_win = stats.total_profit / stats.winning_trades as f64;
        }

        if stats.losing_trades > 0 {
            stats.avg_loss = stats.total_loss / stats.losing_trades as f64;
        }

        if stats.total_loss > 0.0 {
            stats.profit_factor = stats.total_profit / stats.total_loss;
        } else if stats.total_profit > 0.0 {
            stats.profit_factor = f64::INFINITY;
        }

        stats
    }
}

pub fn save_journal(entries: &[JournalEntry]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::write_json_to_file(&json, JOURNAL_PATH)
}

pub fn load_journal() -> Vec<JournalEntry> {
    let path = crate::data_path(Some(JOURNAL_PATH));
    let Ok(mut file) = File::open(&path) else {
        return Vec::new();
    };
    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() {
        return Vec::new();
    }
    serde_json::from_str(&contents).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_pnl_long() {
        let (pnl, pct) = compute_pnl(TradeSide::Long, 50000.0, 55000.0, 0.1, 5.0);
        // raw diff: 5000 * 0.1 = 500. pnl = 500 - 5 = 495.
        assert!((pnl - 495.0).abs() < 1e-4);
        // pct: 5000 / 50000 = 10%
        assert!((pct - 10.0).abs() < 1e-4);
    }

    #[test]
    fn test_compute_pnl_short() {
        let (pnl, pct) = compute_pnl(TradeSide::Short, 50000.0, 45000.0, 0.1, 5.0);
        // raw diff: 5000 * 0.1 = 500. pnl = 500 - 5 = 495.
        assert!((pnl - 495.0).abs() < 1e-4);
        assert!((pct - 10.0).abs() < 1e-4);
    }

    #[test]
    fn test_journal_stats() {
        let mut trade1 = JournalEntry::new(
            "Binance".into(),
            "BTCUSDT".into(),
            TradeSide::Long,
            50000.0,
            1.0,
            0.0,
            Some("Breakout".into()),
            "".into(),
        );
        trade1.close(51000.0, 0.0); // +1000

        let mut trade2 = JournalEntry::new(
            "Binance".into(),
            "ETHUSDT".into(),
            TradeSide::Long,
            3000.0,
            1.0,
            0.0,
            None,
            "".into(),
        );
        trade2.close(2800.0, 0.0); // -200

        let trade3 = JournalEntry::new(
            "Bybit".into(),
            "SOLUSDT".into(),
            TradeSide::Short,
            150.0,
            10.0,
            0.0,
            None,
            "".into(),
        ); // Open

        let stats = JournalStats::compute(&[trade1, trade2, trade3]);
        assert_eq!(stats.total_trades, 3);
        assert_eq!(stats.open_trades, 1);
        assert_eq!(stats.closed_trades, 2);
        assert_eq!(stats.winning_trades, 1);
        assert_eq!(stats.losing_trades, 1);
        assert!((stats.win_rate - 50.0).abs() < 1e-4);
        assert!((stats.net_pnl - 800.0).abs() < 1e-4);
        assert!((stats.profit_factor - 5.0).abs() < 1e-4);
        assert!((stats.avg_win - 1000.0).abs() < 1e-4);
        assert!((stats.avg_loss - 200.0).abs() < 1e-4);
    }

    #[test]
    fn test_journal_mode_defaults_and_display() {
        assert_eq!(JournalMode::default(), JournalMode::Basic);
        assert_eq!(JournalMode::Disabled.to_string(), "Disabled");
        assert_eq!(JournalMode::Basic.to_string(), "Basic (Sidebar)");
        assert_eq!(JournalMode::Extended.to_string(), "Extended (Window)");
    }

    #[test]
    fn test_journal_mode_serde() {
        let json_disabled = serde_json::to_string(&JournalMode::Disabled).unwrap();
        assert_eq!(json_disabled, "\"Disabled\"");
        let de: JournalMode = serde_json::from_str(&json_disabled).unwrap();
        assert_eq!(de, JournalMode::Disabled);

        let json_extended = serde_json::to_string(&JournalMode::Extended).unwrap();
        assert_eq!(json_extended, "\"Extended\"");
        let de: JournalMode = serde_json::from_str(&json_extended).unwrap();
        assert_eq!(de, JournalMode::Extended);
    }

    #[test]
    fn test_state_backward_compatibility() {
        // Simulating an existing saved-state.json that doesn't have "journal_mode"
        let json_without_journal_mode = "{}";
        let state: crate::State = serde_json::from_str(json_without_journal_mode).unwrap();
        assert_eq!(state.journal_mode, JournalMode::Basic);
    }
}
