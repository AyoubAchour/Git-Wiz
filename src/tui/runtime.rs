use std::io;

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

/// Minimal blocking adapter for the current synchronous TUI loop.
///
/// This is a pragmatic bridge while the UI is still driven by a synchronous
/// crossterm event loop. It lets you call async APIs (LLM generation, HTTP, etc.)
/// without rewriting the whole TUI as async.
///
/// Notes:
/// - This will block the UI while the future runs.
/// - The long-term solution is to spawn background tasks (tokio::spawn) and
///   communicate results back to the UI via channels.
pub fn tui_block_on<F, T>(fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    // If we're already inside a tokio runtime (common in tests / other runtimes),
    // use it. Otherwise create a small runtime for this one-off call.
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(fut),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
            rt.block_on(fut)
        }
    }
}

/// Temporarily suspends the full-screen TUI so an interactive command can run safely.
///
/// Why this exists:
/// - While the TUI is active, the terminal is usually in raw mode and alternate screen.
/// - Interactive programs (e.g. `git add -p`, password prompts, the setup wizard) need
///   the normal terminal mode to behave correctly.
///
/// Behavior:
/// 1) Leaves alternate screen + disables raw mode
/// 2) Runs the provided closure
/// 3) Re-enters alternate screen + re-enables raw mode (best-effort even if the closure errors)
///
/// Important:
/// - The closure should do any interactive terminal I/O it needs.
/// - After returning, the caller should redraw the UI (the event loop will do this naturally).
pub fn with_tui_suspended<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    // Best-effort suspend. If these fail, still attempt to run the closure, but
    // try to restore the TUI afterwards.
    let mut stdout = io::stdout();

    // Leave TUI mode
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen);

    // Run interactive work
    let result = f();

    // Restore TUI mode
    let _ = execute!(io::stdout(), EnterAlternateScreen);
    let _ = enable_raw_mode();

    result
}
