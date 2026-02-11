use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{Input, TextArea};

use crate::config::{Config, Provider};
use crate::generator::{
    AnthropicGenerator, GeminiGenerator, Generator, MockGenerator, OpenAIGenerator,
};
use crate::git;
use crate::release;
use crate::setup;
use crate::tui::runtime;
use crate::tui::tasks::{TaskEvent, TaskKind, TaskResult, TaskRunner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    None,
    Confirm,
    TextInput,
}





#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmPurpose {
    ClearConfig,
    PushAllTags,

    // Release flow confirmations
    ReleaseTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputPurpose {
    PushSpecificTag,

    // Release flow inputs
    ReleaseCustomVersion,
}

#[derive(Debug, Clone)]
pub struct ModalState {
    pub kind: ModalKind,
    pub title: String,
    pub message: String,

    // Confirm modal
    pub confirm_purpose: Option<ConfirmPurpose>,

    // Text input modal
    pub input_purpose: Option<TextInputPurpose>,
    pub input_value: String,
}

impl ModalState {
    pub fn none() -> Self {
        Self {
            kind: ModalKind::None,
            title: String::new(),
            message: String::new(),
            confirm_purpose: None,
            input_purpose: None,
            input_value: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffViewSource {
    Staged,
    Unstaged,
    Both,
}

impl DiffViewSource {
    pub fn label(self) -> &'static str {
        match self {
            DiffViewSource::Staged => "Staged",
            DiffViewSource::Unstaged => "Unstaged",
            DiffViewSource::Both => "Both",
        }
    }

    pub fn to_git_source(self) -> git::DiffSource {
        match self {
            DiffViewSource::Staged => git::DiffSource::Staged,
            DiffViewSource::Unstaged => git::DiffSource::Unstaged,
            DiffViewSource::Both => git::DiffSource::Both,
        }
    }
}

/// Per-tab selectable action menu items (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionItem {
    // Generate tab
    GenerateFromStaged,
    Commit,
    ClearMessage,

    // Stage tab (wired)
    StagePatch,
    StageAll,
    UnstagePatch,
    UnstageAll,

    // Diff tab (wired)
    ViewStaged,
    ViewUnstaged,
    ViewBoth,

    // Push tab (wired)
    PushBranch,
    PushSpecificTag,
    PushAllTags,

    // Release tab (wired v1)
    ReleasePatch,
    ReleaseMinor,
    ReleaseMajor,
    ReleaseCustom,

    // Config tab (wired)
    RunSetupWizard,
    ReloadConfig,
    ClearConfig,
}

impl ActionItem {
    pub fn label(self) -> &'static str {
        match self {
            ActionItem::GenerateFromStaged => "Generate (staged)",
            ActionItem::Commit => "Commit",
            ActionItem::ClearMessage => "Clear message",

            ActionItem::StagePatch => "Stage patch (git add -p)",
            ActionItem::StageAll => "Stage all (git add -A)",
            ActionItem::UnstagePatch => "Unstage patch (interactive)",
            ActionItem::UnstageAll => "Unstage all",

            ActionItem::ViewStaged => "View staged diff",
            ActionItem::ViewUnstaged => "View unstaged diff",
            ActionItem::ViewBoth => "View both diffs",

            ActionItem::PushBranch => "Push branch",
            ActionItem::PushSpecificTag => "Push specific tag",
            ActionItem::PushAllTags => "Push all tags",

            ActionItem::ReleasePatch => "Release (patch): bump, commit, tag, push",
            ActionItem::ReleaseMinor => "Release (minor): bump, commit, tag, push",
            ActionItem::ReleaseMajor => "Release (major): bump, commit, tag, push",
            ActionItem::ReleaseCustom => "Release (custom): bump, commit, tag, push",

            ActionItem::RunSetupWizard => "Run setup wizard",
            ActionItem::ReloadConfig => "Reload config",
            ActionItem::ClearConfig => "Clear config",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Generate,
    Stage,
    Diff,
    Push,
    Release,
    Config,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Generate,
        Tab::Stage,
        Tab::Diff,
        Tab::Push,
        Tab::Release,
        Tab::Config,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Generate => "Generate",
            Tab::Stage => "Stage",
            Tab::Diff => "Diff",
            Tab::Push => "Push",
            Tab::Release => "Release",
            Tab::Config => "Config",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    TabBar,
    CommitEditor,
    LeftPane,
    RightPane,
}

#[derive(Debug, Clone)]
pub enum StatusLevel {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct StatusLine {
    pub level: StatusLevel,
    pub message: String,
}

pub struct RunningTaskSnapshot {
    pub label: String,
    pub started_at: std::time::Instant,
    pub spinner_index: usize,
}

pub struct App {
    pub active_tab: Tab,
    pub focus: Focus,

    // Help modal
    pub show_help: bool,

    // Lightweight modal state (confirm / text input) used by tabs like Push/Config/Release.
    pub modal: ModalState,

    // Selectable action menu (left-side actions)
    pub action_index: usize,

