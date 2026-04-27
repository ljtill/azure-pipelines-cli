//! TOML configuration loading, validation, and persistence.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The highest config schema version this binary understands.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

const MAX_CONNECTION_VALUE_BYTES: usize = 256;
const MIN_TIMEOUT_SECS: u64 = 1;
const MAX_TIMEOUT_SECS: u64 = 600;

/// Error returned when loading or parsing a config file.
#[derive(Debug)]
pub enum ConfigError {
    /// TOML parsing failed.
    Parse(toml::de::Error),
    /// The config declares a `schema_version` newer than this binary supports.
    SchemaTooNew { found: u32, supported: u32 },
    /// The Azure DevOps organization or project value is invalid.
    InvalidConnectionValue {
        field: &'static str,
        reason: ConnectionValidationReason,
    },
    /// A network timeout value is invalid.
    InvalidTimeoutValue {
        field: &'static str,
        reason: TimeoutValidationReason,
    },
}

/// Explains why an Azure DevOps connection config value is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionValidationReason {
    /// Indicates that the value is empty or contains only whitespace.
    Empty,
    /// Indicates that the value contains a control character.
    ControlCharacter,
    /// Indicates that the value exceeds the maximum allowed byte length.
    TooLong { max_bytes: usize },
}

/// Explains why a timeout config value is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutValidationReason {
    /// Indicates that the value is zero.
    Zero,
    /// Indicates that the value exceeds the maximum allowed seconds.
    TooLarge { max_secs: u64 },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{e}"),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "Config was written by a newer devops (schema v{found}, this binary supports v{supported}). Upgrade the CLI to v2+ or remove the `schema_version` line from your config to reset to defaults."
            ),
            Self::InvalidConnectionValue { field, reason } => {
                write!(
                    f,
                    "Invalid [devops.connection].{field}: {reason}. Set `{field}` to the Azure DevOps {field} name from your dev.azure.com URL."
                )
            }
            Self::InvalidTimeoutValue { field, reason } => {
                write!(f, "Invalid [devops.connection.timeouts].{field}: {reason}.")
            }
        }
    }
}

impl std::fmt::Display for ConnectionValidationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "value must not be empty or whitespace-only"),
            Self::ControlCharacter => write!(f, "value must not contain control characters"),
            Self::TooLong { max_bytes } => write!(f, "value must be {max_bytes} bytes or fewer"),
        }
    }
}

impl std::fmt::Display for TimeoutValidationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero => write!(f, "value must be at least {MIN_TIMEOUT_SECS} second"),
            Self::TooLarge { max_secs } => write!(f, "value must be {max_secs} seconds or fewer"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(e) => Some(e),
            Self::SchemaTooNew { .. }
            | Self::InvalidConnectionValue { .. }
            | Self::InvalidTimeoutValue { .. } => None,
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
    pub devops: DevOpsConfig,
}

// Always deserialize as Some(CURRENT_SCHEMA_VERSION) when the field is missing
// from TOML. Serde can't directly default to `Some(value)` without this helper,
// so the Option wrapper is intentional and not actually unnecessary.
#[allow(clippy::unnecessary_wraps)]
fn default_schema_version() -> Option<u32> {
    Some(CURRENT_SCHEMA_VERSION)
}

/// All app config tables, grouped under the top-level `[devops]` table.
#[derive(Debug, Deserialize, Serialize)]
pub struct DevOpsConfig {
    pub connection: ConnectionConfig,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct ConnectionConfig {
    pub organization: String,
    pub project: String,
    #[serde(default)]
    pub timeouts: ConnectionTimeoutConfig,
}

/// Configures Azure DevOps HTTP request timeouts.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConnectionTimeoutConfig {
    /// Sets the default end-to-end request timeout in seconds.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    /// Sets the TCP connection timeout in seconds.
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    /// Sets the end-to-end timeout for build log downloads in seconds.
    #[serde(default = "default_log_timeout_secs")]
    pub log_timeout_secs: u64,
}

impl Default for ConnectionTimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: default_request_timeout_secs(),
            connect_timeout_secs: default_connect_timeout_secs(),
            log_timeout_secs: default_log_timeout_secs(),
        }
    }
}

impl ConnectionTimeoutConfig {
    /// Returns the configured request timeout.
    pub fn request_timeout(self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }

    /// Returns the configured connection timeout.
    pub fn connect_timeout(self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs)
    }

    /// Returns the configured log download timeout.
    pub fn log_timeout(self) -> Duration {
        Duration::from_secs(self.log_timeout_secs)
    }

    /// Validates every timeout value.
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_timeout_value("request_timeout_secs", self.request_timeout_secs)?;
        validate_timeout_value("connect_timeout_secs", self.connect_timeout_secs)?;
        validate_timeout_value("log_timeout_secs", self.log_timeout_secs)?;
        Ok(())
    }
}

impl ConnectionConfig {
    /// Validates the Azure DevOps organization and project names.
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_connection_value("organization", &self.organization)?;
        validate_connection_value("project", &self.project)?;
        self.timeouts.validate()?;
        Ok(())
    }
}

fn validate_connection_value(field: &'static str, value: &str) -> Result<(), ConfigError> {
    let reason = if value.trim().is_empty() {
        Some(ConnectionValidationReason::Empty)
    } else if value.chars().any(char::is_control) {
        Some(ConnectionValidationReason::ControlCharacter)
    } else if value.len() > MAX_CONNECTION_VALUE_BYTES {
        Some(ConnectionValidationReason::TooLong {
            max_bytes: MAX_CONNECTION_VALUE_BYTES,
        })
    } else {
        None
    };

    reason.map_or(Ok(()), |reason| {
        Err(ConfigError::InvalidConnectionValue { field, reason })
    })
}

fn validate_timeout_value(field: &'static str, value: u64) -> Result<(), ConfigError> {
    let reason = if value == 0 {
        Some(TimeoutValidationReason::Zero)
    } else if value > MAX_TIMEOUT_SECS {
        Some(TimeoutValidationReason::TooLarge {
            max_secs: MAX_TIMEOUT_SECS,
        })
    } else {
        None
    };

    reason.map_or(Ok(()), |reason| {
        Err(ConfigError::InvalidTimeoutValue { field, reason })
    })
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_connect_timeout_secs() -> u64 {
    10
}

fn default_log_timeout_secs() -> u64 {
    60
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
    /// Parses a config from a TOML string and validates schema and connection values.
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
        config.devops.connection.validate()?;
        Ok(config)
    }

    pub async fn load(path: Option<&PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };

        tracing::debug!(path = %config_path.display(), "loading config");

        if !tokio::fs::try_exists(&config_path)
            .await
            .with_context(|| format!("Failed to check config path {}", config_path.display()))?
        {
            tracing::warn!(path = %config_path.display(), "config file not found");
            anyhow::bail!(
                "Config file not found at {}. Create it with:\n\n\
                 [devops.connection]\n\
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
        let exists = tokio::fs::try_exists(&path)
            .await
            .with_context(|| format!("Failed to check config path {}", path.display()))?;
        tracing::debug!(path = %path.display(), exists, "resolved config path");
        Ok((path, exists))
    }

    /// Writes a minimal config file with the given org and project.
    pub async fn write_initial(path: &PathBuf, organization: &str, project: &str) -> Result<()> {
        validate_connection_value("organization", organization)?;
        validate_connection_value("project", project)?;

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
        let mut devops = toml::map::Map::new();

        let mut connection = toml::map::Map::new();
        connection.insert(
            "organization".to_string(),
            toml::Value::String(organization.to_string()),
        );
        connection.insert(
            "project".to_string(),
            toml::Value::String(project.to_string()),
        );
        devops.insert("connection".to_string(), toml::Value::Table(connection));

        let mut filters = toml::map::Map::new();
        filters.insert(
            "pinned_definition_ids".to_string(),
            toml::Value::Array(Vec::new()),
        );
        filters.insert(
            "pinned_work_item_ids".to_string(),
            toml::Value::Array(Vec::new()),
        );
        devops.insert("filters".to_string(), toml::Value::Table(filters));

        table.insert("devops".to_string(), toml::Value::Table(devops));

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

        if !config_path
            .try_exists()
            .with_context(|| format!("Failed to check config path {}", config_path.display()))?
        {
            anyhow::bail!(
                "Config file not found at {}. Create it with:\n\n\
                 [devops.connection]\n\
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
[devops.connection]
organization = "myorg"
project = "myproject"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.devops.connection.organization, "myorg");
        assert_eq!(config.devops.connection.project, "myproject");
        assert_eq!(
            config.devops.connection.timeouts,
            ConnectionTimeoutConfig::default()
        );
        assert!(config.devops.filters.folders.is_empty());
        assert!(config.devops.filters.definition_ids.is_empty());
        // Update config defaults.
        assert!(config.devops.update.check_for_updates);
        // Logging defaults.
        assert_eq!(config.devops.logging.level, "info");
        assert!(config.devops.logging.log_directory.is_none());
        assert_eq!(config.devops.logging.max_log_files, 5);
        // Notifications defaults.
        assert!(config.devops.notifications.enabled);
        // Display defaults.
        assert_eq!(config.devops.display.refresh_interval_secs, 15);
        assert_eq!(config.devops.display.log_refresh_interval_secs, 5);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[devops.connection]
organization = "myorg"
project = "myproject"

[devops.filters]
folders = ["\\Infra", "\\Deploy"]
definition_ids = [42, 99]

[devops.connection.timeouts]
request_timeout_secs = 45
connect_timeout_secs = 15
log_timeout_secs = 120
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.devops.filters.folders, vec!["\\Infra", "\\Deploy"]);
        assert_eq!(config.devops.filters.definition_ids, vec![42, 99]);
        assert_eq!(config.devops.connection.timeouts.request_timeout_secs, 45);
        assert_eq!(config.devops.connection.timeouts.connect_timeout_secs, 15);
        assert_eq!(config.devops.connection.timeouts.log_timeout_secs, 120);
    }

    #[test]
    fn parse_config_missing_connection_fails() {
        let toml = r"
[devops.filters]
folders = []
";
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_empty_connection_values() {
        let toml = r#"
[devops.connection]
organization = "   "
project = "myproject"
"#;
        let err = Config::parse_str(toml).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidConnectionValue {
                field: "organization",
                reason: ConnectionValidationReason::Empty
            }
        ));
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn parse_rejects_control_characters_in_connection_values() {
        let toml = r#"
[devops.connection]
organization = "myorg"
project = "my\u0007project"
"#;
        let err = Config::parse_str(toml).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidConnectionValue {
                field: "project",
                reason: ConnectionValidationReason::ControlCharacter
            }
        ));
        assert!(err.to_string().contains("control characters"));
    }

    #[test]
    fn parse_rejects_pathologically_long_connection_values() {
        let organization = "o".repeat(MAX_CONNECTION_VALUE_BYTES + 1);
        let toml = format!(
            r#"
[devops.connection]
organization = "{organization}"
project = "myproject"
"#
        );
        let err = Config::parse_str(&toml).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidConnectionValue {
                field: "organization",
                reason: ConnectionValidationReason::TooLong {
                    max_bytes: MAX_CONNECTION_VALUE_BYTES
                }
            }
        ));
        assert!(err.to_string().contains("bytes or fewer"));
    }

    #[test]
    fn parse_config_with_connection_timeouts() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.connection.timeouts]
