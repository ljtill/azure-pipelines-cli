//! Self-update mechanism that downloads new releases from GitHub.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::shared::SecretString;

const GITHUB_REPO: &str = "ljtill/azure-devops-cli";
const GITHUB_API_BASE: &str = "https://api.github.com/repos";
const GITHUB_DOWNLOAD_BASE: &str = "https://github.com";
const CHECKSUMS_FILE_NAME: &str = "SHA256SUMS";

/// Defines the number of old versions to keep when pruning.
///
/// Kept at >= 2 so that startup rollback always has a previous version to
/// revert to if a self-update is interrupted.
const VERSIONS_TO_KEEP: usize = 3;

/// Name of the update lock file written under `install_root` during a
/// two-phase self-update.
const UPDATE_LOCK_FILE: &str = ".update-lock";

/// Returns a GitHub token from the environment, if available.
///
/// Checks `GITHUB_TOKEN` first, then falls back to `GH_TOKEN`. The token is
/// wrapped in a [`SecretString`] so it cannot accidentally leak into logs.
fn github_token() -> Option<SecretString> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.is_empty())
        .map(SecretString::from)
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
        format!("devops-{os}-{arch}.zip")
    } else {
        format!("devops-{os}-{arch}.tar.gz")
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
fn compute_sha256_sync(path: &std::path::Path) -> Result<String> {
    // Safe: invoked only via `spawn_blocking` from `compute_sha256`; external
    // `sha256sum`/`shasum` processes are inherently blocking.
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
fn compute_sha256_sync(path: &std::path::Path) -> Result<String> {
    // Safe: invoked only via `spawn_blocking` from `compute_sha256`; `certutil`
    // is inherently blocking.
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

async fn compute_sha256(path: &std::path::Path) -> Result<String> {
    let owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || compute_sha256_sync(&owned))
        .await
        .context("SHA-256 computation task panicked")?
}

async fn verify_sha256(path: &std::path::Path, expected: &str) -> Result<()> {
    let actual = compute_sha256(path).await?;
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

// --- Sigstore / cosign keyless signature verification ---

/// Name of the cosign bundle asset uploaded alongside `SHA256SUMS`.
const COSIGN_BUNDLE_FILE_NAME: &str = "SHA256SUMS.cosign.bundle";

/// OIDC issuer expected in the Fulcio-issued signing certificate. GitHub
/// Actions workflow identities are issued by this exact URL.
pub(crate) const SIGSTORE_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// Prefix of the X.509v3 SAN URI we expect to see in the signing certificate,
/// up to and including the `refs/tags/v` portion. The remainder must be a
/// semantic version (`MAJOR.MINOR.PATCH`) matching the release tag.
pub(crate) const SIGSTORE_CERT_IDENTITY_PREFIX: &str =
    "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v";

/// Documentation-only equivalent regex for `SIGSTORE_CERT_IDENTITY_PREFIX` +
/// trailing semver, used by the install scripts via `cosign verify-blob
/// --certificate-identity-regexp`. Kept in sync with `expected_cert_identity`
/// and `cert_identity_matches_expected` below.
#[allow(dead_code)] // Referenced by tests + install scripts (out-of-process), not by runtime code.
pub(crate) const SIGSTORE_CERT_IDENTITY_RE: &str = r"^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/tags/v\d+\.\d+\.\d+$";

fn cosign_bundle_download_url(version: &str) -> String {
    format!(
        "{GITHUB_DOWNLOAD_BASE}/{GITHUB_REPO}/releases/download/v{version}/{COSIGN_BUNDLE_FILE_NAME}"
    )
}

/// Returns the exact SAN URI we expect in the Fulcio-issued signing certificate
/// for a release tagged `v{version}`.
fn expected_cert_identity(version: &str) -> String {
    format!("{SIGSTORE_CERT_IDENTITY_PREFIX}{version}")
}

/// Validates that a certificate-identity string matches the shape we expect
/// from our release workflow: the fixed prefix plus a strict `MAJOR.MINOR.PATCH`
/// suffix. This is the Rust analogue of `SIGSTORE_CERT_IDENTITY_RE` and is used
/// by unit tests; runtime verification uses [`expected_cert_identity`] for an
/// exact match via sigstore's `Identity` policy.
#[allow(dead_code)] // Referenced by tests; runtime uses expected_cert_identity for exact match.
fn cert_identity_matches_expected(identity: &str) -> bool {
    let Some(tag) = identity.strip_prefix(SIGSTORE_CERT_IDENTITY_PREFIX) else {
        return false;
    };
    let parts: Vec<&str> = tag.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Verifies a cosign Sigstore bundle against the signed payload using
/// **keyless** (Fulcio + Rekor) verification.
///
/// # Security model
///
/// Fails closed. The signing certificate must:
/// 1. Chain back to Sigstore's Fulcio root (via the TUF-distributed trust root).
/// 2. Contain an OIDC issuer extension equal to [`SIGSTORE_OIDC_ISSUER`].
/// 3. Contain a SAN URI exactly equal to `expected_cert_identity(version)`.
///
/// The bundle's embedded Rekor log entry is checked for consistency with the
/// signing materials in *offline* mode — we trust the SET in the bundle rather
/// than calling Rekor. (As of `sigstore` 0.13 the Rust implementation has not
/// yet implemented Merkle inclusion proof or SET signature verification beyond
/// consistency checks; see tracking issue sigstore-rs#285. Keyless cert-chain,
/// identity, and signature verification are still enforced — which is the
/// primary defense against a tampered CDN/mirror.)
async fn verify_sigstore_bundle(
    signed_payload: &[u8],
    bundle_json: &[u8],
    version: &str,
) -> Result<()> {
    use sigstore::bundle::Bundle;
    use sigstore::bundle::verify::{Verifier, policy::Identity};

    let bundle: Bundle = serde_json::from_slice(bundle_json)
        .context("Failed to parse cosign Sigstore bundle JSON")?;

    let verifier = Verifier::production()
        .await
        .context("Failed to initialize Sigstore trust root (public-good)")?;

    let identity = expected_cert_identity(version);
    let policy = Identity::new(&identity, SIGSTORE_OIDC_ISSUER);

    verifier
        .verify(signed_payload, bundle, &policy, true)
        .await
        .map_err(|e| anyhow::anyhow!("Signature verification failed: {e}"))?;

    Ok(())
}

/// Returns the root directory under which versioned binaries and the update
/// lock file live. This is the same directory that holds `versions/`.
pub fn install_root() -> Result<PathBuf> {
    let root = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".local/share/devops");
    Ok(root)
}

/// Returns the directory where versioned binaries are stored.
pub fn versions_dir() -> Result<PathBuf> {
    Ok(install_root()?.join("versions"))
}

/// Returns the path to the update lock file under the default install root.
pub fn lock_path() -> Result<PathBuf> {
    Ok(install_root()?.join(UPDATE_LOCK_FILE))
}

/// Returns the path where the symlink lives.
pub fn symlink_path() -> Result<PathBuf> {
    let bin_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".local/bin");

    let name = if cfg!(target_os = "windows") {
        "devops.exe"
    } else {
        "devops"
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
        .header("User-Agent", format!("devops/{}", current_version()))
        .header("Accept", "application/vnd.github+json");

    if let Some(ref token) = github_token() {
        request = request.header("Authorization", format!("token {}", token.expose_secret()));
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
async fn extract_archive(archive_path: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Safe: `std::process::Command` is inherently blocking; this closure runs on the blocking pool.
        let status = std::process::Command::new("tar")
            .args(["xzf"])
            .arg(&archive)
            .arg("-C")
            .arg(&dest)
            .status()
            .context("Failed to execute tar")?;
        if !status.success() {
            bail!("tar exited with status {status}");
        }
        Ok(())
    })
    .await
    .context("tar extraction task panicked")?
}

/// Extracts the binary from a `.zip` archive into the given directory.
#[cfg(windows)]
async fn extract_archive(archive_path: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Safe: `std::process::Command` is inherently blocking; this closure runs on the blocking pool.
        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    dest.display()
                ),
            ])
            .status()
            .context("Failed to execute Expand-Archive")?;
        if !status.success() {
            bail!("Expand-Archive exited with status {status}");
        }
        Ok(())
    })
    .await
    .context("Expand-Archive task panicked")?
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
    tokio::fs::create_dir_all(&version_dir)
        .await
        .with_context(|| format!("Failed to create directory: {}", version_dir.display()))?;

    let binary_name = if cfg!(target_os = "windows") {
        "devops.exe"
    } else {
        "devops"
    };
    let binary_path = version_dir.join(binary_name);
    let archive_path = version_dir.join(&artifact);
    // Stages the download under a hidden temp name. Only promoted to the final
    // archive path after SHA-256 verification succeeds.
    let staging_path = version_dir.join(format!(".tmp-{artifact}"));

    // Downloads the archive.
    tracing::info!(url = &*download_url, "downloading archive");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(1))
        .build()?;

    let token = github_token();

    let mut archive_req = client
        .get(&download_url)
        .header("User-Agent", format!("devops/{}", current_version()));
    if let Some(ref token) = token {
        archive_req =
            archive_req.header("Authorization", format!("token {}", token.expose_secret()));
    }
    let resp = archive_req
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {download_url}"))?;

    tracing::debug!(url = &*checksums_url, "downloading checksums");
    let mut checksums_req = client
        .get(&checksums_url)
        .header("User-Agent", format!("devops/{}", current_version()));
    if let Some(ref token) = token {
        checksums_req =
            checksums_req.header("Authorization", format!("token {}", token.expose_secret()));
    }
    let checksums_bytes = checksums_req
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {checksums_url}"))?
        .bytes()
        .await
        .context("Failed to read checksum manifest")?;

    // Downloads the cosign Sigstore bundle and verifies the SHA256SUMS payload
    // BEFORE trusting any hash it contains. Keyless verification checks:
    //   - Fulcio-issued signing cert chains to the Sigstore TUF trust root
    //   - cert SAN identity == our release workflow at the matching tag
    //   - cert OIDC issuer == GitHub Actions
    //   - signature over SHA256SUMS bytes
    // Fails closed: no skip flag, no fallback, no "missing bundle" path.
    let bundle_url = cosign_bundle_download_url(&latest);
    tracing::debug!(url = &*bundle_url, "downloading cosign bundle");
    let mut bundle_req = client
        .get(&bundle_url)
        .header("User-Agent", format!("devops/{}", current_version()));
    if let Some(ref token) = token {
        bundle_req = bundle_req.header("Authorization", format!("token {}", token.expose_secret()));
    }
    let bundle_bytes = bundle_req
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {bundle_url}"))?
        .bytes()
        .await
        .context("Failed to read cosign bundle")?;

    verify_sigstore_bundle(&checksums_bytes, &bundle_bytes, &latest)
        .await
        .context("Cosign/Sigstore verification of SHA256SUMS failed")?;
    tracing::info!("cosign/Sigstore signature verified for SHA256SUMS");

    let checksums =
        std::str::from_utf8(&checksums_bytes).context("SHA256SUMS is not valid UTF-8")?;
    let expected_sha256 = parse_checksum_manifest(checksums, &artifact)?;

    let bytes = resp.bytes().await?;
    tracing::debug!(size_bytes = bytes.len(), "download complete");

    // Clears any stale staging file from a previous interrupted run.
    if tokio::fs::try_exists(&staging_path).await.unwrap_or(false) {
        tokio::fs::remove_file(&staging_path)
            .await
            .with_context(|| format!("Failed to remove stale {}", staging_path.display()))?;
    }
    tokio::fs::write(&staging_path, &bytes)
        .await
        .with_context(|| format!("Failed to write archive to {}", staging_path.display()))?;
    if let Err(err) = verify_sha256(&staging_path, &expected_sha256).await {
        tracing::warn!(error = %err, "SHA256 verification failed");
        let _ = tokio::fs::remove_file(&staging_path).await;
        return Err(err);
    }
    tracing::debug!("SHA256 verification passed");

    // Promotes the verified archive atomically into place.
    if tokio::fs::try_exists(&archive_path).await.unwrap_or(false) {
        tokio::fs::remove_file(&archive_path)
            .await
            .with_context(|| format!("Failed to remove stale {}", archive_path.display()))?;
    }
    tokio::fs::rename(&staging_path, &archive_path)
        .await
        .with_context(|| {
            format!(
                "Failed to promote staged archive {} to {}",
                staging_path.display(),
                archive_path.display()
            )
        })?;

    // Extracts the binary from the archive.
    tracing::debug!("extracting archive");
    extract_archive(&archive_path, &version_dir).await?;
    let _ = tokio::fs::remove_file(&archive_path).await;

    // Removes the platform-named binary left by extraction (e.g. devops-darwin-arm64).
    let extracted_name = if cfg!(target_os = "windows") {
        artifact.strip_suffix(".zip").unwrap_or(&artifact)
    } else {
        artifact.strip_suffix(".tar.gz").unwrap_or(&artifact)
    };
    let extracted_path = version_dir.join(extracted_name);
    if tokio::fs::try_exists(&extracted_path)
        .await
        .unwrap_or(false)
        && extracted_path != binary_path
    {
        tokio::fs::rename(&extracted_path, &binary_path)
            .await
            .with_context(|| {
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
        tokio::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).await?;
    }

    tracing::info!(path = %binary_path.display(), "binary installed");

    // --- Two-phase commit with startup rollback ---
    //
    // Write an `InProgress` lock BEFORE the symlink swap so that, if the
    // process is killed mid-swap, the next startup can roll back to the
    // previous version. Once the swap succeeds we mark the lock `Committed`
    // and then delete it; old versions are only pruned after the lock is
    // gone so the rollback target always exists.
    let install_root_path = install_root()?;
    tokio::fs::create_dir_all(&install_root_path)
        .await
        .with_context(|| {
            format!(
                "Failed to create install root: {}",
                install_root_path.display()
            )
        })?;
    let lock_file = install_root_path.join(UPDATE_LOCK_FILE);
    let from_version = current_version().to_string();
    let to_version = latest.clone();
    let lock = UpdateLock {
        from_version: from_version.clone(),
        to_version: to_version.clone(),
        status: UpdateLockStatus::InProgress,
        started_at: chrono::Utc::now(),
    };
    write_lock(&lock_file, &lock).await?;

    // Updates the binary in the user's PATH.
    install_to_bin(&binary_path).await?;
    tracing::debug!(target = %binary_path.display(), "binary link updated");

    // Mark the update as committed, then delete the lock. If either step
    // fails, the lock stays on disk and startup will recover on next launch.
    let committed = UpdateLock {
        from_version,
        to_version,
        status: UpdateLockStatus::Committed,
        started_at: lock.started_at,
    };
    write_lock(&lock_file, &committed).await?;
    delete_lock(&lock_file).await?;

    // Prunes old versions — only after the lock is cleared, to guarantee a
    // rollback target remained intact through the whole swap.
    if let Err(e) = prune_old_versions(VERSIONS_TO_KEEP).await {
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
async fn install_to_bin(target: &std::path::Path) -> Result<()> {
    let dest = symlink_path()?;
    install_to_path(target, &dest).await
}

/// Install the given target file at `dest` using the same semantics as
/// `install_to_bin`. Extracted so tests can exercise the swap logic against a
/// temporary destination without touching the user's real install path.
async fn install_to_path(target: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    // Ensures the parent directory exists.
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    #[cfg(unix)]
    {
        let tmp_link = dest.with_extension("tmp");

        // Cleans up any stale temp symlink.
        let _ = tokio::fs::remove_file(&tmp_link).await;

        // Creates symlink at temp path, then atomically renames over the real path.
        tokio::fs::symlink(target, &tmp_link)
            .await
            .with_context(|| format!("Failed to create temp symlink at {}", tmp_link.display()))?;

        tokio::fs::rename(&tmp_link, dest).await.with_context(|| {
            let _ = std::fs::remove_file(&tmp_link); // Safe: best-effort cleanup after rename failure.
            format!("Failed to rename symlink to {}", dest.display())
        })?;
    }

    #[cfg(windows)]
    {
        // Rename the existing binary out of the way (Windows allows renaming
        // a running executable even though it cannot delete one).
        let old_path = dest.with_extension("exe.old");
        if tokio::fs::try_exists(dest).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&old_path).await;
            tokio::fs::rename(dest, &old_path).await.with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    dest.display(),
                    old_path.display()
                )
            })?;
        }

        tokio::fs::copy(target, dest).await.with_context(|| {
            format!("Failed to copy {} to {}", target.display(), dest.display())
        })?;

        // Best-effort cleanup of the old binary.
        let _ = tokio::fs::remove_file(&old_path).await;
    }

    Ok(())
}

