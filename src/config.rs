//! TOML configuration loading, validation, and persistence.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The highest config schema version this binary understands.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Error returned when loading or parsing a config file.
#[derive(Debug)]
pub enum ConfigError {
    /// TOML parsing failed.
    Parse(toml::de::Error),
    /// The config declares a `schema_version` newer than this binary supports.
    SchemaTooNew { found: u32, supported: u32 },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{e}"),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "Config was written by a newer devops (schema v{found}, this binary supports v{supported}). Upgrade the CLI to v2+ or remove the `schema_version` line from your config to reset to defaults."
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(e) => Some(e),
            Self::SchemaTooNew { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Optional config schema version. The current binary understands
    /// [`CURRENT_SCHEMA_VERSION`]. Older configs without this field are
    /// treated as the current version. A value greater than
    /// [`CURRENT_SCHEMA_VERSION`] is rejected at load time.
    #[serde(
        default = "default_schema_version",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema_version: Option<u32>,
    pub azure_devops: AzureDevOpsConfig,
    #[serde(default)]
    pub filters: FiltersConfig,
    #[serde(default)]
    pub update: UpdateConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub display: DisplayConfig,
}

// Always deserialize as Some(CURRENT_SCHEMA_VERSION) when the field is missing
// from TOML. Serde can't directly default to `Some(value)` without this helper,
// so the Option wrapper is intentional and not actually unnecessary.
#[allow(clippy::unnecessary_wraps)]
fn default_schema_version() -> Option<u32> {
    Some(CURRENT_SCHEMA_VERSION)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AzureDevOpsConfig {
    pub organization: String,
    pub project: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FiltersConfig {
    /// Shows only definitions under these folder paths (e.g. `["\\Infra", "\\Deploy"]`).
    /// Defaults to all folders when empty.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Shows only these specific definition IDs. Defaults to all when empty.
    #[serde(default)]
    pub definition_ids: Vec<u32>,
    /// Pipeline definition IDs pinned to the Dashboard.
    #[serde(default)]
    pub pinned_definition_ids: Vec<u32>,
    /// Work item IDs pinned to the Dashboard.
    #[serde(default)]
    pub pinned_work_item_ids: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateConfig {
    #[serde(default = "default_check_for_updates")]
    pub check_for_updates: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_for_updates: default_check_for_updates(),
        }
    }
}

fn default_check_for_updates() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Specifies the log file directory. Defaults to `~/.local/state/devops`.
    #[serde(default)]
    pub log_directory: Option<String>,
    /// Limits the number of daily log files retained. Defaults to 5.
    #[serde(default = "default_max_log_files")]
    pub max_log_files: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            log_directory: None,
            max_log_files: default_max_log_files(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_max_log_files() -> usize {
    5
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotificationsConfig {
    #[serde(default = "default_notifications_enabled")]
    pub enabled: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_notifications_enabled(),
        }
    }
}

fn default_notifications_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DisplayConfig {
    #[serde(default = "default_refresh_interval_secs")]
    pub refresh_interval_secs: u64,
    #[serde(default = "default_log_refresh_interval_secs")]
    pub log_refresh_interval_secs: u64,
    /// Caps the number of log lines held in memory by the log viewer.
    /// Clamped to at least [`crate::shared::log_buffer::MIN_CAPACITY`] on load.
    #[serde(default = "default_max_log_lines")]
    pub max_log_lines: usize,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_refresh_interval_secs(),
            log_refresh_interval_secs: default_log_refresh_interval_secs(),
            max_log_lines: default_max_log_lines(),
        }
    }
}

fn default_refresh_interval_secs() -> u64 {
    15
}

fn default_log_refresh_interval_secs() -> u64 {
    5
}

fn default_max_log_lines() -> usize {
    crate::shared::log_buffer::DEFAULT_CAPACITY
}

