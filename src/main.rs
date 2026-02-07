/// Lutris Art Fetcher — download cover art for Lutris games from `SteamGridDB`.
///
/// A modern TUI application built with ratatui. Reads installed games from the
/// Lutris `SQLite` database and downloads grids, heroes, logos, and icons.
mod api;
mod app;
mod config;
mod db;
mod download;
mod event;
mod tui;
mod ui;

use std::collections::HashSet;

use clap::Parser;
use color_eyre::eyre::{Context, Result, eyre};

use crate::api::models::AssetType;
use crate::api::SteamGridDbClient;
use crate::app::App;
use crate::config::Config;
use crate::download::{asset_exists, asset_path};
use crate::event::{AppEvent, EventHandler};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "lutrisartfetcher",
    about = "Download cover art for Lutris games from SteamGridDB",
    version
)]
struct Cli {
    /// Run without TUI (headless stdout output).
    #[arg(long)]
    no_tui: bool,

    /// Re-download existing covers.
    #[arg(long)]
    force: bool,

    /// Show what would be downloaded without actually downloading.
    #[arg(long)]
    dry_run: bool,

    /// Asset types to download (comma-separated: grids,heroes,logos,icons).
    #[arg(long, value_delimiter = ',', default_value = "grids,heroes,logos,icons")]
    assets: Vec<String>,

    /// Max parallel downloads.
    #[arg(long, default_value = "3")]
    concurrency: u8,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    // Load configuration
    let mut config = Config::load()?;
    config.max_concurrent_downloads = cli.concurrency;

    // Parse asset types
    let assets: HashSet<AssetType> = cli
        .assets
        .iter()
        .map(|s| s.parse::<AssetType>())
        .collect::<Result<HashSet<_>>>()
        .wrap_err("Invalid asset type")?;

    if assets.is_empty() {
        return Err(eyre!("No asset types selected"));
    }

    // Validate Lutris database
    let db_path = config::lutris_db_path()?;
    db::validate_db(&db_path)?;

    // Read installed games (synchronous — must finish before async work)
    let games = db::get_installed_games(&db_path)?;
    if games.is_empty() {
        println!("No installed games found in the Lutris database.");
        return Ok(());
    }

    if cli.dry_run {
        run_dry_run(&games, &assets)?;
    } else if cli.no_tui {
        run_headless(config, games, assets, cli.force).await?;
    } else {
        run_tui(config, games, assets, cli.force).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// TUI mode
// ---------------------------------------------------------------------------

async fn run_tui(
    config: Config,
    games: Vec<db::Game>,
    assets: HashSet<AssetType>,
    force: bool,
) -> Result<()> {
    let mut terminal = tui::init()?;
    let mut events = EventHandler::new(250);
    let mut app = App::new(config, games, assets, force);

    loop {
        terminal
            .draw(|frame| ui::render(frame, &app))
            .wrap_err("Failed to render frame")?;

        match events.next().await? {
            AppEvent::Key(key) => {
                let tx = events.sender();
                app.handle_key(key, &tx);
            }
            AppEvent::Tick => {
                app.tick_count += 1;
            }
            AppEvent::Download(ref progress) => {
                app.handle_download_progress(progress);
            }
            AppEvent::Resize(_, _) => {
                // ratatui handles resize automatically on next draw
            }
        }

        if app.should_quit {
            break;
        }
    }

    tui::restore()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Headless mode
// ---------------------------------------------------------------------------

async fn run_headless(
    config: Config,
    games: Vec<db::Game>,
    assets: HashSet<AssetType>,
    force: bool,
) -> Result<()> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| eyre!("No API key configured. Run without --no-tui to set one interactively."))?;

    let client = SteamGridDbClient::new(api_key, config.request_delay_ms)?;

    println!("Found {} installed games", games.len());
    println!(
        "Downloading: {}",
        assets
            .iter()
            .map(|a| a.display_name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let games_clone = games.clone();
    let assets_clone = assets.clone();
    let grid_dim = config.preferred_grid_dimension.clone();
    let nsfw = config.nsfw_filter;
    let humor = config.humor_filter;
    let max_conc = config.max_concurrent_downloads as usize;

    // Spawn download pipeline
    let opts = download::DownloadOpts {
        grid_dim: grid_dim.clone(),
        nsfw_filter: nsfw,
        humor_filter: humor,
        force,
    };
    tokio::spawn(async move {
        download::download_all(
            &client,
            &games_clone,
            &assets_clone,
            &opts,
            max_conc,
            tx,
        )
        .await;
    });

    // Consume progress messages
    let mut downloaded = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    while let Some(progress) = rx.recv().await {
        let display = games
            .iter()
            .find(|g| g.slug == progress.game_slug)
            .map_or_else(|| progress.game_slug.clone(), |g| g.name.clone());

        match &progress.status {
            api::models::DownloadStatus::Done(path) => {
                downloaded += 1;
                println!("  ✓ {display} — {} saved", path.display());
            }
            api::models::DownloadStatus::Skipped(reason) => {
                skipped += 1;
                println!("  ─ {display} — {} skipped: {reason}", progress.asset_type);
            }
            api::models::DownloadStatus::Failed(msg) => {
                failed += 1;
                println!("  ✗ {display} — {} failed: {msg}", progress.asset_type);
            }
            api::models::DownloadStatus::Searching => {
                print!("  ⟳ Searching for {display}...");
            }
            api::models::DownloadStatus::Downloading => {
                println!(" downloading {}", progress.asset_type);
            }
            api::models::DownloadStatus::Pending => {}
        }
    }

    println!();
    println!("Done! Downloaded: {downloaded}, Skipped: {skipped}, Failed: {failed}");
    println!("Restart Lutris to see the changes.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Dry-run mode
// ---------------------------------------------------------------------------

fn run_dry_run(games: &[db::Game], assets: &HashSet<AssetType>) -> Result<()> {
    println!("DRY RUN — no files will be downloaded\n");
    println!("Found {} installed games\n", games.len());

    let mut would_download = 0u32;
    let mut already_exist = 0u32;

    for game in games {
        let mut statuses = Vec::new();
        for asset in assets {
            if asset_exists(*asset, &game.slug) {
                already_exist += 1;
                statuses.push(format!("{}: exists", asset.display_name()));
            } else {
                would_download += 1;
                let path = asset_path(*asset, &game.slug)?;
                statuses.push(format!("{}: would download → {}", asset.display_name(), path.display()));
            }
        }
        println!("  {} ({})", game.name, game.slug);
        for s in &statuses {
            println!("    {s}");
        }
    }

    println!("\nSummary: {would_download} assets to download, {already_exist} already exist");
    Ok(())
}
