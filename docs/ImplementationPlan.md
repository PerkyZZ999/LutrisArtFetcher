# Implementation Plan — `lutrisartfetcher`

> Derived from [ProjectPlan.md](./ProjectPlan.md)
> Target: Rust 2021 edition, MSRV 1.80+, Linux primary

---

## Phase 0: Project Bootstrap

**Goal**: Clean slate — remove Python, scaffold Rust project, verify toolchain.

### 0.1 — Remove Python artifacts

- [x] Delete `main.py`
- [x] Delete `requirements.txt`
- [x] Delete `run.sh`
- [x] Delete `env/` directory (entire Python venv)
- [x] Keep `docs/` folder with planning documents
- [x] Keep `.git/` history intact

### 0.2 — Initialize Cargo project

```bash
cargo init --name lutrisartfetcher .
```

### 0.3 — Write `Cargo.toml`

```toml
[package]
name = "lutrisartfetcher"
version = "0.1.0"
edition = "2021"
description = "TUI tool to download cover art for Lutris games from SteamGridDB"
license = "MIT"
rust-version = "1.80"

[dependencies]
ratatui = "0.30"
crossterm = { version = "0.29", features = ["event-stream"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "sync", "time"] }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls"] }
rusqlite = { version = "0.38", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
dirs = "6.0"
color-eyre = "0.6"
clap = { version = "4.5", features = ["derive"] }
futures = "0.3"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

### 0.4 — Create `.gitignore`

```gitignore
/target
*.swp
*.swo
.env
```

### 0.5 — Create module file stubs

Create all files from the module structure with minimal contents (`// TODO`) so the project compiles from the start:

```
src/
├── main.rs
├── app.rs
├── ui.rs
├── event.rs
├── tui.rs
├── db.rs
├── config.rs
├── download.rs
└── api/
    ├── mod.rs
    ├── client.rs
    └── models.rs
```

**Exit criteria**: `cargo check` passes, `cargo clippy` clean.

---

## Phase 1: Core Data Layer (no TUI yet)

**Goal**: Config, DB, and API types — all the foundational data structures.  
**Dependencies**: Phase 0 complete.

### 1.1 — `src/config.rs` — Configuration management

**Structs**:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub preferred_grid_dimension: String,    // "600x900" default
    pub max_concurrent_downloads: u8,        // default 3
    pub nsfw_filter: bool,                   // default true (filter out)
    pub humor_filter: bool,                  // default true (filter out)
    pub request_delay_ms: u64,               // default 100
}
```

**Functions to implement**:

| Function | Signature | Notes |
|---|---|---|
| `config_dir()` | `-> PathBuf` | `dirs::config_dir().unwrap().join("lutrisartfetcher")` |
| `config_path()` | `-> PathBuf` | `config_dir().join("config.toml")` |
| `Config::default()` | `-> Self` | Sensible defaults for all fields |
| `Config::load()` | `-> Result<Self>` | Read TOML file, create dir + default if missing |
| `Config::save(&self)` | `-> Result<()>` | Write TOML, create parent dirs with `fs::create_dir_all` |

**Edge cases**:
- Config dir doesn't exist → create it
- Config file is malformed → log warning, use defaults
- `dirs::config_dir()` returns `None` → fallback to `$HOME/.config`

**Test**: Unit test that creates a temp dir, saves a config, loads it back, asserts equality.

### 1.2 — `src/db.rs` — Lutris database reader

**Structs**:

```rust
#[derive(Debug, Clone)]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub runner: Option<String>,
    pub platform: Option<String>,
    pub service: Option<String>,
    pub service_id: Option<String>,
    pub has_custom_banner: bool,
    pub has_custom_coverart: bool,
}
```

**Functions to implement**:

| Function | Signature | Notes |
|---|---|---|
| `db_path()` | `-> PathBuf` | `dirs::data_dir().unwrap().join("lutris/pga.db")` |
| `validate_db()` | `(path: &Path) -> Result<()>` | Check file exists, is a file, is readable |
| `get_installed_games()` | `(path: &Path) -> Result<Vec<Game>>` | Query, map rows, sort by name |

**SQL query**:

```sql
SELECT id, name, slug, runner, platform, service, service_id,
       has_custom_banner, has_custom_coverart_big
