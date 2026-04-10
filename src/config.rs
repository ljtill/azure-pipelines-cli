use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
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

#[derive(Debug, Deserialize, Serialize)]
pub struct AzureDevOpsConfig {
    pub organization: String,
    pub project: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FiltersConfig {
    /// Only show definitions under these folder paths (e.g. `["\\Infra", "\\Deploy"]`).
    /// Empty means show all folders.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Only show these specific definition IDs. Empty means show all.
    #[serde(default)]
    pub definition_ids: Vec<u32>,
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
    /// Directory for log files. Defaults to `~/.local/state/pipelines`.
    #[serde(default)]
    pub log_directory: Option<String>,
    /// Maximum number of daily log files to retain. Defaults to 5.
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
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_refresh_interval_secs(),
            log_refresh_interval_secs: default_log_refresh_interval_secs(),
        }
    }
}

fn default_refresh_interval_secs() -> u64 {
    15
}

fn default_log_refresh_interval_secs() -> u64 {
    5
}

impl Config {
    pub fn load(path: Option<&PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };

        tracing::debug!(path = %config_path.display(), "loading config");

        if !config_path.exists() {
            tracing::warn!(path = %config_path.display(), "config file not found");
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

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config from {}", config_path.display()))?;

        tracing::debug!(path = %config_path.display(), "config loaded");
        Ok(config)
    }

    /// Resolve the config file path, returning whether it exists.
    pub fn resolve_path(cli_path: Option<&PathBuf>) -> Result<(PathBuf, bool)> {
        let path = match cli_path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };
        let exists = path.exists();
        tracing::debug!(path = %path.display(), exists, "resolved config path");
        Ok((path, exists))
    }

    /// Write a minimal config file with the given org and project.
    pub fn write_initial(path: &PathBuf, organization: &str, project: &str) -> Result<()> {
        tracing::info!(
            path = %path.display(),
            organization,
            project,
            "writing initial config"
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
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
        let contents = toml::to_string_pretty(&toml::Value::Table(table))
            .context("Failed to serialize config")?;

        std::fs::write(path, &contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }

    /// Serialize the full config and write it to the given path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        tracing::info!(path = %path.display(), "saving config");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config for save")?;

        std::fs::write(path, &contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }
}

/// Check that Azure CLI (`az`) or Azure Developer CLI (`azd`) is available on PATH.
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
    // Prefer XDG_CONFIG_HOME (~/.config) over platform default (~/Library/Application Support on macOS)
    let config_dir = xdg_config_home
        .filter(|p| p.is_absolute())
        .or_else(|| home_dir.map(|h| h.join(".config")))
        .context("Could not determine config directory")?;

    Ok(config_dir.join("pipelines").join("config.toml"))
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
        // Update config defaults
        assert!(config.update.check_for_updates);
        // Logging defaults
        assert_eq!(config.logging.level, "info");
        assert!(config.logging.log_directory.is_none());
        assert_eq!(config.logging.max_log_files, 5);
        // Notifications defaults
        assert!(config.notifications.enabled);
        // Display defaults
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
        let toml = r#"
[filters]
folders = []
"#;
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
        let expected = PathBuf::from(test_dir)
            .join("pipelines")
            .join("config.toml");
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
            home.join(".config").join("pipelines").join("config.toml")
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

    #[test]
    fn config_save_and_reload() {
        let dir = std::env::temp_dir().join("pipelines-test-save-config");
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

        config.save(&path).unwrap();
        let reloaded = Config::load(Some(&path)).unwrap();
        assert_eq!(reloaded.azure_devops.organization, "save-org");
        assert_eq!(reloaded.display.refresh_interval_secs, 60);
        assert_eq!(reloaded.display.log_refresh_interval_secs, 20);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
