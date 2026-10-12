use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceAlert {
    pub id: uuid::Uuid,
    #[serde(default)]
    pub ticker: Option<exchange::Ticker>,
    pub ticker_symbol: String,
    pub target_price: f32,
    pub condition: AlertCondition,
    pub status: AlertStatus,
    pub sound: bool,
    pub created_at: u64,
    #[serde(default)]
    pub initial_price: Option<f32>,
    #[serde(default)]
    pub triggered_at: Option<u64>,
    #[serde(default)]
    pub last_checked_time: Option<u64>,
    #[serde(default)]
    pub pane_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub is_synced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertCondition {
    CrossAbove,
    CrossBelow,
    Crossing,
}

impl std::fmt::Display for AlertCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertCondition::CrossAbove => write!(f, "Cross Above"),
            AlertCondition::CrossBelow => write!(f, "Cross Below"),
            AlertCondition::Crossing => write!(f, "Crossing"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    Active,
    Triggered,
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AlertFilter {
    #[default]
    ThisChart,
    AllCharts,
}

impl PriceAlert {
    pub fn new(
        ticker_symbol: impl Into<String>,
        target_price: f32,
        condition: AlertCondition,
    ) -> Self {
        Self::with_details(None, ticker_symbol, target_price, None, condition)
    }

    pub fn with_details(
        ticker: Option<exchange::Ticker>,
        ticker_symbol: impl Into<String>,
        target_price: f32,
        initial_price: Option<f32>,
        condition: AlertCondition,
    ) -> Self {
        Self::with_pane(
            ticker,
            ticker_symbol,
            target_price,
            initial_price,
            condition,
            None,
        )
    }

    pub fn with_pane(
        ticker: Option<exchange::Ticker>,
        ticker_symbol: impl Into<String>,
        target_price: f32,
        initial_price: Option<f32>,
        condition: AlertCondition,
        pane_id: Option<uuid::Uuid>,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            id: uuid::Uuid::new_v4(),
            ticker,
            ticker_symbol: ticker_symbol.into(),
            target_price,
            condition,
            status: AlertStatus::Active,
            sound: true,
            created_at: now,
            initial_price,
            triggered_at: None,
            last_checked_time: Some(now),
            pane_id,
            is_synced: false,
        }
    }

    pub fn trigger(&mut self, timestamp: Option<u64>) {
        self.status = AlertStatus::Triggered;
        self.triggered_at =
            timestamp.or_else(|| Some(chrono::Utc::now().timestamp_millis() as u64));
    }

    /// Check whether a price movement from `prev_price` to `current_price` triggers this alert.
    pub fn check_trigger(&self, prev_price: f32, current_price: f32) -> bool {
        self.check_range_trigger(
            prev_price,
            prev_price.min(current_price),
            prev_price.max(current_price),
            current_price,
        )
    }

    /// Check whether a price movement spanning [low, high] between `prev_price` and `current_price` triggers this alert.
    pub fn check_range_trigger(
        &self,
        prev_price: f32,
        low: f32,
        high: f32,
        current_price: f32,
    ) -> bool {
        if self.status != AlertStatus::Active {
            return false;
        }

        let min_p = low.min(prev_price).min(current_price);
        let max_p = high.max(prev_price).max(current_price);

        match self.condition {
            AlertCondition::CrossAbove => {
                if prev_price < self.target_price {
                    max_p >= self.target_price
                } else {
                    min_p < self.target_price && current_price >= self.target_price
                }
            }
            AlertCondition::CrossBelow => {
                if prev_price > self.target_price {
                    min_p <= self.target_price
                } else {
                    max_p > self.target_price && current_price <= self.target_price
                }
            }
            AlertCondition::Crossing => {
                (prev_price < self.target_price && max_p >= self.target_price)
                    || (prev_price > self.target_price && min_p <= self.target_price)
                    || (min_p <= self.target_price && max_p >= self.target_price)
            }
        }
    }

    /// Check whether a kline's high/low extremes triggered this alert while offline/historical scan.
    pub fn check_kline_extremes(&self, kline: &exchange::Kline) -> bool {
        if self.status != AlertStatus::Active {
            return false;
        }

        let high = kline.high.to_f32_lossy();
        let low = kline.low.to_f32_lossy();
        let open = kline.open.to_f32_lossy();
        let close = kline.close.to_f32_lossy();

        let prev = self.initial_price.unwrap_or(open);
        self.check_range_trigger(prev, low, high, close)
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};

static GLOBAL_ALERTS: LazyLock<RwLock<Vec<PriceAlert>>> =
    LazyLock::new(|| RwLock::new(crate::load_alerts()));

static ALERT_SAVE_LOCK: Mutex<()> = Mutex::new(());
static ALERT_SAVE_VERSION: AtomicU64 = AtomicU64::new(0);
static ALERT_LAST_WRITTEN_VERSION: AtomicU64 = AtomicU64::new(0);

#[inline]
fn read_alerts() -> std::sync::RwLockReadGuard<'static, Vec<PriceAlert>> {
    GLOBAL_ALERTS.read().unwrap_or_else(|p| p.into_inner())
}

#[inline]
fn write_alerts() -> std::sync::RwLockWriteGuard<'static, Vec<PriceAlert>> {
    GLOBAL_ALERTS.write().unwrap_or_else(|p| p.into_inner())
}