FROM games
WHERE installed = 1
ORDER BY name COLLATE NOCASE
```

**Error handling**:
- `pga.db` not found → `color_eyre` error with message: "Lutris database not found at {path}. Is Lutris installed?"
- `pga.db` exists but empty/corrupt → SQLite error, wrap with context
- `games` table missing columns → graceful fallback (use `Option` + column_count check)

**Important**: Open connection, read all games into `Vec<Game>`, drop connection immediately. Do NOT hold `Connection` across async boundaries.

### 1.3 — `src/api/models.rs` — SteamGridDB response types

**Structs** (all derive `Debug, Clone, Deserialize`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Vec<T>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GridImage {
    pub id: u64,
    pub score: i32,
    pub style: String,
    pub width: u32,
    pub height: u32,
    pub nsfw: bool,
    pub humor: bool,
    pub mime: String,
    pub url: String,
    pub thumb: String,
}
```

**Enums**:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetType {
    Grid,
    Hero,
    Logo,
    Icon,
}

impl AssetType {
    pub fn api_path(&self) -> &str { /* "grids", "heroes", "logos", "icons" */ }
    pub fn display_name(&self) -> &str { /* "Grid", "Hero", "Logo", "Icon" */ }
    pub fn lutris_subdir(&self) -> &str { /* "banners", "heroes", "logos", "icons" */ }
}
```

**Note**: Use the same `GridImage` struct for heroes/logos/icons since the response shape is identical. Add a type alias if desired.

### 1.4 — `src/api/client.rs` — HTTP client

**Struct**:

```rust
pub struct SteamGridDbClient {
    client: reqwest::Client,
    api_key: String,
    request_delay: Duration,
}
```

**Methods to implement**:

| Method | Endpoint | Returns |
|---|---|---|
| `new(api_key, delay_ms)` | — | `Self` (builds client with auth header) |
| `validate_key()` | `GET /grids/game/1?dimensions=600x900` | `Result<bool>` |
| `search(term)` | `GET /search/autocomplete/{term}` | `Result<Vec<SearchResult>>` |
| `get_assets(type, game_id, dims)` | `GET /{type}/game/{id}?dimensions={dims}` | `Result<Vec<GridImage>>` |
| `get_assets_by_platform(type, platform, id)` | `GET /{type}/{platform}/{id}` | `Result<Vec<GridImage>>` |
| `download_image(url)` | `GET {cdn_url}` | `Result<Bytes>` |

**Implementation details**:
- Build `reqwest::Client` once with `Authorization: Bearer {key}` as a default header
- All methods `async` — caller decides concurrency
- Insert `tokio::time::sleep(self.request_delay)` between API calls (not image downloads)
- Convert slugs for search: `term.replace('-', " ")`
- On HTTP error → wrap status code in `color_eyre` context

### 1.5 — `src/api/mod.rs` — Re-exports

```rust
pub mod client;
pub mod models;

pub use client::SteamGridDbClient;
pub use models::*;
```

**Exit criteria**: `cargo check` passes, all types defined, `cargo test` for config round-trip.

---

## Phase 2: Download Engine

**Goal**: Wire up the search → fetch → download → save pipeline.  
**Dependencies**: Phase 1 complete.

### 2.1 — `src/download.rs` — Download orchestration

**Types**:

```rust
#[derive(Debug, Clone)]
pub enum DownloadStatus {
    Pending,
    Searching,
    Downloading,
    Done(PathBuf),
    Skipped(String),    // reason
    Failed(String),     // error message
}

