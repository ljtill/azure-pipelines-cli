//! Self-update mechanism that downloads new releases from GitHub.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

const GITHUB_REPO: &str = "ljtill/azure-pipelines-cli";
const GITHUB_API_BASE: &str = "https://api.github.com/repos";
const GITHUB_DOWNLOAD_BASE: &str = "https://github.com";
const CHECKSUMS_FILE_NAME: &str = "SHA256SUMS";

/// Defines the number of old versions to keep when pruning.
const VERSIONS_TO_KEEP: usize = 3;

/// Returns a GitHub token from the environment, if available.
///
/// Checks `GITHUB_TOKEN` first, then falls back to `GH_TOKEN`.
fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.is_empty())
}

/// Returns the compiled-in version from `Cargo.toml`.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns `true` if `remote` is strictly newer than `current` (semver comparison).
pub fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u64, u64, u64)> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.splitn(3, '.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.split('-').next()?.parse().ok()?;
        Some((major, minor, patch))
    };

    match (parse(remote), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

/// Returns the expected GitHub Release archive name for the current platform.
pub fn platform_artifact_name() -> Result<String> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        bail!("Unsupported operating system");
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        bail!("Unsupported architecture");
    };

    let name = if cfg!(target_os = "windows") {
        format!("pipelines-{os}-{arch}.zip")
    } else {
        format!("pipelines-{os}-{arch}.tar.gz")
    };

    Ok(name)
}

fn artifact_download_url(version: &str) -> Result<String> {
    let artifact = platform_artifact_name()?;
    Ok(format!(
        "{GITHUB_DOWNLOAD_BASE}/{GITHUB_REPO}/releases/download/v{version}/{artifact}"
    ))
}

fn checksums_download_url(version: &str) -> String {
    format!(
        "{GITHUB_DOWNLOAD_BASE}/{GITHUB_REPO}/releases/download/v{version}/{CHECKSUMS_FILE_NAME}"
    )
}

fn parse_checksum_manifest(manifest: &str, artifact: &str) -> Result<String> {
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };

        if name == artifact {
            let normalized = hash.trim().to_ascii_lowercase();
            if normalized.len() == 64 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Ok(normalized);
            }
            bail!("Checksum for {artifact} is not a valid SHA-256 digest");
        }
    }

    bail!("Checksum for {artifact} not found in manifest");
}

fn parse_posix_hash_output(stdout: &[u8]) -> Result<String> {
    let stdout = String::from_utf8_lossy(stdout);
    let hash = stdout
        .split_whitespace()
        .next()
        .context("Hash command returned no digest")?
        .trim()
        .to_ascii_lowercase();

    if hash.len() == 64 && hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        bail!("Hash command returned an invalid SHA-256 digest");
    }
}

#[cfg(unix)]
fn compute_sha256(path: &std::path::Path) -> Result<String> {
    let sha256sum = std::process::Command::new("sha256sum").arg(path).output();
    if let Ok(output) = sha256sum
        && output.status.success()
    {
        return parse_posix_hash_output(&output.stdout);
    }

    let output = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .context("Failed to execute shasum for SHA-256 verification")?;
    if !output.status.success() {
        bail!("shasum exited with status {}", output.status);
    }

    parse_posix_hash_output(&output.stdout)
}

#[cfg(windows)]
fn compute_sha256(path: &std::path::Path) -> Result<String> {
    let output = std::process::Command::new("certutil")
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .context("Failed to execute certutil for SHA-256 verification")?;
    if !output.status.success() {
        bail!("certutil exited with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let normalized: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        if normalized.len() == 64 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Ok(normalized.to_ascii_lowercase());
        }
    }

    bail!("certutil output did not contain a SHA-256 digest");
}

fn verify_sha256(path: &std::path::Path, expected: &str) -> Result<()> {
    let actual = compute_sha256(path)?;
    let expected = expected.to_ascii_lowercase();
    if actual == expected {
        Ok(())
    } else {
        bail!(
            "SHA-256 mismatch for {} (expected {expected}, got {actual})",
            path.display()
        );
    }
}

/// Returns the directory where versioned binaries are stored.
pub fn versions_dir() -> Result<PathBuf> {
    let data_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".local/share/pipelines/versions");
    Ok(data_dir)
}

/// Returns the path where the symlink lives.
pub fn symlink_path() -> Result<PathBuf> {
    let bin_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".local/bin");

    let name = if cfg!(target_os = "windows") {
        "pipelines.exe"
    } else {
        "pipelines"
    };

    Ok(bin_dir.join(name))
}

