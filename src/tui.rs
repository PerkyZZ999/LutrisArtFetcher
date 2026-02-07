/// Terminal lifecycle — setup and teardown for the ratatui TUI.
///
/// Handles raw mode, alternate screen, and panic hooks to ensure the terminal
/// is always restored even on crashes.
use std::io::{self, stdout, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

/// Type alias for our terminal backend.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Enter the TUI: enable raw mode, switch to the alternate screen, clear it.
///
/// # Errors
///
/// Returns an error if terminal capabilities cannot be enabled.
pub fn init() -> Result<Tui> {
    // Install panic hook that restores the terminal before printing the panic message
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

/// Leave the TUI: disable raw mode and return to the main screen.
///
/// # Errors
///
/// Returns an error if terminal capabilities cannot be restored.
pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}