request_timeout_secs = 45
connect_timeout_secs = 12
log_timeout_secs = 180
"#;
        let config = Config::parse_str(toml).unwrap();

        assert_eq!(config.devops.connection.timeouts.request_timeout_secs, 45);
        assert_eq!(config.devops.connection.timeouts.connect_timeout_secs, 12);
        assert_eq!(config.devops.connection.timeouts.log_timeout_secs, 180);
    }

    #[test]
    fn parse_rejects_zero_timeout_values() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.connection.timeouts]
request_timeout_secs = 0
"#;
        let err = Config::parse_str(toml).unwrap_err();

        assert!(matches!(
            err,
            ConfigError::InvalidTimeoutValue {
                field: "request_timeout_secs",
                reason: TimeoutValidationReason::Zero
            }
        ));
        assert!(err.to_string().contains("at least 1 second"));
    }

    #[test]
    fn parse_rejects_pathologically_large_timeout_values() {
        let toml = format!(
            r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.connection.timeouts]
log_timeout_secs = {}
"#,
            MAX_TIMEOUT_SECS + 1
        );
        let err = Config::parse_str(&toml).unwrap_err();

        assert!(matches!(
            err,
            ConfigError::InvalidTimeoutValue {
                field: "log_timeout_secs",
                reason: TimeoutValidationReason::TooLarge {
                    max_secs: MAX_TIMEOUT_SECS
                }
            }
        ));
        assert!(err.to_string().contains("seconds or fewer"));
    }

    #[tokio::test]
    async fn write_initial_rejects_invalid_connection_values() {
        let path = PathBuf::from("target/should-not-write-invalid-config.toml");
        let err = Config::write_initial(&path, "", "project")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("[devops.connection].organization"));
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
[devops.connection]
organization = "org"
project = "proj"