/// Checks GitHub Releases for a newer version.
///
/// Returns `Some(version_string)` if a newer version exists, `None` otherwise.
/// Swallows all errors — this must never fail the app.
pub async fn check_for_update() -> Option<String> {
    check_for_update_inner().await.ok().flatten()
}

async fn check_for_update_inner() -> Result<Option<String>> {
    tracing::debug!(current = current_version(), "checking for updates");
    let version = fetch_latest_version().await?;
    if is_newer(&version, current_version()) {
        tracing::info!(
            remote = &*version,
            current = current_version(),
            "update available"
        );
        Ok(Some(version))
    } else {
        tracing::debug!(remote = &*version, "already on latest version");
        Ok(None)
    }
}

/// Fetches the latest release version string from GitHub.
async fn fetch_latest_version() -> Result<String> {
    let url = format!("{GITHUB_API_BASE}/{GITHUB_REPO}/releases/latest");
    tracing::debug!(url = &*url, "fetching latest version from GitHub");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let mut request = client
        .get(&url)
        .header("User-Agent", format!("pipelines/{}", current_version()))
        .header("Accept", "application/vnd.github+json");

    if let Some(token) = github_token() {
        request = request.header("Authorization", format!("token {token}"));
    }

    let resp = request.send().await?.error_for_status()?;

    let body: serde_json::Value = resp.json().await?;
    let tag = body["tag_name"]
        .as_str()
        .context("Missing tag_name in release response")?;

    // Strips leading 'v' if present.
    let version = tag.strip_prefix('v').unwrap_or(tag);
    tracing::debug!(version, "parsed latest version");
    Ok(version.to_string())
}

/// Represents the result of a successful self-update.
pub struct UpdateResult {
    pub version: String,
    pub path: PathBuf,
}

/// Extracts the binary from a `.tar.gz` archive into the given directory.
#[cfg(unix)]
fn extract_archive(archive_path: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
    let status = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(archive_path)
        .arg("-C")
        .arg(dest_dir)
        .status()
        .context("Failed to execute tar")?;
    if !status.success() {
        bail!("tar exited with status {status}");
    }
    Ok(())
}

/// Extracts the binary from a `.zip` archive into the given directory.
#[cfg(windows)]
fn extract_archive(archive_path: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                archive_path.display(),
                dest_dir.display()
            ),
        ])
        .status()
        .context("Failed to execute Expand-Archive")?;
    if !status.success() {
        bail!("Expand-Archive exited with status {status}");
    }
    Ok(())
}

