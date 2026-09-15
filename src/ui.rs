/// TUI rendering — all ratatui layout and widget code.
///
/// Dispatches to a screen-specific renderer based on `App.screen`, then
/// optionally overlays the help popup.
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use crate::api::models::{AssetType, DownloadStatus};
use crate::app::{App, AppScreen, LogLevel};
use crate::download;

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

const BORDER: Color = Color::Cyan;
const BORDER_DIM: Color = Color::DarkGray;
const TITLE: Color = Color::White;
const ACCENT: Color = Color::Cyan;
const HIGHLIGHT_FG: Color = Color::Yellow;
const HIGHLIGHT_BG: Color = Color::DarkGray;
const SUCCESS: Color = Color::Green;
const ERROR: Color = Color::LightRed;
const WARNING: Color = Color::Yellow;
const MUTED: Color = Color::DarkGray;
const TEXT: Color = Color::White;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[allow(clippy::cast_possible_truncation)]
fn spinner(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

/// Ordered asset types that are currently enabled (stable order, not `HashSet` order).
fn ordered_assets(app: &App) -> Vec<AssetType> {
    AssetType::all()
        .iter()
        .copied()
        .filter(|a| app.selected_assets.contains(a))
        .collect()
}

fn asset_description(asset: AssetType) -> &'static str {
    match asset {
        AssetType::Grid => "Cover grid · 600×900 library art",
        AssetType::Hero => "Wide hero · detail-page banner",
        AssetType::Logo => "Transparent logo overlay",
        AssetType::Icon => "128×128 application icon",
    }
}

fn status_icon(status: &DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Pending => "·",
        DownloadStatus::Searching => "⟳",
        DownloadStatus::Downloading => "↓",
        DownloadStatus::Done(_) => "✓",
        DownloadStatus::Skipped(_) => "─",
        DownloadStatus::Failed(_) => "✗",
    }
}

fn status_color(status: &DownloadStatus) -> Color {
    match status {
        DownloadStatus::Pending | DownloadStatus::Skipped(_) => MUTED,
        DownloadStatus::Searching | DownloadStatus::Downloading => WARNING,
        DownloadStatus::Done(_) => SUCCESS,
        DownloadStatus::Failed(_) => ERROR,
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

/// Consistent rounded block used across all screens.
fn styled_block(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(TITLE).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { BORDER } else { BORDER_DIM }))
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Render the entire TUI for one frame.
///
/// Takes `&mut App` so stateful widgets (list scroll offset) survive between frames.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 70 || area.height < 20 {
        render_too_small(frame, area);
        return;
    }

    // Copy out the discriminant first so sub-renderers can borrow `app` mutably.
    let kind = match app.screen {
        AppScreen::ApiKeyEntry { .. } => 0,
        AppScreen::AssetTypeSelection { .. } => 1,
        AppScreen::GameList | AppScreen::Downloading { .. } => 2,
        AppScreen::Done { .. } => 3,
    };
    match kind {
        0 => render_api_key_screen(frame, app),
        1 => render_asset_selection(frame, app),
        2 => render_main_view(frame, app),
        _ => render_done_screen(frame, app),
    }

    if app.show_help {
        render_help_popup(frame);
    }
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let msg = Paragraph::new(vec![
        Line::from(Span::styled(
            "Terminal too small",
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Resize to at least 70×20 to use Lutris Art Fetcher.",
            Style::default().fg(MUTED),
        )),
    ])
    .alignment(Alignment::Center)
    .block(styled_block(" Lutris Art Fetcher ", true));
    frame.render_widget(msg, area);
}

// ---------------------------------------------------------------------------
// Shared header / footer
// ---------------------------------------------------------------------------

