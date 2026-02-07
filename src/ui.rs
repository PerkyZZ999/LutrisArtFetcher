/// TUI rendering — all ratatui layout and widget code.
///
/// Dispatches to a screen-specific renderer based on `App.screen`, then
/// optionally overlays the help popup.
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
};

use crate::api::models::AssetType;
use crate::app::{App, AppScreen, LogLevel};
use crate::download;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

const BORDER_COLOR: Color = Color::Cyan;
const TITLE_COLOR: Color = Color::White;
const HIGHLIGHT_COLOR: Color = Color::Yellow;
const SUCCESS_COLOR: Color = Color::Green;
const ERROR_COLOR: Color = Color::Red;
const MUTED_COLOR: Color = Color::DarkGray;
const INFO_COLOR: Color = Color::White;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Render the entire TUI for one frame.
pub fn render(frame: &mut Frame, app: &App) {
    match &app.screen {
        AppScreen::ApiKeyEntry { .. } => render_api_key_screen(frame, app),
        AppScreen::AssetTypeSelection { .. } => render_asset_selection(frame, app),
        AppScreen::GameList | AppScreen::Downloading { .. } => render_main_view(frame, app),
        AppScreen::Done { .. } => render_done_screen(frame, app),
    }

    if app.show_help {
        render_help_popup(frame);
    }
}

// ---------------------------------------------------------------------------
// API Key Entry
// ---------------------------------------------------------------------------

fn render_api_key_screen(frame: &mut Frame, app: &App) {
    let AppScreen::ApiKeyEntry {
        ref input,
        cursor_pos: _,
        ref error_msg,
        validating,
    } = app.screen
    else {
        return;
    };

    let area = frame.area();
    let block = Block::default()
        .title(" Lutris Art Fetcher — Setup ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));
    frame.render_widget(block, area);

    let inner = centered_rect(60, 40, area);

    let chunks = Layout::vertical([
        Constraint::Length(3), // Description
        Constraint::Length(1), // Spacer
        Constraint::Length(3), // Input
        Constraint::Length(2), // Error / status
        Constraint::Min(0),   // URL info
    ])
    .split(inner);

    // Description
    let desc = Paragraph::new("Enter your SteamGridDB API key to get started.")
        .alignment(Alignment::Center)
        .style(Style::default().fg(INFO_COLOR));
    frame.render_widget(desc, chunks[0]);

    // Input field
    let display = if validating {
        " Validating...".to_owned()
    } else {
        format!(" {input}█")
    };
    let input_block = Block::default()
        .title(" API Key ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if validating {
            HIGHLIGHT_COLOR
        } else {
            BORDER_COLOR
        }));
    let input_widget = Paragraph::new(display)
        .block(input_block)
        .style(Style::default().fg(TITLE_COLOR));
    frame.render_widget(input_widget, chunks[2]);

    // Error message
    if let Some(ref msg) = error_msg {
        let err = Paragraph::new(msg.as_str())
            .style(Style::default().fg(ERROR_COLOR))
            .alignment(Alignment::Center);
        frame.render_widget(err, chunks[3]);
    }

    // URL info
    let url_text = Paragraph::new(
        "Get your key at: https://www.steamgriddb.com/profile/preferences/api",
    )
    .alignment(Alignment::Center)
    .style(Style::default().fg(MUTED_COLOR));
    frame.render_widget(url_text, chunks[4]);
}

// ---------------------------------------------------------------------------
// Asset Type Selection
// ---------------------------------------------------------------------------

fn render_asset_selection(frame: &mut Frame, app: &App) {
    let AppScreen::AssetTypeSelection { cursor } = app.screen else {
        return;
    };

    let area = frame.area();
    let block = Block::default()
        .title(" Lutris Art Fetcher — Select Asset Types ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));
    frame.render_widget(block, area);

    let inner = centered_rect(50, 50, area);

    let chunks = Layout::vertical([
        Constraint::Length(2),  // Instructions
        Constraint::Length(1),  // Spacer
        Constraint::Min(6),    // List
        Constraint::Length(2),  // Footer
    ])
    .split(inner);

    let instructions = Paragraph::new("Select which asset types to download (Space to toggle, 'a' for all):")
        .alignment(Alignment::Center)
        .style(Style::default().fg(INFO_COLOR));
    frame.render_widget(instructions, chunks[0]);

    let all_types = AssetType::all();
    let items: Vec<ListItem> = all_types
        .iter()
        .enumerate()
        .map(|(i, asset)| {
            let checked = if app.selected_assets.contains(asset) {
                "[x]"
            } else {
                "[ ]"
            };
            let style = if i == cursor {
                Style::default()
                    .fg(HIGHLIGHT_COLOR)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INFO_COLOR)
            };
            ListItem::new(format!(" {checked} {}", asset.display_name())).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_COLOR))
            .title(" Assets "),
    );
    frame.render_widget(list, chunks[2]);

    let footer = Paragraph::new(" ↑↓:Navigate  Space:Toggle  a:All  Enter:Confirm  q:Quit")
        .style(Style::default().fg(MUTED_COLOR))
        .alignment(Alignment::Center);
    frame.render_widget(footer, chunks[3]);
}