/// Downloads the latest release, installs to the versioned directory, updates the symlink, and prunes old versions.
pub async fn self_update() -> Result<UpdateResult> {
    let latest = fetch_latest_version().await?;

    if !is_newer(&latest, current_version()) {
        tracing::info!(version = current_version(), "already on latest version");
        bail!("Already on latest version (v{})", current_version());
    }

    let artifact = platform_artifact_name()?;
    let download_url = artifact_download_url(&latest)?;
    let checksums_url = checksums_download_url(&latest);

    tracing::info!(
        version = &*latest,
        artifact = &*artifact,
        "starting self-update"
    );

    // Prepares the version directory.
    let version_dir = versions_dir()?.join(&latest);
    std::fs::create_dir_all(&version_dir)
        .with_context(|| format!("Failed to create directory: {}", version_dir.display()))?;

    let binary_name = if cfg!(target_os = "windows") {
        "pipelines.exe"
    } else {
        "pipelines"
    };
    let binary_path = version_dir.join(binary_name);
    let archive_path = version_dir.join(&artifact);

    // Downloads the archive.
    tracing::info!(url = &*download_url, "downloading archive");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let token = github_token();

    let mut archive_req = client
        .get(&download_url)
        .header("User-Agent", format!("pipelines/{}", current_version()));
    if let Some(ref token) = token {
        archive_req = archive_req.header("Authorization", format!("token {token}"));
    }
    let resp = archive_req
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {download_url}"))?;

    tracing::debug!(url = &*checksums_url, "downloading checksums");
    let mut checksums_req = client
        .get(&checksums_url)
        .header("User-Agent", format!("pipelines/{}", current_version()));
    if let Some(ref token) = token {
        checksums_req = checksums_req.header("Authorization", format!("token {token}"));
    }
    let checksums = checksums_req
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {checksums_url}"))?
        .text()
        .await
        .context("Failed to read checksum manifest")?;
    let expected_sha256 = parse_checksum_manifest(&checksums, &artifact)?;

    let bytes = resp.bytes().await?;
    tracing::debug!(size_bytes = bytes.len(), "download complete");
    if archive_path.exists() {
        std::fs::remove_file(&archive_path)
            .with_context(|| format!("Failed to remove stale {}", archive_path.display()))?;
    }
    std::fs::write(&archive_path, &bytes)
        .with_context(|| format!("Failed to write archive to {}", archive_path.display()))?;
    if let Err(err) = verify_sha256(&archive_path, &expected_sha256) {
        tracing::warn!(error = %err, "SHA256 verification failed");
        let _ = std::fs::remove_file(&archive_path);
        return Err(err);
    }
    tracing::debug!("SHA256 verification passed");

    // Extracts the binary from the archive.
    tracing::debug!("extracting archive");
    extract_archive(&archive_path, &version_dir)?;
    let _ = std::fs::remove_file(&archive_path);

    // Removes the platform-named binary left by extraction (e.g. pipelines-darwin-arm64).
    let extracted_name = if cfg!(target_os = "windows") {
        artifact.strip_suffix(".zip").unwrap_or(&artifact)
    } else {
        artifact.strip_suffix(".tar.gz").unwrap_or(&artifact)
    };
    let extracted_path = version_dir.join(extracted_name);
    if extracted_path.exists() && extracted_path != binary_path {
        std::fs::rename(&extracted_path, &binary_path).with_context(|| {
            format!(
                "Failed to rename {} to {}",
                extracted_path.display(),
                binary_path.display()
            )
        })?;
    }

    // Sets executable permission (Unix).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))?;
    }

    tracing::info!(path = %binary_path.display(), "binary installed");

    // Updates the binary in the user's PATH.
    install_to_bin(&binary_path)?;
    tracing::debug!(target = %binary_path.display(), "binary link updated");

    // Prunes old versions.
    if let Err(e) = prune_old_versions(VERSIONS_TO_KEEP) {
        tracing::warn!(error = %e, "failed to prune old versions");
    }

    Ok(UpdateResult {
        version: latest,
        path: binary_path,
    })
}

/// Installs the updated binary into the user's PATH.
///
/// On Unix this creates an atomic symlink swap. On Windows, symlinks require
/// elevated privileges so we copy the binary directly, renaming any existing
/// file out of the way first (Windows allows renaming a running executable).
fn install_to_bin(target: &std::path::Path) -> Result<()> {
    let dest = symlink_path()?;

    // Ensures the parent directory exists.
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        let tmp_link = dest.with_extension("tmp");

        // Cleans up any stale temp symlink.
        let _ = std::fs::remove_file(&tmp_link);

        // Creates symlink at temp path, then atomically renames over the real path.
        std::os::unix::fs::symlink(target, &tmp_link)
            .with_context(|| format!("Failed to create temp symlink at {}", tmp_link.display()))?;

        std::fs::rename(&tmp_link, &dest).with_context(|| {
            let _ = std::fs::remove_file(&tmp_link);
            format!("Failed to rename symlink to {}", dest.display())
        })?;
    }

    #[cfg(windows)]
    {
        // Rename the existing binary out of the way (Windows allows renaming
        // a running executable even though it cannot delete one).
        let old_path = dest.with_extension("exe.old");
        if dest.exists() {
            let _ = std::fs::remove_file(&old_path);
            std::fs::rename(&dest, &old_path).with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    dest.display(),
                    old_path.display()
                )
            })?;
        }

        std::fs::copy(target, &dest).with_context(|| {
            format!("Failed to copy {} to {}", target.display(), dest.display())
        })?;

        // Best-effort cleanup of the old binary.
        let _ = std::fs::remove_file(&old_path);
    }

    Ok(())
}

