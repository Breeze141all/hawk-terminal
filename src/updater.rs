use log::{info, warn};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const VERSION_CHECK_URL: &str = "https://breeze141all.github.io/hawk-site/version.json";

/// Upper bound for the installer download; anything larger is rejected as malformed.
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

const ALLOWED_RELEASE_HOST: &str = "github.com";
const ALLOWED_RELEASE_PATH_PREFIX: &str = "/Breeze141all/hawk-terminal/releases/download/";
const ALLOWED_SITE_HOST: &str = "breeze141all.github.io";
const ALLOWED_SITE_PATH_PREFIX: &str = "/hawk-site/";

/// Accepts only HTTPS URLs that point to this project's own GitHub release or site assets.
pub fn validate_download_url(raw: &str) -> Result<(), String> {
    let url = url::Url::parse(raw).map_err(|e| format!("Invalid download URL: {e}"))?;
    if url.scheme() != "https" {
        return Err(format!("Download URL must use https, got {}", url.scheme()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Download URL must not contain credentials".to_string());
    }
    let host = url.host_str().unwrap_or_default();
    let allowed = (host == ALLOWED_RELEASE_HOST
        && url.path().starts_with(ALLOWED_RELEASE_PATH_PREFIX))
        || (host == ALLOWED_SITE_HOST && url.path().starts_with(ALLOWED_SITE_PATH_PREFIX));
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "Download URL is not an official release location: {host}"
        ))
    }
}

/// Parses a SHA-256 hex digest (64 hex chars, case-insensitive) into canonical lowercase form.
pub fn normalize_sha256(raw: &str) -> Option<String> {
    let clean = raw.trim();
    (clean.len() == 64 && clean.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| clean.to_ascii_lowercase())
}

