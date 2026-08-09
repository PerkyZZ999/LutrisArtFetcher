/// Download orchestration — resolves game IDs, fetches assets, saves images.
///
/// Each download task sends progress updates through an `mpsc` channel so the
/// TUI can display real-time status.
use std::collections::HashSet;
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result};
use tokio::sync::{mpsc, Semaphore};

use crate::api::models::{AssetType, DownloadProgress, DownloadStatus, ImageAsset};
use crate::api::{SteamAppDetails, SteamGridDbClient, SteamStoreClient};
use crate::config;
use crate::db::Game;

/// Entry combining a game and per-asset download status.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GameEntry {
    pub game: Game,
    /// Whether this game is selected for download in the TUI game list.
    pub selected: bool,
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
            selected: true,
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
        let statuses: Vec<&DownloadStatus> =
            active_assets.iter().map(|a| self.status(*a)).collect();

        // Any downloading? Show downloading
        if statuses
            .iter()
            .any(|s| matches!(s, DownloadStatus::Downloading | DownloadStatus::Searching))
        {
            return "↓";
        }
        // Any failed? Show failed
        if statuses
            .iter()
            .any(|s| matches!(s, DownloadStatus::Failed(_)))
        {
            return "✗";
        }
        // All done or skipped? Show done
        if statuses
            .iter()
            .all(|s| matches!(s, DownloadStatus::Done(_) | DownloadStatus::Skipped(_)))
        {
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
    asset_path(asset, slug).map(|p| p.exists()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Download pipeline
// ---------------------------------------------------------------------------

/// Filter assets based on NSFW / humor preferences.
fn filter_assets(
    assets: &[ImageAsset],
    nsfw_filter: bool,
    humor_filter: bool,
) -> Option<&ImageAsset> {
    assets
        .iter()
        .find(|a| (!nsfw_filter || !a.nsfw) && (!humor_filter || !a.humor))
}

fn normalize_title(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_overlap_score(target: &str, candidate: &str) -> i32 {
    let target_tokens: HashSet<&str> = target.split_whitespace().collect();
    let candidate_tokens: HashSet<&str> = candidate.split_whitespace().collect();

    if target_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0;
    }

    let common = target_tokens.intersection(&candidate_tokens).count() as i32;
    let union = target_tokens.union(&candidate_tokens).count() as i32;

    if union == 0 {
        0
    } else {
        (common * 100) / union
    }
}

fn match_score(target: &str, candidate: &str) -> i32 {
    if target.is_empty() || candidate.is_empty() {
        return 0;
    }

    if target == candidate {
        return 100;
    }

    if target.contains(candidate) || candidate.contains(target) {
        return 82;
    }

    let mut score = token_overlap_score(target, candidate);
    if target.starts_with(candidate) || candidate.starts_with(target) {
        score += 8;
    }
    score.min(95)
}

fn is_non_game_type(app_type: &str) -> bool {
    let kind = app_type.to_ascii_lowercase();
    matches!(
        kind.as_str(),
        "dlc" | "demo" | "advertising" | "video" | "movie" | "episode" | "series" | "mod" | "music"
    )
}

/// Resolve a Steam app for fallback matching.
///
/// Strategy:
/// 1) Trust numeric `service_id` when available.
/// 2) Otherwise, search by game name/slug and only accept high-confidence matches.
async fn resolve_steam_app(
    steam_client: &SteamStoreClient,
    game: &Game,
) -> Result<Option<SteamAppDetails>> {
    if let Some(service_id) = game.service_id.as_deref() {
        if let Ok(app_id) = service_id.parse::<u32>() {
            let details = steam_client
                .get_app_details(app_id)
                .await?
                .unwrap_or_else(|| SteamAppDetails::placeholder(app_id));
            return Ok(Some(details));
        }
    }

    let target = normalize_title(&game.name);
    let slug_target = normalize_title(&game.slug.replace('-', " "));

    let mut queries = Vec::with_capacity(2);
    if !game.name.trim().is_empty() {
        queries.push(game.name.clone());
    }

    let slug_name = game.slug.replace('-', " ");
    if !slug_name.trim().is_empty() && normalize_title(&slug_name) != target {
        queries.push(slug_name);
    }

    if queries.is_empty() {
        return Ok(None);
    }

    #[derive(Debug, Clone)]
    struct Candidate {
        app_id: u32,
        score: i32,
        name: String,
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for query in queries {
        let results = steam_client.search_apps(&query).await?;
        for item in results.into_iter().take(15) {
            if is_non_game_type(&item.app_type) {
                continue;
            }

            let name_norm = normalize_title(&item.name);
            if name_norm.is_empty() {
                continue;
            }

            let score = match_score(&target, &name_norm).max(match_score(&slug_target, &name_norm));
            if score < 60 {
                continue;
            }

            if let Some(existing) = candidates.iter_mut().find(|c| c.app_id == item.id) {
                existing.score = existing.score.max(score);
            } else {
                candidates.push(Candidate {
                    app_id: item.id,
                    score,
                    name: item.name,
                });
            }
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.len().cmp(&b.name.len()))
    });
    candidates.truncate(5);

    let mut game_matches: Vec<(SteamAppDetails, i32)> = Vec::new();

    for candidate in candidates {
        let Some(details) = steam_client.get_app_details(candidate.app_id).await? else {
            continue;
        };

        if is_non_game_type(&details.app_type) {
            continue;
        }

        let details_norm = normalize_title(&details.name);
        let validated = candidate
            .score
            .max(match_score(&target, &details_norm))
            .max(match_score(&slug_target, &details_norm));

        game_matches.push((details, validated));
    }

    if game_matches.is_empty() {
        return Ok(None);
    }

    game_matches.sort_by(|a, b| b.1.cmp(&a.1));

    let second_best = game_matches.get(1).map_or(0, |m| m.1);
    let (best_details, best_score) = game_matches.remove(0);
    let best_name_norm = normalize_title(&best_details.name);
    let contains_exact_target = !target.is_empty() && best_name_norm.contains(&target);

    let confident = best_score >= 85
        || (best_score >= 75 && (second_best == 0 || best_score - second_best >= 10))
        || (best_score >= 80 && contains_exact_target);

    if confident {
        return Ok(Some(best_details));
    }

    Ok(None)
}

/// Resolve a game's `SteamGridDB` ID — using platform lookup if available, otherwise text search.
async fn resolve_game_id(client: &SteamGridDbClient, game: &Game) -> Result<Option<u64>> {
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
    sgdb_client: &SteamGridDbClient,
    steam_client: &SteamStoreClient,
    game_id: Option<u64>,
    steam_app: Option<&SteamAppDetails>,
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
    let dimensions: Option<&str> = if asset == AssetType::Grid {
        Some(&opts.grid_dim)
    } else {
        None
    };

    let mut sgdb_error: Option<String> = None;
    if let Some(game_id) = game_id {
        // Try platform-specific endpoint first for steam games.
        let assets_result = if game.service.as_deref() == Some("steam") {
            if let Some(ref sid) = game.service_id {
                sgdb_client
                    .get_assets_by_platform(asset, "steam", sid.as_str(), dimensions)
                    .await
            } else {
                sgdb_client.get_assets(asset, game_id, dimensions).await
            }
        } else {
            sgdb_client.get_assets(asset, game_id, dimensions).await
        };

        match assets_result {
            Ok(assets) => {
                if let Some(chosen) = filter_assets(&assets, opts.nsfw_filter, opts.humor_filter) {
                    let image_url = chosen.url.clone();
                    match sgdb_client.download_image(&image_url).await {
                        Ok(bytes) if !bytes.is_empty() => {
                            match save_asset_to_disk(asset, slug, &bytes).await {
                                Ok(target) => {
                                    let _ = tx.send(DownloadProgress {
                                        game_slug: slug.clone(),
                                        asset_type: asset,
                                        status: DownloadStatus::Done(target),
                                    });
                                    return;
                                }
                                Err(e) => {
                                    sgdb_error = Some(format!("save error: {e}"));
                                }
                            }
                        }
                        Ok(_) => {
                            sgdb_error = Some("downloaded 0 bytes from SteamGridDB".into());
                        }
                        Err(e) => {
                            sgdb_error = Some(format!("SteamGridDB download error: {e}"));
                        }
                    }
                } else {
                    sgdb_error = Some("no art found on SteamGridDB".into());
                }
            }
            Err(e) => {
                sgdb_error = Some(format!("SteamGridDB fetch error: {e}"));
            }
        }
    }

    let mut steam_error: Option<String> = None;
    if let Some(details) = steam_app {
        let candidate_urls = steam_client.candidate_image_urls(asset, details, &opts.grid_dim);
        for image_url in candidate_urls {
            let bytes = match steam_client.download_image(&image_url).await {
                Ok(b) if !b.is_empty() => b,
                Ok(_) => continue,
                Err(_) => continue,
            };

            match save_asset_to_disk(asset, slug, &bytes).await {
                Ok(target) => {
                    let _ = tx.send(DownloadProgress {
                        game_slug: slug.clone(),
                        asset_type: asset,
                        status: DownloadStatus::Done(target),
                    });
                    return;
                }
                Err(e) => {
                    steam_error = Some(format!("save error: {e}"));
                    break;
                }
            }
        }

        if steam_error.is_none() {
            steam_error = Some("no art found on Steam Store".into());
        }
    } else if sgdb_error.is_some() {
        steam_error = Some("Steam Store fallback unavailable (no app match)".into());
    }

    let message = match (sgdb_error, steam_error) {
        (Some(a), Some(b)) => format!("{a}; {b}"),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => "no art source available".into(),
    };

    let _ = tx.send(DownloadProgress {
        game_slug: slug.clone(),
        asset_type: asset,
        status: DownloadStatus::Failed(message),
    });
}

/// Write bytes to disk atomically: write to `.tmp` then rename.
async fn save_asset_to_disk(asset: AssetType, slug: &str, bytes: &[u8]) -> Result<PathBuf> {
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
    sgdb_client: &SteamGridDbClient,
    steam_client: &SteamStoreClient,
    games: &[Game],
    assets: &HashSet<AssetType>,
    opts: &DownloadOpts,
    max_concurrent: usize,
    tx: mpsc::UnboundedSender<DownloadProgress>,
) {
    let semaphore = std::sync::Arc::new(Semaphore::new(max_concurrent));

    // We process game-by-game so we can share resolved IDs
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

        let mut sgdb_lookup_error: Option<String> = None;
        let sgdb_game_id = match resolve_game_id(sgdb_client, game).await {
            Ok(id) => id,
            Err(e) => {
                sgdb_lookup_error = Some(format!("SteamGridDB search error: {e}"));
                None
            }
        };

        let mut steam_lookup_error: Option<String> = None;
        let steam_app = match resolve_steam_app(steam_client, game).await {
            Ok(app) => app,
            Err(e) => {
                steam_lookup_error = Some(format!("Steam store lookup error: {e}"));
                None
            }
        };

        if sgdb_game_id.is_none() && steam_app.is_none() {
            let message = match (sgdb_lookup_error, steam_lookup_error) {
                (Some(a), Some(b)) => format!("{a}; {b}"),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => "game not found on SteamGridDB or Steam Store".into(),
            };

            for &asset in assets {
                let _ = tx.send(DownloadProgress {
                    game_slug: game.slug.clone(),
                    asset_type: asset,
                    status: DownloadStatus::Failed(message.clone()),
                });
            }
            continue;
        }

        // Download each selected asset type for this game
        for &asset in assets {
            download_single_asset(
                sgdb_client,
                steam_client,
                sgdb_game_id,
                steam_app.as_ref(),
                game,
                asset,
                opts,
                &tx,
            )
            .await;
        }
    }
}