fn step_index(screen: &AppScreen) -> usize {
    match screen {
        AppScreen::ApiKeyEntry { .. } => 0,
        AppScreen::AssetTypeSelection { .. } => 1,
        AppScreen::GameList => 2,
        AppScreen::Downloading { .. } => 3,
        AppScreen::Done { .. } => 4,
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let steps = ["Key", "Assets", "Games", "Fetch", "Done"];
    let current = step_index(&app.screen);

    let mut spans = vec![Span::styled(
        "◈ Lutris Art Fetcher ",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )];

    for (i, name) in steps.iter().enumerate() {
        let sep = if i == 0 { "  " } else { " › " };
        spans.push(Span::styled(sep, Style::default().fg(MUTED)));
        match i.cmp(&current) {
            std::cmp::Ordering::Less => {
                spans.push(Span::styled(
                    format!("✓ {name}"),
                    Style::default().fg(SUCCESS),
                ));
            }
            std::cmp::Ordering::Equal => {
                spans.push(Span::styled(
                    format!("● {name}"),
                    Style::default()
                        .fg(HIGHLIGHT_FG)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            std::cmp::Ordering::Greater => {
                spans.push(Span::styled(
                    format!("○ {name}"),
                    Style::default().fg(MUTED),
                ));
            }
        }
    }
    spans.push(Span::styled("   ? help", Style::default().fg(MUTED)));

    let bar = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    frame.render_widget(bar, area);
}

/// A single `key action` hint: key in cyan bold, description in muted.
fn hint<'a>(key: &'a str, desc: &'a str) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            key,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {desc}"), Style::default().fg(MUTED)),
        Span::styled("   ", Style::default().fg(MUTED)),
    ]
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();
    match &app.screen {
        AppScreen::ApiKeyEntry { .. } => {
            spans.extend(hint("Enter", "confirm"));
            spans.extend(hint("Esc", "quit"));
            spans.extend(hint("?", "help"));
        }
        AppScreen::AssetTypeSelection { .. } => {
            spans.extend(hint("↑↓/jk", "move"));
            spans.extend(hint("Space", "toggle"));
            spans.extend(hint("a", "all"));
            spans.extend(hint("Enter", "confirm"));
            spans.extend(hint("q", "quit"));
        }
        AppScreen::GameList => {
            if app.filter_active {
                spans.extend(hint("type", "to filter"));
                spans.extend(hint("Enter", "apply"));
                spans.extend(hint("Esc", "clear"));
            } else {
                spans.extend(hint("↑↓/jk", "move"));
                spans.extend(hint("Space", "select"));
                spans.extend(hint("a", "all"));
                spans.extend(hint("/", "filter"));
                spans.extend(hint("Enter", "fetch"));
                spans.extend(hint("Esc", "back"));
                spans.extend(hint("q", "quit"));
            }
        }
        AppScreen::Downloading { .. } => {
            spans.push(Span::styled(
                format!("{} fetching…  ", spinner(app.tick_count)),
                Style::default().fg(WARNING),
            ));
            spans.extend(hint("q", "abort"));
            spans.extend(hint("?", "help"));
        }
        AppScreen::Done { .. } => {
            spans.extend(hint("Enter/q", "exit"));
            spans.push(Span::styled(
                "Restart Lutris to see the new art",
                Style::default().fg(MUTED),
            ));
        }
    }
    let footer = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    frame.render_widget(footer, area);
}

// ---------------------------------------------------------------------------
// API Key Entry
// ---------------------------------------------------------------------------

fn render_api_key_screen(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(0),    // card
        Constraint::Length(1), // footer
    ])
    .split(area);
    render_header(frame, rows[0], app);

    let card = centered_card(74, 17, rows[1]);
    frame.render_widget(styled_block("Setup · SteamGridDB API key", true), card);
    let inner = padded_inner(card);

    let chunks = Layout::vertical([
        Constraint::Length(2), // description
        Constraint::Length(3), // input
        Constraint::Length(2), // status / error
        Constraint::Min(0),    // key URL + tips
    ])
    .split(inner);

    let desc = Paragraph::new(vec![
        Line::from(Span::styled(
            "Connect SteamGridDB to download artwork for your Lutris library.",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "Paste your API key below — it is stored locally in your config.",
            Style::default().fg(MUTED),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(desc, chunks[0]);

    render_api_key_field(frame, chunks[1], app);
    render_api_key_status(frame, chunks[2], app);

    let url = Paragraph::new(vec![
        Line::from(Span::styled(
            "Get a free key at steamgriddb.com → Profile → Preferences → API",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Your key never leaves this machine except to call SteamGridDB.",
            Style::default().fg(MUTED),
        )),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(url, chunks[3]);

    render_footer(frame, rows[2], app);
}

/// Input row of the API-key screen, with real cursor rendering or a spinner.
fn render_api_key_field(frame: &mut Frame, area: Rect, app: &App) {
    let AppScreen::ApiKeyEntry {
        input,
        cursor_pos,
        error_msg: _,
        validating,
    } = &app.screen
    else {
        return;
    };

    if *validating {
        let body = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", spinner(app.tick_count)),
                Style::default().fg(WARNING),
            ),
            Span::styled(
                "Validating key with SteamGridDB…",
                Style::default().fg(TEXT),
            ),
        ]))
        .alignment(Alignment::Center)
        .block(styled_block("API key", true));
        frame.render_widget(body, area);
        return;
    }

    let width = area.width.saturating_sub(4) as usize;
    let cursor = (*cursor_pos).min(input.len());
    // Keep cursor visible: show the tail that fits.
    let visible = if input.len() > width && width > 4 {
        let start = input.len().saturating_sub(width);
        &input[start..]
    } else {
        input.as_str()
    };
    let rel_cursor = cursor.saturating_sub(input.len().saturating_sub(visible.len()));

    let (before, after) = visible.split_at(rel_cursor.min(visible.len()));
    let cursor_char = after.chars().next().unwrap_or(' ').to_string();
    let rest: String = if after.is_empty() {
        String::new()
    } else {
        after.chars().skip(1).collect()
    };

    let mut line_spans = vec![Span::raw(" "), Span::raw(before)];
    if input.is_empty() {
        line_spans.push(Span::styled("paste key here…", Style::default().fg(MUTED)));
        line_spans.push(Span::styled(
            "▌",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::SLOW_BLINK),
        ));
    } else {
        line_spans.push(Span::styled(
            cursor_char,
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        line_spans.push(Span::raw(rest));
    }
    let body = Paragraph::new(Line::from(line_spans)).block(styled_block("API key", true));
    frame.render_widget(body, area);
}

/// Status / error line beneath the API-key input.
fn render_api_key_status(frame: &mut Frame, area: Rect, app: &App) {
    let AppScreen::ApiKeyEntry {
        error_msg,
        validating,
        ..
    } = &app.screen
    else {
        return;
    };

    if let Some(msg) = error_msg {
        let err = Paragraph::new(Line::from(vec![
            Span::styled("✗ ", Style::default().fg(ERROR)),
            Span::styled(msg.as_str(), Style::default().fg(ERROR)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(err, area);
    } else if *validating {
        let note = Paragraph::new("This usually takes a second…")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED));
        frame.render_widget(note, area);
    } else {
        let note = Paragraph::new("Press Enter to validate and continue")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED));
        frame.render_widget(note, area);
    }
}

// ---------------------------------------------------------------------------
// Asset Type Selection
// ---------------------------------------------------------------------------

fn render_asset_selection(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    render_header(frame, rows[0], app);

    let card = centered_card(74, 18, rows[1]);
    frame.render_widget(styled_block("Step 2 · Choose artwork to fetch", true), card);
    let inner = padded_inner(card);

    let chunks = Layout::vertical([
        Constraint::Length(1), // instructions
        Constraint::Min(4),    // list
        Constraint::Length(2), // count + hint
    ])
    .split(inner);

    let instructions = Paragraph::new("Pick what to download — Space toggles, Enter continues.")
        .alignment(Alignment::Center)
        .style(Style::default().fg(MUTED));
    frame.render_widget(instructions, chunks[0]);

    if let AppScreen::AssetTypeSelection { cursor } = &app.screen {
        let all = AssetType::all();
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(*cursor));

        let items: Vec<ListItem> = all
            .iter()
            .enumerate()
            .map(|(i, asset)| {
                let checked = app.selected_assets.contains(asset);
                let box_span = if checked {
                    Span::styled(
                        "[●] ",
                        Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("[○] ", Style::default().fg(MUTED))
                };
                let name_style = if i == *cursor {
                    Style::default()
                        .fg(HIGHLIGHT_FG)
                        .add_modifier(Modifier::BOLD)
                } else if checked {
                    Style::default().fg(TEXT)
                } else {
                    Style::default().fg(MUTED)
                };
                ListItem::new(Line::from(vec![
                    Span::raw(" "),
                    box_span,
                    Span::styled(asset.display_name(), name_style),
                    Span::styled(
                        format!("  — {}", asset_description(*asset)),
                        Style::default().fg(MUTED),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        frame.render_stateful_widget(list, chunks[1], &mut state);

        let count = Paragraph::new(if app.selected_assets.is_empty() {
            Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(WARNING)),
                Span::styled(
                    "Select at least one asset type to continue.",
                    Style::default().fg(WARNING),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{} of {} selected", app.selected_assets.len(), all.len()),
                    Style::default().fg(SUCCESS),
                ),
                Span::styled("   ·   a toggles everything", Style::default().fg(MUTED)),
            ])
        })
        .alignment(Alignment::Center);
        frame.render_widget(count, chunks[2]);
    }

    render_footer(frame, rows[2], app);
}

// ---------------------------------------------------------------------------
// Main View (GameList + Downloading)
// ---------------------------------------------------------------------------

fn render_main_view(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(8),    // body
        Constraint::Length(8), // log
        Constraint::Length(1), // footer
    ])
    .split(area);
    render_header(frame, rows[0], app);

    let body =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(rows[1]);
    render_game_list(frame, app, body[0]);
    render_details_panel(frame, app, body[1]);
    render_log_panel(frame, app, rows[2]);
    render_footer(frame, rows[3], app);
}

fn render_game_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let visible = app.visible_indices();
    let total = app.games.len();
    let selected = app.selected_count();
    let title = if app.filter_query.is_empty() {
        format!("Games · {selected}/{total} selected")
    } else {
        format!(
            "Games · {selected}/{total} selected · {} shown",
            visible.len()
        )
    };

    let outer = styled_block(&title, true);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // filter bar
        Constraint::Min(0),    // list
    ])
    .split(inner);

    render_filter_bar(frame, app, &visible, chunks[0]);

    if total == 0 {
        let empty = Paragraph::new("No installed games found in the Lutris database.")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED));
        frame.render_widget(empty, chunks[1]);
        return;
    }
    if visible.is_empty() {
        let empty = Paragraph::new(format!(
            "No games match “{}”. Press Esc to clear the filter.",
            app.filter_query
        ))
        .alignment(Alignment::Center)
        .style(Style::default().fg(WARNING));
        frame.render_widget(empty, chunks[1]);
        return;
    }

    clamp_list_selection(app, visible.len());
    let items = build_game_items(app, &visible);
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(HIGHLIGHT_FG)
                .bg(HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);

    // Scrollbar on the list's right edge.
    if visible.len() > 1 {
        let mut sb_state =
            ScrollbarState::new(visible.len()).position(app.list_state.selected().unwrap_or(0));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .style(Style::default().fg(BORDER_DIM));
        frame.render_stateful_widget(scrollbar, chunks[1], &mut sb_state);
    }
}