/// Keeps the `keep` most recent versions and deletes the rest.
async fn prune_old_versions(keep: usize) -> Result<()> {
    let base = versions_dir()?;
    if !tokio::fs::try_exists(&base).await.unwrap_or(false) {
        return Ok(());
    }

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&base).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;
        if file_type.is_dir()
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
        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
            tracing::warn!(path = %path.display(), error = %e, "failed to remove old version");
        }
    }

    Ok(())
}

// --- Update lock: two-phase commit with startup rollback ---

/// Persistent state describing an in-flight or just-finished self-update.
///
/// Written to `{install_root}/.update-lock` before the symlink swap, updated
/// to `Committed` after the swap succeeds, then deleted before pruning old
/// versions. If the process is killed at any point between the `InProgress`
/// write and the final delete, startup will detect the stale lock and roll
/// back as needed.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateLock {
    pub from_version: String,
    pub to_version: String,
    pub status: UpdateLockStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum UpdateLockStatus {
    InProgress,
    Committed,
}

/// Writes the lock to `path` atomically. Writes to a sibling `.tmp` file,
/// `fsync`s its contents to disk, then renames over the destination so a
/// concurrent reader never observes a half-written file.
pub async fn write_lock(path: &Path, lock: &UpdateLock) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "Failed to create lock parent directory: {}",
                parent.display()
            )
        })?;
    }

    let bytes = serde_json::to_vec_pretty(lock).context("Failed to serialize update lock")?;
    let tmp_path = path.with_extension("tmp");
    // Clean up any stale `.tmp` from a previous crashed write.
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .with_context(|| format!("Failed to create {}", tmp_path.display()))?;
    file.write_all(&bytes)
        .await
        .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("Failed to fsync {}", tmp_path.display()))?;
    drop(file);

    tokio::fs::rename(&tmp_path, path).await.with_context(|| {
        format!(
            "Failed to rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

/// Reads the lock at `path`. Returns `Ok(None)` if the file does not exist.
pub async fn read_lock(path: &Path) -> Result<Option<UpdateLock>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let lock: UpdateLock = serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to parse update lock at {}", path.display()))?;
            Ok(Some(lock))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            Err(e).with_context(|| format!("Failed to read update lock at {}", path.display()))
        }
    }
}