fn spawn_save_alerts(snapshot: Vec<PriceAlert>) {
    let version = ALERT_SAVE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        let _guard = ALERT_SAVE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if version < ALERT_LAST_WRITTEN_VERSION.load(Ordering::Relaxed) {
            return;
        }
        let _ = crate::save_alerts(&snapshot);
        ALERT_LAST_WRITTEN_VERSION.store(version, Ordering::Relaxed);
    });
}

pub struct AlertStore;

impl AlertStore {
    /// Retrieve a clone of all alerts
    pub fn all() -> Vec<PriceAlert> {
        read_alerts().clone()
    }

    /// Retrieve alerts matching a specific ticker symbol
    pub fn for_symbol(symbol: &str) -> Vec<PriceAlert> {
        read_alerts()
            .iter()
            .filter(|a| a.ticker_symbol.eq_ignore_ascii_case(symbol))
            .cloned()
            .collect()
    }

    /// Retrieve alerts for a specific ticker symbol visible to a given pane.
    /// Returns alerts that are synced to all charts, or alerts specific to this pane.
    pub fn for_symbol_and_pane(symbol: &str, pane_id: uuid::Uuid) -> Vec<PriceAlert> {
        read_alerts()
            .iter()
            .filter(|a| {
                a.ticker_symbol.eq_ignore_ascii_case(symbol)
                    && (a.is_synced || a.pane_id == Some(pane_id))
            })
            .cloned()
            .collect()
    }

    /// Add a new alert and persist to disk asynchronously
    pub fn add(alert: PriceAlert) {
        let snapshot = {
            let mut alerts = write_alerts();
            alerts.push(alert);
            alerts.clone()
        };
        spawn_save_alerts(snapshot);
    }

    /// Remove an alert by ID and persist to disk asynchronously
    pub fn remove(id: uuid::Uuid) {
        let snapshot = {
            let mut alerts = write_alerts();
            alerts.retain(|a| a.id != id);
            alerts.clone()
        };
        spawn_save_alerts(snapshot);
    }

