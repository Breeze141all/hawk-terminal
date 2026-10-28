pub mod aggr;
pub mod audio;
pub mod chart;
pub mod config;
pub mod journal;
pub mod layout;
pub mod log;
pub mod metadata;
pub mod panel;
pub mod tickers_table;
pub mod util;

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub use metadata::{MarketMetadata, MetadataSource};

pub use audio::AudioStream;
#[cfg(feature = "volume-search")]
pub use chart::volume_search;
pub use config::ScaleFactor;
pub use config::bundle::{
    BundleMetadata, BundlePayload, BundleValidationError, ConfigBundle, ExportType, WorkspaceBundle,
};
pub use config::network::{Network, TradeFetchMode};
pub use config::sidebar::{self, Sidebar};
pub use config::state::{
    DEFAULT_HAWK_STATE_JSON, Layouts, State, default_hawk_layout, default_state,
};
pub use config::theme::{
    Theme, breez_theme, deeptrades_theme, default_theme, flowsurface_legacy_theme,
};
pub use config::timezone::{TimeLabelKind, UserTimezone};
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

pub fn user_downloads_dir() -> PathBuf {
    dirs_next::download_dir()
        .or_else(|| dirs_next::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn save_layout_to_downloads(json: &str, layout_name: &str) -> std::io::Result<PathBuf> {
    let downloads = user_downloads_dir();
    if !downloads.exists() {
        let _ = std::fs::create_dir_all(&downloads);
    }

    let safe_name: String = layout_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let safe_name = safe_name.trim();
    let base_name = if safe_name.is_empty() {
        "layout"
    } else {
        safe_name
    };

    let mut file_path = downloads.join(format!("hawk_layout_{base_name}.json"));
    if file_path.exists() {
        let mut counter = 1;
        loop {
            let candidate = downloads.join(format!("hawk_layout_{base_name} ({counter}).json"));
            if !candidate.exists() {
                file_path = candidate;
                break;
            }
            counter += 1;
        }
    }

    let mut file = File::create(&file_path)?;
    file.write_all(json.as_bytes())?;
    Ok(file_path)
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
pub use chart::drawing::{DrawingRecord, DrawingStore, DrawingTemplate, DrawingTemplateStore};

pub const DRAWINGS_PATH: &str = "drawings.json";
pub const DRAWING_TEMPLATES_PATH: &str = "drawing-templates.json";

pub fn save_drawing_templates(
    templates: &[chart::drawing::DrawingTemplate],
) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(templates)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_json_to_file(&json, DRAWING_TEMPLATES_PATH)
}

pub fn load_drawing_templates() -> Vec<chart::drawing::DrawingTemplate> {
    load_json_list(&data_path(Some(DRAWING_TEMPLATES_PATH)))
}

pub fn save_drawings(drawings: &[chart::drawing::DrawingRecord]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(drawings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_json_to_file(&json, DRAWINGS_PATH)
}

pub fn load_drawings() -> Vec<chart::drawing::DrawingRecord> {
    load_json_list(&data_path(Some(DRAWINGS_PATH)))
}

pub fn save_alerts(alerts: &[chart::alert::PriceAlert]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(alerts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_json_to_file(&json, ALERTS_PATH)
}

pub fn load_alerts() -> Vec<chart::alert::PriceAlert> {
    let mut alerts: Vec<chart::alert::PriceAlert> = load_json_list(&data_path(Some(ALERTS_PATH)));
    for alert in &mut alerts {
        if alert.status == chart::alert::AlertStatus::Active {
            alert.triggered_at = None;
        }
    }
    alerts
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum InternalError {
    #[error("Fetch error: {0}")]
    Fetch(String),
    #[error("Layout error: {0}")]
    Layout(String),
}

/// Loads a JSON array from `path`. A missing file yields an empty list. A file that exists but
/// cannot be parsed is moved aside (never silently discarded) so the next save cannot overwrite
/// the user's data.
fn load_json_list<T: serde::de::DeserializeOwned>(path: &Path) -> Vec<T> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!("Failed to read '{}': {}", path.display(), e);
            return Vec::new();
        }
    };

    match serde_json::from_str(&contents) {
        Ok(items) => items,
        Err(e) => {
            warn!("Failed to parse '{}': {}", path.display(), e);
            quarantine_corrupt_file(path);
            Vec::new()
        }
    }
}

/// Renames an unreadable data file to `<stem>_corrupt_<unix-secs>.<ext>` next to the original.
fn quarantine_corrupt_file(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let stem = path.file_stem()?.to_string_lossy();
    let backup_name = match path.extension() {
        Some(ext) => format!("{stem}_corrupt_{secs}.{}", ext.to_string_lossy()),
        None => format!("{stem}_corrupt_{secs}"),
    };
    let backup_path = path.with_file_name(backup_name);

    match std::fs::rename(path, &backup_path) {
        Ok(()) => {
            info!(
                "Backed up corrupted file to '{}'. It can be restored manually.",
                backup_path.display()
            );
            Some(backup_path)
        }
        Err(e) => {
            warn!(
                "Failed to back up corrupted file '{}' to '{}': {}",
                path.display(),
                backup_path.display(),
                e
            );
            None
        }
    }
}

pub fn write_json_to_file(json: &str, file_name: &str) -> std::io::Result<()> {
    write_file_atomic(&data_path(Some(file_name)), json.as_bytes())
}

/// Writes via a temp file in the same directory and renames it into place. The temp file is
/// fsynced first so a crash or power loss cannot leave an empty or truncated target behind.
fn write_file_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid state file path")
    })?;

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut file = File::create(&tmp_path)?;
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
    }

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp_path, path).map_err(|_| e)?;
        } else {
            return Err(e);
        }
    }
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