impl Config {
    /// Parses a config from a TOML string and enforces `schema_version`
    /// compatibility. Unknown top-level fields other than `schema_version`
    /// remain non-fatal (serde's default behavior) — only a `schema_version`
    /// higher than [`CURRENT_SCHEMA_VERSION`] is rejected.
    pub fn parse_str(s: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(s).map_err(ConfigError::Parse)?;
        if let Some(found) = config.schema_version
            && found > CURRENT_SCHEMA_VERSION
        {
            return Err(ConfigError::SchemaTooNew {
                found,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(config)
    }

    pub async fn load(path: Option<&PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };

        tracing::debug!(path = %config_path.display(), "loading config");

        if !tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
            tracing::warn!(path = %config_path.display(), "config file not found");
            anyhow::bail!(
                "Config file not found at {}. Create it with:\n\n\
                 [azure_devops]\n\
                 organization = \"your-org\"\n\
                 project = \"your-project\"\n",
                config_path.display()
            );
        }

        let contents = tokio::fs::read_to_string(&config_path)
            .await
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        let config = Self::parse_str(&contents)
            .with_context(|| format!("Failed to parse config from {}", config_path.display()))?;

        tracing::debug!(path = %config_path.display(), "config loaded");
        Ok(config)
    }

    /// Resolves the config file path, returning whether it exists.
    pub async fn resolve_path(cli_path: Option<&PathBuf>) -> Result<(PathBuf, bool)> {
        let path = match cli_path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };
        let exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
        tracing::debug!(path = %path.display(), exists, "resolved config path");
        Ok((path, exists))
    }

    /// Writes a minimal config file with the given org and project.
    pub async fn write_initial(path: &PathBuf, organization: &str, project: &str) -> Result<()> {
        tracing::info!(
            path = %path.display(),
            organization,
            project,
            "writing initial config"
        );
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let mut table = toml::map::Map::new();
        let mut ado = toml::map::Map::new();
        ado.insert(
            "organization".to_string(),
            toml::Value::String(organization.to_string()),
        );
        ado.insert(
            "project".to_string(),
            toml::Value::String(project.to_string()),
        );
        table.insert("azure_devops".to_string(), toml::Value::Table(ado));

        let mut filters = toml::map::Map::new();
        filters.insert(
            "pinned_definition_ids".to_string(),
            toml::Value::Array(Vec::new()),
        );
        filters.insert(
            "pinned_work_item_ids".to_string(),
            toml::Value::Array(Vec::new()),
        );
        table.insert("filters".to_string(), toml::Value::Table(filters));

        let contents = toml::to_string_pretty(&toml::Value::Table(table))
            .context("Failed to serialize config")?;

        tokio::fs::write(path, &contents)
            .await
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }

    /// Serializes the full config and writes it to the given path.
    pub async fn save(&self, path: &PathBuf) -> Result<()> {
        tracing::info!(path = %path.display(), "saving config");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config for save")?;

        tokio::fs::write(path, &contents)
            .await
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }

    /// Blocking wrapper around [`Config::save`] for use from synchronous event
    /// handlers that already run inside the tokio multi-thread runtime.
    ///
    /// Uses `block_in_place` so the current worker thread is temporarily
    /// repurposed for blocking, allowing other tasks to migrate away while the
    /// async save runs to completion. The actual filesystem I/O still occurs
    /// on tokio's blocking pool (via `tokio::fs`).
    ///
    /// Outside a tokio runtime (e.g. in unit tests), falls back to synchronous
    /// `std::fs` so callers don't have to care which context they're in.
    pub fn save_blocking(&self, path: &PathBuf) -> Result<()> {
        tokio::runtime::Handle::try_current().map_or_else(
            |_| self.save_sync(path),
            |handle| tokio::task::block_in_place(|| handle.block_on(self.save(path))),
        )
    }

    /// Blocking wrapper around [`Config::load`]. See [`Config::save_blocking`].
    pub fn load_blocking(path: Option<&PathBuf>) -> Result<Self> {
        tokio::runtime::Handle::try_current().map_or_else(
            |_| Self::load_sync(path),
            |handle| tokio::task::block_in_place(|| handle.block_on(Self::load(path))),
        )
    }

