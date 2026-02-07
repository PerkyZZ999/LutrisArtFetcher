## Plan: Rewrite Lutris Art Downloader as Rust TUI (`lutrisartfetcher`)

Full rewrite of the Python script into a modern Rust TUI application using **ratatui**, **tokio**, **reqwest**, and **rusqlite**. The app will provide an interactive terminal interface to browse installed Lutris games and download cover art (Grids, Heroes, Logos, Icons) from SteamGridDB, with CLI flags for headless automation.

**Steps**

### 1. Scaffold the Rust project

Replace the current Python project contents. Initialize a new Cargo project in-place at LutrisArtDownloader with `cargo init --name lutrisartfetcher`. Remove main.py, requirements.txt, run.sh, and the `env/` venv directory. Create a `.gitignore` for Rust (`/target`, etc.).

Set up `Cargo.toml` with these dependencies:

| Crate | Version | Features | Purpose |
|---|---|---|---|
| `ratatui` | `0.30` | default (crossterm backend) | TUI framework |
| `crossterm` | `0.29` | `event-stream` | Terminal event polling (async-compatible) |
| `tokio` | `1` | `rt-multi-thread`, `macros`, `fs`, `sync`, `time` | Async runtime |
| `reqwest` | `0.13` | `json`, `rustls-tls` | HTTP client for SteamGridDB API |
| `rusqlite` | `0.38` | `bundled` | SQLite access for Lutris DB |
| `serde` | `1.0` | `derive` | Serialization |
| `serde_json` | `1.0` | — | JSON parsing |
| `toml` | `0.8` | — | Config file parsing/writing |
| `dirs` | `6.0` | — | XDG directory resolution |
| `color-eyre` | `0.6` | — | Error handling + panic hooks |
| `clap` | `4.5` | `derive` | CLI argument parsing |
| `futures` | `0.3` | — | Stream utilities for async event polling |

### 2. Create the module structure

```
src/
├── main.rs          # Entry point: CLI parsing, terminal init, run loop
├── app.rs           # App state machine + update logic
├── ui.rs            # All ratatui rendering (layout, widgets)
├── event.rs         # Async event loop (crossterm events + download progress channel)
├── tui.rs           # Terminal setup/teardown helpers
├── api/
│   ├── mod.rs       # Re-exports
│   ├── client.rs    # SteamGridDB API client wrapper
│   └── models.rs    # Serde structs for all API responses
├── db.rs            # Lutris SQLite queries
├── config.rs        # Config file & API key management
└── download.rs      # Image download orchestration
```

### 3. Implement `config.rs` — Configuration management

- Define a `Config` struct with fields: `api_key: Option<String>`, `preferred_dimensions: GridDimensions`, `max_concurrent_downloads: u8`, `nsfw_filter: bool`, `humor_filter: bool`
- Store at `~/.config/lutrisartfetcher/config.toml` (using `dirs::config_dir()`)
- Implement `Config::load()` → reads and deserializes from TOML, creates default if missing
- Implement `Config::save()` → serializes and writes to TOML
- The API key is stored inside the TOML file (no more separate `apikey.txt`)

### 4. Implement `db.rs` — Lutris database reader

- Define a `Game` struct: `id: i64`, `name: String`, `slug: String`, `runner: Option<String>`, `platform: Option<String>`, `service: Option<String>`, `service_id: Option<String>`, `has_custom_banner: bool`, `has_custom_coverart: bool`
- Resolve DB path via `dirs::data_dir().join("lutris/pga.db")` — verify file exists and is readable before opening
- Query: `SELECT id, name, slug, runner, platform, installed, service, service_id, has_custom_banner, has_custom_coverart_big FROM games WHERE installed = 1`
- Return `Vec<Game>` sorted alphabetically by name
- All `rusqlite` work is synchronous — read everything into memory, then drop the connection before any async work begins (rusqlite `Connection` is not `Send`)

### 5. Implement `api/models.rs` — SteamGridDB data types

Define serde structs for:
- `ApiResponse<T>` — wraps `{ success: bool, data: Vec<T> }`  
- `SearchResult` — `{ id: u64, name: String, types: Vec<String>, verified: bool }`
- `GridImage` — `{ id: u64, score: i32, style: String, width: u32, height: u32, nsfw: bool, humor: bool, mime: String, url: String, thumb: String }`
- `HeroImage`, `LogoImage`, `IconImage` — similar structures for each asset type
- Enum `AssetType { Grid, Hero, Logo, Icon }` with associated path + API endpoint + dimension info
- Enum `GridDimension` with variants for all supported sizes: `D460x215`, `D920x430`, `D600x900`, etc.

### 6. Implement `api/client.rs` — SteamGridDB API client