static MIGRATION_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ensure_migration_from_legacy(hawk_dir: &Path, flowsurface_dir: &Path) {
    if MIGRATION_ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if !flowsurface_dir.exists() || !flowsurface_dir.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(hawk_dir);

    let critical_files = [
        "saved-state.json",
        "drawings.json",
        "alerts.json",
        "drawing-templates.json",
        "journal.json",
        "metadata-cache.json",
    ];

    for file_name in critical_files {
        let src = flowsurface_dir.join(file_name);
        let dst = hawk_dir.join(file_name);
        if src.exists() && src.is_file() {
            let should_copy = match std::fs::metadata(&dst) {
                Ok(m) => m.len() <= 2,
                Err(_) => true,
            };
            if should_copy {
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }

    let src_journal = flowsurface_dir.join("journal_images");
    let dst_journal = hawk_dir.join("journal_images");
    if src_journal.exists() && !dst_journal.exists() {
        let _ = copy_dir_all_recursive(&src_journal, &dst_journal);
    }
}

fn copy_dir_all_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub fn data_path(path_name: Option<&str>) -> PathBuf {
    if let Ok(path) =
        std::env::var("HAWK_DATA_PATH").or_else(|_| std::env::var("FLOWSURFACE_DATA_PATH"))
    {
        PathBuf::from(path)
    } else if let Some(test_dir) = cargo_test_data_dir() {
        match path_name {
            Some(path_name) => test_dir.join(path_name),
            None => test_dir,
        }
    } else {
        let data_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let hawk_dir = data_dir.join("hawk-terminal");
        let flowsurface_dir = data_dir.join("flowsurface");

        ensure_migration_from_legacy(&hawk_dir, &flowsurface_dir);

        let base_dir = if hawk_dir.exists() {
            hawk_dir
        } else if flowsurface_dir.exists() {
            flowsurface_dir
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

/// A per-process temp dir when this is a `cargo test` binary (it runs from `target/*/deps/`),
/// so tests that touch the global drawing/alert/template stores never read or write the
/// user's real data dir. The app itself (`cargo run`, installed builds) never runs from `deps/`.
fn cargo_test_data_dir() -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        if exe.parent()?.file_name()? != "deps" {
            return None;
        }
        let dir = std::env::temp_dir().join(format!("hawk-terminal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    })
    .clone()
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

    let max_days = exchange::fetcher::trade_history_lookback_days() as i64;

    for entry in entries.filter_map(Result::ok) {
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

/// Wall-clock budget for a performance test. Shared CI runners are ~3x slower than a
/// dev machine, so the budget is widened there (`CI` is set by GitHub Actions); locally
/// it stays strict. Still catches complexity regressions, which cost far more than 4x.
#[cfg(test)]
pub(crate) fn perf_budget_ms(local_ms: u128) -> u128 {
    if std::env::var_os("CI").is_some() {
        local_ms * 4
    } else {
        local_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_path_stays_out_of_the_user_dir_under_cargo_test() {
        if std::env::var_os("HAWK_DATA_PATH").is_some()
            || std::env::var_os("FLOWSURFACE_DATA_PATH").is_some()
        {
            return; // an explicit override wins, by design
        }
        // Tests used to load and save the user's real drawings.json/alerts.json.
        let path = data_path(Some(DRAWINGS_PATH));
        assert!(path.starts_with(std::env::temp_dir()), "{}", path.display());
    }

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

    #[test]
    fn test_save_layout_to_downloads() {
        let dummy_json = r#"{"test": true}"#;
        let layout_name = "Test/Special:Layout*";
        let saved =
            save_layout_to_downloads(dummy_json, layout_name).expect("Must save to downloads");
        assert!(saved.exists());
        let file_name = saved.file_name().unwrap().to_string_lossy();
        assert!(file_name.starts_with("hawk_layout_Test_Special_Layout_"));
        assert!(file_name.ends_with(".json"));

        let content = std::fs::read_to_string(&saved).unwrap();
        assert_eq!(content, dummy_json);

        let _ = std::fs::remove_file(saved);
    }

    #[test]
    fn test_write_json_to_file_atomic() {
        let test_file = format!("test_atomic_{}.json", uuid::Uuid::new_v4());
        let data1 = r#"{"version": 1}"#;
        let data2 = r#"{"version": 2}"#;

        assert!(write_json_to_file(data1, &test_file).is_ok());
        let read1 = std::fs::read_to_string(data_path(Some(&test_file))).unwrap();
        assert_eq!(read1, data1);

        assert!(write_json_to_file(data2, &test_file).is_ok());
        let read2 = std::fs::read_to_string(data_path(Some(&test_file))).unwrap();
        assert_eq!(read2, data2);

        let _ = std::fs::remove_file(data_path(Some(&test_file)));
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Item {
        id: u32,
    }

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hawk-data-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn corrupt_backups(dir: &Path, prefix: &str) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(prefix) && n.contains("_corrupt_"))
            })
            .collect()
    }

    #[test]
    fn test_load_json_list_missing_file_is_empty_without_side_effects() {
        let dir = scratch_dir();
        let items: Vec<Item> = load_json_list(&dir.join("drawings.json"));
        assert!(items.is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_load_json_list_reads_valid_file_and_keeps_it() {
        let dir = scratch_dir();
        let path = dir.join("drawings.json");
        std::fs::write(&path, r#"[{"id":1},{"id":2}]"#).unwrap();

        let items: Vec<Item> = load_json_list(&path);
        assert_eq!(items, vec![Item { id: 1 }, Item { id: 2 }]);
        assert!(path.exists());
        assert!(corrupt_backups(&dir, "drawings").is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_load_json_list_corrupt_file_is_quarantined_not_lost() {
        let dir = scratch_dir();
        let path = dir.join("drawings.json");
        let corrupt = r#"[{"id":1},{"id":"#;
        std::fs::write(&path, corrupt).unwrap();

        let items: Vec<Item> = load_json_list(&path);
        assert!(items.is_empty());
        assert!(!path.exists(), "corrupt file must be moved away");

        let backups = corrupt_backups(&dir, "drawings");
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), corrupt);
        assert_eq!(backups[0].extension().unwrap(), "json");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_save_after_corrupt_load_cannot_destroy_original_data() {
        let dir = scratch_dir();
        let path = dir.join("alerts.json");
        std::fs::write(&path, "not json at all").unwrap();

        let items: Vec<Item> = load_json_list(&path);
        assert!(items.is_empty());
        write_file_atomic(&path, b"[]").unwrap();

        let backups = corrupt_backups(&dir, "alerts");
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&backups[0]).unwrap(),
            "not json at all"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_quarantine_handles_names_without_extension() {
        let dir = scratch_dir();
        let path = dir.join("state");
        std::fs::write(&path, "x").unwrap();

        let backup = quarantine_corrupt_file(&path).unwrap();
        assert!(!path.exists());
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("state_corrupt_")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_write_file_atomic_creates_dirs_replaces_and_leaves_no_temp_files() {
        let dir = scratch_dir();
        let path = dir.join("nested").join("saved.json");

        write_file_atomic(&path, b"first").unwrap();
        write_file_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "temp file must not remain after rename");
        std::fs::remove_dir_all(dir).ok();
    }
}
