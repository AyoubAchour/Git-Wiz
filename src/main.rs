mod config;
mod generator;
mod git;
mod setup;
mod ui;

use anyhow::Result;
use clap::Parser;
use cliclack::{input, log, select};
use config::{Config, Provider};
use generator::{AnthropicGenerator, GeminiGenerator, Generator, MockGenerator, OpenAIGenerator};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Optional hint to guide commit message generation (used in the Generate flow)
    #[arg(long)]
    hint: Option<String>,

    /// Force use of the mock generator (no API calls)
    #[arg(long, default_value_t = false)]
    mock: bool,

    /// Re-run the setup wizard (also accessible via the main menu)
    #[arg(long, default_value_t = false)]
    config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainAction {
    Generate,
    Stage,
    View,
    Config,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSource {
    Staged,
    Unstaged,
    Both,
}

impl From<DiffSource> for git::DiffSource {
    fn from(value: DiffSource) -> Self {
        match value {
            DiffSource::Staged => git::DiffSource::Staged,
            DiffSource::Unstaged => git::DiffSource::Unstaged,
            DiffSource::Both => git::DiffSource::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageAction {
    Patch,
    All,
    UnstagePatch,
    UnstageAll,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewAction {
    Summary,
    Staged,
    Unstaged,
    Both,
    Back,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Ensure terminal colors are enabled on Windows
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    let args = Args::parse();

    // 1) Display Banner
    ui::print_banner();

    // Keep flag for quick access, but the default experience is menu-driven.
    if args.config {
        setup::run_setup()?;
        return Ok(());
    }

    // 2) Resolve generator early (so we can run generate flow quickly when chosen),
    // but do NOT call any LLM until the user explicitly selects Generate.
    let generator = build_generator(args.mock)?;

    // 3) Always show the main menu first
    loop {
        let action = select("What would you like to do?")
            .item(
                MainAction::Generate,
                "Generate commit message",
                "Generate from staged/unstaged changes, then commit/edit/regenerate",
            )
            .item(
                MainAction::Stage,
                "Stage changes",
                "Stage interactively (git add -p) or stage all",
            )
            .item(
                MainAction::View,
                "View diff",
                "Preview staged/unstaged diff before generating",
            )
            .item(
                MainAction::Config,
                "Config / Setup",
                "Switch provider, model, or update API key",
            )
            .item(MainAction::Exit, "Exit", "Close Git Wiz")
            .interact()?;

        match action {
            MainAction::Generate => {
                if let Err(e) = run_generate_flow(&generator, args.hint.clone()).await {
                    ui::print_error(&e.to_string());
                    // Return to main menu
                }
            }
            MainAction::Stage => {
                if let Err(e) = run_stage_flow() {
                    ui::print_error(&e.to_string());
                }
            }
            MainAction::View => {
                if let Err(e) = run_view_flow() {
                    ui::print_error(&e.to_string());
                }
            }
            MainAction::Config => {
                if let Err(e) = setup::run_setup() {
                    ui::print_error(&e.to_string());
                }
            }
            MainAction::Exit => {
                ui::print_outro("Done.");
                break;
            }
        }
    }

    Ok(())
}

fn build_generator(force_mock: bool) -> Result<Generator> {
    if force_mock {
        return Ok(Generator::Mock(MockGenerator::new()));
    }

    match Config::load()? {
        Some(cfg) => Ok(match cfg.provider {
            Provider::OpenAI => Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model)),
            Provider::Anthropic => {
                Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
            }
            Provider::Gemini => Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model)),
        }),
        None => {
            // First run flow
            let cfg = setup::run_setup()?;
            Ok(match cfg.provider {
                Provider::OpenAI => Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model)),
                Provider::Anthropic => {
                    Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
                }
                Provider::Gemini => Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model)),
            })
        }
    }
}