/// Deletes the lock at `path`. Idempotent — returns `Ok(())` if it is absent.
pub async fn delete_lock(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(e).with_context(|| format!("Failed to delete update lock at {}", path.display()))
        }
    }
}

/// Describes a rollback that happened at startup so the UI can surface it.
#[derive(Debug, Clone)]
pub struct RollbackReport {
    pub from_version: String,
    pub to_version: String,
}

/// Returns the platform-appropriate binary filename for a version directory.
fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "devops.exe"
    } else {
        "devops"
    }
}

/// Checks for an interrupted self-update under `install_root` and rolls back
/// if necessary.
///
/// - If no lock is present, returns `Ok(None)`.
/// - If the lock is `Committed`, the prior update finished successfully but
///   cleanup was interrupted: the stale lock is deleted and `Ok(None)` is
///   returned.
/// - If the lock is `InProgress`, the symlink is repointed at the previous
///   version's binary (best-effort), the lock is deleted, and an
///   `Ok(Some(RollbackReport))` is returned so the UI can notify the user.
pub async fn recover_from_interrupted_update(
    install_root: &Path,
) -> Result<Option<RollbackReport>> {
    let symlink_dest = symlink_path()?;
    recover_from_interrupted_update_with_paths(install_root, &symlink_dest).await
}

