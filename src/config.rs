use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub azure_devops: AzureDevOpsConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    #[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct FiltersConfig {
    #[serde(default)]
    pub folders: Vec<String>,
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