/// Single-line filter bar above the game list.
fn render_filter_bar(frame: &mut Frame, app: &App, visible: &[usize], area: Rect) {
    if app.filter_active {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled(
                "/ ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(app.filter_query.as_str()),
            Span::styled(
                "▌",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]))
        .style(Style::default().fg(TEXT));
        frame.render_widget(bar, area);
    } else if app.filter_query.is_empty() {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled("  / to filter", Style::default().fg(MUTED)),
            Span::styled(
                format!("      {} games", visible.len()),
                Style::default().fg(MUTED),
            ),
        ]));
        frame.render_widget(bar, area);
    } else {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled("  Filter: ", Style::default().fg(MUTED)),
            Span::styled(app.filter_query.as_str(), Style::default().fg(TEXT)),
            Span::styled("   (C clears)", Style::default().fg(MUTED)),
        ]));
        frame.render_widget(bar, area);
    }
}

/// Keep the list cursor inside the filtered view.
fn clamp_list_selection(app: &mut App, len: usize) {
    if len == 0 {
        app.list_state.select(None);
    } else if let Some(cur) = app.list_state.selected() {
        if cur >= len {
            app.list_state.select(Some(len - 1));
        }
    } else {
        app.list_state.select(Some(0));
    }
}

/// Build one list row per visible game: checkbox, status, name, badges.
fn build_game_items(app: &App, visible: &[usize]) -> Vec<ListItem<'static>> {
    let assets = ordered_assets(app);
    visible
        .iter()
        .map(|&real_idx| {
            let entry = &app.games[real_idx];
            let check = if entry.selected {
                Span::styled("[x] ", Style::default().fg(SUCCESS))
            } else {
                Span::styled("[ ] ", Style::default().fg(MUTED))
            };
            let icon = entry.overall_icon(&app.selected_assets);
            let icon_color = match icon {
                "✓" => SUCCESS,
                "↓" | "⟳" => WARNING,
                "✗" => ERROR,
                _ => MUTED,
            };
            let mut spans = vec![
                Span::raw(" "),
                check,
                Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
                Span::styled(
                    entry.game.name.clone(),
                    Style::default().fg(if entry.selected { TEXT } else { MUTED }),
                ),
            ];
            if let Some(runner) = entry.game.runner.as_deref() {
                spans.push(Span::styled(
                    format!("  · {runner}"),
                    Style::default().fg(MUTED),
                ));
            }
            if !assets.is_empty() {
                spans.push(Span::styled("   ", Style::default()));
                for asset in &assets {
                    let st = entry.status(*asset);
                    let short = match asset {
                        AssetType::Grid => "G",
                        AssetType::Hero => "H",
                        AssetType::Logo => "L",
                        AssetType::Icon => "I",
                    };
                    spans.push(Span::styled(
                        format!("{short}{} ", status_icon(st)),
                        Style::default().fg(status_color(st)),
                    ));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
}

fn render_details_panel(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(styled_block("Details", false), area);
    let inner = padded_inner(area);

    // Resolve the highlighted game (falls back to the first visible one).
    let real = app
        .visible_cursor_real_index()
        .or_else(|| app.visible_indices().first().copied());

    let Some(real_idx) = real else {
        let empty = Paragraph::new("Nothing to show.")
            .style(Style::default().fg(MUTED))
            .alignment(Alignment::Center);
        frame.render_widget(empty, inner);
        return;
    };
    let Some(entry) = app.games.get(real_idx) else {
        return;
    };

    let mut rows: Vec<Constraint> = vec![
        Constraint::Length(1), // name
        Constraint::Length(1), // slug
        Constraint::Length(1), // runner/service
        Constraint::Length(1), // blank
    ];
    let assets = ordered_assets(app);
    rows.extend(std::iter::repeat_n(Constraint::Length(1), assets.len()));
    rows.push(Constraint::Length(1)); // blank
    rows.push(Constraint::Min(3)); // progress / summary
    let chunks = Layout::vertical(rows).split(inner);

    render_details_meta(frame, entry, &chunks);
    render_details_assets(frame, entry, &assets, &chunks);
    render_details_footer(frame, app, &chunks, 4 + assets.len() + 1);
}

/// Name / slug / runner-service header of the details panel.
fn render_details_meta(frame: &mut Frame, entry: &crate::download::GameEntry, chunks: &[Rect]) {
    let name = Paragraph::new(Line::from(Span::styled(
        format!(" {}", entry.game.name),
        Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(name, chunks[0]);

    let slug = Paragraph::new(Line::from(Span::styled(
        format!(" {}", entry.game.slug),
        Style::default().fg(MUTED),
    )));
    frame.render_widget(slug, chunks[1]);

    let meta = [
        entry.game.runner.as_deref().unwrap_or("—"),
        entry.game.service.as_deref().unwrap_or("—"),
    ];
    let meta_line = Paragraph::new(Line::from(vec![
        Span::styled(" runner ", Style::default().fg(MUTED)),
        Span::styled(meta[0], Style::default().fg(TEXT)),
        Span::styled("  service ", Style::default().fg(MUTED)),
        Span::styled(meta[1], Style::default().fg(TEXT)),
    ]));
    frame.render_widget(meta_line, chunks[2]);
}

/// Per-asset status rows of the details panel.
fn render_details_assets(
    frame: &mut Frame,
    entry: &crate::download::GameEntry,
    assets: &[AssetType],
    chunks: &[Rect],
) {
    for (i, asset) in assets.iter().enumerate() {
        let st = entry.status(*asset);
        let detail = match st {
            DownloadStatus::Done(path) => path.display().to_string(),
            DownloadStatus::Skipped(reason) => format!("skipped · {reason}"),
            DownloadStatus::Failed(msg) => msg.clone(),
            DownloadStatus::Searching => "searching…".to_owned(),
            DownloadStatus::Downloading => "downloading…".to_owned(),
            DownloadStatus::Pending => {
                if download::asset_exists(*asset, &entry.game.slug) {
                    "already on disk".to_owned()
                } else {
                    "queued".to_owned()
                }
            }
        };
        // Truncate long paths to fit narrow panels.
        let max_w = chunks[4 + i].width.saturating_sub(12) as usize;
        let short = if detail.len() > max_w && max_w > 8 {
            format!("…{}", &detail[detail.len() - (max_w - 1)..])
        } else {
            detail
        };
        let line = Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", status_icon(st)),
                Style::default()
                    .fg(status_color(st))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<5}", asset.display_name()),
                Style::default().fg(TEXT),
            ),
            Span::styled(format!(" {short}"), Style::default().fg(MUTED)),
        ]));
        frame.render_widget(line, chunks[4 + i]);
    }
}

/// Progress gauge (while fetching) or selection summary below the asset rows.
fn render_details_footer(frame: &mut Frame, app: &App, chunks: &[Rect], foot_idx: usize) {
    if foot_idx >= chunks.len() {
        return;
    }
    match &app.screen {
        AppScreen::Downloading {
            current,
            total,
            started_at,
        } => {
            #[allow(clippy::cast_precision_loss)]
            let ratio = if *total == 0 {
                1.0
            } else {
                (*current as f64 / *total as f64).clamp(0.0, 1.0)
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pct = (ratio * 100.0).round() as u64;
            let label = format!(
                "{} {current}/{total} · {pct}% · {}",
                spinner(app.tick_count),
                format_duration(started_at.elapsed().as_secs())
            );
            let gauge = Gauge::default()
                .block(styled_block("Progress", true))
                .gauge_style(Style::default().fg(SUCCESS).bg(Color::DarkGray))
                .ratio(ratio)
                .label(label);
            frame.render_widget(gauge, chunks[foot_idx]);
        }
        AppScreen::GameList => {
            let existing: usize = app
                .games
                .iter()
                .filter(|e| e.selected)
                .filter(|e| {
                    ordered_assets(app)
                        .iter()
                        .all(|a| download::asset_exists(*a, &e.game.slug))
                })
                .count();
            let sel = app.selected_count();
            let info = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(format!(" {existing}"), Style::default().fg(SUCCESS)),
                    Span::styled(" game(s) already complete", Style::default().fg(MUTED)),
                ]),
                Line::from(vec![
                    Span::styled(format!(" {sel}"), Style::default().fg(ACCENT)),
                    Span::styled(" selected · Enter starts", Style::default().fg(MUTED)),
                ]),
            ]);
            frame.render_widget(info, chunks[foot_idx]);
        }
        _ => {}
    }
}