// ---------------------------------------------------------------------------
// Main View (GameList + Downloading)
// ---------------------------------------------------------------------------

fn render_main_view(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Outer block
    let outer = Block::default()
        .title(" Lutris Art Fetcher ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));
    frame.render_widget(outer, area);

    let inner = inner_area(area);

    // Vertical layout: top area (game list + status) + log + footer
    let main_chunks = Layout::vertical([
        Constraint::Min(8),    // Game list + status
        Constraint::Length(8), // Log
        Constraint::Length(1), // Footer
    ])
    .split(inner);

    // Horizontal split: game list (60%) | status (40%)
    let top_chunks = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Percentage(40),
    ])
    .split(main_chunks[0]);

    render_game_list(frame, app, top_chunks[0]);
    render_status_panel(frame, app, top_chunks[1]);
    render_log_panel(frame, app, main_chunks[1]);
    render_footer(frame, app, main_chunks[2]);
}

fn render_game_list(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Games ({} installed) ", app.games.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));

    let items: Vec<ListItem> = app
        .games
        .iter()
        .map(|entry| {
            let icon = entry.overall_icon(&app.selected_assets);
            let icon_color = match icon {
                "✓" => SUCCESS_COLOR,
                "↓" => HIGHLIGHT_COLOR,
                "✗" => ERROR_COLOR,
                "─" => MUTED_COLOR,
                _ => INFO_COLOR,
            };
            let line = Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
                Span::raw(&entry.game.name),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(HIGHLIGHT_COLOR)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.list_state.clone());
}

fn render_status_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Status ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // Mode
        Constraint::Length(1), // Spacer
        Constraint::Length(3), // Progress gauge
        Constraint::Length(1), // Spacer
        Constraint::Min(2),   // Current info
    ])
    .split(inner);

    // Mode line
    let asset_names: Vec<&str> = app.selected_assets.iter().map(|a| a.display_name()).collect();
    let mode = Paragraph::new(format!(" Mode: {}", asset_names.join(", ")))
        .style(Style::default().fg(INFO_COLOR));
    frame.render_widget(mode, chunks[0]);

    // Progress gauge
    match &app.screen {
        AppScreen::Downloading {
            current, total, ..
        } => {
            #[allow(clippy::cast_precision_loss)]
            let progress = if *total == 0 {
                1.0
            } else {
                *current as f64 / *total as f64
            };
            let label = format!("{current} / {total}");
            let gauge = Gauge::default()
                .block(Block::default().title(" Progress ").borders(Borders::ALL).border_style(Style::default().fg(BORDER_COLOR)))
                .gauge_style(Style::default().fg(SUCCESS_COLOR).bg(Color::DarkGray))
                .ratio(progress.min(1.0))
                .label(label);
            frame.render_widget(gauge, chunks[2]);
        }
        AppScreen::GameList => {
            let existing: usize = app
                .games
                .iter()
                .filter(|e| {
                    app.selected_assets
                        .iter()
                        .all(|a| download::asset_exists(*a, &e.game.slug))
                })
                .count();
            let info = Paragraph::new(format!(
                " {existing} games already have all selected art"
            ))
            .style(Style::default().fg(MUTED_COLOR));
            frame.render_widget(info, chunks[2]);
        }
        _ => {}
    }

    // Current game info
    if let Some(selected) = app.list_state.selected() {
        if let Some(entry) = app.games.get(selected) {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(" {}", entry.game.name),
                    Style::default().fg(TITLE_COLOR).add_modifier(Modifier::BOLD),
                )),
            ];
            if let Some(ref runner) = entry.game.runner {
                lines.push(Line::from(Span::styled(
                    format!(" Runner: {runner}"),
                    Style::default().fg(MUTED_COLOR),
                )));
            }
            if let Some(ref service) = entry.game.service {
                lines.push(Line::from(Span::styled(
                    format!(" Service: {service}"),
                    Style::default().fg(MUTED_COLOR),
                )));
            }
            let info = Paragraph::new(lines);
            frame.render_widget(info, chunks[4]);
        }
    }
}

