/// Download orchestration — resolves game IDs, fetches assets, saves images.
///
/// Each download task sends progress updates through an `mpsc` channel so the
/// TUI can display real-time status.
use std::collections::HashSet;
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result};
use tokio::sync::{Semaphore, mpsc};

use crate::api::models::{AssetType, DownloadProgress, DownloadStatus, ImageAsset};
use crate::api::SteamGridDbClient;
use crate::config;
use crate::db::Game;

/// Entry combining a game and per-asset download status.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GameEntry {
    pub game: Game,
    pub grid_status: DownloadStatus,
    pub hero_status: DownloadStatus,
    pub logo_status: DownloadStatus,
    pub icon_status: DownloadStatus,
    /// Cached `SteamGridDB` game ID after first successful search.
    pub steamgriddb_id: Option<u64>,
}

impl GameEntry {
    pub fn new(game: Game) -> Self {
        Self {
            game,
            grid_status: DownloadStatus::Pending,
            hero_status: DownloadStatus::Pending,
            logo_status: DownloadStatus::Pending,
            icon_status: DownloadStatus::Pending,
            steamgriddb_id: None,
        }
    }

    /// Get a mutable reference to the status field for a given asset type.
    pub fn status_mut(&mut self, asset: AssetType) -> &mut DownloadStatus {
        match asset {
            AssetType::Grid => &mut self.grid_status,
            AssetType::Hero => &mut self.hero_status,
            AssetType::Logo => &mut self.logo_status,
            AssetType::Icon => &mut self.icon_status,
        }
    }

    /// Get a reference to the status field for a given asset type.
    pub fn status(&self, asset: AssetType) -> &DownloadStatus {
        match asset {
            AssetType::Grid => &self.grid_status,
            AssetType::Hero => &self.hero_status,
            AssetType::Logo => &self.logo_status,
            AssetType::Icon => &self.icon_status,
        }
    }

    /// Returns the most representative icon for TUI display based on all active asset statuses.
    pub fn overall_icon(&self, active_assets: &HashSet<AssetType>) -> &'static str {
        let statuses: Vec<&DownloadStatus> = active_assets
            .iter()
            .map(|a| self.status(*a))
            .collect();

        // Any downloading? Show downloading
        if statuses.iter().any(|s| matches!(s, DownloadStatus::Downloading | DownloadStatus::Searching)) {
            return "↓";
        }
        // Any failed? Show failed
        if statuses.iter().any(|s| matches!(s, DownloadStatus::Failed(_))) {
            return "✗";
        }
        // All done or skipped? Show done
        if statuses.iter().all(|s| matches!(s, DownloadStatus::Done(_) | DownloadStatus::Skipped(_))) {
            return "✓";
        }
        // Otherwise pending
        "·"
    }
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Resolve the full filesystem path where an asset should be saved.
pub fn asset_path(asset: AssetType, slug: &str) -> Result<PathBuf> {
    if asset == AssetType::Icon {
        let dir = config::lutris_icon_dir()?;
        Ok(dir.join(format!("lutris_{slug}.png")))
    } else {
        let dir = config::lutris_asset_dir(asset.lutris_subdir())?;
        Ok(dir.join(format!("{slug}.jpg")))
    }
}

