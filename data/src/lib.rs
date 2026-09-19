pub mod aggr;
pub mod audio;
pub mod chart;
pub mod config;
pub mod journal;
pub mod layout;
pub mod log;
pub mod panel;
pub mod tickers_table;
pub mod util;

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

pub use audio::AudioStream;
pub use config::ScaleFactor;
pub use config::bundle::{
    BundleMetadata, BundlePayload, BundleValidationError, ConfigBundle, ExportType, WorkspaceBundle,
};
pub use config::sidebar::{self, Sidebar};
pub use config::state::{
    DEFAULT_HAWK_STATE_JSON, Layouts, State, default_hawk_layout, default_state,
};
pub use config::theme::Theme;
pub use config::timezone::UserTimezone;
pub use journal::{
    JOURNAL_IMAGES_DIR, JournalEntry, JournalMode, JournalStats, PositionAutofill, TradeSide,
    TradeStatus, delete_journal_image_files, journal_image_path, journal_images_dir,
    journal_thumb_path, load_journal, save_journal,
};

use ::log::{error, info, warn};
pub use layout::{Dashboard, Layout, Pane};

pub const SAVED_STATE_PATH: &str = "saved-state.json";
pub const ALERTS_PATH: &str = "alerts.json";
pub const EXPORTS_DIR: &str = "exports";

pub fn exports_path(file_name: Option<&str>) -> PathBuf {
    let base = data_path(Some(EXPORTS_DIR));
    if let Some(file_name) = file_name {
        base.join(file_name)
    } else {
        base
    }
}

pub fn save_export_file(json: &str, file_name: &str) -> std::io::Result<PathBuf> {
    let path = exports_path(Some(file_name));
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(path)
}

pub use chart::alert::AlertStore;

pub fn save_alerts(alerts: &[chart::alert::PriceAlert]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(alerts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_json_to_file(&json, ALERTS_PATH)
}

pub fn load_alerts() -> Vec<chart::alert::PriceAlert> {
    let path = data_path(Some(ALERTS_PATH));
    let Ok(mut file) = File::open(&path) else {
        return Vec::new();
    };
    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() {
        return Vec::new();
    }
    serde_json::from_str(&contents).unwrap_or_default()
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum InternalError {
    #[error("Fetch error: {0}")]
    Fetch(String),
    #[error("Layout error: {0}")]
    Layout(String),
}

pub fn write_json_to_file(json: &str, file_name: &str) -> std::io::Result<()> {
    let path = data_path(Some(file_name));

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid state file path")
    })?;

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn read_from_file(file_name: &str) -> Result<State, Box<dyn std::error::Error>> {
    let path = data_path(Some(file_name));

    let file_open_result = File::open(&path);
    let mut file = match file_open_result {
        Ok(file) => file,
        Err(e) => return Err(Box::new(e)),
    };

    let mut contents = String::new();
    if let Err(e) = file.read_to_string(&mut contents) {
        return Err(Box::new(e));
    }

    match serde_json::from_str(&contents) {
        Ok(state) => Ok(state),
        Err(e) => {
            // If parsing fails, backup the file
            drop(file); // Close the file before renaming

            // Create backup file with different name to prevent overwriting it
            let backup_file_name = if let Some(pos) = file_name.rfind('.') {
                format!("{}_old{}", &file_name[..pos], &file_name[pos..])
            } else {
                format!("{}_old", file_name)
            };

            let backup_path = data_path(Some(&backup_file_name));

            if let Err(rename_err) = std::fs::rename(&path, &backup_path) {
                warn!(
                    "Failed to backup corrupted state file '{}' to '{}': {}",
                    path.display(),
                    backup_path.display(),
                    rename_err
                );
            } else {
                info!(
                    "Backed up corrupted state file to '{}'. It can be restored manually.",
                    backup_path.display()
                );
            }

            Err(Box::new(e))
        }
    }
}

pub fn open_data_folder() -> Result<(), InternalError> {
    let pathbuf = data_path(None);

    if pathbuf.exists() {
        if let Err(err) = open::that(&pathbuf) {
            Err(InternalError::Layout(format!(
                "Failed to open data folder: {:?}, error: {}",
                pathbuf, err
            )))
        } else {
            info!("Opened data folder: {:?}", pathbuf);
            Ok(())
        }
    } else {
        Err(InternalError::Layout(format!(
            "Data folder does not exist: {:?}",
            pathbuf
        )))
    }
}

pub fn open_exports_folder() -> Result<(), InternalError> {
    let pathbuf = exports_path(None);
    if !pathbuf.exists() {
        let _ = std::fs::create_dir_all(&pathbuf);
    }

    if let Err(err) = open::that(&pathbuf) {
        Err(InternalError::Layout(format!(
            "Failed to open exports folder: {:?}, error: {}",
            pathbuf, err
        )))
    } else {
        info!("Opened exports folder: {:?}", pathbuf);
        Ok(())
    }
}

pub fn data_path(path_name: Option<&str>) -> PathBuf {
    if let Ok(path) =
        std::env::var("HAWK_DATA_PATH").or_else(|_| std::env::var("FLOWSURFACE_DATA_PATH"))
    {
        PathBuf::from(path)
    } else {
        let data_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let hawk_dir = data_dir.join("hawk-terminal");
        let base_dir = if hawk_dir.exists() {
            hawk_dir
        } else if data_dir.join("flowsurface").exists() {
            data_dir.join("flowsurface")
        } else {
            hawk_dir
        };
        if let Some(path_name) = path_name {
            base_dir.join(path_name)
        } else {
            base_dir
        }
    }
}

fn cleanup_directory(data_path: &PathBuf) -> usize {
    if !data_path.exists() {
        warn!("Data path {:?} does not exist, skipping cleanup", data_path);
        return 0;
    }

    let re = regex::Regex::new(r".*-(\d{4}-\d{2}-\d{2})\.(?:zip|bin)$")
        .expect("Cleanup regex pattern is valid");
    let today = chrono::Local::now().date_naive();
    let mut deleted_files = Vec::new();

    let entries = match std::fs::read_dir(data_path) {
        Ok(entries) => entries,
        Err(e) => {
            error!("Failed to read data directory {:?}: {}", data_path, e);
            return 0;
        }
    };

    for entry in entries.filter_map(Result::ok) {
        let symbol_name = entry.file_name().to_string_lossy().to_uppercase();
        let max_days = if symbol_name.starts_with("BTC") {
            130
        } else {
            30
        };

        let symbol_dir = match std::fs::read_dir(entry.path()) {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to read symbol directory {:?}: {}", entry.path(), e);
                continue;
            }
        };

        for file in symbol_dir.filter_map(Result::ok) {
            let path = file.path();
            let Some(filename) = path.to_str() else {
                continue;
            };

            if let Some(cap) = re.captures(filename)
                && let Ok(file_date) = chrono::NaiveDate::parse_from_str(&cap[1], "%Y-%m-%d")
            {
                let days_old = today.signed_duration_since(file_date).num_days();
                if days_old > max_days {
                    if let Err(e) = std::fs::remove_file(&path) {
                        error!("Failed to remove old file {}: {}", filename, e);
                    } else {
                        deleted_files.push(filename.to_string());
                        info!("Removed old file: {}", filename);
                    }
                }
            }
        }
    }

    deleted_files.len()
}