    /// Toggle alert status between Active and Muted (or re-arm Triggered to Active)
    pub fn toggle(id: uuid::Uuid) {
        let snapshot = {
            let mut alerts = write_alerts();
            if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
                let now = chrono::Utc::now().timestamp_millis() as u64;
                alert.status = match alert.status {
                    AlertStatus::Active => AlertStatus::Muted,
                    AlertStatus::Muted | AlertStatus::Triggered => {
                        alert.triggered_at = None;
                        alert.last_checked_time = Some(now);
                        AlertStatus::Active
                    }
                };
            }
            alerts.clone()
        };
        spawn_save_alerts(snapshot);
    }

    /// Update an alert's target price (e.g. from canvas drag)
    pub fn update_price(id: uuid::Uuid, price: f32) {
        let snapshot = {
            let mut alerts = write_alerts();
            if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
                alert.target_price = price;
            }
            alerts.clone()
        };
        spawn_save_alerts(snapshot);
    }

    /// Clear all triggered alerts
    pub fn clear_triggered() {
        let snapshot = {
            let mut alerts = write_alerts();
            alerts.retain(|a| a.status != AlertStatus::Triggered);
            alerts.clone()
        };
        spawn_save_alerts(snapshot);
    }

    /// Check live price movement and trigger matching alerts.
    /// Returns any alerts that transitioned to Triggered.
    pub fn check_price_triggers(
        symbol: &str,
        prev_price: f32,
        current_price: f32,
    ) -> Vec<PriceAlert> {
        let low = prev_price.min(current_price);
        let high = prev_price.max(current_price);
        Self::check_price_triggers_range(symbol, prev_price, low, high, current_price)
    }

    /// Check live price movement spanning [low, high] extremes (e.g. candle wicks or trade batches).
    /// Returns any alerts that transitioned to Triggered.
    pub fn check_price_triggers_range(
        symbol: &str,
        prev_price: f32,
        low: f32,
        high: f32,
        current_price: f32,
    ) -> Vec<PriceAlert> {
        let has_trigger = {
            let alerts = read_alerts();
            if alerts.is_empty() {
                return Vec::new();
            }
            alerts.iter().any(|alert| {
                if alert.status != AlertStatus::Active {
                    return false;
                }
                let matches_symbol = alert.ticker_symbol.eq_ignore_ascii_case(symbol)
                    || alert
                        .ticker
                        .as_ref()
                        .map(|t| {
                            t.as_str().eq_ignore_ascii_case(symbol)
                                || t.display_symbol_and_type().0.eq_ignore_ascii_case(symbol)
                        })
                        .unwrap_or(false);
                matches_symbol && alert.check_range_trigger(prev_price, low, high, current_price)
            })
        };

        if !has_trigger {
            return Vec::new();
        }

        let mut triggered = Vec::new();
        let snapshot_to_save = {
            let mut alerts = write_alerts();
            for alert in alerts.iter_mut() {
                let matches_symbol = alert.ticker_symbol.eq_ignore_ascii_case(symbol)
                    || alert
                        .ticker
                        .as_ref()
                        .map(|t| {
                            t.as_str().eq_ignore_ascii_case(symbol)
                                || t.display_symbol_and_type().0.eq_ignore_ascii_case(symbol)
                        })
                        .unwrap_or(false);

                if matches_symbol
                    && alert.status == AlertStatus::Active
                    && alert.check_range_trigger(prev_price, low, high, current_price)
                {
                    alert.trigger(None);
                    triggered.push(alert.clone());
                }
            }
            if !triggered.is_empty() {
                Some(alerts.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot_to_save {
            spawn_save_alerts(snapshot);
        }
        triggered
    }

    /// Check historical klines during offline catch-up.
    /// Returns any alerts that transitioned to Triggered.
    pub fn check_offline_klines(
        ticker: &exchange::Ticker,
        klines: &[exchange::Kline],
    ) -> Vec<PriceAlert> {
        let mut triggered = Vec::new();
        let snapshot_to_save = {
            let mut alerts = write_alerts();
            let now = chrono::Utc::now().timestamp_millis() as u64;
            for alert in alerts.iter_mut() {
                if alert.status == AlertStatus::Active {
                    let matches = if let Some(ref t) = alert.ticker {
                        t == ticker
                    } else {
                        alert
                            .ticker_symbol
                            .eq_ignore_ascii_case(&ticker.to_full_symbol_and_type().0)
                            || alert
                                .ticker_symbol
                                .eq_ignore_ascii_case(&ticker.display_symbol_and_type().0)
                    };

                    if matches {
                        let filter_time = alert
                            .last_checked_time
                            .unwrap_or(alert.created_at)
                            .saturating_sub(3_600_000); // 1h buffer for the candle start timestamp
                        for kline in klines {
                            if kline.time >= filter_time && alert.check_kline_extremes(kline) {
                                alert.trigger(Some(kline.time));
                                triggered.push(alert.clone());
                                break;
                            }
                        }
                        alert.last_checked_time = Some(now);
                    }
                }
            }
            if !triggered.is_empty() {
                Some(alerts.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot_to_save {
            spawn_save_alerts(snapshot);
        }
        triggered
    }

    /// Save current alerts to disk asynchronously
    pub fn save() {
        let snapshot = read_alerts().clone();
        spawn_save_alerts(snapshot);
    }

    /// Force synchronous save of all alerts to disk
    pub fn save_sync() -> std::io::Result<()> {
        let snapshot = read_alerts().clone();
        crate::save_alerts(&snapshot)
    }

    /// Reload alerts from disk
    pub fn reload() {
        let loaded = crate::load_alerts();
        let mut alerts = write_alerts();
        *alerts = loaded;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_triggers_cross_above() {
        let alert = PriceAlert::new("BTCUSDT", 50000.0, AlertCondition::CrossAbove);
        assert!(!alert.check_trigger(49000.0, 49999.0));
        assert!(alert.check_trigger(49999.0, 50000.0));
        assert!(alert.check_trigger(49000.0, 50500.0));
        assert!(!alert.check_trigger(50500.0, 50000.0)); // downward crossing does not trigger CrossAbove
    }

    #[test]
    fn test_alert_triggers_cross_below() {
        let alert = PriceAlert::new("BTCUSDT", 50000.0, AlertCondition::CrossBelow);
        assert!(!alert.check_trigger(51000.0, 50001.0));
        assert!(alert.check_trigger(50001.0, 50000.0));
        assert!(alert.check_trigger(51000.0, 49500.0));
        assert!(!alert.check_trigger(49500.0, 50000.0)); // upward crossing does not trigger CrossBelow
    }

    #[test]
    fn test_alert_crossing_both_directions() {
        let alert = PriceAlert::new("BTCUSDT", 50000.0, AlertCondition::Crossing);
        assert!(alert.check_trigger(49900.0, 50100.0));
        assert!(alert.check_trigger(50100.0, 49900.0));
        assert!(!alert.check_trigger(50100.0, 50200.0));
    }

    #[test]
    fn test_alert_crossing_kline_extremes_cross_above() {
        let mut alert = PriceAlert::with_details(
            None,
            "BTCUSDT",
            55000.0,
            Some(50000.0),
            AlertCondition::CrossAbove,
        );
        // Kline where price spiked to 56000 and closed back down at 52000 (crossed and returned)
        let kline = exchange::Kline {
            time: 1700000000000,
            open: exchange::util::Price::from_f32(51000.0),
            high: exchange::util::Price::from_f32(56000.0),
            low: exchange::util::Price::from_f32(50500.0),
            close: exchange::util::Price::from_f32(52000.0),
            volume: (10.0, 10.0),
        };
        assert!(alert.check_kline_extremes(&kline));
        alert.trigger(Some(kline.time));
        assert_eq!(alert.status, AlertStatus::Triggered);
        assert_eq!(alert.triggered_at, Some(1700000000000));
    }

    #[test]
    fn test_alert_crossing_kline_extremes_cross_below() {
        let alert = PriceAlert::with_details(
            None,
            "BTCUSDT",
            50000.0,
            Some(55000.0),
            AlertCondition::CrossBelow,
        );
        // Kline where price dipped to 49000 and bounced back up to 53000
        let kline = exchange::Kline {
            time: 1700000000000,
            open: exchange::util::Price::from_f32(54000.0),
            high: exchange::util::Price::from_f32(54500.0),
            low: exchange::util::Price::from_f32(49000.0),
            close: exchange::util::Price::from_f32(53000.0),
            volume: (10.0, 10.0),
        };
        assert!(alert.check_kline_extremes(&kline));
    }

    #[test]
    fn test_alert_crossing_kline_extremes_no_trigger() {
        let alert = PriceAlert::with_details(
            None,
            "BTCUSDT",
            55000.0,
            Some(50000.0),
            AlertCondition::CrossAbove,
        );
        // Kline between 51000 and 54000 (did not reach 55000)
        let kline = exchange::Kline {
            time: 1700000000000,
            open: exchange::util::Price::from_f32(51000.0),
            high: exchange::util::Price::from_f32(54000.0),
            low: exchange::util::Price::from_f32(50500.0),
            close: exchange::util::Price::from_f32(53000.0),
            volume: (10.0, 10.0),
        };
        assert!(!alert.check_kline_extremes(&kline));
    }

    #[test]
    fn test_price_alert_serde_roundtrip() {
        let alert = PriceAlert::with_details(
            Some(exchange::Ticker::new(
                "BTCUSDT",
                exchange::adapter::Exchange::BinanceLinear,
            )),
            "BTCUSDT",
            55000.0,
            Some(50000.0),
            AlertCondition::CrossAbove,
        );
        let serialized = serde_json::to_string(&alert).unwrap();
        let deserialized: PriceAlert = serde_json::from_str(&serialized).unwrap();
        assert_eq!(alert, deserialized);
    }

    #[test]
    fn test_alert_store_operations() {
        let alert = PriceAlert::with_details(
            None,
            "TEST_SYMBOL",
            100.0,
            Some(90.0),
            AlertCondition::CrossAbove,
        );
        let id = alert.id;
        AlertStore::add(alert);

        let for_sym = AlertStore::for_symbol("TEST_SYMBOL");
        assert!(for_sym.iter().any(|a| a.id == id));

        AlertStore::update_price(id, 105.0);
        let updated = AlertStore::for_symbol("TEST_SYMBOL");
        assert_eq!(
            updated.iter().find(|a| a.id == id).unwrap().target_price,
            105.0
        );

        AlertStore::toggle(id);
        let muted = AlertStore::for_symbol("TEST_SYMBOL");
        assert_eq!(
            muted.iter().find(|a| a.id == id).unwrap().status,
            AlertStatus::Muted
        );

        AlertStore::toggle(id); // active again

        // Trigger the alert
        let triggered = AlertStore::check_price_triggers("TEST_SYMBOL", 104.0, 106.0);
        assert!(triggered.iter().any(|a| a.id == id));

        // Clean up
        AlertStore::remove(id);
        assert!(
            !AlertStore::for_symbol("TEST_SYMBOL")
                .iter()
                .any(|a| a.id == id)
        );
    }

    #[test]
    fn test_alert_pane_isolation() {
        let sym = "TEST_PANE_SYM";
        let pane_1 = uuid::Uuid::new_v4();
        let pane_2 = uuid::Uuid::new_v4();

        // 1. Alert created on pane 1
        let a1 = PriceAlert::with_pane(
            None,
            sym,
            50000.0,
            Some(49000.0),
            AlertCondition::CrossAbove,
            Some(pane_1),
        );
        let id1 = a1.id;
        AlertStore::add(a1);

        // Pane 1 can see it
        let for_p1 = AlertStore::for_symbol_and_pane(sym, pane_1);
        assert_eq!(for_p1.len(), 1);
        assert_eq!(for_p1[0].id, id1);

        // Pane 2 cannot see it
        let for_p2 = AlertStore::for_symbol_and_pane(sym, pane_2);
        assert!(for_p2.is_empty());

        // 2. Alert created on pane 2
        let a2 = PriceAlert::with_pane(
            None,
            sym,
            52000.0,
            Some(51000.0),
            AlertCondition::CrossBelow,
            Some(pane_2),
        );
        let id2 = a2.id;
        AlertStore::add(a2);

        // Pane 1 still only sees a1
        let for_p1_after = AlertStore::for_symbol_and_pane(sym, pane_1);
        assert_eq!(for_p1_after.len(), 1);
        assert_eq!(for_p1_after[0].id, id1);

        // Pane 2 now sees a2
        let for_p2_after = AlertStore::for_symbol_and_pane(sym, pane_2);
        assert_eq!(for_p2_after.len(), 1);
        assert_eq!(for_p2_after[0].id, id2);

        // Global list sees both
        let all_sym = AlertStore::for_symbol(sym);
        assert_eq!(all_sym.len(), 2);

        // Clean up
        AlertStore::remove(id1);
        AlertStore::remove(id2);
        assert!(AlertStore::for_symbol(sym).is_empty());
    }

    #[test]
    fn test_alert_wick_retracement_triggers() {
        // Price was at 81000, dropped to 79800 (wick), and returned to 80500.
        // Target is 79904.63.
        let alert_cross_below = PriceAlert::new("BTCUSDT", 79904.63, AlertCondition::CrossBelow);
        assert!(alert_cross_below.check_range_trigger(81000.0, 79800.0, 81100.0, 80500.0));

        let alert_crossing = PriceAlert::new("BTCUSDT", 79904.63, AlertCondition::Crossing);
        assert!(alert_crossing.check_range_trigger(81000.0, 79800.0, 81100.0, 80500.0));

        // Upward wick test: price at 79000, wicked to 80500, closed back down at 79200
        let alert_cross_above = PriceAlert::new("BTCUSDT", 79904.63, AlertCondition::CrossAbove);
        assert!(alert_cross_above.check_range_trigger(79000.0, 78900.0, 80500.0, 79200.0));
    }

    #[test]
    fn test_alert_rearm_clears_triggered_at() {
        let mut alert = PriceAlert::new("BTCUSDT", 80000.0, AlertCondition::Crossing);
        alert.trigger(Some(1700000000000));
        assert_eq!(alert.status, AlertStatus::Triggered);
        assert_eq!(alert.triggered_at, Some(1700000000000));

        let id = alert.id;
        AlertStore::add(alert);

        // Toggle to re-arm
        AlertStore::toggle(id);
        let rearmed = AlertStore::for_symbol("BTCUSDT");
        let a = rearmed.iter().find(|x| x.id == id).unwrap();
        assert_eq!(a.status, AlertStatus::Active);
        assert_eq!(a.triggered_at, None);

        // Retrigger
        let tr = AlertStore::check_price_triggers("BTCUSDT", 80100.0, 79900.0);
        assert!(tr.iter().any(|x| x.id == id));

        // Moving price preserves status
        AlertStore::update_price(id, 82000.0);
        let moved = AlertStore::for_symbol("BTCUSDT");
        let a_moved = moved.iter().find(|x| x.id == id).unwrap();
        assert_eq!(a_moved.status, AlertStatus::Triggered);
        assert_eq!(a_moved.target_price, 82000.0);

        // Toggle re-arms to Active
        AlertStore::toggle(id);
        let toggled = AlertStore::for_symbol("BTCUSDT");
        let a_toggled = toggled.iter().find(|x| x.id == id).unwrap();
        assert_eq!(a_toggled.status, AlertStatus::Active);
        assert_eq!(a_toggled.triggered_at, None);

        AlertStore::remove(id);
    }

    #[test]
    fn test_dat06_alert_store_lock_poisoning_resilience() {
        use std::panic;

        let _ = panic::catch_unwind(|| {
            let _guard = GLOBAL_ALERTS.write().unwrap_or_else(|p| p.into_inner());
            panic!("simulated panic while holding GLOBAL_ALERTS write lock");
        });

        assert!(GLOBAL_ALERTS.is_poisoned());

        let alert = PriceAlert::new("POISON_TEST_ALERT", 50000.0, AlertCondition::CrossAbove);
        let id = alert.id;

        AlertStore::add(alert);
        let alerts = AlertStore::for_symbol("POISON_TEST_ALERT");
        assert!(alerts.iter().any(|item| item.id == id));

        AlertStore::remove(id);
        assert!(
            !AlertStore::for_symbol("POISON_TEST_ALERT")
                .iter()
                .any(|item| item.id == id)
        );
    }
}
