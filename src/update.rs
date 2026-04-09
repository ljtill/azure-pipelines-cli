use std::path::PathBuf;

use anyhow::{Context, Result, bail};

const GITHUB_REPO: &str = "ljtill/azure-pipelines-cli";
const GITHUB_API_BASE: &str = "https://api.github.com/repos";
const GITHUB_DOWNLOAD_BASE: &str = "https://github.com";
const CHECKSUMS_FILE_NAME: &str = "SHA256SUMS";

/// Number of old versions to keep when pruning.
const VERSIONS_TO_KEEP: usize = 3;

/// Return the compiled-in version from `Cargo.toml`.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Return `true` if `remote` is strictly newer than `current` (semver comparison).
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

/// Return the expected GitHub Release artifact name for the current platform.
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
        format!("pipelines-{os}-{arch}.exe")
    } else {
        format!("pipelines-{os}-{arch}")
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

/// Directory where versioned binaries are stored.
pub fn versions_dir() -> Result<PathBuf> {
    let data_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".local/share/pipelines/versions");
    Ok(data_dir)
}

/// Path where the symlink lives.
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

/// Check GitHub Releases for a newer version.
///
/// Returns `Some(version_string)` if a newer version exists, `None` otherwise.
/// Swallows all errors — this must never fail the app.
pub async fn check_for_update() -> Option<String> {
    check_for_update_inner().await.ok().flatten()
}

async fn check_for_update_inner() -> Result<Option<String>> {
    let version = fetch_latest_version().await?;
    if is_newer(&version, current_version()) {
        Ok(Some(version))
    } else {
        Ok(None)
    }
}

/// Fetch the latest release version string from GitHub.
async fn fetch_latest_version() -> Result<String> {
    let url = format!("{GITHUB_API_BASE}/{GITHUB_REPO}/releases/latest");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let resp = client
        .get(&url)
        .header("User-Agent", format!("pipelines/{}", current_version()))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;

    let body: serde_json::Value = resp.json().await?;
    let tag = body["tag_name"]
        .as_str()
        .context("Missing tag_name in release response")?;

    // Strip leading 'v' if present
    let version = tag.strip_prefix('v').unwrap_or(tag);
    Ok(version.to_string())
}

/// Result of a successful self-update.
pub struct UpdateResult {
    pub version: String,
    pub path: PathBuf,
}

/// Download the latest release, install to versioned directory, update symlink, prune old versions.
pub async fn self_update() -> Result<UpdateResult> {
    let latest = fetch_latest_version().await?;

    if !is_newer(&latest, current_version()) {
        bail!("Already on latest version (v{})", current_version());
    }

    let artifact = platform_artifact_name()?;
    let download_url = artifact_download_url(&latest)?;
    let checksums_url = checksums_download_url(&latest);

    // Prepare version directory
    let version_dir = versions_dir()?.join(&latest);
    std::fs::create_dir_all(&version_dir)
        .with_context(|| format!("Failed to create directory: {}", version_dir.display()))?;

    let binary_name = if cfg!(target_os = "windows") {
        "pipelines.exe"
    } else {
        "pipelines"
    };
    let binary_path = version_dir.join(binary_name);
    let temp_path = version_dir.join(format!("{binary_name}.download"));

    // Download
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let resp = client
        .get(&download_url)
        .header("User-Agent", format!("pipelines/{}", current_version()))
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {download_url}"))?;

    let checksums = client
        .get(&checksums_url)
        .header("User-Agent", format!("pipelines/{}", current_version()))
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {checksums_url}"))?
        .text()
        .await
        .context("Failed to read checksum manifest")?;
    let expected_sha256 = parse_checksum_manifest(&checksums, &artifact)?;

    let bytes = resp.bytes().await?;
    if temp_path.exists() {
        std::fs::remove_file(&temp_path)
            .with_context(|| format!("Failed to remove stale {}", temp_path.display()))?;
    }
    std::fs::write(&temp_path, &bytes)
        .with_context(|| format!("Failed to write binary to {}", temp_path.display()))?;
    if let Err(err) = verify_sha256(&temp_path, &expected_sha256) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err);
    }

    // Set executable permission (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    if binary_path.exists() {
        std::fs::remove_file(&binary_path)
            .with_context(|| format!("Failed to remove existing {}", binary_path.display()))?;
    }
    std::fs::rename(&temp_path, &binary_path)
        .with_context(|| format!("Failed to install binary to {}", binary_path.display()))?;

    // Update symlink
    update_symlink(&binary_path)?;

    // Prune old versions
    if let Err(e) = prune_old_versions(VERSIONS_TO_KEEP) {
        tracing::warn!("Failed to prune old versions: {e}");
    }

    Ok(UpdateResult {
        version: latest,
        path: binary_path,
    })
}

/// Remove the existing symlink (if any) and create a new one pointing to `target`.
/// Uses atomic replacement via a temporary symlink to avoid TOCTOU races.
fn update_symlink(target: &std::path::Path) -> Result<()> {
    let link = symlink_path()?;

    // Ensure parent directory exists
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_link = link.with_extension("tmp");

    // Clean up any stale temp symlink
    let _ = std::fs::remove_file(&tmp_link);

    // Create symlink at temp path, then atomically rename over the real path
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &tmp_link)
        .with_context(|| format!("Failed to create temp symlink at {}", tmp_link.display()))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, &tmp_link)
        .with_context(|| format!("Failed to create temp symlink at {}", tmp_link.display()))?;

    std::fs::rename(&tmp_link, &link).with_context(|| {
        // Clean up temp symlink on rename failure
        let _ = std::fs::remove_file(&tmp_link);
        format!("Failed to rename symlink to {}", link.display())
    })?;

    Ok(())
}

/// Keep the `keep` most recent versions, delete the rest.
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

    // Sort by semver descending (newest first)
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

    // Delete everything beyond `keep`
    for (_, path) in versions.into_iter().skip(keep) {
        tracing::info!("Pruning old version: {}", path.display());
        if let Err(e) = std::fs::remove_dir_all(&path) {
            tracing::warn!("Failed to remove {}: {e}", path.display());
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
        // Pre-release suffix is stripped — "0.2.0-beta" compares as "0.2.0"
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
        // Should be parseable as semver
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn platform_artifact_name_succeeds() {
        let name = platform_artifact_name().unwrap();
        assert!(name.starts_with("pipelines-"));
        assert!(name.contains("amd64") || name.contains("arm64"));
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
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  pipelines-linux-amd64
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  pipelines-windows-amd64.exe
";
        let hash = parse_checksum_manifest(manifest, "pipelines-windows-amd64.exe").unwrap();
        assert_eq!(
            hash,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn parse_checksum_manifest_rejects_missing_artifact() {
        let manifest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  pipelines-linux-amd64";
        let err = parse_checksum_manifest(manifest, "pipelines-darwin-arm64").unwrap_err();
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
        let path_str = path.to_string_lossy();
        assert!(path_str.contains(".local/bin/pipelines"));
    }
}
