use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub azure_devops: AzureDevOpsConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub filters: FiltersConfig,
    #[serde(default)]
    pub update: UpdateConfig,
}

#[derive(Debug, Deserialize)]
pub struct AzureDevOpsConfig {
    pub organization: String,
    pub project: String,
}

#[derive(Debug, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_secs: u64,
    #[serde(default = "default_log_refresh_interval")]
    pub log_refresh_interval_secs: u64,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_refresh_interval(),
            log_refresh_interval_secs: default_log_refresh_interval(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct FiltersConfig {
    /// Only show definitions under these folder paths (e.g. `["\\Infra", "\\Deploy"]`).
    /// Empty means show all folders.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Only show these specific definition IDs. Empty means show all.
    #[serde(default)]
    pub definition_ids: Vec<u32>,
}

#[derive(Debug, Deserialize)]
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

fn default_refresh_interval() -> u64 {
    30
}

fn default_log_refresh_interval() -> u64 {
    5
}

impl Config {
    pub fn load(path: Option<&PathBuf>) -> Result<Self> {
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

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config from {}", config_path.display()))?;

        Ok(config)
    }

    /// Resolve the config file path, returning whether it exists.
    pub fn resolve_path(cli_path: Option<&PathBuf>) -> Result<(PathBuf, bool)> {
        let path = match cli_path {
            Some(p) => p.clone(),
            None => default_config_path()?,
        };
        let exists = path.exists();
        Ok((path, exists))
    }

    /// Write a minimal config file with the given org and project.
    pub fn write_initial(path: &PathBuf, organization: &str, project: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let contents =
            format!("[azure_devops]\norganization = \"{organization}\"\nproject = \"{project}\"\n");

        std::fs::write(path, &contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }
}

/// Check that Azure CLI (`az`) or Azure Developer CLI (`azd`) is available on PATH.
/// Returns `Ok(())` if at least one is found, or an error with install guidance.
pub fn check_azure_cli() -> Result<()> {
    if which("az") || which("azd") {
        return Ok(());
    }

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
    // Prefer XDG_CONFIG_HOME (~/.config) over platform default (~/Library/Application Support on macOS)
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
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
        assert_eq!(config.display.refresh_interval_secs, 30);
        assert_eq!(config.display.log_refresh_interval_secs, 5);
        assert!(config.filters.folders.is_empty());
        assert!(config.filters.definition_ids.is_empty());
        // Update config defaults
        assert!(config.update.check_for_updates);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[azure_devops]
organization = "myorg"
project = "myproject"

[display]
refresh_interval_secs = 60
log_refresh_interval_secs = 10

[filters]
folders = ["\\Infra", "\\Deploy"]
definition_ids = [42, 99]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.refresh_interval_secs, 60);
        assert_eq!(config.display.log_refresh_interval_secs, 10);
        assert_eq!(config.filters.folders, vec!["\\Infra", "\\Deploy"]);
        assert_eq!(config.filters.definition_ids, vec![42, 99]);
    }

    #[test]
    fn parse_config_missing_azure_devops_fails() {
        let toml = r#"
[display]
refresh_interval_secs = 30
"#;
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn default_config_path_with_xdg_override() {
        let test_dir = "/test/custom/xdg";
        unsafe { std::env::set_var("XDG_CONFIG_HOME", test_dir) };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(
            path,
            PathBuf::from("/test/custom/xdg/pipelines/config.toml")
        );
    }

    #[test]
    fn default_config_path_falls_back_to_home() {
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        let path = default_config_path().unwrap();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("pipelines/config.toml"),
            "Expected path to contain 'pipelines/config.toml', got: {path_str}"
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
}