#[derive(Debug, Clone)]
pub struct GameEntry {
    pub game: Game,
    pub grid_status: DownloadStatus,
    pub hero_status: DownloadStatus,
    pub logo_status: DownloadStatus,
    pub icon_status: DownloadStatus,
    pub steamgriddb_id: Option<u64>,  // cached after first search
}
```

**Key functions**:

| Function | Signature | Purpose |
|---|---|---|
| `asset_path()` | `(asset: AssetType, slug: &str) -> PathBuf` | Resolve full Lutris path for an asset |
| `asset_exists()` | `(asset: AssetType, slug: &str) -> bool` | Check if cover already saved |
| `resolve_game_id()` | `async (client, game) -> Result<u64>` | Platform lookup or search fallback |
| `download_asset()` | `async (client, game_id, asset, slug) -> Result<DownloadStatus>` | Full pipeline: fetch asset list → pick best → download → save |
| `download_all()` | `async (client, games, assets, tx, force) -> Result<()>` | Orchestrate all downloads, send progress via `mpsc` |

**Asset path resolution** (using `dirs::data_dir()`):

| Asset Type | Path Pattern |
|---|---|
| Grid (banner) | `{data_dir}/lutris/banners/{slug}.jpg` |
| Grid (coverart) | `{data_dir}/lutris/coverart/{slug}.jpg` |
| Hero | `{data_dir}/lutris/heroes/{slug}.jpg` |
| Logo | `{data_dir}/lutris/logos/{slug}.jpg` |
| Icon | `{data_dir}/icons/hicolor/128x128/apps/lutris_{slug}.png` |

**Download logic per game**:

```
for each (game, asset_type):
  1. if !force && asset_exists(asset_type, game.slug):
       → send Skipped("already exists"), continue
  2. send Searching
  3. if game.steamgriddb_id is None:
       if game.service == "steam" && game.service_id.is_some():
         → try get_assets_by_platform("steam", service_id)
       else:
         → search(slug.replace('-', ' ')), cache id
  4. fetch asset list for game_id + asset_type
     → filter out nsfw/humor based on config
     → take first result
  5. if no results → send Failed("no art found"), continue
  6. send Downloading
  7. download_image(url) → save to path
     → create parent dirs if missing
  8. send Done(path)
```

**Concurrency**: Use `tokio::sync::Semaphore` with `config.max_concurrent_downloads` permits. Each game's download acquires a permit.

**Progress channel**: `tokio::sync::mpsc::UnboundedSender<DownloadProgress>`:

```rust
pub struct DownloadProgress {
    pub game_slug: String,
    pub asset_type: AssetType,
    pub status: DownloadStatus,
}
```

**Exit criteria**: Can run download pipeline headlessly from a test harness. All edge cases (no results, network error, existing file) handled gracefully.

---

## Phase 3: TUI Infrastructure

**Goal**: Terminal lifecycle, event loop, basic rendering — before wiring to real data.  
**Dependencies**: Phase 0 complete (Phases 1-2 can proceed in parallel for data work).

### 3.1 — `src/tui.rs` — Terminal lifecycle

```rust
use std::io::{self, stdout, Stdout};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
```

**Panic hook**: Install via `color_eyre` — `restore()` must be called even on panic to avoid borked terminal state.

### 3.2 — `src/event.rs` — Async event system

**Types**:

```rust
pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Tick,
    Download(DownloadProgress),
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: UnboundedReceiver<AppEvent>,
    _tx: UnboundedSender<AppEvent>,  // kept alive; cloned for download tasks
}
```

**Implementation**:

```rust
impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Input task
        let tx_input = tx.clone();
        tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            loop {
                if let Some(Ok(event)) = reader.next().await {
                    match event {
                        Event::Key(key) => { let _ = tx_input.send(AppEvent::Key(key)); }
                        Event::Resize(w, h) => { let _ = tx_input.send(AppEvent::Resize(w, h)); }
                        _ => {}
                    }
                }
            }
        });

        // Tick task
        let tx_tick = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(tick_rate_ms));
            loop {
                interval.tick().await;
                let _ = tx_tick.send(AppEvent::Tick);
            }
        });

        Self { rx, _tx: tx }
    }

    pub fn sender(&self) -> UnboundedSender<AppEvent> {
        self._tx.clone()
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx.recv().await.ok_or_else(|| eyre!("Event channel closed"))
    }
}
```

### 3.3 — `src/app.rs` — Application state

**Core types**:

```rust
pub enum AppScreen {
    ApiKeyEntry {
        input: String,
        cursor_pos: usize,
        error_msg: Option<String>,
    },
    AssetTypeSelection {
        selected: HashSet<AssetType>,
        cursor: usize,
    },
    GameList,
    Downloading {
        current: usize,
        total: usize,
    },
    Done {
        downloaded: usize,
        skipped: usize,
        failed: usize,
    },
}

