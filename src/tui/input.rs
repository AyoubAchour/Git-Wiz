use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::app::{ActionItem, App, Focus, Tab};
use super::runtime;

/// Dispatch a key event into the TUI application.
///
/// Order of operations:
/// 1) Ignore non-press events
/// 2) Global overlay handling (help modal toggle and capture)
/// 3) Global navigation (quit, focus cycle, tab switching)
/// 4) Focus-specific routing (left action list vs editor)
/// 5) Diff tab scrolling (when not in the action list)
/// 6) Tab-specific handlers (only for text editing shortcuts, etc.)
///
/// Returns `true` if the key was handled (consumed).
pub fn dispatch_key(app: &mut App, key: KeyEvent) -> bool {
    // Only process key presses; ignore repeats/releases to avoid accidental double actions.
    if key.kind != KeyEventKind::Press {
        return false;
    }

    // 1) Help modal / overlays get first priority and may capture all input.
    if app.handle_global_key(&key) {
        return true;
    }

    // 2) Global navigation (quit/focus/tabs)
    if app.handle_nav_key(&key) {
        return true;
    }

    // Hint: On non-Generate tabs, Enter does nothing unless Actions (LeftPane) is focused.
    if matches!(
        app.active_tab,
        Tab::Stage | Tab::Diff | Tab::Push | Tab::Release | Tab::Config
    ) && app.focus != Focus::LeftPane
        && key.modifiers == KeyModifiers::NONE
        && key.code == KeyCode::Enter
    {
        app.set_status(
            super::app::StatusLevel::Info,
            "Tip: Tab to focus Actions, then ↑/↓ and Enter to run.",
        );
        return true;
    }

    // 3) If focus is on the left pane, arrows should be meaningful:
    //    - Up/Down moves selection
    //    - Enter activates selection
    if app.focus == Focus::LeftPane {
        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => {
                app.action_up();
                return true;
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                app.action_down();
                return true;
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                // Some actions require suspending the TUI (raw mode + alt screen)
                // so they can run interactive terminal I/O safely (e.g. setup wizard, git add -p),
                // OR to avoid terminal corruption while streaming command output (release pipeline).
                if let Some(action) = app.selected_action() {
                    return match action {
                        ActionItem::RunSetupWizard
                        | ActionItem::StagePatch
                        | ActionItem::UnstagePatch
                        | ActionItem::ReleasePatch
                        | ActionItem::ReleaseMinor
                        | ActionItem::ReleaseMajor
                        | ActionItem::ReleaseCustom => {
                            // Ensure interactive operations (and long-running, output-heavy operations)
                            // run outside raw mode / alt screen. This avoids the "TUI crashes and clippy output floods"
                            // symptom by letting the terminal behave normally.
                            let _ = runtime::with_tui_suspended(|| {
                                let _handled = app.activate_selected_action();
                                Ok(())
                            });
                            true
                        }
                        _ => app.activate_selected_action(),
                    };
                }

                // No selected action (shouldn't happen), but consume Enter anyway.
                return true;
            }
            _ => {}
        }
    }

    // 4) Diff tab scrolling (only when not focusing the action list)
    //
    // We intentionally keep scrolling out of the action list focus, so arrows remain
    // meaningful (Up/Down select actions). When the editor is focused, its handler
    // should consume arrow keys.
    if app.active_tab == Tab::Diff && app.focus != Focus::LeftPane {
        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => {
                if app.diff_scroll > 0 {
                    app.diff_scroll -= 1;
                }
                return true;
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                app.diff_scroll = app.diff_scroll.saturating_add(1);
                return true;
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                app.diff_scroll = app.diff_scroll.saturating_sub(20);
                return true;
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                app.diff_scroll = app.diff_scroll.saturating_add(20);
                return true;
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                app.diff_scroll = 0;
                return true;
            }
            _ => {}
        }
    }

    // 5) Stage/Push/Release/Config actions are driven by the selectable Actions list.
    // If you're not focused on the Actions list, don't trigger actions on Enter here.
    // (This prevents accidental actions while still allowing Generate tab shortcuts.)
    //
    // If you want to run actions, Tab focus to Actions, then press Enter.

    // 6) Tab-specific input
    match app.active_tab {
        // Generate is special: it supports editor typing and shortcuts even when not focused on Actions.
        Tab::Generate => app.handle_generate_key(&key),

        // Diff/Stage/Push/Release/Config: all interactions should come from Actions list (LeftPane)
        // and/or modals, so we don't consume keys here.
        Tab::Stage | Tab::Diff | Tab::Push | Tab::Release | Tab::Config => false,
    }
}
