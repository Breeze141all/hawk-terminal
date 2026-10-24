use log::{info, warn};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const VERSION_CHECK_URL: &str = "https://breeze141all.github.io/hawk-site/version.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteRelease {
    pub version: String,
    pub release_date: Option<String>,
    pub changelog: Option<Vec<String>>,
    pub download_url: String,
    pub direct_site_url: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(RemoteRelease),
    Downloading {
        release: RemoteRelease,
        progress: f32, // 0.0 .. 1.0
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },
    ReadyToInstall {
        release: RemoteRelease,
        installer_path: PathBuf,
        backup_path: Option<PathBuf>,
    },
    Error(String),
}

/// Parses a version string leniently (e.g. "v0.9.6" or "0.9.6").
pub fn parse_version(raw: &str) -> Option<Version> {
    let clean = raw.trim().trim_start_matches('v').trim_start_matches('V');
    Version::parse(clean).ok()
}

/// Compares remote version with current package version.
/// Returns true if remote is strictly newer than current.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    match (parse_version(current), parse_version(remote)) {
        (Some(curr), Some(rem)) => rem > curr,
        _ => false,
    }
}

/// Asynchronously checks if a newer version is available on the remote site.
pub async fn check_for_updates() -> Result<Option<RemoteRelease>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent(format!("HawkTerminal/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    info!("Checking for updates at {VERSION_CHECK_URL}...");
    let response = client
        .get(VERSION_CHECK_URL)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Server returned HTTP {}", response.status()));
    }

    let release: RemoteRelease = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse version.json: {e}"))?;

    let current_version = env!("CARGO_PKG_VERSION");
    if is_newer_version(current_version, &release.version) {
        info!(
            "Newer version available: v{} (current is v{})",
            release.version, current_version
        );
        Ok(Some(release))
    } else {
        info!("Hawk Terminal is up to date (v{current_version})");
        Ok(None)
    }
}

/// Creates a full backup of all user configuration and state in `%APPDATA%\hawk-terminal\backups\`.
pub fn create_user_data_backup() -> Result<PathBuf, String> {
    let base_dir = data::data_path(None);
    if !base_dir.exists() {
        return Err("Data directory does not exist".to_string());
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let backup_dir_name = format!("backup_v{}_{}", current_version, timestamp);
    let backup_path = data::data_path(Some(&format!("backups/{backup_dir_name}")));

    std::fs::create_dir_all(&backup_path)
        .map_err(|e| format!("Failed to create backup directory: {e}"))?;

    // Copy critical JSON files
    let json_files = [
        "saved-state.json",
        "alerts.json",
        "drawings.json",
        "drawing-templates.json",
    ];

    for file_name in json_files {
        let src = base_dir.join(file_name);
        if src.exists() && src.is_file() {
            let dst = backup_path.join(file_name);
            if let Err(e) = std::fs::copy(&src, &dst) {
                warn!("Failed to backup {file_name}: {e}");
            }
        }
    }

    // Recursively copy journal images if present
    let journal_imgs_src = base_dir.join(data::JOURNAL_IMAGES_DIR);
    if journal_imgs_src.exists() && journal_imgs_src.is_dir() {
        let journal_imgs_dst = backup_path.join(data::JOURNAL_IMAGES_DIR);
        let _ = copy_dir_all(&journal_imgs_src, &journal_imgs_dst);
    }

    info!("Successfully created user data backup at {:?}", backup_path);
    Ok(backup_path)
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Downloads the release installer asynchronously to a temporary directory.
/// Emits progress callback updates.
pub async fn download_installer_file<F>(
    release: RemoteRelease,
    on_progress: F,
    cancel_flag: Arc<AtomicBool>,
) -> Result<PathBuf, String>
where
    F: Fn(f32, u64, Option<u64>) + Send + Sync + 'static,
{
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .user_agent(format!("HawkTerminal/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    // Try primary download url first, fallback to direct site url if present
    let download_url = release.download_url.clone();
    let mut response = match client.get(&download_url).send().await {
        Ok(resp) if resp.status().is_success() => resp,
        first_err => {
            if let Some(ref fallback) = release.direct_site_url {
                warn!(
                    "Primary download URL failed ({:?}), trying direct site URL...",
                    first_err
                );
                client
                    .get(fallback)
                    .send()
                    .await
                    .map_err(|e| format!("Download request failed: {e}"))?
            } else {
                return Err(format!("Download request failed: {:?}", first_err));
            }
        }
    };

    if !response.status().is_success() {
        return Err(format!("Download failed with HTTP {}", response.status()));
    }

    let total_bytes = response.content_length();
    let temp_dir = std::env::temp_dir();
    let file_name = format!("hawk-terminal-setup-v{}.exe", release.version);
    let final_path = temp_dir.join(file_name);
    let tmp_path = temp_dir.join(format!("hawk-setup-v{}.tmp", release.version));

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp installer file: {e}"))?;

    let mut downloaded: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Error downloading chunk: {e}"))?
    {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Download cancelled by user".to_string());
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed writing to installer file: {e}"))?;

        downloaded += chunk.len() as u64;
        let progress = match total_bytes {
            Some(total) if total > 0 => (downloaded as f32 / total as f32).min(1.0),
            _ => 0.0,
        };
        on_progress(progress, downloaded, total_bytes);
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush installer file: {e}"))?;
    drop(file);

    if final_path.exists() {
        let _ = tokio::fs::remove_file(&final_path).await;
    }

    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .map_err(|e| format!("Failed to finalize installer file: {e}"))?;

    info!("Installer saved to {:?}", final_path);
    Ok(final_path)
}

/// Spawns the installer executable and exits the current process.
pub fn execute_installer_and_exit(installer_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        info!("Spawning installer at {:?}", installer_path);
        std::process::Command::new(installer_path)
            .spawn()
            .map_err(|e| format!("Failed to launch installer executable: {e}"))?;

        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = open::that(installer_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing_and_comparison() {
        assert!(is_newer_version("0.9.6", "0.9.7"));
        assert!(is_newer_version("0.9.6", "v0.9.7"));
        assert!(is_newer_version("v0.9.6", "0.10.0"));
        assert!(is_newer_version("0.9.6", "1.0.0"));
        assert!(!is_newer_version("0.9.6", "0.9.6"));
        assert!(!is_newer_version("0.9.7", "0.9.6"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
    }
}