[devops.filters]
folders = []
definition_ids = []
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.devops.filters.folders.is_empty());
        assert!(config.devops.filters.definition_ids.is_empty());
    }

    #[test]
    fn parse_config_with_update_section() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.update]
check_for_updates = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.devops.update.check_for_updates);
    }

    #[test]
    fn parse_config_update_defaults_to_true() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.devops.update.check_for_updates);
    }

    #[test]
    fn parse_config_with_logging_level() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.logging]
level = "debug"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.devops.logging.level, "debug");
    }

    #[test]
    fn parse_config_notifications_defaults_to_enabled() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.devops.notifications.enabled);
    }

    #[test]
    fn parse_config_with_notifications_disabled() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.notifications]
enabled = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.devops.notifications.enabled);
    }

    #[test]
    fn parse_config_with_display_section() {
        let toml = r#"
[devops.connection]
organization = "org"
project = "proj"

[devops.display]
refresh_interval_secs = 30
log_refresh_interval_secs = 10
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.devops.display.refresh_interval_secs, 30);
        assert_eq!(config.devops.display.log_refresh_interval_secs, 10);
    }

    #[test]
    fn config_round_trip() {
        let toml_input = r#"
[devops.connection]
organization = "myorg"
project = "myproject"

[devops.filters]
folders = ["\\Infra", "\\Deploy"]
definition_ids = [42, 99]

[devops.update]
check_for_updates = false

[devops.connection.timeouts]
request_timeout_secs = 45
connect_timeout_secs = 12
log_timeout_secs = 180

[devops.logging]
level = "debug"

[devops.notifications]
enabled = false

[devops.display]
refresh_interval_secs = 30
log_refresh_interval_secs = 10
"#;
        let config: Config = toml::from_str(toml_input).unwrap();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let config2: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config2.devops.connection.organization, "myorg");
        assert_eq!(config2.devops.connection.project, "myproject");
        assert_eq!(config2.devops.filters.folders, vec!["\\Infra", "\\Deploy"]);
        assert_eq!(config2.devops.filters.definition_ids, vec![42, 99]);
        assert!(!config2.devops.update.check_for_updates);
        assert_eq!(config2.devops.connection.timeouts.request_timeout_secs, 45);
        assert_eq!(config2.devops.connection.timeouts.connect_timeout_secs, 12);
        assert_eq!(config2.devops.connection.timeouts.log_timeout_secs, 180);
        assert_eq!(config2.devops.logging.level, "debug");
        assert!(!config2.devops.notifications.enabled);
        assert_eq!(config2.devops.display.refresh_interval_secs, 30);
        assert_eq!(config2.devops.display.log_refresh_interval_secs, 10);
    }

    #[tokio::test]
    async fn config_save_and_reload() {
        let dir = std::env::temp_dir().join("devops-test-save-config");
        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        let config: Config = toml::from_str(
            r#"
[devops.connection]
organization = "save-org"
project = "save-proj"

[devops.display]
refresh_interval_secs = 60
log_refresh_interval_secs = 20
"#,
        )
        .unwrap();

        config.save(&path).await.unwrap();
        let reloaded = Config::load(Some(&path)).await.unwrap();
        assert_eq!(reloaded.devops.connection.organization, "save-org");
        assert_eq!(reloaded.devops.display.refresh_interval_secs, 60);
        assert_eq!(reloaded.devops.display.log_refresh_interval_secs, 20);

        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_without_schema_version_loads_as_v1() {
        let toml = r#"
[devops.connection]
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

[devops.connection]
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

[devops.connection]
organization = "myorg"
project = "myproject"
"#;
        let err = Config::parse_str(toml).unwrap_err();
        match err {
            ConfigError::SchemaTooNew { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, 1);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn load_accepts_current_schema_version() {
        let toml = r#"
schema_version = 1

[devops.connection]
organization = "myorg"
project = "myproject"
"#;
        let config = Config::parse_str(toml).expect("current schema version should load");
        assert_eq!(config.schema_version, Some(1));
    }

    #[test]
    fn load_accepts_missing_schema_version() {
        let toml = r#"
[devops.connection]
organization = "myorg"
project = "myproject"
"#;
        let config = Config::parse_str(toml).expect("missing schema version should load");
        assert_eq!(config.schema_version, Some(CURRENT_SCHEMA_VERSION));
    }
}
