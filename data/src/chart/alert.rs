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
        }
    }

    pub fn trigger(&mut self, timestamp: Option<u64>) {
        self.status = AlertStatus::Triggered;
        self.triggered_at =
            timestamp.or_else(|| Some(chrono::Utc::now().timestamp_millis() as u64));
    }

    /// Check whether a price movement from `prev_price` to `current_price` triggers this alert.
    pub fn check_trigger(&self, prev_price: f32, current_price: f32) -> bool {
        if self.status != AlertStatus::Active {
            return false;
        }

        match self.condition {
            AlertCondition::CrossAbove => {
                prev_price < self.target_price && current_price >= self.target_price
            }
            AlertCondition::CrossBelow => {
                prev_price > self.target_price && current_price <= self.target_price
            }
            AlertCondition::Crossing => {
                (prev_price < self.target_price && current_price >= self.target_price)
                    || (prev_price > self.target_price && current_price <= self.target_price)
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

        match self.condition {
            AlertCondition::CrossAbove => {
                if let Some(init) = self.initial_price
                    && init >= self.target_price
                {
                    return false;
                }
                high >= self.target_price
            }
            AlertCondition::CrossBelow => {
                if let Some(init) = self.initial_price
                    && init <= self.target_price
                {
                    return false;
                }
                low <= self.target_price
            }
            AlertCondition::Crossing => {
                if let Some(init) = self.initial_price {
                    if init < self.target_price {
                        high >= self.target_price
                    } else if init > self.target_price {
                        low <= self.target_price
                    } else {
                        low <= self.target_price && high >= self.target_price
                    }
                } else {
                    low <= self.target_price && high >= self.target_price
                }
            }
        }
    }
}

use std::sync::{LazyLock, RwLock};

static GLOBAL_ALERTS: LazyLock<RwLock<Vec<PriceAlert>>> =
    LazyLock::new(|| RwLock::new(crate::load_alerts()));

pub struct AlertStore;

impl AlertStore {
    /// Retrieve a clone of all alerts
    pub fn all() -> Vec<PriceAlert> {
        GLOBAL_ALERTS.read().unwrap().clone()
    }

    /// Retrieve alerts matching a specific ticker symbol
    pub fn for_symbol(symbol: &str) -> Vec<PriceAlert> {
        GLOBAL_ALERTS
            .read()
            .unwrap()
            .iter()
            .filter(|a| a.ticker_symbol == symbol)
            .cloned()
            .collect()
    }

    /// Add a new alert and persist to disk
    pub fn add(alert: PriceAlert) {
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
        alerts.push(alert);
        let _ = crate::save_alerts(&alerts);
    }

    /// Remove an alert by ID and persist to disk
    pub fn remove(id: uuid::Uuid) {
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
        alerts.retain(|a| a.id != id);
        let _ = crate::save_alerts(&alerts);
    }

    /// Toggle alert status between Active and Muted
    pub fn toggle(id: uuid::Uuid) {
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.status = match alert.status {
                AlertStatus::Active => AlertStatus::Muted,
                AlertStatus::Muted | AlertStatus::Triggered => AlertStatus::Active,
            };
        }
        let _ = crate::save_alerts(&alerts);
    }

    /// Update an alert's target price (e.g. from canvas drag)
    pub fn update_price(id: uuid::Uuid, price: f32) {
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.target_price = price;
        }
        let _ = crate::save_alerts(&alerts);
    }

    /// Clear all triggered alerts
    pub fn clear_triggered() {
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
        alerts.retain(|a| a.status != AlertStatus::Triggered);
        let _ = crate::save_alerts(&alerts);
    }

    /// Check live price movement and trigger matching alerts.
    /// Returns any alerts that transitioned to Triggered.
    pub fn check_price_triggers(
        symbol: &str,
        prev_price: f32,
        current_price: f32,
    ) -> Vec<PriceAlert> {
        let mut triggered = Vec::new();
        {
            let mut alerts = GLOBAL_ALERTS.write().unwrap();
            for alert in alerts.iter_mut() {
                if alert.ticker_symbol == symbol
                    && alert.status == AlertStatus::Active
                    && alert.check_trigger(prev_price, current_price)
                {
                    alert.trigger(None);
                    triggered.push(alert.clone());
                }
            }
            if !triggered.is_empty() {
                let _ = crate::save_alerts(&alerts);
            }
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
        {
            let mut alerts = GLOBAL_ALERTS.write().unwrap();
            let now = chrono::Utc::now().timestamp_millis() as u64;
            for alert in alerts.iter_mut() {
                if alert.status == AlertStatus::Active {
                    let matches = if let Some(ref t) = alert.ticker {
                        t == ticker
                    } else {
                        alert.ticker_symbol == ticker.to_full_symbol_and_type().0
                            || alert.ticker_symbol == ticker.display_symbol_and_type().0
                    };

                    if matches {
                        for kline in klines {
                            if alert.check_kline_extremes(kline) {
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
                let _ = crate::save_alerts(&alerts);
            }
        }
        triggered
    }

    /// Save current alerts to disk
    pub fn save() {
        let alerts = GLOBAL_ALERTS.read().unwrap();
        let _ = crate::save_alerts(&alerts);
    }

    /// Reload alerts from disk
    pub fn reload() {
        let loaded = crate::load_alerts();
        let mut alerts = GLOBAL_ALERTS.write().unwrap();
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
}