/// Streams the file through SHA-256 and returns the lowercase hex digest.
pub fn sha256_file_hex(path: &Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let digest = ctx.finish();
    Ok(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

/// Verifies the downloaded file against the expected checksum from the release manifest.
pub fn verify_installer_checksum(path: &Path, expected: &str) -> Result<(), String> {
    let expected = normalize_sha256(expected)
        .ok_or_else(|| "Release checksum is not a valid SHA-256 digest".to_string())?;
    let actual =
        sha256_file_hex(path).map_err(|e| format!("Failed to hash downloaded installer: {e}"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(
            "Installer checksum mismatch: the download is corrupted or has been tampered with"
                .to_string(),
        )
    }
}

/// Verifies the temp download and, only on success, moves it to its final location.
/// A failed verification deletes the temp file so an unverified installer never survives.
fn finalize_download(
    tmp_path: &Path,
    final_path: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    if let Err(e) = verify_installer_checksum(tmp_path, expected_sha256) {
        let _ = std::fs::remove_file(tmp_path);
        return Err(e);
    }
    if final_path.exists() {
        let _ = std::fs::remove_file(final_path);
    }
    std::fs::rename(tmp_path, final_path)
        .map_err(|e| format!("Failed to finalize installer file: {e}"))
}

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

    // Refuse to download anything we cannot verify afterwards.
    let expected_sha256 = release
        .sha256
        .as_deref()
        .and_then(normalize_sha256)
        .ok_or_else(|| "Release manifest has no valid sha256; refusing to install".to_string())?;
    validate_download_url(&release.download_url)?;
    if let Some(fallback) = &release.direct_site_url {
        validate_download_url(fallback)?;
    }

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
    if total_bytes.is_some_and(|total| total > MAX_INSTALLER_BYTES) {
        return Err("Installer is unexpectedly large; refusing to download".to_string());
    }
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
        if downloaded > MAX_INSTALLER_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Installer exceeded the maximum allowed size".to_string());
        }
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

    {
        let (tmp, fin) = (tmp_path.clone(), final_path.clone());
        tokio::task::spawn_blocking(move || finalize_download(&tmp, &fin, &expected_sha256))
            .await
            .map_err(|e| format!("Checksum task failed: {e}"))??;
    }

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

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(installer_path)
            .spawn();
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(installer_path)
            .spawn();
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = installer_path;
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

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hawk-updater-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_download_url_accepts_official_locations() {
        assert!(
            validate_download_url(
                "https://github.com/Breeze141all/hawk-terminal/releases/download/v0.9.8/hawk-terminal-x86_64-setup.exe"
            )
            .is_ok()
        );
        assert!(
            validate_download_url(
                "https://breeze141all.github.io/hawk-site/hawk-terminal-setup.exe"
            )
            .is_ok()
        );
    }

    #[test]
    fn test_download_url_rejects_untrusted_locations() {
        for bad in [
            "http://github.com/Breeze141all/hawk-terminal/releases/download/v1/x.exe",
            "https://evil.example.com/hawk.exe",
            "https://github.com.evil.com/Breeze141all/hawk-terminal/releases/download/v1/x.exe",
            "https://github.com@evil.com/Breeze141all/hawk-terminal/releases/download/v1/x.exe",
            "https://user:pw@github.com/Breeze141all/hawk-terminal/releases/download/v1/x.exe",
            "https://github.com/attacker/hawk-terminal/releases/download/v1/x.exe",
            "https://breeze141all.github.io/other-site/x.exe",
            "file:///C:/Windows/System32/calc.exe",
            "not a url",
            "",
        ] {
            assert!(validate_download_url(bad).is_err(), "must reject {bad:?}");
        }
    }

    #[test]
    fn test_normalize_sha256() {
        assert_eq!(normalize_sha256(ABC_SHA256).as_deref(), Some(ABC_SHA256));
        assert_eq!(
            normalize_sha256(&format!("  {}\n", ABC_SHA256.to_uppercase())).as_deref(),
            Some(ABC_SHA256)
        );
        assert_eq!(normalize_sha256(""), None);
        assert_eq!(normalize_sha256("abc"), None);
        assert_eq!(normalize_sha256(&"g".repeat(64)), None);
        assert_eq!(normalize_sha256(&format!("{ABC_SHA256}00")), None);
    }

    #[test]
    fn test_sha256_file_matches_known_vector() {
        let dir = scratch_dir();
        let path = dir.join("abc.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file_hex(&path).unwrap(), ABC_SHA256);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_verify_installer_checksum_rejects_mismatch_and_garbage() {
        let dir = scratch_dir();
        let path = dir.join("setup.exe");
        std::fs::write(&path, b"abc").unwrap();

        assert!(verify_installer_checksum(&path, ABC_SHA256).is_ok());
        assert!(verify_installer_checksum(&path, &ABC_SHA256.to_uppercase()).is_ok());
        assert!(verify_installer_checksum(&path, &"0".repeat(64)).is_err());
        assert!(verify_installer_checksum(&path, "not-a-hash").is_err());
        assert!(verify_installer_checksum(&dir.join("missing.exe"), ABC_SHA256).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_finalize_download_installs_only_verified_file() {
        let dir = scratch_dir();
        let tmp = dir.join("setup.tmp");
        let fin = dir.join("setup.exe");

        std::fs::write(&tmp, b"abc").unwrap();
        finalize_download(&tmp, &fin, ABC_SHA256).unwrap();
        assert!(!tmp.exists());
        assert_eq!(std::fs::read(&fin).unwrap(), b"abc");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_finalize_download_deletes_tampered_file_and_keeps_old_installer() {
        let dir = scratch_dir();
        let tmp = dir.join("setup.tmp");
        let fin = dir.join("setup.exe");
        std::fs::write(&fin, b"previous-good-installer").unwrap();
        std::fs::write(&tmp, b"tampered").unwrap();

        let err = finalize_download(&tmp, &fin, ABC_SHA256).unwrap_err();
        assert!(err.contains("mismatch"));
        assert!(!tmp.exists(), "tampered download must be deleted");
        assert_eq!(std::fs::read(&fin).unwrap(), b"previous-good-installer");
        std::fs::remove_dir_all(dir).ok();
    }

    fn release_with(sha256: Option<&str>, url: &str) -> RemoteRelease {
        RemoteRelease {
            version: "9.9.9".into(),
            release_date: None,
            changelog: None,
            download_url: url.into(),
            direct_site_url: None,
            sha256: sha256.map(String::from),
        }
    }

    #[tokio::test]
    async fn test_download_refuses_release_without_checksum() {
        let release = release_with(
            None,
            "https://github.com/Breeze141all/hawk-terminal/releases/download/v9.9.9/x.exe",
        );
        let err = download_installer_file(release, |_, _, _| {}, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err();
        assert!(err.contains("sha256"));
    }

    #[tokio::test]
    async fn test_download_refuses_untrusted_url_before_any_request() {
        let release = release_with(Some(ABC_SHA256), "https://evil.example.com/x.exe");
        let err = download_installer_file(release, |_, _, _| {}, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err();
        assert!(err.contains("official release location"));
    }
}
