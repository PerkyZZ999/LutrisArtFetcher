# Lutris Art Fetcher

A fast, interactive TUI application that downloads cover art (grids, heroes, logos, and icons) for your installed [Lutris](https://lutris.net/) games from [SteamGridDB](https://www.steamgriddb.com/).

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **Full TUI** — interactive terminal interface built with [ratatui](https://ratatui.rs/)
- **4 asset types** — grids, heroes, logos, and icons
- **Smart matching** — resolves games by Steam app ID first, falls back to name search
- **Concurrent downloads** — configurable parallelism with semaphore-limited tasks
- **Atomic writes** — saves images via `.tmp` → `rename` to prevent corruption
- **Headless mode** — `--no-tui` for scripting and CI
- **Dry-run mode** — `--dry-run` to preview what would be downloaded
- **XDG config** — persists API key and preferences at `~/.config/lutrisartfetcher/config.toml`
- **Vim keybindings** — `j`/`k` navigation, space to toggle, `?` for help

## Requirements

- Rust 1.80+ (builds SQLite from source via `rusqlite` bundled feature)
- A [SteamGridDB API key](https://www.steamgriddb.com/profile/preferences/api) (free)
- Lutris installed with at least one game

## Installation

### From source

```bash
git clone https://github.com/PerkyZZ999/LutrisArtFetcher.git
cd LutrisArtFetcher
cargo build --release
```

The binary will be at `target/release/lutrisartfetcher` (≈6 MB with LTO + strip).

### Add to PATH

To run `lutrisartfetcher` from anywhere, add the release directory to your shell's PATH:

```bash
# Bash
echo 'export PATH="$HOME/path/to/LutrisArtFetcher/target/release:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Zsh
echo 'export PATH="$HOME/path/to/LutrisArtFetcher/target/release:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Fish
fish_add_path $HOME/path/to/LutrisArtFetcher/target/release
```

Replace `path/to/LutrisArtFetcher` with the actual location of the cloned repo. After that, just run:

```bash
lutrisartfetcher
```

### Run directly (without PATH)

```bash
cargo run --release
```

## Usage

### Interactive TUI (default)

```bash
./target/release/lutrisartfetcher
```

1. Enter your SteamGridDB API key (saved for future runs)
2. Select which asset types to download
3. Review your game list
4. Press Enter to start downloading
5. Watch real-time progress

### Headless mode

```bash
./target/release/lutrisartfetcher --no-tui
```

### Dry run

```bash
./target/release/lutrisartfetcher --dry-run
```

### CLI options

```
Options:
      --no-tui                     Run without TUI (headless stdout output)
      --force                      Re-download existing covers
      --dry-run                    Show what would be downloaded
      --assets <ASSETS>            Asset types (comma-separated: grids,heroes,logos,icons)
                                   [default: grids,heroes,logos,icons]
      --concurrency <CONCURRENCY>  Max parallel downloads [default: 3]
  -h, --help                       Print help
  -V, --version                    Print version
```

## Configuration

Config is stored at `~/.config/lutrisartfetcher/config.toml`:

```toml
api_key = "your-steamgriddb-api-key"
preferred_grid_dimension = "600x900"
max_concurrent_downloads = 3
nsfw_filter = true
humor_filter = false
request_delay_ms = 200
```

## File layout

Assets are saved to Lutris's standard directories:

| Asset | Path |
|-------|------|
| Grid  | `~/.local/share/lutris/coverart/{slug}.jpg` |
| Hero  | `~/.local/share/lutris/heroes/{slug}.jpg` |
| Logo  | `~/.local/share/lutris/logos/{slug}.jpg` |
| Icon  | `~/.local/share/icons/hicolor/128x128/apps/lutris_{slug}.png` |

Restart Lutris after downloading to see the new art.

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `PgDn` / `PgUp` | Page down / up |
| `Home` / `End` | Jump to first / last |
| `Space` | Toggle selection |
| `a` | Toggle all |
| `Enter` | Confirm / proceed |
| `q` / `Esc` | Quit / go back |
| `?` | Toggle help |
| `Ctrl+C` | Force quit |

## Project structure

```
src/
├── main.rs          # CLI parsing, mode dispatch
├── config.rs        # TOML config + XDG paths
├── db.rs            # Lutris SQLite reader
├── api/
│   ├── mod.rs       # Module re-exports
│   ├── models.rs    # API response types + enums
│   └── client.rs    # SteamGridDB HTTP client
├── download.rs      # Download orchestration + atomic writes
├── tui.rs           # Terminal lifecycle (raw mode, alternate screen)
├── event.rs         # Async event system (keys, ticks, progress)
├── app.rs           # State machine + key handling
└── ui.rs            # ratatui rendering (all screens)
```

## License

MIT