pub enum LogLevel { Info, Ok, Warn, Error }

pub struct App {
    pub screen: AppScreen,
    pub games: Vec<GameEntry>,
    pub list_state: ListState,
    pub log: Vec<(LogLevel, String)>,
    pub progress: f64,
    pub selected_assets: HashSet<AssetType>,
    pub config: Config,
    pub should_quit: bool,
    pub show_help: bool,
    pub force_download: bool,
}
```

**Methods to implement**:

| Method | Purpose |
|---|---|
| `App::new(config, games)` | Initialize app state; determine starting screen |
| `App::handle_key(key)` | Dispatch key events based on current screen |
| `App::handle_download_progress(progress)` | Update game entry status, log, progress bar |
| `App::log(level, msg)` | Append to log buffer |
| `App::overall_progress()` | Calculate `completed / total` ratio |

**Screen transitions**:

```
Start → [no API key?] → ApiKeyEntry
      → [has API key] → AssetTypeSelection
AssetTypeSelection → [Enter] → GameList
GameList → [Enter] → Downloading
Downloading → [all done] → Done
Done → [q] → Quit

Any screen → [q/Esc] → Quit (with confirmation on Downloading)
Any screen → [?] → Toggle help overlay
```

**Key bindings per screen**:

| Screen | Key | Action |
|---|---|---|
| ApiKeyEntry | chars | Append to input |
| ApiKeyEntry | Backspace | Delete char |
| ApiKeyEntry | Enter | Validate key → save → next screen |
| AssetTypeSelection | ↑/↓ | Move cursor |
| AssetTypeSelection | Space | Toggle asset type |
| AssetTypeSelection | Enter | Confirm → GameList |
| GameList | ↑/↓/PgUp/PgDn | Scroll game list |
| GameList | Enter | Start downloading |
| GameList | q | Quit |
| Downloading | q | Quit (ask confirmation) |
| Done | q/Enter | Quit |
| All | ? | Toggle help popup |

### 3.4 — `src/ui.rs` — Rendering

**Main entry point**:

```rust
pub fn render(frame: &mut Frame, app: &App) {
    match &app.screen {
        AppScreen::ApiKeyEntry { .. } => render_api_key_screen(frame, app),
        AppScreen::AssetTypeSelection { .. } => render_asset_selection(frame, app),
        AppScreen::GameList => render_game_list(frame, app),
        AppScreen::Downloading { .. } => render_downloading(frame, app),
        AppScreen::Done { .. } => render_done(frame, app),
    }
    if app.show_help {
        render_help_popup(frame);
    }
}
```

**Layout for GameList / Downloading screens** (the main view):

```
Vertical split:
┌────────────────────────────────────────────────┐
│  Title bar (1 line)                            │
├──────────────────────┬─────────────────────────┤
│  Game list (60%)     │  Status panel (40%)     │
│                      │    - Mode               │
│                      │    - Progress gauge     │
│                      │    - Current game       │
├──────────────────────┴─────────────────────────┤
│  Log panel (30% height)                        │
├────────────────────────────────────────────────┤
│  Footer keybindings (1 line)                   │
└────────────────────────────────────────────────┘
```

**Widgets used**:

| Widget | Where | Ratatui type |
|---|---|---|
| Title | Top | `Block::bordered().title()` |
| Game list | Left panel | `List` + `ListState` |
| Progress bar | Right panel | `Gauge` |
| Status text | Right panel | `Paragraph` |
| Activity log | Bottom panel | `Paragraph` (reverse order, scrollable) |
| Footer | Bottom line | `Paragraph` with `Span` styling |
| Help overlay | Centered popup | `Clear` + `Paragraph` in `Block` |
| Text input | API key screen | `Paragraph` with cursor indicator |
| Checkbox list | Asset selection | `List` with `[x]`/`[ ]` prefixes |

**Status icons in game list**:

| Icon | Meaning | Color |
|---|---|---|
| `✓` | All selected assets downloaded | Green |
| `↓` | Currently downloading | Yellow (animated?) |
| `✗` | At least one asset failed | Red |
| `─` | Skipped (already exists) | Dark gray |
| `·` | Pending | White |

**Exit criteria**: TUI renders all screens correctly with placeholder data.

---

## Phase 4: Integration & Main Loop

**Goal**: Wire everything together — CLI parsing, app loop, download orchestration through the TUI.  
**Dependencies**: Phases 1, 2, 3 all complete.

### 4.1 — `src/main.rs` — CLI args with `clap`

```rust
#[derive(Parser, Debug)]
#[command(name = "lutrisartfetcher", about = "Download cover art for Lutris games from SteamGridDB")]
struct Cli {
    /// Run without TUI (headless stdout output)
    #[arg(long)]
    no_tui: bool,

