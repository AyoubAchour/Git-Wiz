use anyhow::Result;

mod config;
mod generator;
mod git;
mod release;
mod setup;
mod tui;

fn main() -> Result<()> {
    // Ensure terminal colors are enabled on Windows (useful for any non-TUI fallback/logging)
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    // Full-screen TUI is the entrypoint.
    tui::run_tui()
}