/// Check if an asset file already exists on disk.
pub fn asset_exists(asset: AssetType, slug: &str) -> bool {
    asset_path(asset, slug)
        .map(|p| p.exists())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Download pipeline
// ---------------------------------------------------------------------------

/// Filter assets based on NSFW / humor preferences.
fn filter_assets(assets: &[ImageAsset], nsfw_filter: bool, humor_filter: bool) -> Option<&ImageAsset> {
    assets.iter().find(|a| {
        (!nsfw_filter || !a.nsfw) && (!humor_filter || !a.humor)
    })
}

/// Resolve a game's `SteamGridDB` ID — using platform lookup if available, otherwise text search.
async fn resolve_game_id(
    client: &SteamGridDbClient,
    game: &Game,
) -> Result<Option<u64>> {
    // Try platform-specific lookup first (more accurate)
    if game.service.as_deref() == Some("steam") {
        if let Some(ref _sid) = game.service_id {
            // Search endpoint to get the SteamGridDB game ID from a Steam app ID
            let search_term = game.name.as_str();
            let results = client.search(search_term).await?;
            if let Some(first) = results.first() {
                return Ok(Some(first.id));
            }
        }
    }

    // Fallback: text search using the slug converted to a human-readable name
    let search_term = game.slug.replace('-', " ");
    let results = client.search(&search_term).await?;
    Ok(results.first().map(|r| r.id))
}

/// Shared download configuration passed to pipeline functions.
pub struct DownloadOpts {
    pub grid_dim: String,
    pub nsfw_filter: bool,
    pub humor_filter: bool,
    pub force: bool,
}

/// Download a single asset for a game, sending progress through the channel.
async fn download_single_asset(
    client: &SteamGridDbClient,
    game_id: u64,
    game: &Game,
    asset: AssetType,
    opts: &DownloadOpts,
    tx: &mpsc::UnboundedSender<DownloadProgress>,
) {
    let slug = &game.slug;

    // Check existence
    if !opts.force && asset_exists(asset, slug) {
        let _ = tx.send(DownloadProgress {
            game_slug: slug.clone(),
            asset_type: asset,
            status: DownloadStatus::Skipped("already exists".into()),
        });
        return;
    }

    // Notify: downloading
    let _ = tx.send(DownloadProgress {
        game_slug: slug.clone(),
        asset_type: asset,
        status: DownloadStatus::Downloading,
    });

    // Fetch asset list
    let dimensions: Option<&str> = if asset == AssetType::Grid { Some(&opts.grid_dim) } else { None };

    // Try platform-specific endpoint first for steam games
    let assets_result = if game.service.as_deref() == Some("steam") {
        if let Some(ref sid) = game.service_id {
            client.get_assets_by_platform(asset, "steam", sid.as_str(), dimensions).await
        } else {
            client.get_assets(asset, game_id, dimensions).await
        }
    } else {
        client.get_assets(asset, game_id, dimensions).await
    };

    let assets = match assets_result {
        Ok(a) => a,
        Err(e) => {
            let _ = tx.send(DownloadProgress {
                game_slug: slug.clone(),
                asset_type: asset,
                status: DownloadStatus::Failed(format!("fetch error: {e}")),
            });
            return;
        }
    };

    // Pick best asset
    let Some(chosen) = filter_assets(&assets, opts.nsfw_filter, opts.humor_filter) else {
        let _ = tx.send(DownloadProgress {
            game_slug: slug.clone(),
            asset_type: asset,
            status: DownloadStatus::Failed("no art found".into()),
        });
        return;
    };

    // Download image bytes
    let image_url = chosen.url.clone();
    let bytes: Vec<u8> = match client.download_image(&image_url).await {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(DownloadProgress {
                game_slug: slug.clone(),
                asset_type: asset,
                status: DownloadStatus::Failed(format!("download error: {e}")),
            });
            return;
        }
    };

    if bytes.is_empty() {
        let _ = tx.send(DownloadProgress {
            game_slug: slug.clone(),
            asset_type: asset,
            status: DownloadStatus::Failed("downloaded 0 bytes".into()),
        });
        return;
    }

    // Save to disk atomically
    match save_asset_to_disk(asset, slug, &bytes).await {
        Ok(target) => {
            let _ = tx.send(DownloadProgress {
                game_slug: slug.clone(),
                asset_type: asset,
                status: DownloadStatus::Done(target),
            });
        }
        Err(e) => {
            let _ = tx.send(DownloadProgress {
                game_slug: slug.clone(),
                asset_type: asset,
                status: DownloadStatus::Failed(format!("{e}")),
            });
        }
    }
}

/// Write bytes to disk atomically: write to `.tmp` then rename.
async fn save_asset_to_disk(
    asset: AssetType,
    slug: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    let target = asset_path(asset, slug)?;

    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .wrap_err("mkdir failed")?;
    }

    let tmp_path = target.with_extension("tmp");
    tokio::fs::write(&tmp_path, bytes)
        .await
        .wrap_err("write failed")?;
    tokio::fs::rename(&tmp_path, &target)
        .await
        .wrap_err("rename failed")?;
    Ok(target)
}

/// Run the entire download pipeline for all games and selected asset types.
///
/// Spawns concurrent tasks limited by a semaphore. Sends progress updates
/// through `tx` for each asset of each game.
pub async fn download_all(
    client: &SteamGridDbClient,
    games: &[Game],
    assets: &HashSet<AssetType>,
    opts: &DownloadOpts,
    max_concurrent: usize,
    tx: mpsc::UnboundedSender<DownloadProgress>,
) {
    let semaphore = std::sync::Arc::new(Semaphore::new(max_concurrent));

    // We process game-by-game so we can share the resolved SteamGridDB ID
    // across asset types for the same game.
    for game in games {
        let permit = semaphore.clone().acquire_owned().await;
        let Ok(_permit) = permit else { break };

        // Notify: searching
        for &asset in assets {
            let _ = tx.send(DownloadProgress {
                game_slug: game.slug.clone(),
                asset_type: asset,
                status: DownloadStatus::Searching,
            });
        }

        // Resolve game ID once per game
        let game_id = match resolve_game_id(client, game).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                for &asset in assets {
                    let _ = tx.send(DownloadProgress {
                        game_slug: game.slug.clone(),
                        asset_type: asset,
                        status: DownloadStatus::Failed("game not found on `SteamGridDB`".into()),
                    });
                }
                continue;
            }
            Err(e) => {
                for &asset in assets {
                    let _ = tx.send(DownloadProgress {
                        game_slug: game.slug.clone(),
                        asset_type: asset,
                        status: DownloadStatus::Failed(format!("search error: {e}")),
                    });
                }
                continue;
            }
        };

        // Download each selected asset type for this game
        for &asset in assets {
            download_single_asset(
                client, game_id, game, asset, opts, &tx,
            )
            .await;
        }
    }
}