async fn run_generate_flow(generator: &Generator, hint: Option<String>) -> Result<()> {
    if !git::is_repo() {
        ui::print_error("Not a git repository (or git is not installed).");
        return Ok(());
    }

    let source = select("Generate from which changes?")
        .item(
            DiffSource::Staged,
            "Staged (recommended)",
            "Uses: git diff --cached",
        )
        .item(DiffSource::Unstaged, "Unstaged", "Uses: git diff")
        .item(
            DiffSource::Both,
            "Both staged + unstaged",
            "Combines: git diff --cached + git diff",
        )
        .interact()?;

    let diff = match source {
        DiffSource::Staged => get_staged_diff_or_offer_stage()?,
        DiffSource::Unstaged => get_unstaged_diff_or_offer_stage()?,
        DiffSource::Both => get_both_diff_or_offer_stage()?,
    };

    if diff.trim().is_empty() {
        // User chose Back / nothing to do.
        return Ok(());
    }

    let summary = git::diff_summary(source.into())?;
    ui::print_success(&format!(
        "Summary: {} files, +{} -{}, ~{} bytes",
        summary.files_changed, summary.insertions, summary.deletions, summary.bytes
    ));

    // Pre-flight confirmation to avoid accidental spend
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Preflight {
        Proceed,
        Preview,
        Cancel,
    }

    loop {
        let choice = select("Before I call the model...")
            .item(
                Preflight::Proceed,
                "Proceed",
                "Generate the commit message now",
            )
            .item(
                Preflight::Preview,
                "Preview diff",
                "Show the diff that will be analyzed",
            )
            .item(Preflight::Cancel, "Cancel", "Return to main menu")
            .interact()?;

        match choice {
            Preflight::Proceed => break,
            Preflight::Preview => {
                println!();
                println!("{}", diff);
                println!();
            }
            Preflight::Cancel => return Ok(()),
        }
    }

    let mut current_message = String::new();
    let mut needs_generation = true;

    loop {
        if needs_generation {
            let result: anyhow::Result<String> =
                ui::with_spinner("Thinking...", "Analysis complete", || async {
                    generator.generate(&diff, hint.clone()).await
                })
                .await;

            match result {
                Ok(msg) => {
                    current_message = msg;
                }
                Err(e) => {
                    ui::print_error(&e.to_string());
                }
            }
            needs_generation = false;
        }

        if !current_message.is_empty() {
            ui::print_commit_preview(&current_message);
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Action {
            Commit,
            Edit,
            Regenerate,
            Back,
        }

        let action = select("What would you like to do?")
            .item(
                Action::Commit,
                "Commit",
                "Run git commit using this message",
            )
            .item(Action::Edit, "Edit", "Refine the message")
            .item(Action::Regenerate, "Regenerate", "Try again")
            .item(Action::Back, "Back", "Return to main menu")
            .interact()?;

        match action {
            Action::Commit => {
                // If the user generated from unstaged or both, committing may not match.
                // We warn and offer staging actions to avoid confusion.
                if source != DiffSource::Staged {
                    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                    enum CommitGuard {
                        StageAllThenCommit,
                        StagePatchThenCommit,
                        CommitStagedOnly,
                        Cancel,
                    }

                    let guard = select("You generated from unstaged changes. Git commits staged changes only. What next?")
                        .item(CommitGuard::StageAllThenCommit, "Stage all then commit", "git add -A, then git commit")
                        .item(CommitGuard::StagePatchThenCommit, "Stage interactively then commit", "git add -p, then git commit")
                        .item(CommitGuard::CommitStagedOnly, "Commit staged only", "Proceed without staging anything new")
                        .item(CommitGuard::Cancel, "Cancel", "Go back")
                        .interact()?;

                    match guard {
                        CommitGuard::StageAllThenCommit => {
                            if let Err(e) = git::stage_all() {
                                ui::print_error(&format!("Failed to stage all changes: {}", e));
                                continue;
                            }
                        }
                        CommitGuard::StagePatchThenCommit => {
                            if let Err(e) = git::stage_patch() {
                                ui::print_error(&format!("Failed to stage interactively: {}", e));
                                continue;
                            }
                        }
                        CommitGuard::CommitStagedOnly => {}
                        CommitGuard::Cancel => continue,
                    }
                }

                let result: anyhow::Result<()> = ui::with_spinner(
                    "Committing...",
                    "Changes committed successfully!",
                    || async { git::commit_changes(&current_message) },
                )
                .await;

                match result {
                    Ok(_) => {
                        ui::print_outro("Done.");
                        return Ok(());
                    }
                    Err(e) => {
                        ui::print_error(&format!("Commit failed: {}", e));
                        // Loop back to allow edit/regenerate/back
                    }
                }
            }
            Action::Edit => {
                let new_msg = input("Edit commit message")
                    .default_input(&current_message)
                    .interact()?;

                current_message = new_msg;
                ui::print_info("Message updated.");
            }
            Action::Regenerate => {
                ui::print_info("Regenerating...");
                needs_generation = true;
            }
            Action::Back => return Ok(()),
        }
    }
}

fn run_stage_flow() -> Result<()> {
    if !git::is_repo() {
        ui::print_error("Not a git repository (or git is not installed).");
        return Ok(());
    }

    loop {
        let action = select("Stage / Unstage changes")
            .item(
                StageAction::Patch,
                "Stage interactively",
                "Runs: git add -p",
            )
            .item(StageAction::All, "Stage all", "Runs: git add -A")
            .item(
                StageAction::UnstagePatch,
                "Unstage interactively",
                "Runs: git restore --staged -p (fallback: git reset -p)",
            )
            .item(
                StageAction::UnstageAll,
                "Unstage all",
                "Runs: git restore --staged . (fallback: git reset)",
            )
            .item(StageAction::Back, "Back", "Return to main menu")
            .interact()?;

        match action {
            StageAction::Patch => {
                // interactive; don't wrap in spinner.
                match git::stage_patch() {
                    Ok(_) => ui::print_success("Staging complete."),
                    Err(e) => ui::print_error(&format!("{}", e)),
                }
            }
            StageAction::All => match git::stage_all() {
                Ok(_) => ui::print_success("Staged all changes."),
                Err(e) => ui::print_error(&format!("{}", e)),
            },
            StageAction::UnstagePatch => match git::unstage_patch() {
                Ok(_) => ui::print_success("Unstaging complete."),
                Err(e) => ui::print_error(&format!("{}", e)),
            },
            StageAction::UnstageAll => match git::unstage_all() {
                Ok(_) => ui::print_success("Unstaged all changes."),
                Err(e) => ui::print_error(&format!("{}", e)),
            },
            StageAction::Back => return Ok(()),
        }
    }
}

fn run_view_flow() -> Result<()> {
    if !git::is_repo() {
        ui::print_error("Not a git repository (or git is not installed).");
        return Ok(());
    }

    loop {
        let action = select("View diff / summary")
            .item(
                ViewAction::Summary,
                "Summary (staged + unstaged)",
                "Shows files changed, insertions, deletions (no diff content)",
            )
            .item(
                ViewAction::Staged,
                "Staged diff",
                "Shows: git diff --cached",
            )
            .item(ViewAction::Unstaged, "Unstaged diff", "Shows: git diff")
            .item(ViewAction::Both, "Both", "Shows staged then unstaged")
            .item(ViewAction::Back, "Back", "Return to main menu")
            .interact()?;

        match action {
            ViewAction::Summary => {
                let staged = git::diff_summary(git::DiffSource::Staged)?;
                let unstaged = git::diff_summary(git::DiffSource::Unstaged)?;

                ui::print_success(&format!(
                    "Staged:   {} files, +{} -{}, ~{} bytes",
                    staged.files_changed, staged.insertions, staged.deletions, staged.bytes
                ));
                ui::print_success(&format!(
                    "Unstaged: {} files, +{} -{}, ~{} bytes",
                    unstaged.files_changed, unstaged.insertions, unstaged.deletions, unstaged.bytes
                ));
            }
            ViewAction::Staged => {
                let text = git::get_diff_allow_empty(git::DiffSource::Staged)?;
                if text.trim().is_empty() {
                    log::info("No staged changes.").ok();
                } else {
                    println!();
                    println!("{}", text);
                    println!();
                }
            }
            ViewAction::Unstaged => {
                let text = git::get_diff_allow_empty(git::DiffSource::Unstaged)?;
                if text.trim().is_empty() {
                    log::info("No unstaged changes.").ok();
                } else {
                    println!();
                    println!("{}", text);
                    println!();
                }
            }
            ViewAction::Both => {
                let text = git::get_diff_allow_empty(git::DiffSource::Both)?;
                if text.trim().is_empty() {
                    log::info("No staged or unstaged changes.").ok();
                } else {
                    println!();
                    println!("{}", text);
                    println!();
                }
            }
            ViewAction::Back => return Ok(()),
        }
    }
}

fn get_staged_diff_or_offer_stage() -> Result<String> {
    match git::get_diff(git::DiffSource::Staged) {
        Ok(d) => Ok(d),
        Err(e) => {
            // If it's the common case (no staged changes), offer staging.
            ui::print_error(&e.to_string());

            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum Offer {
                StagePatch,
                StageAll,
                Back,
            }

            let offer = select("No staged changes. What would you like to do?")
                .item(Offer::StagePatch, "Stage interactively", "Runs: git add -p")
                .item(Offer::StageAll, "Stage all", "Runs: git add -A")
                .item(Offer::Back, "Back", "Return to main menu")
                .interact()?;

            match offer {
                Offer::StagePatch => {
                    let _ = git::stage_patch();
                }
                Offer::StageAll => {
                    let _ = git::stage_all();
                }
                Offer::Back => return Ok(String::new()),
            }

            // Retry
            Ok(git::get_diff(git::DiffSource::Staged)?)
        }
    }
}

fn get_unstaged_diff_or_offer_stage() -> Result<String> {
    match git::get_diff(git::DiffSource::Unstaged) {
        Ok(d) => Ok(d),
        Err(e) => {
            ui::print_error(&e.to_string());
            Ok(String::new())
        }
    }
}

fn get_both_diff_or_offer_stage() -> Result<String> {
    match git::get_diff(git::DiffSource::Both) {
        Ok(d) => Ok(d),
        Err(e) => {
            ui::print_error(&e.to_string());
            Ok(String::new())
        }
    }
}
