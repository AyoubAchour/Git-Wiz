//! Full-screen TUI (Option B) entrypoint.
//!
//! This module is intentionally small: it wires together the terminal runtime
//! and delegates state/event/rendering to submodules.
//!
//! Modules:
//! - `app`: application state + domain actions (generate, commit, etc.)
//! - `input`: key dispatch + focus/navigation rules
//! - `view`: rendering/layout (ratatui)
//! - `runtime`: async bridging helpers (blocking for now)

pub mod app;
pub mod input;
pub mod runtime;
pub mod view;

use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;

/// Run the full-screen TUI.
///
/// Notes:
/// - This uses a synchronous event loop.
/// - Async operations (LLM requests) are bridged via `runtime::tui_block_on` and will block the UI
///   until we migrate to background tasks + channels.
pub fn run_tui() -> Result<()> {
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal backend")?;
    terminal.clear().ok();

    let tick_rate = Duration::from_millis(33);
    let mut last_tick = Instant::now();

    let mut app = App::new();

    loop {
        terminal
            .draw(|f| view::draw(f, &mut app))
            .context("Failed to draw frame")?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("Failed to poll events")? {
            if let Event::Key(key) = event::read().context("Failed to read event")? {
                input::dispatch_key(&mut app, key);
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal state
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    Ok(())
}