- `SteamGridDbClient` struct holding `reqwest::Client` and `api_key: String`
- Methods:
  - `validate_key() -> Result<bool>` — hits `/grids/game/1` to verify
  - `search(term: &str) -> Result<Vec<SearchResult>>` — `GET /search/autocomplete/{term}` (convert slugs: replace `-` with spaces for better results)
  - `get_grids(game_id: u64, dimensions: &str) -> Result<Vec<GridImage>>`
  - `get_heroes(game_id: u64) -> Result<Vec<HeroImage>>`
  - `get_logos(game_id: u64) -> Result<Vec<LogoImage>>`
  - `get_icons(game_id: u64) -> Result<Vec<IconImage>>`
  - `get_grids_by_platform(platform: &str, platform_id: &str) -> Result<Vec<GridImage>>` — direct lookup when `service`/`service_id` is available (avoids fuzzy search)
- Add a configurable delay between requests (default 100ms) to avoid rate limiting
- All methods return `color_eyre::Result<T>`

### 7. Implement `download.rs` — Download orchestration

- `DownloadTask` struct: `game: Game`, `asset_type: AssetType`, `status: DownloadStatus`
- `DownloadStatus` enum: `Pending`, `Searching`, `Downloading`, `Done(PathBuf)`, `Skipped(reason)`, `Failed(String)`
- Determine Lutris asset paths:
  - Banners/Grids → `$XDG_DATA_HOME/lutris/banners/{slug}.jpg`
  - Coverart → `$XDG_DATA_HOME/lutris/coverart/{slug}.jpg`
  - Icons → `$XDG_DATA_HOME/icons/hicolor/128x128/apps/lutris_{slug}.png`
  - Heroes → `$XDG_DATA_HOME/lutris/heroes/{slug}.jpg` (if Lutris supports it)
- Key logic per game:
  1. Check if file already exists → `Skipped("already exists")`
  2. If `service == "steam"` and `service_id` exists → use platform-specific endpoint (exact match)
  3. Otherwise → search by `slug.replace('-', ' ')` → take first result's `id`
  4. Fetch asset list for that `id` → take first non-NSFW/humor result
  5. Download image bytes → save to target path
- Send progress updates through a `tokio::sync::mpsc` channel back to the TUI
- Use a `tokio::sync::Semaphore` to limit concurrent downloads (default 3)

### 8. Implement `app.rs` — Application state machine

- `App` struct holding:
  - `games: Vec<GameEntry>` (game + per-asset download status)
  - `list_state: ListState` (ratatui scrollable list state)
  - `state: AppScreen` enum: `ApiKeyEntry { input, cursor_pos }`, `AssetTypeSelection`, `GameList`, `Downloading { current, total }`, `Done { summary }`
  - `log_messages: Vec<(LogLevel, String)>` — activity log
  - `progress: f64` (0.0–1.0 overall)
  - `selected_assets: HashSet<AssetType>` — which asset types to download
  - `should_quit: bool`
  - `show_help: bool`
- `App::update(event: AppEvent)` method handles:
  - Key events → navigate list, toggle selections, trigger downloads, enter API key, quit
  - Download progress events → update game statuses, advance progress, append log
  - Tick events → refresh UI (for spinners/animations)
- `GameEntry` struct: `game: Game`, `grid_status: DownloadStatus`, `hero_status: DownloadStatus`, `logo_status: DownloadStatus`, `icon_status: DownloadStatus`

### 9. Implement `event.rs` — Async event system

- `AppEvent` enum: `Key(KeyEvent)`, `Tick`, `DownloadProgress { game_slug, asset_type, status }`, `Resize(u16, u16)`
- `EventHandler` struct containing a `tokio::sync::mpsc::UnboundedReceiver<AppEvent>`
- Spawn two tasks:
  1. **Input task**: Polls `crossterm::event::EventStream` (from `event-stream` feature) → sends `AppEvent::Key` / `AppEvent::Resize`
  2. **Tick task**: Sends `AppEvent::Tick` every 250ms
- Download tasks send `AppEvent::DownloadProgress` through a clone of the sender

### 10. Implement `ui.rs` — TUI rendering

Layout (responsive based on terminal size):

```
┌─── Lutris Art Fetcher ─────────────────────────────────┐
│ ┌── Games (47 installed) ────┐ ┌── Status ───────────┐ │
│ │  ✓ Celeste                 │ │ Mode: All Assets    │ │
│ │  ✓ Cyberpunk 2077          │ │ Progress: 23/47     │ │
│ │  ↓ Hades                   │ │ ████████░░░░░ 49%   │ │
│ │    Hollow Knight            │ │                     │ │
│ │    The Witcher 3            │ │ Current:            │ │
│ │                             │ │  Hades — Grid ↓    │ │
│ │                             │ │                     │ │
│ └─────────────────────────────┘ └─────────────────────┘ │
│ ┌── Log ─────────────────────────────────────────────┐  │
│ │ [INFO] Found Hades (SteamGridDB #12345)            │  │
│ │ [INFO] Downloading grid (600x900)...               │  │
│ │ [OK]   Celeste — all 4 assets saved                │  │
│ └────────────────────────────────────────────────────┘  │
│ q:Quit  Enter:Start All  Space:Toggle  ↑↓:Nav  ?:Help  │
└─────────────────────────────────────────────────────────┘
```

