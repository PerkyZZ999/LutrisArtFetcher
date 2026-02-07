/// Serde structs for `SteamGridDB` API v2 responses and local enums.
use std::fmt;
use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// API response wrapper
// ---------------------------------------------------------------------------

/// Generic API response envelope from `SteamGridDB`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Vec<T>,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// A game result returned by the `/search/autocomplete` endpoint.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SearchResult {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub verified: bool,
}

// ---------------------------------------------------------------------------
// Grid / Hero / Logo / Icon images
// ---------------------------------------------------------------------------

/// A single image asset returned by any of the grid/hero/logo/icon endpoints.
///
/// The response schema is identical across asset types, so we reuse one struct.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ImageAsset {
    pub id: u64,
    #[serde(default)]
    pub score: i32,
    #[serde(default)]
    pub style: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub humor: bool,
    #[serde(default)]
    pub mime: String,
    pub url: String,
    #[serde(default)]
    pub thumb: String,
}

// ---------------------------------------------------------------------------
// Asset types
// ---------------------------------------------------------------------------

/// The four categories of visual assets we can download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetType {
    Grid,
    Hero,
    Logo,
    Icon,
}

impl AssetType {
    /// The `SteamGridDB` API path segment for this asset type.
    pub fn api_path(self) -> &'static str {
        match self {
            Self::Grid => "grids",
            Self::Hero => "heroes",
            Self::Logo => "logos",
            Self::Icon => "icons",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::Hero => "Hero",
            Self::Logo => "Logo",
            Self::Icon => "Icon",
        }
    }

    /// The sub-directory under `$XDG_DATA_HOME/lutris/` for this asset type.
    /// Icons use a completely different base path — handled separately.
    pub fn lutris_subdir(self) -> &'static str {
        match self {
            Self::Grid => "coverart",
            Self::Hero => "heroes",
            Self::Logo => "logos",
            Self::Icon => "icons", // not used directly — see `asset_path()`
        }
    }

    /// All supported asset types.
    pub fn all() -> &'static [Self] {
        &[Self::Grid, Self::Hero, Self::Logo, Self::Icon]
    }
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Parse an asset type from a CLI string (case-insensitive).
impl std::str::FromStr for AssetType {
    type Err = color_eyre::eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "grid" | "grids" => Ok(Self::Grid),
            "hero" | "heroes" => Ok(Self::Hero),
            "logo" | "logos" => Ok(Self::Logo),
            "icon" | "icons" => Ok(Self::Icon),
            _ => Err(eyre!("Unknown asset type: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Download status tracking
// ---------------------------------------------------------------------------

/// Tracks the state of a single asset download.
#[derive(Debug, Clone)]
pub enum DownloadStatus {
    /// Not yet started.
    Pending,
    /// Searching for the game on `SteamGridDB`.
    Searching,
    /// Downloading the image bytes.
    Downloading,
    /// Successfully saved to disk.
    Done(PathBuf),
    /// Skipped (e.g. file already exists).
    Skipped(String),
    /// Failed with an error message.
    Failed(String),
}

impl DownloadStatus {
    /// Whether this status represents a terminal (finished) state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Skipped(_) | Self::Failed(_))
    }

    /// Status icon for the TUI.
    #[allow(dead_code)]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Pending => "·",
            Self::Searching => "⟳",
            Self::Downloading => "↓",
            Self::Done(_) => "✓",
            Self::Skipped(_) => "─",
            Self::Failed(_) => "✗",
        }
    }
}

// ---------------------------------------------------------------------------
// Progress message sent from download tasks to the TUI
// ---------------------------------------------------------------------------

/// A progress update for a single asset download, sent through the event channel.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub game_slug: String,
    pub asset_type: AssetType,
    pub status: DownloadStatus,
}