    // Safe: invoked only via `load_blocking` when no tokio runtime is active
    // (e.g. unit tests that construct an `App` without spinning up a runtime).
    fn load_sync(path: Option<&PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };

        if !config_path.exists() {
            anyhow::bail!(
                "Config file not found at {}. Create it with:\n\n\
                 [azure_devops]\n\
                 organization = \"your-org\"\n\
                 project = \"your-project\"\n",
                config_path.display()
            );
        }

        let contents = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
        Self::parse_str(&contents)
            .with_context(|| format!("Failed to parse config from {}", config_path.display()))
    }

    // Safe: invoked only via `save_blocking` when no tokio runtime is active.
    fn save_sync(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }
        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config for save")?;
        std::fs::write(path, &contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))
    }
}

/// Checks that Azure CLI (`az`) or Azure Developer CLI (`azd`) is available on PATH.
/// Returns `Ok(())` if at least one is found, or an error with install guidance.
pub fn check_azure_cli() -> Result<()> {
    if which("az") {
        tracing::debug!(cli = "az", "Azure CLI found");
        return Ok(());
    }
    if which("azd") {
        tracing::debug!(cli = "azd", "Azure Developer CLI found");
        return Ok(());
    }

    tracing::warn!("neither az nor azd found on PATH");
    anyhow::bail!(
        "Azure CLI or Azure Developer CLI is required for authentication.\n\n\
         Install one of the following:\n\
         • Azure CLI:           https://aka.ms/install-azure-cli\n\
         • Azure Developer CLI: https://aka.ms/install-azd\n\n\
         Then sign in with `az login` or `azd auth login`."
    );
}

fn which(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn default_config_path() -> Result<PathBuf> {
    default_config_path_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        dirs::home_dir(),
    )
}

fn default_config_path_from(
    xdg_config_home: Option<PathBuf>,
    home_dir: Option<PathBuf>,
) -> Result<PathBuf> {
    // Prefers XDG_CONFIG_HOME (~/.config) over platform default (~/Library/Application Support on macOS).
    let config_dir = xdg_config_home
        .filter(|p| p.is_absolute())
        .or_else(|| home_dir.map(|h| h.join(".config")))
        .context("Could not determine config directory")?;

    Ok(config_dir.join("devops").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.azure_devops.organization, "myorg");
        assert_eq!(config.azure_devops.project, "myproject");
        assert!(config.filters.folders.is_empty());
        assert!(config.filters.definition_ids.is_empty());
        // Update config defaults.
        assert!(config.update.check_for_updates);
        // Logging defaults.
        assert_eq!(config.logging.level, "info");
        assert!(config.logging.log_directory.is_none());
        assert_eq!(config.logging.max_log_files, 5);
        // Notifications defaults.
        assert!(config.notifications.enabled);
        // Display defaults.
        assert_eq!(config.display.refresh_interval_secs, 15);
        assert_eq!(config.display.log_refresh_interval_secs, 5);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[azure_devops]
organization = "myorg"
project = "myproject"

[filters]
folders = ["\\Infra", "\\Deploy"]
definition_ids = [42, 99]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.filters.folders, vec!["\\Infra", "\\Deploy"]);
        assert_eq!(config.filters.definition_ids, vec![42, 99]);
    }

    #[test]
    fn parse_config_missing_azure_devops_fails() {
        let toml = r"
[filters]
folders = []
";
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn default_config_path_with_xdg_override() {
        #[cfg(unix)]
        let test_dir = "/test/custom/xdg";
        #[cfg(windows)]
        let test_dir = "C:\\test\\custom\\xdg";

        let path = default_config_path_from(Some(PathBuf::from(test_dir)), None).unwrap();
        let expected = PathBuf::from(test_dir).join("devops").join("config.toml");
        assert_eq!(path, expected);
    }

    #[test]
    fn default_config_path_falls_back_to_home() {
        #[cfg(unix)]
        let home = PathBuf::from("/test/home");
        #[cfg(windows)]
        let home = PathBuf::from("C:\\test\\home");

        let path = default_config_path_from(None, Some(home.clone())).unwrap();
        assert_eq!(
            path,
            home.join(".config").join("devops").join("config.toml")
        );
    }

    #[test]
    fn parse_config_empty_filters() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"

[filters]
folders = []
definition_ids = []
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.filters.folders.is_empty());
        assert!(config.filters.definition_ids.is_empty());
    }

    #[test]
    fn parse_config_with_update_section() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"