/// Variant of [`recover_from_interrupted_update`] that accepts an explicit
/// symlink destination. Used by tests so the user's real install tree is
/// never touched.
async fn recover_from_interrupted_update_with_paths(
    install_root: &Path,
    symlink_dest: &Path,
) -> Result<Option<RollbackReport>> {
    let lock_file = install_root.join(UPDATE_LOCK_FILE);
    let Some(lock) = read_lock(&lock_file).await? else {
        return Ok(None);
    };

    match lock.status {
        UpdateLockStatus::Committed => {
            tracing::info!(
                from = %lock.from_version,
                to = %lock.to_version,
                "previous update committed but cleanup was interrupted; removing stale lock"
            );
            delete_lock(&lock_file).await?;
            Ok(None)
        }
        UpdateLockStatus::InProgress => {
            tracing::warn!(
                from = %lock.from_version,
                to = %lock.to_version,
                "previous update was interrupted; rolling back"
            );
            let rollback_binary = install_root
                .join("versions")
                .join(&lock.from_version)
                .join(binary_name());
            if tokio::fs::try_exists(&rollback_binary)
                .await
                .unwrap_or(false)
            {
                if let Err(e) = install_to_path(&rollback_binary, symlink_dest).await {
                    tracing::error!(
                        error = %e,
                        target = %rollback_binary.display(),
                        "failed to restore symlink during rollback"
                    );
                }
            } else {
                tracing::warn!(
                    path = %rollback_binary.display(),
                    "previous version binary not found; cannot restore symlink"
                );
            }
            // Always clear the lock so we don't loop on the next start.
            delete_lock(&lock_file).await?;
            Ok(Some(RollbackReport {
                from_version: lock.from_version,
                to_version: lock.to_version,
            }))
        }
    }
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
        assert!(name.starts_with("devops-"));
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
        assert!(url.contains("devops-"));
        assert!(!url.contains("azure-devops-cli-"));
    }

    #[test]
    fn checksums_download_url_points_to_manifest() {
        let url = checksums_download_url("1.2.3");
        assert!(url.ends_with("/releases/download/v1.2.3/SHA256SUMS"));
    }

    #[test]
    fn parse_checksum_manifest_returns_matching_hash() {
        let manifest = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  devops-linux-amd64.tar.gz
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  devops-windows-amd64.zip
";
        let hash = parse_checksum_manifest(manifest, "devops-windows-amd64.zip").unwrap();
        assert_eq!(
            hash,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn parse_checksum_manifest_rejects_missing_artifact() {
        let manifest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  devops-linux-amd64.tar.gz";
        let err = parse_checksum_manifest(manifest, "devops-darwin-arm64.tar.gz").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // --- Sigstore / cosign verification ---

    #[test]
    fn cosign_bundle_url_is_on_release_asset_path() {
        let url = cosign_bundle_download_url("1.2.3");
        assert!(url.ends_with("/releases/download/v1.2.3/SHA256SUMS.cosign.bundle"));
    }

    #[test]
    fn expected_cert_identity_embeds_version() {
        let id = expected_cert_identity("1.2.3");
        assert_eq!(
            id,
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v1.2.3"
        );
    }

    #[test]
    fn cert_identity_matches_known_good_urls() {
        // Exact identity for a real release tag.
        assert!(cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v1.0.0"
        ));
        assert!(cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v10.20.30"
        ));
        // Round-trips expected_cert_identity().
        assert!(cert_identity_matches_expected(&expected_cert_identity(
            "1.0.1"
        )));
    }

    #[test]
    fn cert_identity_rejects_wrong_identity() {
        // Different owner.
        assert!(!cert_identity_matches_expected(
            "https://github.com/attacker/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v1.0.0"
        ));
        // Different repo.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/evil-repo/.github/workflows/ci.release.yml@refs/tags/v1.0.0"
        ));
        // Different workflow file.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/other.yml@refs/tags/v1.0.0"
        ));
        // Branch ref instead of tag ref.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/heads/main"
        ));
        // Missing "v" prefix on the tag.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/1.0.0"
        ));
        // Non-semver tag.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v1.0"
        ));
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/vbeta.1.0"
        ));
        // Empty.
        assert!(!cert_identity_matches_expected(""));
        // Trailing junk.
        assert!(!cert_identity_matches_expected(
            "https://github.com/ljtill/azure-devops-cli/.github/workflows/ci.release.yml@refs/tags/v1.0.0/extra"
        ));
    }

    /// The documented identity regex (used by `install.sh` and `install.ps1`
    /// as `cosign verify-blob --certificate-identity-regexp`) must stay in
    /// sync with [`cert_identity_matches_expected`] so the two verification
    /// paths accept the same set of inputs.
    #[test]
    fn cert_identity_regex_constant_is_stable() {
        assert_eq!(
            SIGSTORE_CERT_IDENTITY_RE,
            r"^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/tags/v\d+\.\d+\.\d+$"
        );
    }

    #[test]
    fn sigstore_oidc_issuer_is_github_actions() {
        // Exact-match guard: anything else indicates a misconfigured or forged
        // cert (e.g., a token from google, sigstore-staging, etc.).
        assert_eq!(
            SIGSTORE_OIDC_ISSUER,
            "https://token.actions.githubusercontent.com"
        );
    }

    /// Negative test: verifying random non-bundle JSON must be rejected.
    ///
    /// End-to-end keyless verification against a real Fulcio-issued cert
    /// chain is covered by the release pipeline integration (the self-update
    /// path on a signed release), not by fixtures — generating a valid
    /// Sigstore bundle at test time would require mocking Fulcio, Rekor, and
    /// CTFE, which is out of scope for this unit test module.
    #[tokio::test]
    async fn verify_sigstore_bundle_rejects_invalid_bundle_json() {
        let payload = b"SHA256SUMS content";
        let garbage = br#"{"not":"a real bundle"}"#;
        let err = verify_sigstore_bundle(payload, garbage, "1.0.0")
            .await
            .expect_err("bogus bundle JSON must not verify");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("bundle") || msg.contains("parse") || msg.contains("sigstore"),
            "unexpected error surface: {err}"
        );
    }

    #[tokio::test]
    async fn verify_sigstore_bundle_rejects_non_json_bytes() {
        let payload = b"SHA256SUMS content";
        let err = verify_sigstore_bundle(payload, b"\xff\xff\xff\xff not json", "1.0.0")
            .await
            .expect_err("non-JSON bytes must not verify");
        // Just assert it's the parse/trust-root layer failing, not a panic.
        let _ = err.to_string();
    }

    #[test]
    fn versions_dir_is_under_home() {
        let dir = versions_dir().unwrap();
        assert!(dir.ends_with("devops/versions"));
        assert!(dir.components().any(|c| c.as_os_str() == ".local"));
    }

    #[test]
    fn symlink_path_is_under_bin() {
        let path = symlink_path().unwrap();
        assert!(path.ends_with(".local/bin/devops") || path.ends_with(".local/bin/devops.exe"));
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
            assert_eq!(
                github_token().as_ref().map(SecretString::expose_secret),
                Some("test-token-123")
            );

            // Falls back to GH_TOKEN when GITHUB_TOKEN is absent.
            std::env::remove_var("GITHUB_TOKEN");
            std::env::set_var("GH_TOKEN", "gh-token-456");
            assert_eq!(
                github_token().as_ref().map(SecretString::expose_secret),
                Some("gh-token-456")
            );

            // Returns None when neither is set.
            std::env::remove_var("GITHUB_TOKEN");
            std::env::remove_var("GH_TOKEN");
            assert!(github_token().is_none());

            // Empty string is treated as absent.
            std::env::set_var("GITHUB_TOKEN", "");
            assert!(github_token().is_none());

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

    #[cfg(unix)]
    #[tokio::test]
    async fn install_to_path_atomically_swaps_symlink() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!(
            "devops-install-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap(); // Safe: test setup.

        let v1 = tmp.join("v1");
        let v2 = tmp.join("v2");
        fs::write(&v1, b"version 1").unwrap(); // Safe: test setup.
        fs::write(&v2, b"version 2").unwrap(); // Safe: test setup.

        let dest = tmp.join("bin").join("devops");

        // First install: no existing link, should succeed.
        install_to_path(&v1, &dest).await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"version 1"); // Safe: test assertion.
        assert!(
            fs::symlink_metadata(&dest) // Safe: test assertion.
                .unwrap()
                .file_type()
                .is_symlink()
        );

        // Second install: overwrites the existing link atomically.
        install_to_path(&v2, &dest).await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"version 2"); // Safe: test assertion.

        // No stale tmp symlink left behind.
        assert!(!dest.with_extension("tmp").exists());

        fs::remove_dir_all(&tmp).ok(); // Safe: test cleanup.
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn install_to_path_copies_and_swaps_on_windows() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!(
            "devops-install-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap(); // Safe: test setup.

        let v1 = tmp.join("v1.exe");
        let v2 = tmp.join("v2.exe");
        fs::write(&v1, b"version 1").unwrap(); // Safe: test setup.
        fs::write(&v2, b"version 2").unwrap(); // Safe: test setup.

        let dest = tmp.join("bin").join("devops.exe");

        // First install: no existing file.
        install_to_path(&v1, &dest).await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"version 1"); // Safe: test assertion.

        // Second install: replaces the prior file.
        install_to_path(&v2, &dest).await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"version 2"); // Safe: test assertion.

        // No stale .exe.old file left behind.
        assert!(!dest.with_extension("exe.old").exists());

        fs::remove_dir_all(&tmp).ok(); // Safe: test cleanup.
    }

    // --- Update lock / rollback tests ---

    /// Returns a unique temp directory for a test, created on disk.
    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "devops-{}-test-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap(); // Safe: test setup.
        dir
    }

    fn sample_lock() -> UpdateLock {
        UpdateLock {
            from_version: "1.0.0".to_string(),
            to_version: "1.0.1".to_string(),
            status: UpdateLockStatus::InProgress,
            // Fixed timestamp so JSON schema tests are deterministic.
            started_at: chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }

    #[tokio::test]
    async fn write_lock_and_read_lock_round_trip() {
        let dir = unique_temp_dir("lock-roundtrip");
        let path = dir.join(".update-lock");
        let original = sample_lock();

        write_lock(&path, &original).await.unwrap();
        let loaded = read_lock(&path).await.unwrap().expect("lock present");

        assert_eq!(loaded, original);

        // No stale `.tmp` sibling left behind by the atomic write.
        assert!(!path.with_extension("tmp").exists());

        std::fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }

    #[tokio::test]
    async fn read_lock_missing_returns_none() {
        let dir = unique_temp_dir("lock-missing");
        let path = dir.join(".update-lock");
        assert!(read_lock(&path).await.unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }

    #[tokio::test]
    async fn delete_lock_is_idempotent() {
        let dir = unique_temp_dir("lock-delete");
        let path = dir.join(".update-lock");

        // Missing file: should still succeed.
        delete_lock(&path).await.unwrap();

        // Present file: gets removed.
        write_lock(&path, &sample_lock()).await.unwrap();
        assert!(path.exists());
        delete_lock(&path).await.unwrap();
        assert!(!path.exists());

        // Second call on the now-missing file: still succeeds.
        delete_lock(&path).await.unwrap();

        std::fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }

    /// Guards against accidental field renames — the on-disk format is a
    /// compatibility boundary (an older binary may read a lock written by a
    /// newer one after rollback).
    #[test]
    fn update_lock_json_schema_is_stable() {
        let lock = sample_lock();
        let json = serde_json::to_string(&lock).unwrap();
        assert_eq!(
            json,
            r#"{"from_version":"1.0.0","to_version":"1.0.1","status":"in_progress","started_at":"2024-01-02T03:04:05Z"}"#
        );

        let committed = UpdateLock {
            status: UpdateLockStatus::Committed,
            ..sample_lock()
        };
        let json = serde_json::to_string(&committed).unwrap();
        assert!(json.contains(r#""status":"committed""#));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recover_from_interrupted_update_rolls_back_in_progress() {
        use std::fs;

        let dir = unique_temp_dir("recover-inprogress");
        let install_root = dir.clone();
        let versions = install_root.join("versions");
        let v1_dir = versions.join("1.0.0");
        let v2_dir = versions.join("1.0.1");
        fs::create_dir_all(&v1_dir).unwrap(); // Safe: test setup.
        fs::create_dir_all(&v2_dir).unwrap(); // Safe: test setup.
        // v1 has a real binary; v2 is "corrupt" (directory exists, no binary).
        fs::write(v1_dir.join("devops"), b"binary v1").unwrap(); // Safe: test setup.

        // Symlink currently points at the corrupt v2 binary path (which does
        // not exist yet — simulates a swap that happened before extraction
        // completed).
        let bin_dir = install_root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap(); // Safe: test setup.
        let symlink = bin_dir.join("devops");
        std::os::unix::fs::symlink(v2_dir.join("devops"), &symlink).unwrap(); // Safe: test setup.

        // Write the InProgress lock.
        let lock_file = install_root.join(".update-lock");
        let lock = UpdateLock {
            from_version: "1.0.0".to_string(),
            to_version: "1.0.1".to_string(),
            status: UpdateLockStatus::InProgress,
            started_at: chrono::Utc::now(),
        };
        write_lock(&lock_file, &lock).await.unwrap();

        // Execute recovery.
        let report = recover_from_interrupted_update_with_paths(&install_root, &symlink)
            .await
            .unwrap()
            .expect("rollback report expected");

        assert_eq!(report.from_version, "1.0.0");
        assert_eq!(report.to_version, "1.0.1");

        // Lock was cleared.
        assert!(!lock_file.exists());

        // Symlink now resolves to the v1 binary content.
        assert_eq!(fs::read(&symlink).unwrap(), b"binary v1"); // Safe: test assertion.

        fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recover_from_interrupted_update_removes_stale_committed_lock() {
        let dir = unique_temp_dir("recover-committed");
        let install_root = dir.clone();
        let lock_file = install_root.join(".update-lock");

        let lock = UpdateLock {
            status: UpdateLockStatus::Committed,
            ..sample_lock()
        };
        write_lock(&lock_file, &lock).await.unwrap();

        // Any path will do for symlink_dest since Committed path doesn't touch it.
        let fake_symlink = install_root.join("bin").join("devops");
        let report = recover_from_interrupted_update_with_paths(&install_root, &fake_symlink)
            .await
            .unwrap();
        assert!(
            report.is_none(),
            "committed lock should not produce a report"
        );
        assert!(!lock_file.exists());

        std::fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }

    #[tokio::test]
    async fn recover_from_interrupted_update_returns_none_when_no_lock() {
        let dir = unique_temp_dir("recover-empty");
        let fake_symlink = dir.join("bin").join("devops");
        let report = recover_from_interrupted_update_with_paths(&dir, &fake_symlink)
            .await
            .unwrap();
        assert!(report.is_none());
        std::fs::remove_dir_all(&dir).ok(); // Safe: test cleanup.
    }
}