/// Keeps the `keep` most recent versions and deletes the rest.
fn prune_old_versions(keep: usize) -> Result<()> {
    let base = versions_dir()?;
    if !base.exists() {
        return Ok(());
    }

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&base)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            versions.push((name.to_string(), path));
        }
    }

    // Sorts by semver descending (newest first).
    versions.sort_by(|(a, _), (b, _)| {
        let parse = |s: &str| -> (u64, u64, u64) {
            let mut parts = s.splitn(3, '.');
            let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            (major, minor, patch)
        };
        parse(b).cmp(&parse(a))
    });

    // Deletes everything beyond `keep`.
    for (_, path) in versions.into_iter().skip(keep) {
        tracing::info!(path = %path.display(), "pruning old version");
        if let Err(e) = std::fs::remove_dir_all(&path) {
            tracing::warn!(path = %path.display(), error = %e, "failed to remove old version");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_basic() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
    }

    #[test]
    fn is_newer_equal() {
        assert!(!is_newer("0.1.0", "0.1.0"));
    }

    #[test]
    fn is_newer_older() {
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn is_newer_with_v_prefix() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.2.0", "v0.1.0"));
        assert!(!is_newer("v0.1.0", "v0.1.0"));
    }

    #[test]
    fn is_newer_with_prerelease() {
        // Pre-release suffix is stripped — "0.2.0-beta" compares as "0.2.0".
        assert!(is_newer("0.2.0-beta", "0.1.0"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
    }

    #[test]
    fn is_newer_malformed() {
        assert!(!is_newer("not-a-version", "0.1.0"));
        assert!(!is_newer("0.1.0", "bad"));
        assert!(!is_newer("", "0.1.0"));
    }

    #[test]
    fn current_version_is_set() {
        let v = current_version();
        assert!(!v.is_empty());
        // Should be parseable as semver.
        assert_eq!(v.split('.').count(), 3);
    }

    #[test]
    fn platform_artifact_name_succeeds() {
        let name = platform_artifact_name().unwrap();
        assert!(name.starts_with("pipelines-"));
        assert!(name.contains("amd64") || name.contains("arm64"));
        let path = std::path::Path::new(&name);
        // Archive must be .tar.gz or .zip.
        assert!(
            name.ends_with(".tar.gz")
                || path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        );
    }

    #[test]
    fn artifact_download_url_uses_canonical_artifact_name() {
        let url = artifact_download_url("1.2.3").unwrap();
        assert!(url.contains("/releases/download/v1.2.3/"));
        assert!(url.contains("pipelines-"));
        assert!(!url.contains("azure-pipelines-cli-"));
    }

    #[test]
    fn checksums_download_url_points_to_manifest() {
        let url = checksums_download_url("1.2.3");
        assert!(url.ends_with("/releases/download/v1.2.3/SHA256SUMS"));
    }

    #[test]
    fn parse_checksum_manifest_returns_matching_hash() {
        let manifest = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  pipelines-linux-amd64.tar.gz
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  pipelines-windows-amd64.zip
";
        let hash = parse_checksum_manifest(manifest, "pipelines-windows-amd64.zip").unwrap();
        assert_eq!(
            hash,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn parse_checksum_manifest_rejects_missing_artifact() {
        let manifest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  pipelines-linux-amd64.tar.gz";
        let err = parse_checksum_manifest(manifest, "pipelines-darwin-arm64.tar.gz").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn versions_dir_is_under_home() {
        let dir = versions_dir().unwrap();
        let dir_str = dir.to_string_lossy();
        assert!(dir_str.contains(".local/share/pipelines/versions"));
    }

    #[test]
    fn symlink_path_is_under_bin() {
        let path = symlink_path().unwrap();
        assert!(
            path.ends_with(".local/bin/pipelines") || path.ends_with(".local/bin/pipelines.exe")
        );
    }

    #[test]
    fn github_token_reads_github_token_env() {
        // SAFETY: This test runs serially (single-threaded test harness) and
        // saves/restores env vars before returning.
        unsafe {
            let saved_gh = std::env::var("GITHUB_TOKEN").ok();
            let saved_cli = std::env::var("GH_TOKEN").ok();

            std::env::set_var("GITHUB_TOKEN", "test-token-123");
            std::env::remove_var("GH_TOKEN");
            assert_eq!(github_token().as_deref(), Some("test-token-123"));

            // Falls back to GH_TOKEN when GITHUB_TOKEN is absent.
            std::env::remove_var("GITHUB_TOKEN");
            std::env::set_var("GH_TOKEN", "gh-token-456");
            assert_eq!(github_token().as_deref(), Some("gh-token-456"));

            // Returns None when neither is set.
            std::env::remove_var("GITHUB_TOKEN");
            std::env::remove_var("GH_TOKEN");
            assert_eq!(github_token(), None);

            // Empty string is treated as absent.
            std::env::set_var("GITHUB_TOKEN", "");
            assert_eq!(github_token(), None);

            // Restore.
            match saved_gh {
                Some(v) => std::env::set_var("GITHUB_TOKEN", v),
                None => std::env::remove_var("GITHUB_TOKEN"),
            }
            match saved_cli {
                Some(v) => std::env::set_var("GH_TOKEN", v),
                None => std::env::remove_var("GH_TOKEN"),
            }
        }
    }
}