    /// Re-download existing covers
    #[arg(long)]
    force: bool,

    /// Show what would be downloaded without actually downloading
    #[arg(long)]
    dry_run: bool,

    /// Asset types to download (comma-separated: grids,heroes,logos,icons)
    #[arg(long, value_delimiter = ',', default_value = "grids,heroes,logos,icons")]
    assets: Vec<String>,

    /// Max parallel downloads
    #[arg(long, default_value = "3")]
    concurrency: u8,
}
```

### 4.2 — `main()` function flow

```rust
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    // Load config
    let mut config = Config::load()?;
    config.max_concurrent_downloads = cli.concurrency;

    // Validate Lutris DB
    let db_path = db::db_path();
    db::validate_db(&db_path)?;

    // Read games (sync, before async)
    let games = db::get_installed_games(&db_path)?;
    if games.is_empty() {
        println!("No installed games found in Lutris database.");
        return Ok(());
    }

    // Parse asset types
    let assets: HashSet<AssetType> = /* parse from cli.assets */;

    if cli.no_tui {
        run_headless(config, games, assets, cli.force, cli.dry_run).await
    } else {
        run_tui(config, games, assets, cli.force, cli.dry_run).await
    }
}
```

### 4.3 — TUI run loop

```rust
async fn run_tui(config: Config, games: Vec<Game>, assets: HashSet<AssetType>, force: bool, dry_run: bool) -> Result<()> {
    // Install panic hook BEFORE entering alternate screen
    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = tui::restore();
        panic_hook(info);
    }));

    let mut terminal = tui::init()?;
    let mut events = EventHandler::new(250); // 250ms tick
    let mut app = App::new(config, games, assets, force);

    loop {
        // Render
        terminal.draw(|frame| ui::render(frame, &app))?;

        // Handle events
        match events.next().await? {
            AppEvent::Key(key) => app.handle_key(key, &events.sender()).await?,
            AppEvent::Tick => { /* update spinners, etc */ }
            AppEvent::Download(progress) => app.handle_download_progress(progress),
            AppEvent::Resize(_, _) => { /* terminal auto-resizes */ }
        }

        if app.should_quit {
            break;
        }
    }

    tui::restore()?;
    Ok(())
}
```

### 4.4 — Headless run mode

```rust
async fn run_headless(config: Config, games: Vec<Game>, assets: HashSet<AssetType>, force: bool, dry_run: bool) -> Result<()> {
    let client = SteamGridDbClient::new(
        config.api_key.ok_or_else(|| eyre!("No API key configured. Run without --no-tui to set one."))?,
        config.request_delay_ms,
    );

    println!("Found {} installed games", games.len());

    for (i, game) in games.iter().enumerate() {
        print!("[{}/{}] {} ... ", i + 1, games.len(), game.name);
        // download logic with stdout output
    }

    println!("\nDone!");
    Ok(())
}
```

**Exit criteria**: Full app runs end-to-end — launch TUI → enter API key → select assets → browse games → download → done screen.

---

## Phase 5: Polish & Hardening

**Goal**: Edge cases, UX polish, `clippy`, documentation.  
**Dependencies**: Phase 4 complete.

### 5.1 — Error resilience

- [ ] Network timeout handling (reqwest timeout config: 30s per request)
- [ ] Retry logic: 1 retry on transient HTTP errors (429, 500, 502, 503)
- [ ] Graceful handling of SteamGridDB rate limits (429 → back off 5s, retry)
- [ ] Handle `Ctrl+C` cleanly — restore terminal, print summary of what was done
- [ ] Handle corrupted/partial downloads — write to `.tmp` first, rename on success
- [ ] Validate downloaded image is non-zero bytes before saving

### 5.2 — UX improvements

- [ ] Animated spinner on "Searching..." status (cycle through `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`)
- [ ] Color-coded log entries (green/yellow/red/white)
- [ ] Show elapsed time in Done screen
- [ ] Show download speed / size stats
- [ ] Flash/highlight current game being processed in the list
- [ ] Scroll log to bottom automatically on new entries
- [ ] Show total games count and how many have existing art in GameList screen

### 5.3 — Code quality

- [ ] Run `cargo clippy -- -W clippy::all -W clippy::pedantic` — fix all warnings
- [ ] Run `cargo fmt` — consistent formatting
- [ ] Add doc comments (`///`) to all public types and functions
- [ ] Add `#[cfg(test)]` module to `config.rs` and `db.rs` with basic unit tests
- [ ] Ensure all `unwrap()` calls are replaced with proper error propagation

