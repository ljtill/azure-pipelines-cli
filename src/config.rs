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
}

pub fn default_config_path() -> Result<PathBuf> {
    // Prefer XDG_CONFIG_HOME (~/.config) over platform default (~/Library/Application Support on macOS)
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .context("Could not determine config directory")?;

    Ok(config_dir.join("azure-pipelines-cli").join("config.toml"))
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
}
