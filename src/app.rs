/// Application state machine — holds all state, handles key events and download progress.
use std::collections::HashSet;
use std::time::Instant;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::api::models::{AssetType, DownloadProgress, DownloadStatus};
use crate::api::SteamGridDbClient;
use crate::config::Config;
use crate::db::Game;
use crate::download::{self, GameEntry};
use crate::event::AppEvent;

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// Which screen / modal is currently active.
#[derive(Debug, Clone)]
pub enum AppScreen {
    /// Prompt user to enter their `SteamGridDB` API key.
    ApiKeyEntry {
        input: String,
        cursor_pos: usize,
        error_msg: Option<String>,
        validating: bool,
    },
    /// Let user pick which asset types to download.
    AssetTypeSelection { cursor: usize },
    /// Browse the game list, press Enter to start.
    GameList,
    /// Downloads are in progress.
    Downloading {
        current: usize,
        total: usize,
        started_at: Instant,
    },
    /// All downloads finished.
    Done {
        downloaded: usize,
        skipped: usize,
        failed: usize,
        elapsed_secs: u64,
    },
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Severity level for log entries shown in the TUI.
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
    Error,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Root application state.
pub struct App {
    pub screen: AppScreen,
    pub games: Vec<GameEntry>,
    pub list_state: ListState,
    pub log: Vec<(LogLevel, String)>,
    pub selected_assets: HashSet<AssetType>,
    pub config: Config,
    pub should_quit: bool,
    pub show_help: bool,
    pub force_download: bool,
    /// Spinner animation frame counter.
    pub tick_count: u64,
}

impl App {
    /// Initialize the app. Decides the starting screen based on config state.
    pub fn new(
        config: Config,
        games: Vec<Game>,
        assets: HashSet<AssetType>,
        force: bool,
    ) -> Self {
        let entries: Vec<GameEntry> = games.into_iter().map(GameEntry::new).collect();

        let screen = if config.api_key.is_none() {
            AppScreen::ApiKeyEntry {
                input: String::new(),
                cursor_pos: 0,
                error_msg: None,
                validating: false,
            }
        } else {
            AppScreen::AssetTypeSelection { cursor: 0 }
        };

        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            screen,
            games: entries,
            list_state,
            log: Vec::new(),
            selected_assets: assets,
            config,
            should_quit: false,
            show_help: false,
            force_download: force,
            tick_count: 0,
        }
    }

    /// Handle a key event, dispatching based on current screen.
    pub fn handle_key(&mut self, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
        // Global shortcuts
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        if key.code == KeyCode::Char('?') {
            self.show_help = !self.show_help;
            return;
        }

        if self.show_help {
            // Any key closes help
            self.show_help = false;
            return;
        }

        match &self.screen {
            AppScreen::ApiKeyEntry { validating, .. } => {
                if *validating {
                    return; // ignore input while validating
                }
                self.handle_api_key_input(key, tx);
            }
            AppScreen::AssetTypeSelection { .. } => self.handle_asset_selection(key),
            AppScreen::GameList => self.handle_game_list(key, tx),
            AppScreen::Downloading { .. } => self.handle_downloading(key),
            AppScreen::Done { .. } => self.handle_done(key),
        }
    }

    // -- ApiKeyEntry --------------------------------------------------------

    fn handle_api_key_input(&mut self, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
        let AppScreen::ApiKeyEntry {
            ref mut input,
            ref mut cursor_pos,
            ref mut error_msg,
            ref mut validating,
        } = self.screen
        else {
            return;
        };

        match key.code {
            KeyCode::Char(c) => {
                input.insert(*cursor_pos, c);
                *cursor_pos += 1;
                *error_msg = None;
            }
            KeyCode::Backspace => {
                if *cursor_pos > 0 {
                    *cursor_pos -= 1;
                    input.remove(*cursor_pos);
                }
            }
            KeyCode::Left => {
                *cursor_pos = cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                if *cursor_pos < input.len() {
                    *cursor_pos += 1;
                }
            }
            KeyCode::Enter => {
                if input.trim().is_empty() {
                    *error_msg = Some("API key cannot be empty".into());
                    return;
                }
                let api_key = input.trim().to_owned();
                *validating = true;
                *error_msg = None;

                // Spawn async validation
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = validate_and_store_key(api_key).await;
                    // We send a special progress event to signal validation result
                    let status = match result {
                        Ok(()) => DownloadStatus::Done(std::path::PathBuf::new()),
                        Err(e) => DownloadStatus::Failed(e.to_string()),
                    };
                    let _ = tx.send(AppEvent::Download(DownloadProgress {
                        game_slug: "__api_key_validation__".into(),
                        asset_type: AssetType::Grid,
                        status,
                    }));
                });
            }
            KeyCode::Esc => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    // -- AssetTypeSelection -------------------------------------------------

