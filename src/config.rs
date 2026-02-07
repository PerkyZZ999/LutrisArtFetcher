/// Configuration management for lutrisartfetcher.
///
/// Handles loading/saving the TOML config file at `~/.config/lutrisartfetcher/config.toml`
/// and resolving Lutris XDG paths for the database and asset directories.
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};

/// Application configuration persisted as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// `SteamGridDB` API key (Bearer token).
    pub api_key: Option<String>,

    /// Preferred grid dimension string, e.g. `"600x900"`.
    #[serde(default = "default_grid_dimension")]
    pub preferred_grid_dimension: String,

    /// Maximum number of concurrent download tasks.
    #[serde(default = "default_concurrency")]
    pub max_concurrent_downloads: u8,

    /// Filter out NSFW content from results.
    #[serde(default = "default_true")]
    pub nsfw_filter: bool,

    /// Filter out humor-tagged content from results.
    #[serde(default = "default_true")]
    pub humor_filter: bool,

    /// Delay in milliseconds between `SteamGridDB` API requests (rate-limit protection).
    #[serde(default = "default_request_delay")]
    pub request_delay_ms: u64,
}

fn default_grid_dimension() -> String {
    "600x900".to_owned()
}

const fn default_concurrency() -> u8 {
    3
}

const fn default_true() -> bool {
    true
}

const fn default_request_delay() -> u64 {
    100
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            preferred_grid_dimension: default_grid_dimension(),
            max_concurrent_downloads: default_concurrency(),
            nsfw_filter: true,
            humor_filter: true,
            request_delay_ms: default_request_delay(),
        }
    }
}

impl Config {
    /// Load configuration from disk. Creates a default config file if none exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be created or the file cannot be
    /// read/parsed.
    pub fn load() -> Result<Self> {
        let path = config_path();

        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .wrap_err_with(|| format!("Failed to read config at {}", path.display()))?;

            // Tolerate partially valid TOML — missing fields fall back to defaults via serde
            let config: Self = toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!(
                    "Warning: config file at {} is malformed ({e}), using defaults",
                    path.display()
                );
                Self::default()
            });

            Ok(config)
        } else {
            let config = Self::default();
            // Best-effort save; don't fail startup if we can't write
            if let Err(e) = config.save() {
                eprintln!("Warning: could not write default config: {e}");
            }
            Ok(config)
        }
    }

    /// Persist the current configuration to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be created or the file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err("Failed to create config directory")?;
        }

        let content = toml::to_string_pretty(self)
            .wrap_err("Failed to serialize config")?;

        std::fs::write(&path, content)
            .wrap_err_with(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// XDG path helpers
// ---------------------------------------------------------------------------

/// Directory for our config files: `$XDG_CONFIG_HOME/lutrisartfetcher/`
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .expect("Cannot determine home directory")
                .join(".config")
        })
        .join("lutrisartfetcher")
}

/// Full path to the TOML config file.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Lutris XDG data directory: `$XDG_DATA_HOME/lutris/`
pub fn lutris_data_dir() -> Result<PathBuf> {
    let data = dirs::data_dir()
        .ok_or_else(|| eyre!("Cannot determine XDG data directory"))?;
    Ok(data.join("lutris"))
}

/// Path to the Lutris `SQLite` database.
pub fn lutris_db_path() -> Result<PathBuf> {
    Ok(lutris_data_dir()?.join("pga.db"))
}

/// Resolve the Lutris on-disk directory for a given asset type name.
///
/// `subdir` is one of: `"banners"`, `"coverart"`, `"heroes"`, `"logos"`.
pub fn lutris_asset_dir(subdir: &str) -> Result<PathBuf> {
    Ok(lutris_data_dir()?.join(subdir))
}

/// Resolve the Lutris icons directory (separate XDG location).
pub fn lutris_icon_dir() -> Result<PathBuf> {
    let data = dirs::data_dir()
        .ok_or_else(|| eyre!("Cannot determine XDG data directory"))?;
    Ok(data.join("icons/hicolor/128x128/apps"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_round_trip() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config.preferred_grid_dimension, deserialized.preferred_grid_dimension);
        assert_eq!(config.max_concurrent_downloads, deserialized.max_concurrent_downloads);
        assert_eq!(config.nsfw_filter, deserialized.nsfw_filter);
        assert_eq!(config.humor_filter, deserialized.humor_filter);
        assert_eq!(config.request_delay_ms, deserialized.request_delay_ms);
        assert!(deserialized.api_key.is_none());
    }

    #[test]
    fn config_partial_toml_fills_defaults() {
        let partial = r#"api_key = "test123""#;
        let config: Config = toml::from_str(partial).unwrap();

        assert_eq!(config.api_key.as_deref(), Some("test123"));
        assert_eq!(config.max_concurrent_downloads, 3);
        assert!(config.nsfw_filter);
    }
}