    // Background task progress snapshot (set by TUI runtime each tick)
    pub running_task: Option<RunningTaskSnapshot>,

    // Generate tab state
    pub diff_source_label: String,
    pub diff_summary: String,
    pub provider_label: String,
    pub model_label: String,
    pub mock_mode: bool,

    // Diff tab state
    pub diff_view_source: DiffViewSource,
    pub diff_scroll: usize,
    pub diff_text: String,

    // Release tab state
    pub pending_release_version: Option<String>,

    // Editor
    pub commit_editor: TextArea<'static>,

    // Logs / status
    pub status: Option<StatusLine>,
    pub logs: Vec<String>,

    // Exit control
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let mut editor = TextArea::default();
        editor.set_cursor_line_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );

        Self {
            active_tab: Tab::Generate,
            focus: Focus::CommitEditor,
            show_help: true,

            modal: ModalState::none(),

            action_index: 0,

            running_task: None,

            diff_source_label: "Staged (recommended)".to_string(),
            diff_summary: "No diff loaded".to_string(),
            provider_label: "Not configured".to_string(),
            model_label: "-".to_string(),
            mock_mode: false,

            diff_view_source: DiffViewSource::Staged,
            diff_scroll: 0,
            diff_text: String::new(),

            pending_release_version: None,

            commit_editor: editor,

            status: Some(StatusLine {
                level: StatusLevel::Info,
                message: "Press ? for help. g=generate, Enter=commit, c=clear. Esc quits."
                    .to_string(),
            }),
            logs: vec![],

            should_quit: false,
        }
    }

    pub fn set_status(&mut self, level: StatusLevel, message: impl Into<String>) {
        self.status = Some(StatusLine {
            level,
            message: message.into(),
        });
    }

    pub fn actions_for_active_tab(&self) -> &'static [ActionItem] {
        match self.active_tab {
            Tab::Generate => &[
                ActionItem::GenerateFromStaged,
                ActionItem::Commit,
                ActionItem::ClearMessage,
            ],
            Tab::Stage => &[
                ActionItem::StagePatch,
                ActionItem::StageAll,
                ActionItem::UnstagePatch,
                ActionItem::UnstageAll,
            ],
            Tab::Diff => &[
                ActionItem::ViewStaged,
                ActionItem::ViewUnstaged,
                ActionItem::ViewBoth,
            ],
            Tab::Push => &[
                ActionItem::PushBranch,
                ActionItem::PushSpecificTag,
                ActionItem::PushAllTags,
            ],
            Tab::Release => &[
                ActionItem::ReleasePatch,
                ActionItem::ReleaseMinor,
                ActionItem::ReleaseMajor,
                ActionItem::ReleaseCustom,
            ],
            Tab::Config => &[
                ActionItem::RunSetupWizard,
                ActionItem::ReloadConfig,
                ActionItem::ClearConfig,
            ],
        }
    }

    pub fn clamp_action_index(&mut self) {
        let len = self.actions_for_active_tab().len();
        if len == 0 {
            self.action_index = 0;
            return;
        }
        if self.action_index >= len {
            self.action_index = len - 1;
        }
    }

    pub fn action_up(&mut self) {
        self.clamp_action_index();
        if self.action_index > 0 {
            self.action_index -= 1;
        }
    }

    pub fn action_down(&mut self) {
        self.clamp_action_index();
        let len = self.actions_for_active_tab().len();
        if len == 0 {
            return;
        }
        if self.action_index + 1 < len {
            self.action_index += 1;
        }
    }

    pub fn selected_action(&self) -> Option<ActionItem> {
        let actions = self.actions_for_active_tab();
        actions.get(self.action_index).copied()
    }