- **Left panel**: `List` widget with `ListState` — shows all games with status icons (✓ done, ↓ downloading, ✗ failed, · pending)
- **Right panel**: `Gauge` for overall progress + current game status
- **Bottom panel**: `Paragraph` scrollable log with color-coded log levels (green=OK, yellow=WARN, red=ERR, white=INFO)
- **Footer**: `Paragraph` showing keybindings
- **API Key screen**: Centered `Paragraph` with text input field
- **Asset type selection screen**: `List` with multi-select checkboxes for Grid/Hero/Logo/Icon
- **Help popup**: Overlay `Paragraph` (`Clear` + `Block` centered) with all keybindings
- Color scheme: use ratatui `Style`s — cyan for borders, green for success, yellow for active, red for errors
- All rendering via a single `pub fn render(frame: &mut Frame, app: &App)` that dispatches based on `app.state`

### 11. Implement `tui.rs` — Terminal lifecycle

- `fn init() -> Result<Terminal<CrosstermBackend<Stdout>>>` — enable raw mode, enter alternate screen, enable mouse capture, set panic hook (restore terminal on panic via `color_eyre`)
- `fn restore() -> Result<()>` — disable raw mode, leave alternate screen
- Use ratatui's built-in `ratatui::init()` / `ratatui::restore()` if available in 0.30, otherwise manual crossterm setup

### 12. Implement `main.rs` — Entry point

- Parse CLI args with `clap` derive API:
  - `--no-tui` → headless mode (prints progress to stdout, no ratatui)
  - `--force` → re-download existing covers
  - `--dry-run` → show what would happen without downloading
  - `--assets <types>` → comma-separated: `grids,heroes,logos,icons` (default: all)
  - `--banner-only` / `--cover-only` → shortcuts for grid dimension
  - `--concurrency <n>` → max parallel downloads (default: 3)
- Init `color_eyre`
- Load config from `~/.config/lutrisartfetcher/config.toml`
- Read games from Lutris DB (with existence check + friendly error)
- Branch:
  - **TUI mode** (default): Init terminal → create `App` → run async event loop → restore terminal
  - **Headless mode** (`--no-tui`): Run downloads sequentially with stdout progress

### 13. Update README.md — New documentation

Rewrite the README reflecting:
- New project name (`lutrisartfetcher`)
- Rust installation prerequisite
- Build instructions (`cargo build --release`)
- Usage with screenshots of the TUI
- CLI flags documentation
- Configuration file location and format
- SteamGridDB API key instructions
- Supported asset types

### 14. Clean up old Python artifacts

Remove:
- main.py
- requirements.txt
- run.sh
- `env/` directory (entire Python venv)

**Verification**

1. `cargo build --release` compiles without errors or warnings
2. `cargo clippy` passes clean
3. Run `./target/release/lutrisartfetcher` — should detect Lutris DB, show TUI with game list
4. Run `./target/release/lutrisartfetcher --dry-run` — should list games + planned downloads without writing files
5. Run `./target/release/lutrisartfetcher --no-tui` — should work headlessly with stdout output
6. Verify downloaded covers appear in `~/.local/share/lutris/banners/` and Lutris picks them up after restart
7. Test with no API key configured — should show the in-TUI key entry screen
8. Test with an invalid API key — should show a clear error
9. Test with no Lutris installed (no `pga.db`) — should show a friendly error, not a crash

**Decisions**

- **Chose `reqwest` over `steamgriddb_api` crate**: The existing crate is 4+ years unmaintained and doesn't cover all endpoints (heroes, logos, icons). A thin custom client with `reqwest` + `serde` gives full control and stays current.
- **Chose `rusqlite` with `bundled`**: Avoids requiring system SQLite dev headers — bundles its own SQLite for zero-hassle builds.
- **Chose `rustls-tls` over `native-tls` for reqwest**: Pure Rust TLS, no OpenSSL dependency — simpler cross-compilation and fewer system deps.
- **Corrected asset paths**: The original Python script used `~/.cache/lutris/` but Lutris actually stores art in `~/.local/share/lutris/` (confirmed from Lutris source). The Rust version uses the correct XDG data directory.
- **Platform-aware lookups**: When a game has `service = "steam"` and a `service_id`, we use SteamGridDB's platform endpoint for an exact match instead of fuzzy text search — much more reliable.
- **Config at `~/.config/lutrisartfetcher/config.toml`**: Per your preference — follows XDG conventions and supports extensible settings beyond just the API key.
- **Save as `.jpg` regardless of actual mime type**: Lutris expects `{slug}.jpg` filenames — preserving this behavior for compatibility even though the source image may be PNG/WebP.