fn render_log_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Log ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_COLOR));

    // Show last N messages that fit
    let inner_height = area.height.saturating_sub(2) as usize;
    let start = app.log.len().saturating_sub(inner_height);
    let lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|(level, msg)| {
            let (prefix, color) = match level {
                LogLevel::Info => ("[INFO]", INFO_COLOR),
                LogLevel::Ok => ("[ OK ]", SUCCESS_COLOR),
                LogLevel::Warn => ("[WARN]", HIGHLIGHT_COLOR),
                LogLevel::Error => ("[ ERR]", ERROR_COLOR),
            };
            Line::from(vec![
                Span::styled(format!(" {prefix} "), Style::default().fg(color)),
                Span::raw(msg),
            ])
        })
        .collect();

    let log = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(log, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let text = match &app.screen {
        AppScreen::GameList => " q:Quit  Enter:Start All  ↑↓:Navigate  ?:Help",
        AppScreen::Downloading { .. } => " q:Quit  ?:Help  (downloading...)",
        _ => " q:Quit  ?:Help",
    };
    let footer = Paragraph::new(text)
        .style(Style::default().fg(MUTED_COLOR))
        .alignment(Alignment::Left);
    frame.render_widget(footer, area);
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
    } = app.screen
    else {
        return;
    };

    let area = frame.area();
    let block = Block::default()
        .title(" Lutris Art Fetcher — Complete! ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SUCCESS_COLOR));
    frame.render_widget(block, area);

    let inner = centered_rect(50, 50, area);

    let chunks = Layout::vertical([
        Constraint::Length(2),  // Header
        Constraint::Length(1),  // Spacer
        Constraint::Length(6),  // Stats
        Constraint::Length(1),  // Spacer
        Constraint::Min(6),     // Log tail
        Constraint::Length(1),  // Footer
    ])
    .split(inner);

    let header = Paragraph::new("All downloads complete!")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(SUCCESS_COLOR)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(header, chunks[0]);

    let stats = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("  ✓ Downloaded: {downloaded}"),
            Style::default().fg(SUCCESS_COLOR),
        )),
        Line::from(Span::styled(
            format!("  ─ Skipped:    {skipped}"),
            Style::default().fg(MUTED_COLOR),
        )),
        Line::from(Span::styled(
            format!("  ✗ Failed:     {failed}"),
            Style::default().fg(if failed > 0 { ERROR_COLOR } else { MUTED_COLOR }),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  ⏱ Time: {elapsed_secs}s"),
            Style::default().fg(INFO_COLOR),
        )),
    ])
    .block(
        Block::default()
            .title(" Summary ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_COLOR)),
    );
    frame.render_widget(stats, chunks[2]);

    // Show last few log lines
    let log_height = chunks[4].height.saturating_sub(2) as usize;
    let start = app.log.len().saturating_sub(log_height);
    let lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|(level, msg)| {
            let (prefix, color) = match level {
                LogLevel::Info => ("[INFO]", INFO_COLOR),
                LogLevel::Ok => ("[ OK ]", SUCCESS_COLOR),
                LogLevel::Warn => ("[WARN]", HIGHLIGHT_COLOR),
                LogLevel::Error => ("[ ERR]", ERROR_COLOR),
            };
            Line::from(vec![
                Span::styled(format!(" {prefix} "), Style::default().fg(color)),
                Span::raw(msg),
            ])
        })
        .collect();
    let log = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Recent Log ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(log, chunks[4]);

    let footer = Paragraph::new(" Restart Lutris to see changes. Press q or Enter to exit.")
        .alignment(Alignment::Center)
        .style(Style::default().fg(MUTED_COLOR));
    frame.render_widget(footer, chunks[5]);
}

// ---------------------------------------------------------------------------
// Help Popup
// ---------------------------------------------------------------------------

fn render_help_popup(frame: &mut Frame) {
    let area = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            " Keybindings",
            Style::default().fg(TITLE_COLOR).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(" Navigation"),
        Line::from("  ↑/k        Move up"),
        Line::from("  ↓/j        Move down"),
        Line::from("  PgUp/PgDn  Scroll 10 items"),
        Line::from("  Home/End   Jump to first/last"),
        Line::from(""),
        Line::from(" Actions"),
        Line::from("  Enter      Confirm / Start downloads"),
        Line::from("  Space      Toggle selection"),
        Line::from("  a          Toggle all (asset selection)"),
        Line::from(""),
        Line::from(" General"),
        Line::from("  ?          Toggle this help"),
        Line::from("  q / Esc    Quit"),
        Line::from("  Ctrl+C     Force quit"),
    ];

    let popup = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(HIGHLIGHT_COLOR)),
        )
        .style(Style::default().fg(INFO_COLOR));

    frame.render_widget(popup, area);
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Create a centered rectangle of the given percentage width/height within `area`.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

/// Get the inner area of a bordered block (1-cell inset on each side).
fn inner_area(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}