    pub fn activate_selected_action(&mut self, tasks: &TaskRunner) -> bool {
        let Some(action) = self.selected_action() else {
            return false;
        };

        match action {
            // Generate tab
            ActionItem::GenerateFromStaged => {
                let _started = self.start_generate_from_staged(tasks);
                true
            }
            ActionItem::Commit => {
                let _started = self.start_commit_from_editor(tasks);
                true
            }
            ActionItem::ClearMessage => {
                self.clear_editor();
                true
            }

            // Stage tab (interactive patch ops are suspended by the input layer)
            ActionItem::StagePatch => {
                self.set_status(StatusLevel::Info, "Switching to terminal for interactive staging…");
                self.log("Switching to terminal: git add -p (interactive)");
                if let Err(e) = self.stage_patch() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Stage patch failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Staging complete.");
                    self.log("Staged changes interactively.");
                }
                true
            }
            ActionItem::StageAll => {
                let _started = self.start_stage_all(tasks);
                true
            }
            ActionItem::UnstagePatch => {
                self.set_status(
                    StatusLevel::Info,
                    "Switching to terminal for interactive unstaging…",
                );
                self.log("Switching to terminal: unstage interactively");
                if let Err(e) = self.unstage_patch() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Unstage patch failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Unstaging complete.");
                    self.log("Unstaged changes interactively.");
                }
                true
            }
            ActionItem::UnstageAll => {
                if let Err(e) = self.unstage_all() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Unstage all failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Unstaged all changes.");
                    self.log("Unstaged all changes.");
                }
                true
            }

            // Diff tab (wired)
            ActionItem::ViewStaged => {
                let _started = self.start_load_diff(tasks, DiffViewSource::Staged);
                true
            }
            ActionItem::ViewUnstaged => {
                let _started = self.start_load_diff(tasks, DiffViewSource::Unstaged);
                true
            }
            ActionItem::ViewBoth => {
                let _started = self.start_load_diff(tasks, DiffViewSource::Both);
                true
            }

            // Push tab (wired)
            ActionItem::PushBranch => {
                let _started = self.start_push_branch(tasks);
                true
            }
            ActionItem::PushSpecificTag => {
                self.modal = ModalState {
                    kind: ModalKind::TextInput,
                    title: "Push Tag".to_string(),
                    message: "Enter a tag to push (e.g. v0.2.3)".to_string(),
                    confirm_purpose: None,
                    input_purpose: Some(TextInputPurpose::PushSpecificTag),
                    input_value: String::new(),
                };
                true
            }
            ActionItem::PushAllTags => {
                self.modal = ModalState {
                    kind: ModalKind::Confirm,
                    title: "Confirm".to_string(),
                    message: "Push ALL tags? This may trigger releases (v*).".to_string(),
                    confirm_purpose: Some(ConfirmPurpose::PushAllTags),
                    input_purpose: None,
                    input_value: String::new(),
                };
                true
            }

            // Release tab (v1)
            ActionItem::ReleasePatch => self.start_release_bump("patch"),
            ActionItem::ReleaseMinor => self.start_release_bump("minor"),
            ActionItem::ReleaseMajor => self.start_release_bump("major"),
            ActionItem::ReleaseCustom => {
                self.modal = ModalState {
                    kind: ModalKind::TextInput,
                    title: "Release Version".to_string(),
                    message: "Enter version (e.g. 0.3.0)".to_string(),
                    confirm_purpose: None,
                    input_purpose: Some(TextInputPurpose::ReleaseCustomVersion),
                    input_value: String::new(),
                };
                true
            }

            // Config tab
            ActionItem::RunSetupWizard => {
                if let Err(e) = self.run_setup_wizard() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Setup failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Setup complete.");
                    self.log("Setup complete.");
                }
                true
            }
            ActionItem::ReloadConfig => {
                if let Err(e) = self.reload_config_labels() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Reload config failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Config reloaded.");
                    self.log("Config reloaded.");
                }
                true
            }
            ActionItem::ClearConfig => {
                self.modal = ModalState {
                    kind: ModalKind::Confirm,
                    title: "Confirm".to_string(),
                    message: "Clear config? This will delete the local config file.".to_string(),
                    confirm_purpose: Some(ConfirmPurpose::ClearConfig),
                    input_purpose: None,
                    input_value: String::new(),
                };
                true
            }
        }
    }

    pub fn log(&mut self, line: impl Into<String>) {
        self.logs.push(line.into());
        if self.logs.len() > 200 {
            self.logs.drain(0..self.logs.len().saturating_sub(200));
        }
    }

    pub fn next_tab(&mut self) {
        let idx = Tab::ALL
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0);
        self.active_tab = Tab::ALL[(idx + 1) % Tab::ALL.len()];
        self.action_index = 0;
        self.set_status(
            StatusLevel::Info,
            format!("Tab: {}", self.active_tab.title()),
        );
    }

    pub fn prev_tab(&mut self) {
        let idx = Tab::ALL
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0);
        let next = if idx == 0 {
            Tab::ALL.len() - 1
        } else {
            idx - 1
        };
        self.active_tab = Tab::ALL[next];
        self.action_index = 0;
        self.set_status(
            StatusLevel::Info,
            format!("Tab: {}", self.active_tab.title()),
        );
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::TabBar => Focus::LeftPane,
            Focus::LeftPane => Focus::CommitEditor,
            Focus::CommitEditor => Focus::RightPane,
            Focus::RightPane => Focus::TabBar,
        };
        self.set_status(StatusLevel::Info, format!("Focus: {:?}", self.focus));
    }

    pub fn clear_editor(&mut self) {
        let mut editor = TextArea::default();
        editor.set_cursor_line_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );
        self.commit_editor = editor;
        self.reset_editor_block();

        self.set_status(StatusLevel::Info, "Cleared commit message.");
        self.log("Cleared commit message.");
    }

    pub fn handle_global_key(&mut self, tasks: &TaskRunner, key: &KeyEvent) -> bool {
        // If an app modal is open, it captures keys (except Ctrl+C).
        if self.modal.kind != ModalKind::None {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                    return true;
                }
                // Close modal on Esc
                (KeyCode::Esc, _) => {
                    self.modal = ModalState::none();
                    self.set_status(StatusLevel::Info, "Closed dialog.");
                    return true;
                }
                // Confirm modal: Enter = confirm, Backspace/Delete ignored
                (KeyCode::Enter, KeyModifiers::NONE) if self.modal.kind == ModalKind::Confirm => {
                    let purpose = self.modal.confirm_purpose;
                    self.modal = ModalState::none();
                    if let Some(p) = purpose {
                        self.handle_confirm(tasks, p);
                    }
                    return true;
                }
                // Text input modal: type, backspace, enter to accept
                (KeyCode::Backspace, KeyModifiers::NONE)
                    if self.modal.kind == ModalKind::TextInput =>
                {
                    self.modal.input_value.pop();
                    return true;
                }
                (KeyCode::Enter, KeyModifiers::NONE) if self.modal.kind == ModalKind::TextInput => {
                    let purpose = self.modal.input_purpose;
                    let value = self.modal.input_value.trim().to_string();
                    self.modal = ModalState::none();
                    if let Some(p) = purpose {
                        self.handle_text_input(tasks, p, value);
                    }
                    return true;
                }
                (KeyCode::Char(ch), KeyModifiers::NONE)
                    if self.modal.kind == ModalKind::TextInput =>
                {
                    // Simple input: accept most printable chars
                    if !ch.is_control() {
                        self.modal.input_value.push(ch);
                    }
                    return true;
                }
                _ => return true,
            }
        }

        // Toggle help
        if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Char('?') {
            self.show_help = !self.show_help;
            self.set_status(
                StatusLevel::Info,
                if self.show_help {
                    "Help opened. Press ? again to close."
                } else {
                    "Help closed."
                },
            );
            return true;
        }

        // If help is open, capture all inputs except Esc/Ctrl+C/?.
        if self.show_help {
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {
                    self.show_help = false;
                    self.set_status(StatusLevel::Info, "Help closed.");
                    true
                }
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                    true
                }
                _ => true,
            }
        } else {
            false
        }
    }

    pub fn handle_nav_key(&mut self, key: &KeyEvent) -> bool {
        // Quit
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.should_quit = true;
                return true;
            }
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return true;
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.focus_next();
                return true;
            }
            _ => {}
        }

        // Tabs:
        // - Alt+Left/Right always switches tabs.
        // - Left/Right switches tabs when not editing.
        match (key.code, key.modifiers) {
            (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                self.next_tab();
                true
            }
            (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                self.prev_tab();
                true
            }
            (KeyCode::Right, KeyModifiers::NONE) if self.focus != Focus::CommitEditor => {
                self.next_tab();
                true
            }
            (KeyCode::Left, KeyModifiers::NONE) if self.focus != Focus::CommitEditor => {
                self.prev_tab();
                true
            }
            _ => false,
        }
    }

    pub fn handle_generate_key(&mut self, tasks: &TaskRunner, key: &KeyEvent) -> bool {
        // Actions that should work regardless of focus.
        match (key.code, key.modifiers) {
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                let _started = self.start_generate_from_staged(tasks);
                return true;
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let _started = self.start_commit_from_editor(tasks);
                return true;
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                self.clear_editor();
                return true;
            }
            _ => {}
        }

        // Editor input when focused.
        if self.focus == Focus::CommitEditor {
            if let Some(input) = to_textarea_input(key) {
                self.commit_editor.input(input);
                return true;
            }
        }

        false
    }

    #[allow(dead_code)]
    pub fn commit_from_textarea(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }

        let msg = self.commit_editor.lines().join("\n").trim().to_string();
        if msg.is_empty() {
            anyhow::bail!("Commit message is empty.");
        }

        git::commit_changes(&msg)?;
        self.set_status(StatusLevel::Success, "Committed successfully.");
        self.log("Committed changes.");
        Ok(())
    }

    fn build_generator(&mut self) -> Result<Generator> {
        if self.mock_mode {
            self.provider_label = "Mock".to_string();
            self.model_label = "-".to_string();
            return Ok(Generator::Mock(MockGenerator::new()));
        }

        match Config::load()? {
            Some(cfg) => {
                self.provider_label = cfg.provider.to_string();
                self.model_label = cfg.model.clone();

                Ok(match cfg.provider {
                    Provider::OpenAI => {
                        Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model))
                    }
                    Provider::Anthropic => {
                        Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
                    }
                    Provider::Gemini => {
                        Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model))
                    }
                })
            }
            None => {
                self.provider_label = "Not configured".to_string();
                self.model_label = "-".to_string();
                anyhow::bail!("No config found. Use the Config tab or run setup.")
            }
        }
    }

    fn reload_config_labels(&mut self) -> Result<()> {
        match Config::load()? {
            Some(cfg) => {
                self.provider_label = cfg.provider.to_string();
                self.model_label = cfg.model;
            }
            None => {
                self.provider_label = "Not configured".to_string();
                self.model_label = "-".to_string();
            }
        }
        Ok(())
    }

    fn run_setup_wizard(&mut self) -> Result<()> {
        // NOTE: The TUI runtime suspends raw mode + alt screen when running this.
        let cfg = setup::run_setup()?;
        self.provider_label = cfg.provider.to_string();
        self.model_label = cfg.model;
        Ok(())
    }

    fn clear_config_file(&mut self) -> Result<()> {
        let path = Config::get_path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.provider_label = "Not configured".to_string();
        self.model_label = "-".to_string();
        Ok(())
    }

    fn handle_confirm(&mut self, tasks: &TaskRunner, purpose: ConfirmPurpose) {
        match purpose {
            ConfirmPurpose::ClearConfig => {
                if let Err(e) = self.clear_config_file() {
                    self.set_status(StatusLevel::Error, e.to_string());
                    self.log(format!("Clear config failed: {e}"));
                } else {
                    self.set_status(StatusLevel::Success, "Config cleared.");
                    self.log("Config cleared.");
                }
            }
            ConfirmPurpose::PushAllTags => {
                let _started = self.start_push_all_tags(tasks);
            }
            ConfirmPurpose::ReleaseTrigger => {
                if let Some(v) = self.pending_release_version.clone() {
                    // Suspend the TUI for the whole release execution so cargo/clippy/test output
                    // does not corrupt the terminal UI. The release pipeline intentionally streams
                    // output to stdout/stderr for transparency.
                    let result = runtime::with_tui_suspended(|| self.perform_release(&v));

                    match result {
                        Ok(_) => {
                            let tag = format!("v{}", v);
                            self.set_status(
                                StatusLevel::Success,
                                format!("Release initiated: pushed tag {}", tag),
                            );
                            self.log(format!("Release initiated: {}", tag));

                            if let Some(repo_https) = origin_https_repo_url().ok().flatten() {
                                self.log(format!(
                                    "Track progress (Actions): {}/actions?query=workflow%3ARelease",
                                    repo_https
                                ));
                                self.log(format!(
                                    "Release page: {}/releases/tag/{}",
                                    repo_https, tag
                                ));
                            }
                        }
                        Err(e) => {
                            self.set_status(StatusLevel::Error, e.to_string());
                            self.log(format!("Release failed: {}", e));
                        }
                    }
                } else {
                    self.set_status(StatusLevel::Error, "No pending release version.");
                    self.log("Release failed: missing pending version.");
                }
            }
        }
    }

    fn handle_text_input(&mut self, tasks: &TaskRunner, purpose: TextInputPurpose, value: String) {
        match purpose {
            TextInputPurpose::PushSpecificTag => {
                let v = value.trim();
                if v.is_empty() {
                    self.set_status(StatusLevel::Error, "Tag cannot be empty.");
                    self.log("Push tag failed: empty tag.");
                    return;
                }

                let _started = self.start_push_tag(tasks, v.to_string());
            }
            TextInputPurpose::ReleaseCustomVersion => {
                let v = value.trim();
                if v.is_empty() {
                    self.set_status(StatusLevel::Error, "Version cannot be empty.");
                    self.log("Release failed: empty version.");
                    return;
                }
                self.pending_release_version = Some(v.to_string());
                self.modal = ModalState {
                    kind: ModalKind::Confirm,
                    title: "Final confirmation".to_string(),
                    message: format!(
                        "Create and push tag v{}? This triggers CI release + crates publish.",
                        v
                    ),
                    confirm_purpose: Some(ConfirmPurpose::ReleaseTrigger),
                    input_purpose: None,
                    input_value: String::new(),
                };
            }
        }
    }

    fn start_generate_from_staged(&mut self, tasks: &TaskRunner) -> bool {
        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Generate while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Generate failed: not a git repository.");
            return true;
        }

        let mock_mode = self.mock_mode;

        let started = tasks.start(
            TaskKind::GenerateCommitFromStaged,
            "Generating commit message (staged)…",
            move |tx| {
                let _ = tx.send(TaskEvent::Progress {
                    message: "Collecting staged diff…".to_string(),
                });

                let summary = git::diff_summary(git::DiffSource::Staged)?;
                let summary_text = format!(
                    "{} files, +{} -{}, ~{} bytes",
                    summary.files_changed, summary.insertions, summary.deletions, summary.bytes
                );

                let diff = git::get_diff(git::DiffSource::Staged)?;
                let (generator, provider, model) = build_generator_for_task(mock_mode)?;

                let _ = tx.send(TaskEvent::Progress {
                    message: format!("Generating with {}…", provider),
                });

                let msg = runtime::tui_block_on(generator.generate(&diff, None))?;

                Ok(TaskResult::GeneratedCommitMessage {
                    message: msg,
                    summary: summary_text,
                    provider,
                    model,
                })
            },
        );

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Generate ignored: task runner was busy.");
        }
        started
    }

    fn start_commit_from_editor(&mut self, tasks: &TaskRunner) -> bool {
        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Commit while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Commit failed: not a git repository.");
            return true;
        }

        let msg = self.commit_editor.lines().join("\n").trim().to_string();
        if msg.is_empty() {
            self.set_status(StatusLevel::Error, "Commit message is empty.");
            self.log("Commit failed: empty message.");
            return true;
        }

        let started = tasks.start(TaskKind::CommitFromEditor, "Committing…", move |_tx| {
            git::commit_changes(&msg)?;
            Ok(TaskResult::OkMessage {
                status: "Committed successfully.".to_string(),
                log: Some("Committed changes.".to_string()),
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Commit ignored: task runner was busy.");
        }
        started
    }

    fn start_stage_all(&mut self, tasks: &TaskRunner) -> bool {
        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Stage All while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Stage all failed: not a git repository.");
            return true;
        }

        let started = tasks.start(TaskKind::StageAll, "Staging all changes…", move |_tx| {
            git::stage_all()?;
            Ok(TaskResult::OkMessage {
                status: "Staged all changes.".to_string(),
                log: Some("Staged all changes.".to_string()),
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Stage all ignored: task runner was busy.");
        }
        started
    }

    fn start_load_diff(&mut self, tasks: &TaskRunner, source: DiffViewSource) -> bool {
        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Load Diff while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Load diff failed: not a git repository.");
            return true;
        }

        let label = format!("Loading {} diff…", source.label());
        let status = format!("Loaded {} diff.", source.label().to_lowercase());

        let started = tasks.start(TaskKind::LoadDiff, label, move |_tx| {
            let text = git::get_diff_allow_empty(source.to_git_source())?;
            Ok(TaskResult::LoadedDiff {
                source,
                text,
                status,
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Load diff ignored: task runner was busy.");
        }
        started
    }

    fn start_push_branch(&mut self, tasks: &TaskRunner) -> bool {
        use std::process::Command;

        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Push Branch while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Push branch failed: not a git repository.");
            return true;
        }

        let started = tasks.start(TaskKind::PushBranch, "Pushing branch…", move |_tx| {
            // If upstream exists, `git push` is enough. Otherwise set upstream.
            let has_upstream = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if has_upstream {
                let o = Command::new("git").args(["push"]).output()?;
                if !o.status.success() {
                    anyhow::bail!("git push failed: {}", String::from_utf8_lossy(&o.stderr));
                }
                return Ok(TaskResult::OkMessage {
                    status: "Branch pushed.".to_string(),
                    log: Some("Branch pushed.".to_string()),
                });
            }

            let o = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()?;
            if !o.status.success() {
                anyhow::bail!(
                    "git rev-parse --abbrev-ref HEAD failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();

            let o = Command::new("git")
                .args(["push", "-u", "origin", &branch])
                .output()?;
            if !o.status.success() {
                anyhow::bail!(
                    "git push -u origin {} failed: {}",
                    branch,
                    String::from_utf8_lossy(&o.stderr)
                );
            }

            Ok(TaskResult::OkMessage {
                status: "Branch pushed.".to_string(),
                log: Some("Branch pushed.".to_string()),
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Push branch ignored: task runner was busy.");
        }
        started
    }

    fn start_push_tag(&mut self, tasks: &TaskRunner, tag: String) -> bool {
        use std::process::Command;

        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Push Tag while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Push tag failed: not a git repository.");
            return true;
        }

        let t = tag.trim().to_string();
        if t.is_empty() {
            self.set_status(StatusLevel::Error, "Tag cannot be empty.");
            self.log("Push tag failed: empty tag.");
            return true;
        }

        let label = format!("Pushing tag {}…", t);

        let started = tasks.start(TaskKind::PushTag, label, move |_tx| {
            let o = Command::new("git").args(["push", "origin", &t]).output()?;
            if !o.status.success() {
                anyhow::bail!(
                    "git push origin {} failed: {}",
                    t,
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Ok(TaskResult::OkMessage {
                status: format!("Tag pushed: {}", t),
                log: Some(format!("Tag pushed: {}", t)),
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Push tag ignored: task runner was busy.");
        }
        started
    }

    fn start_push_all_tags(&mut self, tasks: &TaskRunner) -> bool {
        use std::process::Command;

        if tasks.is_busy() {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Ignored: tried to start Push All Tags while another task is running.");
            return false;
        }
        if !git::is_repo() {
            self.set_status(StatusLevel::Error, "Not a git repository (or git is not installed).");
            self.log("Push all tags failed: not a git repository.");
            return true;
        }

        let started = tasks.start(TaskKind::PushAllTags, "Pushing all tags…", move |_tx| {
            let o = Command::new("git").args(["push", "--tags"]).output()?;
            if !o.status.success() {
                anyhow::bail!(
                    "git push --tags failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Ok(TaskResult::OkMessage {
                status: "All tags pushed.".to_string(),
                log: Some("All tags pushed.".to_string()),
            })
        });

        if !started {
            self.set_status(StatusLevel::Info, "Busy: another task is running.");
            self.log("Push all tags ignored: task runner was busy.");
        }
        started
    }

    #[allow(dead_code)]
    fn generate_commit_message_staged_blocking(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }

        self.diff_source_label = "Staged (recommended)".to_string();

        let summary = git::diff_summary(git::DiffSource::Staged)?;
        self.diff_summary = format!(
            "{} files, +{} -{}, ~{} bytes",
            summary.files_changed, summary.insertions, summary.deletions, summary.bytes
        );

        let diff = git::get_diff(git::DiffSource::Staged)?;
        let generator = self.build_generator()?;

        self.set_status(StatusLevel::Info, "Generating commit message...");
        self.log("Generating commit message (staged)…");

        // NOTE: blocking; runtime module will provide non-blocking soon.
        let msg = super::runtime::tui_block_on(generator.generate(&diff, None))?;

        self.set_commit_message_text(&msg);
        self.set_status(StatusLevel::Success, "Generated.");
        self.log("Generated commit message.");
        Ok(())
    }

    fn stage_patch(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }
        // Interactive; caller should run via `with_tui_suspended`.
        git::stage_patch()
    }

    #[allow(dead_code)]
    fn stage_all(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }
        git::stage_all()
    }

    fn unstage_patch(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }
        // Interactive; caller should run via `with_tui_suspended`.
        git::unstage_patch()
    }

    fn unstage_all(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }
        git::unstage_all()
    }

    #[allow(dead_code)]
    fn load_diff_view(&mut self, source: DiffViewSource) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }

        self.diff_view_source = source;
        self.diff_scroll = 0;

        let text = git::get_diff_allow_empty(source.to_git_source())?;
        self.diff_text = text;

        Ok(())
    }

    #[allow(dead_code)]
    fn push_current_branch_with_upstream(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }

        // If upstream exists, `git push` is enough. Otherwise set upstream.
        let has_upstream = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_upstream {
            let o = std::process::Command::new("git").args(["push"]).output()?;
            if !o.status.success() {
                anyhow::bail!("git push failed: {}", String::from_utf8_lossy(&o.stderr));
            }
            return Ok(());
        }

        let branch = self.current_branch()?;
        let o = std::process::Command::new("git")
            .args(["push", "-u", "origin", &branch])
            .output()?;
        if !o.status.success() {
            anyhow::bail!(
                "git push -u origin {} failed: {}",
                branch,
                String::from_utf8_lossy(&o.stderr)
            );
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn push_tag(&mut self, tag: &str) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }
        let t = tag.trim();
        if t.is_empty() {
            anyhow::bail!("Tag name cannot be empty.");
        }

        let o = std::process::Command::new("git")
            .args(["push", "origin", t])
            .output()?;
        if !o.status.success() {
            anyhow::bail!(
                "git push origin {} failed: {}",
                t,
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn push_all_tags(&mut self) -> Result<()> {
        if !git::is_repo() {
            anyhow::bail!("Not a git repository (or git is not installed).");
        }

        let o = std::process::Command::new("git")
            .args(["push", "--tags"])
            .output()?;
        if !o.status.success() {
            anyhow::bail!(
                "git push --tags failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn current_branch(&self) -> Result<String> {
        let o = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()?;

        if !o.status.success() {
            anyhow::bail!(
                "git rev-parse --abbrev-ref HEAD failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    fn start_release_bump(&mut self, bump: &str) -> bool {
        // Compute next version from Cargo.toml using the core release module, then ask for confirmation.
        let bump_kind = match bump {
            "patch" => release::BumpKind::Patch,
            "minor" => release::BumpKind::Minor,
            "major" => release::BumpKind::Major,
            other => {
                self.set_status(StatusLevel::Error, format!("Unknown bump kind: {}", other));
                self.log(format!("Release failed: unknown bump kind {}", other));
                return true;
            }
        };

        let plan = match release::plan_bump("Cargo.toml", bump_kind) {
            Ok(p) => p,
            Err(e) => {
                self.set_status(StatusLevel::Error, e.to_string());
                self.log(format!("Release failed: {e}"));
                return true;
            }
        };

        self.pending_release_version = Some(plan.new_version.clone());
        self.modal = ModalState {
            kind: ModalKind::Confirm,
            title: "Final confirmation".to_string(),
            message: format!(
                "Bump {} -> {} and push tag {}? This triggers CI release + crates publish.",
                plan.old_version, plan.new_version, plan.tag
            ),
            confirm_purpose: Some(ConfirmPurpose::ReleaseTrigger),
            input_purpose: None,
            input_value: String::new(),
        };
        true
    }

    fn perform_release(&mut self, new_version: &str) -> Result<()> {
        // Tag-based CI release pipeline:
        // - Guardrails (repo, origin remote, clean tree, expected branch)
        // - Preflight checks (fmt/clippy/test) BEFORE bump
        // - Bump Cargo.toml + generate lockfile
        // - Stage + commit
        // - Tag collision checks + create annotated tag + push tag
        //
        // The tag push triggers GitHub Actions to build releases and publish to crates.io.
        self.pending_release_version = Some(new_version.to_string());

        let plan = release::plan_custom("Cargo.toml", new_version)?;
        let commit_message = self
            .generate_release_commit_message(&plan.new_version)
            .unwrap_or_else(|_| format!("chore(release): {}", plan.tag));

        release::run_tag_release(
            "Cargo.toml",
            &plan,
            &commit_message,
            &release::PreflightConfig::default(),
            &release::ReleaseGuardrailConfig::default(),
        )?;

        // Also surface helpful URLs in the status/log (best-effort)
        if let Some(repo_https) = origin_https_repo_url().ok().flatten() {
            self.set_status(
                StatusLevel::Success,
                format!(
                    "Release initiated: pushed tag {} (Actions: {}/actions?query=workflow%3ARelease)",
                    plan.tag, repo_https
                ),
            );
            self.log(format!(
                "Track progress (Actions): {}/actions?query=workflow%3ARelease",
                repo_https
            ));
            self.log(format!(
                "Release page: {}/releases/tag/{}",
                repo_https, plan.tag
            ));
        }

        Ok(())
    }

    fn generate_release_commit_message(&mut self, new_version: &str) -> Result<String> {
        // Generate from staged diff; hint keeps the commit deterministic.
        let hint = Some(format!("release: bump version to v{}", new_version));
        let diff = git::get_diff(git::DiffSource::Staged)?;
        let generator = self.build_generator()?;
        super::runtime::tui_block_on(generator.generate(&diff, hint))
    }

    pub fn set_commit_message_text(&mut self, msg: &str) {
        let mut editor = TextArea::default();
        editor.set_cursor_line_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );

        for (i, line) in msg.lines().enumerate() {
            if i > 0 {
                editor.insert_newline();
            }
            editor.insert_str(line);
        }

        self.commit_editor = editor;
        self.reset_editor_block();
    }

    fn reset_editor_block(&mut self) {
        // view.rs will override border styling per-focus each frame,
        // but we keep a default block so the editor is usable even if view changes.
        self.commit_editor.set_block(
            ratatui::widgets::Block::default()
                .title(" Commit Message ")
                .borders(ratatui::widgets::Borders::ALL),
        );
    }

    // NOTE: helper for background tasks (cannot borrow &mut self from worker thread)
    // Returns (Generator, provider_label, model_label)
}

fn build_generator_for_task(mock_mode: bool) -> Result<(Generator, String, String)> {
    if mock_mode {
        return Ok((
            Generator::Mock(MockGenerator::new()),
            "Mock".to_string(),
            "-".to_string(),
        ));
    }

    match Config::load()? {
        Some(cfg) => {
            let provider_label = cfg.provider.to_string();
            let model_label = cfg.model.clone();
            let gen = match cfg.provider {
                Provider::OpenAI => Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model)),
                Provider::Anthropic => {
                    Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
                }
                Provider::Gemini => Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model)),
            };
            Ok((gen, provider_label, model_label))
        }
        None => anyhow::bail!("No config found. Use the Config tab or run setup."),
    }
}

fn origin_https_repo_url() -> Result<Option<String>> {
    let o = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()?;

    if !o.status.success() {
        return Ok(None);
    }

    let url = String::from_utf8_lossy(&o.stdout).trim().to_string();

    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let rest = rest.trim_end_matches(".git");
        return Ok(Some(format!("https://github.com/{}", rest)));
    }

    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.trim_end_matches(".git");
        return Ok(Some(format!("https://github.com/{}", rest)));
    }

    Ok(None)
}

pub fn to_textarea_input(key: &KeyEvent) -> Option<Input> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let input = match key.code {
        KeyCode::Char(c) => Input {
            key: ratatui_textarea::Key::Char(c),
            ctrl,
            alt,
        },
        KeyCode::Enter => Input {
            key: ratatui_textarea::Key::Enter,
            ctrl,
            alt,
        },
        KeyCode::Backspace => Input {
            key: ratatui_textarea::Key::Backspace,
            ctrl,
            alt,
        },
        KeyCode::Delete => Input {
            key: ratatui_textarea::Key::Delete,
            ctrl,
            alt,
        },
        KeyCode::Left => Input {
            key: ratatui_textarea::Key::Left,
            ctrl,
            alt,
        },
        KeyCode::Right => Input {
            key: ratatui_textarea::Key::Right,
            ctrl,
            alt,
        },
        KeyCode::Up => Input {
            key: ratatui_textarea::Key::Up,
            ctrl,
            alt,
        },
        KeyCode::Down => Input {
            key: ratatui_textarea::Key::Down,
            ctrl,
            alt,
        },
        KeyCode::Home => Input {
            key: ratatui_textarea::Key::Home,
            ctrl,
            alt,
        },
        KeyCode::End => Input {
            key: ratatui_textarea::Key::End,
            ctrl,
            alt,
        },
        KeyCode::PageUp => Input {
            key: ratatui_textarea::Key::PageUp,
            ctrl,
            alt,
        },
        KeyCode::PageDown => Input {
            key: ratatui_textarea::Key::PageDown,
            ctrl,
            alt,
        },
        KeyCode::Tab => Input {
            key: ratatui_textarea::Key::Tab,
            ctrl,
            alt,
        },
        _ => return None,
    };

    Some(input)
}

// NOTE: local semver/version parsing helpers were removed.
// Release planning is now handled by the core `release` module.