fn render_log_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!("Log · {} events", app.log.len());
    frame.render_widget(styled_block(&title, false), area);
    let inner = padded_inner(area);

    if app.log.is_empty() {
        let empty =
            Paragraph::new("No activity yet — review the list, then press Enter to fetch art.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(MUTED));
        frame.render_widget(empty, inner);
        return;
    }

    let inner_height = inner.height as usize;
    let start = app.log.len().saturating_sub(inner_height.max(1));
    let lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|(level, msg)| {
            let (glyph, color) = match level {
                LogLevel::Info => ("ℹ", ACCENT),
                LogLevel::Ok => ("✓", SUCCESS),
                LogLevel::Warn => ("⚠", WARNING),
                LogLevel::Error => ("✗", ERROR),
            };
            Line::from(vec![
                Span::styled(format!(" {glyph} "), Style::default().fg(color)),
                Span::styled(msg.clone(), Style::default().fg(TEXT)),
            ])
        })
        .collect();

    let log = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(log, inner);
}

// ---------------------------------------------------------------------------
// Done Screen
// ---------------------------------------------------------------------------

fn render_done_screen(frame: &mut Frame, app: &App) {
    let AppScreen::Done {
        downloaded,
        skipped,
        failed,
        elapsed_secs,
    } = &app.screen
    else {
        return;
    };

    let area = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    render_header(frame, rows[0], app);

    let card = centered_card(78, 24, rows[1]);
    frame.render_widget(styled_block("Complete", true), card);
    let inner = padded_inner(card);

    let failed_list: Vec<&str> = app
        .log
        .iter()
        .filter(|(lvl, _)| matches!(lvl, LogLevel::Error))
        .take(3)
        .map(|(_, msg)| msg.as_str())
        .collect();
    let has_failures = !failed_list.is_empty();

    let mut constraints = vec![
        Constraint::Length(2), // header
        Constraint::Length(7), // stats (4 rows + borders + padding)
    ];
    if has_failures {
        constraints.push(Constraint::Length(5)); // failures
    }
    constraints.push(Constraint::Min(4)); // log tail
    let chunks = Layout::vertical(constraints).split(inner);
    let mut idx = 0;

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "✓ ",
            Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "All downloads complete!",
            Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(header, chunks[idx]);
    idx += 1;

    let stats = done_stats_paragraph(*downloaded, *skipped, *failed, *elapsed_secs);
    frame.render_widget(stats, chunks[idx]);
    idx += 1;

    if has_failures {
        render_done_failures(frame, &failed_list, chunks[idx]);
        idx += 1;
    }

    if idx < chunks.len() {
        render_log_tail(frame, app, chunks[idx]);
    }

    render_footer(frame, rows[2], app);
}