### 5.4 — Documentation

- [ ] Rewrite `README.md` with:
  - Project description and motivation
  - Installation (from source with `cargo install --path .`)
  - Usage examples (TUI mode + headless mode)
  - CLI flags table
  - Configuration file format and location
  - SteamGridDB API key instructions
  - Screenshots placeholder (add after first working build)
  - License
- [ ] Add `LICENSE` file (MIT)

---

## Dependency Graph

```
Phase 0 (Bootstrap)
   │
   ├──► Phase 1 (Data Layer)
   │       │
   │       └──► Phase 2 (Download Engine)
   │               │
   ├──► Phase 3 (TUI Infrastructure) ◄─── can start in parallel with Phase 1
   │               │
   │               ▼
   └──────► Phase 4 (Integration)
               │
               ▼
           Phase 5 (Polish)
```

## Milestone Checkpoints

| Milestone | Phase | Verification |
|---|---|---|
| **M0: Compiles** | 0 | `cargo check` passes with all stubs |
| **M1: Reads data** | 1 | Can load config, read Lutris DB, parse API responses |
| **M2: Downloads work** | 2 | Headless download of 1 game succeeds end-to-end |
| **M3: TUI renders** | 3 | All screens display correctly with mock data |
| **M4: Full integration** | 4 | Complete TUI flow works: key entry → asset select → download → done |
| **M5: Release-ready** | 5 | Clippy clean, documented, handles all edge cases |

## Estimated Complexity

| Module | Est. Lines | Difficulty |
|---|---|---|
| `config.rs` | ~80 | Low |
| `db.rs` | ~60 | Low |
| `api/models.rs` | ~80 | Low |
| `api/client.rs` | ~150 | Medium |
| `download.rs` | ~200 | Medium |
| `event.rs` | ~80 | Medium |
| `tui.rs` | ~30 | Low |
| `app.rs` | ~300 | High |
| `ui.rs` | ~350 | High |
| `main.rs` | ~120 | Medium |
| **Total** | **~1,450** | — |

## Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| SteamGridDB API changes | High | Pin to v2 endpoints, handle unknown fields with `#[serde(default)]` |
| Lutris DB schema changes | Medium | Use `Option<T>` for non-essential columns, test against real DB |
| ratatui breaking changes | Low | Pin to 0.30.x, review changelogs before upgrading |
| Rate limiting on large libraries | Medium | Configurable delay, exponential backoff on 429 |
| Non-UTF8 game names | Low | Rust strings are UTF-8; SQLite text should be fine |
| Terminal rendering issues | Low | Test on common terminals (kitty, alacritty, gnome-terminal) |
