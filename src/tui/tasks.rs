use std::{
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use super::app::{App, DiffViewSource, StatusLevel};

/// A single-task-at-a-time background runner for the TUI.
///
/// Why this exists:
/// - The main crossterm loop is synchronous.
/// - Long-running work (LLM calls, git commands, cargo/clippy) should NOT block rendering.
/// - We want consistent "in progress" feedback (spinner + elapsed time).
///
/// Model:
/// - You call `tasks.start(...)` from the UI thread.
/// - The work runs on a background thread.
/// - Results are delivered back via a channel and applied on the UI thread.
///
/// Safety:
/// - We enforce "single task at a time": if `start` is called while busy, we return `false`.
///
/// Notes:
/// - Tasks that must suspend the TUI (interactive commands like `git add -p`, setup wizard,
///   or release which streams output) should *not* run through this runner. Those should use
///   the suspend mechanism and show a one-frame "Switching to terminal…" status before leaving.
pub struct TaskRunner {
    tx: Sender<TaskEvent>,
    rx: Receiver<TaskEvent>,
    state: Arc<Mutex<TaskState>>,
}

/// State shared between UI thread and worker threads.
#[derive(Debug)]
struct TaskState {
    current: Option<RunningTask>,
}

/// Minimal info for the UI to render progress.
#[derive(Debug, Clone)]
pub struct RunningTask {
    pub label: String,
    pub started_at: Instant,
    pub spinner_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    GenerateCommitFromStaged,
    CommitFromEditor,
    StageAll,
    PushBranch,
    PushTag,
    PushAllTags,
    LoadDiff,
}

#[derive(Debug)]
pub enum TaskEvent {
    Started {
        kind: TaskKind,
        label: String,
        started_at: Instant,
    },
    Progress {
        message: String,
    },
    Completed {
        result: TaskResult,
    },
}

/// High-level results that the UI can apply deterministically.
#[derive(Debug)]
pub enum TaskResult {
    OkMessage {
        status: String,
        log: Option<String>,
    },
    GeneratedCommitMessage {
        message: String,
        summary: String,
        provider: String,
        model: String,
    },
    LoadedDiff {
        source: DiffViewSource,
        text: String,
        status: String,
    },
    Error {
        message: String,
    },
}

impl TaskRunner {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<TaskEvent>();
        Self {
            tx,
            rx,
            state: Arc::new(Mutex::new(TaskState { current: None })),
        }
    }

    /// Returns true if a task is currently running.
    pub fn is_busy(&self) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.current.clone())
            .is_some()
    }

    /// Returns a snapshot of the running task (for rendering).
    pub fn running(&self) -> Option<RunningTask> {
        self.state.lock().ok().and_then(|s| s.current.clone())
    }

    /// Advance spinner frame for the currently running task.
    pub fn tick_spinner(&self) {
        if let Ok(mut s) = self.state.lock() {
            if let Some(ref mut t) = s.current {
                t.spinner_index = t.spinner_index.wrapping_add(1);
            }
        }
    }

    /// Poll and apply all pending task events to the app.
    ///
    /// Call this once per UI tick (or frame). It is non-blocking.
    pub fn drain_events(&self, app: &mut App) {
        loop {
            match self.rx.try_recv() {
                Ok(ev) => self.apply_event(app, ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Nothing we can do; treat as no-op.
                    break;
                }
            }
        }
    }

    fn apply_event(&self, app: &mut App, ev: TaskEvent) {
        match ev {
            TaskEvent::Started {
                kind,
                label,
                started_at,
            } => {
                // The kind is currently not rendered in the UI, but we still
                // destructure it to keep the field "alive" for future diagnostics.
                let _ = kind;
                if let Ok(mut s) = self.state.lock() {
                    s.current = Some(RunningTask {
                        label: label.clone(),
                        started_at,
                        spinner_index: 0,
                    });
                }
                app.set_status(StatusLevel::Info, label);
            }
            TaskEvent::Progress { message } => {
                // Lightweight status updates. Keep logs too.
                app.set_status(StatusLevel::Info, message.clone());
                app.log(message);
            }
            TaskEvent::Completed { result } => {
                // Clear running task first.
                if let Ok(mut s) = self.state.lock() {
                    s.current = None;
                }

                match result {
                    TaskResult::OkMessage { status, log } => {
                        app.set_status(StatusLevel::Success, status.clone());
                        if let Some(l) = log {
                            app.log(l);
                        }
                    }
                    TaskResult::GeneratedCommitMessage {
                        message,
                        summary,
                        provider,
                        model,
                    } => {
                        app.diff_source_label = "Staged (recommended)".to_string();
                        app.diff_summary = summary;
                        app.provider_label = provider;
                        app.model_label = model;
                        app.set_commit_message_text(&message);
                        app.set_status(StatusLevel::Success, "Generated.");
                        app.log("Generated commit message.");
                    }
                    TaskResult::LoadedDiff {
                        source,
                        text,
                        status,
                    } => {
                        app.diff_view_source = source;
                        app.diff_scroll = 0;
                        app.diff_text = text;
                        app.set_status(StatusLevel::Success, status);
                        app.log("Loaded diff.");
                    }
                    TaskResult::Error { message } => {
                        app.set_status(StatusLevel::Error, message.clone());
                        app.log(format!("Error: {}", message));
                    }
                }
            }
        }
    }

    /// Start a background task if idle. Returns `true` if started, `false` if already busy.
    pub fn start<F>(&self, kind: TaskKind, label: impl Into<String>, f: F) -> bool
    where
        F: FnOnce(Sender<TaskEvent>) -> Result<TaskResult> + Send + 'static,
    {
        // Enforce single-task semantics.
        {
            let mut s = match self.state.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };
            if s.current.is_some() {
                return false;
            }
            // Mark as running immediately to prevent races.
            let started_at = Instant::now();
            let label = label.into();
            s.current = Some(RunningTask {
                label: label.clone(),
                started_at,
                spinner_index: 0,
            });

            // Also emit Started event (so UI can show status/log even if state lock differs).
            let _ = self.tx.send(TaskEvent::Started {
                kind,
                label,
                started_at,
            });
        }

        let tx = self.tx.clone();
        thread::spawn(move || {
            // Worker: run task, emit completion.
            let result = f(tx.clone()).unwrap_or_else(|e| TaskResult::Error {
                message: e.to_string(),
            });
            let _ = tx.send(TaskEvent::Completed { result });
        });

        true
    }
}

/// A simple unicode spinner sequence.
///
/// You can render `frames[spinner_index % frames.len()]`.
pub fn spinner_frames() -> &'static [&'static str] {
    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
}

/// Format elapsed time in a compact form for the status bar.
pub fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    }
}