/// Summary statistics block of the done screen.
fn done_stats_paragraph(
    downloaded: usize,
    skipped: usize,
    failed: usize,
    elapsed_secs: u64,
) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ✓ Downloaded  ", Style::default().fg(SUCCESS)),
            Span::styled(
                format!("{downloaded}"),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ─ Skipped     ", Style::default().fg(MUTED)),
            Span::styled(format!("{skipped}"), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled(
                "  ✗ Failed      ",
                Style::default().fg(if failed > 0 { ERROR } else { MUTED }),
            ),
            Span::styled(format!("{failed}"), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  ⏱ Elapsed     ", Style::default().fg(ACCENT)),
            Span::styled(format_duration(elapsed_secs), Style::default().fg(TEXT)),
        ]),
    ])
    .block(styled_block("Summary", false))
}

/// Notable failure lines of the done screen.
fn render_done_failures(frame: &mut Frame, failed_list: &[&str], area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        " Review these in the log above — most failures are missing upstream art.",
        Style::default().fg(MUTED),
    ))];
    for msg in failed_list {
        let short = if msg.len() > 70 {
            format!("{}…", &msg[..69])
        } else {
            (*msg).to_owned()
        };
        lines.push(Line::from(vec![
            Span::styled("  ✗ ", Style::default().fg(ERROR)),
            Span::styled(short, Style::default().fg(TEXT)),
        ]));
    }
    let failures = Paragraph::new(lines).block(styled_block("Needs attention", false));
    frame.render_widget(failures, area);
}