pub fn cleanup_old_market_data() -> usize {
    let paths = ["um", "cm"].map(|market_type| {
        data_path(Some(&format!(
            "market_data/binance/data/futures/{}/daily/aggTrades",
            market_type
        )))
    });

    let total_deleted: usize = paths.iter().map(cleanup_directory).sum();

    info!("File cleanup completed. Deleted {} files", total_deleted);
    total_deleted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_directory_recognizes_bin_and_zip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_cleanup_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let btc_dir = temp_dir.join("BTCUSDT");
        let eth_dir = temp_dir.join("ETHUSDT");
        std::fs::create_dir_all(&btc_dir).unwrap();
        std::fs::create_dir_all(&eth_dir).unwrap();

        let old_date = "2020-01-01";
        let recent_date = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();

        let btc_old_zip = btc_dir.join(format!("BTCUSDT-aggTrades-{old_date}.zip"));
        let btc_old_bin = btc_dir.join(format!("BTCUSDT-aggTrades-{old_date}.bin"));
        let btc_recent_bin = btc_dir.join(format!("BTCUSDT-aggTrades-{recent_date}.bin"));
        std::fs::write(&btc_old_zip, b"test").unwrap();
        std::fs::write(&btc_old_bin, b"test").unwrap();
        std::fs::write(&btc_recent_bin, b"test").unwrap();

        let eth_old_zip = eth_dir.join(format!("ETHUSDT-aggTrades-{old_date}.zip"));
        let eth_old_bin = eth_dir.join(format!("ETHUSDT-aggTrades-{old_date}.bin"));
        let eth_recent_bin = eth_dir.join(format!("ETHUSDT-aggTrades-{recent_date}.bin"));
        std::fs::write(&eth_old_zip, b"test").unwrap();
        std::fs::write(&eth_old_bin, b"test").unwrap();
        std::fs::write(&eth_recent_bin, b"test").unwrap();

        let deleted = cleanup_directory(&temp_dir);
        assert_eq!(deleted, 4);

        assert!(!btc_old_zip.exists());
        assert!(!btc_old_bin.exists());
        assert!(btc_recent_bin.exists());

        assert!(!eth_old_zip.exists());
        assert!(!eth_old_bin.exists());
        assert!(eth_recent_bin.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