[update]
check_for_updates = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.update.check_for_updates);
    }

    #[test]
    fn parse_config_update_defaults_to_true() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.update.check_for_updates);
    }

    #[test]
    fn parse_config_with_logging_level() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"

[logging]
level = "debug"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn parse_config_notifications_defaults_to_enabled() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.notifications.enabled);
    }

    #[test]
    fn parse_config_with_notifications_disabled() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"

[notifications]
enabled = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.notifications.enabled);
    }

    #[test]
    fn parse_config_with_display_section() {
        let toml = r#"
[azure_devops]
organization = "org"
project = "proj"

[display]
refresh_interval_secs = 30
log_refresh_interval_secs = 10
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.refresh_interval_secs, 30);
        assert_eq!(config.display.log_refresh_interval_secs, 10);
    }

    #[test]
    fn config_round_trip() {
        let toml_input = r#"
[azure_devops]
organization = "myorg"
project = "myproject"

[filters]
folders = ["\\Infra", "\\Deploy"]
definition_ids = [42, 99]

[update]
check_for_updates = false

[logging]
level = "debug"

[notifications]
enabled = false

[display]
refresh_interval_secs = 30
log_refresh_interval_secs = 10
"#;
        let config: Config = toml::from_str(toml_input).unwrap();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let config2: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config2.azure_devops.organization, "myorg");
        assert_eq!(config2.azure_devops.project, "myproject");
        assert_eq!(config2.filters.folders, vec!["\\Infra", "\\Deploy"]);
        assert_eq!(config2.filters.definition_ids, vec![42, 99]);
        assert!(!config2.update.check_for_updates);
        assert_eq!(config2.logging.level, "debug");
        assert!(!config2.notifications.enabled);
        assert_eq!(config2.display.refresh_interval_secs, 30);
        assert_eq!(config2.display.log_refresh_interval_secs, 10);
    }

    #[tokio::test]
    async fn config_save_and_reload() {
        let dir = std::env::temp_dir().join("devops-test-save-config");
        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        let config: Config = toml::from_str(
            r#"
[azure_devops]
organization = "save-org"
project = "save-proj"

[display]
refresh_interval_secs = 60
log_refresh_interval_secs = 20
"#,
        )
        .unwrap();

        config.save(&path).await.unwrap();
        let reloaded = Config::load(Some(&path)).await.unwrap();
        assert_eq!(reloaded.azure_devops.organization, "save-org");
        assert_eq!(reloaded.display.refresh_interval_secs, 60);
        assert_eq!(reloaded.display.log_refresh_interval_secs, 20);

        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_without_schema_version_loads_as_v1() {
        let toml = r#"
[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.schema_version, Some(CURRENT_SCHEMA_VERSION));
    }

    #[test]
    fn config_with_explicit_schema_version_round_trips() {
        let toml = r#"
schema_version = 1

[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.schema_version, Some(1));
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("schema_version = 1"));
    }

    #[test]
    fn load_rejects_newer_schema_version() {
        let toml = r#"
schema_version = 99

[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let err = Config::parse_str(toml).unwrap_err();
        match err {
            ConfigError::SchemaTooNew { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, 1);
            }
            other @ ConfigError::Parse(_) => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn load_accepts_current_schema_version() {
        let toml = r#"
schema_version = 1

[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let config = Config::parse_str(toml).expect("current schema version should load");
        assert_eq!(config.schema_version, Some(1));
    }

    #[test]
    fn load_accepts_missing_schema_version() {
        let toml = r#"
[azure_devops]
organization = "myorg"
project = "myproject"
"#;
        let config = Config::parse_str(toml).expect("missing schema version should load");
        assert_eq!(config.schema_version, Some(CURRENT_SCHEMA_VERSION));
    }
}