/// Shared log-tail renderer used by the done screen.
fn render_log_tail(frame: &mut Frame, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let start = app.log.len().saturating_sub(height.max(1));
    let lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|(level, msg)| {
            let (glyph, color) = match level {
                LogLevel::Info => ("ℹ", ACCENT),
                LogLevel::Ok => ("✓", SUCCESS),
                LogLevel::Warn => ("⚠", WARNING),
                LogLevel::Error => ("✗", ERROR),
            };
            Line::from(vec![
                Span::styled(format!(" {glyph} "), Style::default().fg(color)),
                Span::styled(msg.clone(), Style::default().fg(TEXT)),
            ])
        })
        .collect();
    let log = Paragraph::new(lines)
        .block(styled_block("Recent log", false))
        .wrap(Wrap { trim: true });
    frame.render_widget(log, area);
}

// ---------------------------------------------------------------------------
// Help Popup
// ---------------------------------------------------------------------------

fn render_help_popup(frame: &mut Frame) {
    let card = centered_card(62, 24, frame.area());
    frame.render_widget(Clear, card);
    frame.render_widget(
        Block::default()
            .title(" ❓ Keybindings ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(WARNING)),
        card,
    );
    let inner = padded_inner(card);

    let section = |t: &str| {
        Line::from(Span::styled(
            format!(" {t}"),
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ))
    };
    let row = |k: &str, d: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {k:<12}"), Style::default().fg(ACCENT)),
            Span::styled(d.to_owned(), Style::default().fg(TEXT)),
        ])
    };

    let text = vec![
        section("Navigate"),
        row("↑ / k", "move up"),
        row("↓ / j", "move down"),
        row("PgUp / PgDn", "jump 10 games"),
        row("Home / End", "first / last game"),
        Line::from(""),
        section("Select & fetch"),
        row("Space", "select / deselect game"),
        row("a", "select / deselect all shown"),
        row("/", "filter by name"),
        row("C", "clear filter"),
        row("Enter", "confirm · start fetching"),
        Line::from(""),
        section("General"),
        row("?", "toggle this help"),
        row("Esc", "back · clear filter"),
        row("q", "quit"),
        row("Ctrl+C", "force quit"),
        Line::from(""),
        Line::from(Span::styled(
            " Press any key or ? to close ",
            Style::default().fg(MUTED),
        )),
        Line::from(" General"),
        Line::from("  ?          Toggle this help"),
        Line::from("  q          Quit"),
        Line::from("  Ctrl+C     Force quit"),
    ];

    let popup = Paragraph::new(text).alignment(Alignment::Left);
    frame.render_widget(popup, inner);
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Centered card clamped to a maximum size (nicer than pure percentages on
/// very wide or very tall terminals).
fn centered_card(max_w: u16, max_h: u16, area: Rect) -> Rect {
    let w = area.width.saturating_sub(2).min(max_w).max(20);
    let h = area.height.min(max_h).max(8);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// One-cell inset used as faux padding inside bordered cards.
fn padded_inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    }
}