    fn handle_asset_selection(&mut self, key: KeyEvent) {
        let AppScreen::AssetTypeSelection { ref mut cursor } = self.screen else {
            return;
        };

        let all = AssetType::all();

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                *cursor = cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *cursor + 1 < all.len() {
                    *cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                let asset = all[*cursor];
                if self.selected_assets.contains(&asset) {
                    self.selected_assets.remove(&asset);
                } else {
                    self.selected_assets.insert(asset);
                }
            }
            KeyCode::Char('a') => {
                // Toggle all
                if self.selected_assets.len() == all.len() {
                    self.selected_assets.clear();
                } else {
                    self.selected_assets = all.iter().copied().collect();
                }
            }
            KeyCode::Enter => {
                if self.selected_assets.is_empty() {
                    return; // must select at least one
                }
                self.screen = AppScreen::GameList;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    // -- GameList -----------------------------------------------------------

    fn handle_game_list(&mut self, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
        let len = self.games.len();
        if len == 0 {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                self.should_quit = true;
            }
            return;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some((i + 1).min(len - 1)));
            }
            KeyCode::Home => {
                self.list_state.select(Some(0));
            }
            KeyCode::End => {
                self.list_state.select(Some(len - 1));
            }
            KeyCode::PageUp => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(10)));
            }
            KeyCode::PageDown => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some((i + 10).min(len - 1)));
            }
            KeyCode::Enter => {
                self.start_downloads(tx);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    // -- Downloading --------------------------------------------------------

    fn handle_downloading(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
            self.should_quit = true;
        }
    }

    // -- Done ---------------------------------------------------------------

    fn handle_done(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter
        ) {
            self.should_quit = true;
        }
    }

    // -- Downloads ----------------------------------------------------------

    /// Kick off the download pipeline in a background task.
    fn start_downloads(&mut self, tx: &UnboundedSender<AppEvent>) {
        let total = self.games.len() * self.selected_assets.len();
        self.screen = AppScreen::Downloading {
            current: 0,
            total,
            started_at: Instant::now(),
        };

        let games: Vec<Game> = self.games.iter().map(|e| e.game.clone()).collect();
        let assets = self.selected_assets.clone();
        let grid_dim = self.config.preferred_grid_dimension.clone();
        let nsfw = self.config.nsfw_filter;
        let humor = self.config.humor_filter;
        let force = self.force_download;
        let max_conc = self.config.max_concurrent_downloads as usize;
        let api_key = self.config.api_key.clone().unwrap_or_default();
        let delay = self.config.request_delay_ms;
        let event_tx = tx.clone();

        tokio::spawn(async move {
            let Ok(client) = SteamGridDbClient::new(&api_key, delay) else {
                return;
            };
            let opts = download::DownloadOpts {
                grid_dim: grid_dim.clone(),
                nsfw_filter: nsfw,
                humor_filter: humor,
                force,
            };
            // Bridge: download_all sends DownloadProgress, we wrap into AppEvent
            let (dl_tx, mut dl_rx) = mpsc::unbounded_channel::<DownloadProgress>();

            let fwd = tokio::spawn({
                let event_tx = event_tx.clone();
                async move {
                    while let Some(p) = dl_rx.recv().await {
                        let _ = event_tx.send(AppEvent::Download(p));
                    }
                }
            });

            download::download_all(
                &client, &games, &assets, &opts, max_conc, dl_tx,
            )
            .await;
            let _ = fwd.await;
        });
    }

    /// Process a download progress event — update game entry and log.
    pub fn handle_download_progress(&mut self, progress: &DownloadProgress) {
        // Special case: API key validation result
        if progress.game_slug == "__api_key_validation__" {
            match progress.status {
                DownloadStatus::Done(_) => {
                    // Key is valid — save it and advance screen
                    if let AppScreen::ApiKeyEntry { ref input, .. } = self.screen {
                        self.config.api_key = Some(input.trim().to_owned());
                        if let Err(e) = self.config.save() {
                            self.log(LogLevel::Warn, format!("Could not save config: {e}"));
                        }
                    }
                    self.log(LogLevel::Ok, "API key validated and saved".into());
                    self.screen = AppScreen::AssetTypeSelection { cursor: 0 };
                }
                DownloadStatus::Failed(ref msg) => {
                    self.screen = AppScreen::ApiKeyEntry {
                        input: String::new(),
                        cursor_pos: 0,
                        error_msg: Some(format!("Invalid key: {msg}")),
                        validating: false,
                    };
                }
                _ => {}
            }
            return;
        }

        // Normal download progress
        let slug = &progress.game_slug;
        let asset = progress.asset_type;
        let display_name = self
            .games
            .iter()
            .find(|e| e.game.slug == *slug)
            .map_or_else(|| slug.clone(), |e| e.game.name.clone());

        // Log the update
        match &progress.status {
            DownloadStatus::Searching => {
                self.log(
                    LogLevel::Info,
                    format!("Searching for {display_name} ({asset})..."),
                );
            }
            DownloadStatus::Downloading => {
                self.log(
                    LogLevel::Info,
                    format!("Downloading {asset} for {display_name}..."),
                );
            }
            DownloadStatus::Done(path) => {
                self.log(
                    LogLevel::Ok,
                    format!("{display_name} — {asset} saved to {}", path.display()),
                );
            }
            DownloadStatus::Skipped(reason) => {
                self.log(
                    LogLevel::Info,
                    format!("{display_name} — {asset} skipped: {reason}"),
                );
            }
            DownloadStatus::Failed(msg) => {
                self.log(
                    LogLevel::Error,
                    format!("{display_name} — {asset} failed: {msg}"),
                );
            }
            DownloadStatus::Pending => {}
        }

        // Update game entry
        if let Some(entry) = self.games.iter_mut().find(|e| e.game.slug == *slug) {
            *entry.status_mut(asset) = progress.status.clone();
        }

        // Update progress counter
        if let AppScreen::Downloading {
            ref mut current,
            total,
            started_at,
        } = self.screen
        {
            if progress.status.is_terminal() {
                *current += 1;
            }

            // Check if all done
            if *current >= total {
                let elapsed = started_at.elapsed().as_secs();
                let (downloaded, skipped, failed) = self.count_results();
                self.screen = AppScreen::Done {
                    downloaded,
                    skipped,
                    failed,
                    elapsed_secs: elapsed,
                };
            }
        }
    }

    /// Count terminal statuses across all game entries.
    fn count_results(&self) -> (usize, usize, usize) {
        let mut downloaded = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;

        for entry in &self.games {
            for &asset in &self.selected_assets {
                match entry.status(asset) {
                    DownloadStatus::Done(_) => downloaded += 1,
                    DownloadStatus::Skipped(_) => skipped += 1,
                    DownloadStatus::Failed(_) => failed += 1,
                    _ => {}
                }
            }
        }

        (downloaded, skipped, failed)
    }

    /// Calculate overall progress as a ratio [0.0, 1.0].
    #[allow(clippy::cast_precision_loss)]
    #[allow(dead_code)]
    pub fn overall_progress(&self) -> f64 {
        if let AppScreen::Downloading { current, total, .. } = self.screen {
            if total == 0 {
                return 1.0;
            }
            current as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Append a log message.
    pub fn log(&mut self, level: LogLevel, message: String) {
        self.log.push((level, message));
    }
}

/// Validate an API key and save it to config if valid (called from spawned task).
async fn validate_and_store_key(api_key: String) -> Result<()> {
    let client = SteamGridDbClient::new(&api_key, 0)?;
    let valid = client.validate_key().await?;
    if valid {
        let mut config = Config::load()?;
        config.api_key = Some(api_key);
        config.save()?;
        Ok(())
    } else {
        Err(color_eyre::eyre::eyre!("API key rejected by SteamGridDB"))
    }
}
