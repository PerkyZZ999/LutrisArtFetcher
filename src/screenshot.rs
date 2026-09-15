/// README screenshot generator — renders the real TUI with ratatui's
/// `TestBackend` and exports the cell buffer as `assets/tui.svg`.
///
/// Runs with the normal test suite (`rust-tc quick` / `rust-tc doctor`); the
/// output is deterministic (fixed demo data, no timestamps), so repeated runs
/// rewrite an identical file.
#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::fmt::Write as _;
    use std::path::PathBuf;

    use color_eyre::eyre::Result;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;

    use crate::api::models::{AssetType, DownloadStatus};
    use crate::app::{App, AppScreen, LogLevel};
    use crate::config::Config;
    use crate::db::Game;
    use crate::ui;

    const COLS: u16 = 100;
    const ROWS: u16 = 30;
    const CELL_W: u32 = 8;
    const ROW_H: u32 = 17;
    const PAD: u32 = 12;
    const FONT_SIZE: u32 = 13;
    const BASELINE: u32 = 13;

    fn demo_game(id: i64, name: &str, slug: &str, runner: &str) -> Game {
        Game {
            id,
            name: name.to_owned(),
            slug: slug.to_owned(),
            runner: Some(runner.to_owned()),
            platform: None,
            service: Some("steam".to_owned()),
            service_id: None,
            has_custom_banner: false,
            has_custom_coverart: false,
        }
    }

    /// Build the demo app shown in the screenshot: mixed per-asset states so
    /// every status color appears, one unselected game, and a lively log.
    fn demo_app() -> App {
        let config = Config {
            api_key: Some("demo-key-not-real".to_owned()),
            ..Config::default()
        };
        let games = vec![
            demo_game(1, "Hades II", "hades-ii", "steam"),
            demo_game(2, "Elden Ring", "elden-ring", "steam"),
            demo_game(3, "Celeste", "celeste", "linux"),
            demo_game(4, "Hollow Knight", "hollow-knight", "steam"),
            demo_game(5, "Baldur's Gate 3", "baldurs-gate-3", "steam"),
            demo_game(6, "Stardew Valley", "stardew-valley", "steam"),
        ];
        let assets: HashSet<AssetType> = AssetType::all().iter().copied().collect();
        let mut app = App::new(config, games, assets, false);
        app.screen = AppScreen::GameList;
        app.tick_count = 3;

        let home = "/home/user/.local/share/lutris";
        app.games[0].grid_status =
            DownloadStatus::Done(PathBuf::from(format!("{home}/coverart/hades-ii.jpg")));
        app.games[0].hero_status =
            DownloadStatus::Done(PathBuf::from(format!("{home}/heroes/hades-ii.jpg")));
        app.games[0].logo_status = DownloadStatus::Downloading;
        app.games[1].logo_status = DownloadStatus::Failed("no art found on SteamGridDB".to_owned());
        app.games[2].hero_status = DownloadStatus::Searching;
        app.games[3].grid_status = DownloadStatus::Skipped("already exists".to_owned());
        app.games[4].selected = false;

        app.log(LogLevel::Ok, "Hades II — Grid saved".into());
        app.log(LogLevel::Info, "Downloading Logo for Hades II...".into());
        app.log(
            LogLevel::Error,
            "Elden Ring — Logo failed: no art found on SteamGridDB".into(),
        );
        app.log(LogLevel::Info, "Searching for Celeste (Hero)...".into());
        app.log(
            LogLevel::Warn,
            "Nothing selected — press Space to select games".into(),
        );
        app
    }

    /// Map a ratatui color to a GitHub-dark-friendly hex value.
    fn css(color: Color) -> &'static str {
        match color {
            Color::Reset | Color::Rgb(_, _, _) | Color::Indexed(_) => "#c9d1d9",
            Color::Black => "#000000",
            Color::Red => "#f85149",
            Color::Green => "#3fb950",
            Color::Yellow => "#d29922",
            Color::Blue => "#58a6ff",
            Color::Magenta => "#bc8cff",
            Color::Cyan => "#39c5cf",
            Color::Gray => "#8b949e",
            Color::DarkGray => "#6e7681",
            Color::LightRed => "#ffa198",
            Color::LightGreen => "#56d364",
            Color::LightYellow => "#e3b341",
            Color::LightBlue => "#79c0ff",
            Color::LightMagenta => "#d2a8ff",
            Color::LightCyan => "#76e3ea",
            Color::White => "#ffffff",
        }
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// One rendered cell: owned symbol plus its colors and bold flag.
    struct SvgCell {
        symbol: String,
        fg: Color,
        bg: Color,
        bold: bool,
    }

    /// Serialize one terminal row: background rects for highlighted cells plus
    /// a `<text>` element with one `<tspan>` per style run.
    fn render_row(cells: &[SvgCell], y: u32, out: &mut String) {
        // Background runs first (selection highlight, gauge fill, ...).
        let mut col = 0;
        while col < cells.len() {
            let bg = cells[col].bg;
            if bg == Color::Reset {
                col += 1;
                continue;
            }
            let start = col;
            while col < cells.len() && cells[col].bg == bg {
                col += 1;
            }
            let x = PAD + u32::try_from(start).unwrap_or(0) * CELL_W;
            let w = u32::try_from(col - start).unwrap_or(0) * CELL_W;
            let fill = css(bg);
            let y0 = PAD + y * ROW_H;
            write!(
                out,
                "<rect x=\"{x}\" y=\"{y0}\" width=\"{w}\" height=\"{ROW_H}\" fill=\"{fill}\"/>",
            )
            .expect("writing SVG to a String cannot fail");
        }

        // Foreground text, grouped into same-style runs.
        let ty = PAD + y * ROW_H + BASELINE;
        write!(
            out,
            "<text x=\"{PAD}\" y=\"{ty}\" font-family=\"ui-monospace,SFMono-Regular,Menlo,Consolas,monospace\" font-size=\"{FONT_SIZE}\" xml:space=\"preserve\">",
        )
        .expect("writing SVG to a String cannot fail");
        let mut col = 0;
        while col < cells.len() {
            let first = &cells[col];
            let (fg, bold, bg) = (first.fg, first.bold, first.bg);
            let start = col;
            while col < cells.len()
                && cells[col].fg == fg
                && cells[col].bold == bold
                && cells[col].bg == bg
            {
                col += 1;
            }
            let text: String = cells[start..col].iter().map(|c| c.symbol.clone()).collect();
            let x = PAD + u32::try_from(start).unwrap_or(0) * CELL_W;
            let fill = css(fg);
            let body = escape(&text);
            if bold {
                write!(
                    out,
                    "<tspan x=\"{x}\" fill=\"{fill}\" font-weight=\"bold\">{body}</tspan>",
                )
                .expect("writing SVG to a String cannot fail");
            } else {
                write!(out, "<tspan x=\"{x}\" fill=\"{fill}\">{body}</tspan>")
                    .expect("writing SVG to a String cannot fail");
            }
        }
        out.push_str("</text>");
    }

    #[test]
    fn writes_readme_screenshot_svg() -> Result<()> {
        let mut app = demo_app();
        let backend = TestBackend::new(COLS, ROWS);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width;
        let height = buffer.area.height;
        assert_eq!((width, height), (COLS, ROWS));

        let mut out = String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"Lutris Art Fetcher TUI\">",
        );
        let full_w = PAD * 2 + u32::from(width) * CELL_W;
        let full_h = PAD * 2 + u32::from(height) * ROW_H;
        write!(
            out,
            "<title>Lutris Art Fetcher — game list</title><rect width=\"{full_w}\" height=\"{full_h}\" rx=\"8\" fill=\"#0d1117\"/>",
        )
        .expect("writing SVG to a String cannot fail");
        for y in 0..height {
            let mut cells = Vec::with_capacity(usize::from(width));
            for x in 0..width {
                let cell = &buffer.content[usize::from(y) * usize::from(width) + usize::from(x)];
                cells.push(SvgCell {
                    symbol: cell.symbol().to_owned(),
                    fg: cell.fg,
                    bg: cell.bg,
                    bold: cell.modifier.contains(Modifier::BOLD),
                });
            }
            render_row(&cells, u32::from(y), &mut out);
        }
        out.push_str("</svg>");

        std::fs::create_dir_all("assets")?;
        std::fs::write("assets/tui.svg", &out)?;
        assert!(out.len() > 10_000, "screenshot looks unexpectedly small");
        Ok(())
    }
}
