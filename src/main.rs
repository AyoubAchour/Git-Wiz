mod config;
mod generator;
mod git;
mod setup;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use cliclack::{input, log, select};
use config::{Config, Provider};
use generator::{AnthropicGenerator, GeminiGenerator, Generator, MockGenerator, OpenAIGenerator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseFailureAction {
    RunCargoFmt,
    RevertReleaseChanges,
    Back,
}

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
    Push,
    Release,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushAction {
    PushBranch,
    PushSpecificTag,
    PushAllTags,
    PushBranchAndTags,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseBump {
    Patch,
    Minor,
    Major,
    Custom,
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
                "Stage / Unstage changes",
                "Stage interactively (git add -p), stage all, unstage changes",
            )
            .item(
                MainAction::View,
                "View diff / summary",
                "Preview staged/unstaged diff and stats before generating",
            )
            .item(
                MainAction::Push,
                "Push",
                "Push current branch and/or tags (tags trigger CI release)",
            )
            .item(
                MainAction::Release,
                "Release (tag-based CI)",
                "Bump version, run checks, commit, tag vX.Y.Z, and push tag to trigger CI publish",
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
            MainAction::Push => {
                if let Err(e) = run_push_flow() {
                    ui::print_error(&e.to_string());
                }
            }
            MainAction::Release => {
                if let Err(e) = run_release_flow(&generator).await {
                    ui::print_error(&e.to_string());
                    if let Err(e) = handle_release_failure_recovery(&e.to_string()) {
                        ui::print_error(&e.to_string());
                    }
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

fn run_push_flow() -> Result<()> {
    if !git::is_repo() {
        ui::print_error("Not a git repository (or git is not installed).");
        return Ok(());
    }

    loop {
        let action = select("Push")
            .item(
                PushAction::PushBranch,
                "Push current branch",
                "Push commits to the remote (sets upstream if needed)",
            )
            .item(
                PushAction::PushSpecificTag,
                "Push a specific tag",
                "Safer than pushing all tags (recommended for releases)",
            )
            .item(
                PushAction::PushAllTags,
                "Push all tags",
                "Runs: git push --tags (may trigger releases if v* tags exist)",
            )
            .item(
                PushAction::PushBranchAndTags,
                "Push branch + all tags",
                "Push commits and then push --tags",
            )
            .item(PushAction::Back, "Back", "Return to main menu")
            .interact()?;

        match action {
            PushAction::PushBranch => {
                push_current_branch_with_upstream()?;
                ui::print_success("Branch pushed.");
            }
            PushAction::PushSpecificTag => {
                let tag: String = input("Tag to push").placeholder("e.g. v0.1.3").interact()?;
                push_tag(tag.trim())?;
                ui::print_success("Tag pushed.");
            }
            PushAction::PushAllTags => {
                push_tags()?;
                ui::print_success("All tags pushed.");
            }
            PushAction::PushBranchAndTags => {
                push_current_branch_with_upstream()?;
                push_tags()?;
                ui::print_success("Branch and tags pushed.");
            }
            PushAction::Back => return Ok(()),
        }
    }
}

async fn run_release_flow(generator: &Generator) -> Result<()> {
    if !git::is_repo() {
        ui::print_error("Not a git repository (or git is not installed).");
        return Ok(());
    }

    // Guard: require clean working tree (release should be deterministic)
    if !is_working_tree_clean()? {
        ui::print_error(
            "Working tree is not clean. Commit or stash your changes before releasing.",
        );
        return Ok(());
    }

    // Guard: require origin remote (we push tags to origin to trigger CI)
    if remote_url("origin")?.is_none() {
        ui::print_error("No 'origin' remote found. Add it first (git remote add origin <url>).");
        return Ok(());
    }

    // Guard: ensure we're on the expected branch (your repo default is 'master')
    let branch = current_branch()?;
    if branch != "master" {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum BranchGuard {
            Continue,
            Back,
        }

        let choice = select(format!(
            "You are on branch '{}', not 'master'. Continue anyway?",
            branch
        ))
        .item(
            BranchGuard::Continue,
            "Continue",
            "Proceed with release on this branch",
        )
        .item(BranchGuard::Back, "Back", "Return to main menu")
        .interact()?;

        if choice == BranchGuard::Back {
            return Ok(());
        }
    }

    // Preflight checks BEFORE bumping version, so failures don't dirty the working tree.
    ui::print_info("Running preflight checks (before version bump)...");
    run_cmd("cargo", &["fmt", "--check"]).context("Release preflight failed: cargo fmt --check")?;
    run_cmd("cargo", &["clippy", "--", "-D", "warnings"])
        .context("Release preflight failed: cargo clippy")?;
    run_cmd("cargo", &["test", "--locked"])
        .context("Release preflight failed: cargo test --locked")?;

    let bump = select("Release: how do you want to bump the version?")
        .item(ReleaseBump::Patch, "Patch", "x.y.(z+1)")
        .item(ReleaseBump::Minor, "Minor", "x.(y+1).0")
        .item(ReleaseBump::Major, "Major", "(x+1).0.0")
        .item(ReleaseBump::Custom, "Custom", "Enter a version manually")
        .item(ReleaseBump::Back, "Back", "Return to main menu")
        .interact()?;

    let (old_version, new_version) = match bump {
        ReleaseBump::Back => return Ok(()),
        ReleaseBump::Custom => {
            let current = read_cargo_version("Cargo.toml")?;
            let input_version = input("Enter new version")
                .default_input(&current)
                .interact()?;
            (current, input_version)
        }
        other => {
            let current = read_cargo_version("Cargo.toml")?;
            let next = bump_semver(&current, other)?;
            (current, next)
        }
    };

    if old_version == new_version {
        ui::print_error("New version matches current version. Nothing to do.");
        return Ok(());
    }

    // Confirm bump
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Confirm {
        Proceed,
        Cancel,
    }
    let confirm = select(format!("Bump version {} -> {} ?", old_version, new_version))
        .item(
            Confirm::Proceed,
            "Proceed",
            "Update files, generate lockfile, commit, tag, push tag",
        )
        .item(Confirm::Cancel, "Cancel", "Return to main menu")
        .interact()?;
    if confirm == Confirm::Cancel {
        return Ok(());
    }

    // 1) Update Cargo.toml
    update_cargo_version_in_toml("Cargo.toml", &old_version, &new_version)?;

    // 2) Update Cargo.lock (if present) to keep things consistent.
    // We avoid `cargo update` during release automation because it can introduce unrelated dependency changes.
    // Instead, we refresh the lockfile if needed.
    let _ = run_cmd("cargo", &["generate-lockfile"]).ok();

    // 3) Stage version bump files (Cargo.toml + Cargo.lock if changed)
    git::stage_all()?;

    // 4) Generate commit message for release bump (staged diff)
    //    We keep it deterministic: staged-only diff + hint.
    let hint = Some(format!("release: bump version to v{}", new_version));
    let diff = git::get_diff(git::DiffSource::Staged)?;
    let message: String =
        ui::with_spinner("Generating release commit message...", "Generated", || {
            generator.generate(&diff, hint.clone())
        })
        .await
        .unwrap_or_else(|_| format!("chore(release): v{}", new_version));

    // 5) Commit
    git::commit_changes(&message)?;

    // 6) Final confirmation before we create/push the tag.
    // This is the irreversible step that triggers GitHub Actions release + crates.io publish.
    let tag = format!("v{}", new_version);
    let origin = remote_url("origin")?.unwrap_or_else(|| "<missing>".to_string());
    let branch = current_branch()?;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FinalConfirm {
        TriggerRelease,
        Back,
    }

    let confirm = select("Final confirmation: trigger CI release & crates publish?")
        .item(
            FinalConfirm::TriggerRelease,
            "Yes — create & push tag",
            format!(
                "Will tag '{}' on branch '{}' and push to origin ({})",
                tag, branch, origin
            ),
        )
        .item(
            FinalConfirm::Back,
            "No — go back",
            "Return to main menu without tagging",
        )
        .interact()?;

    if confirm == FinalConfirm::Back {
        return Ok(());
    }

    // Safety: avoid collisions (local or remote).
    // Local tag collision:
    if tag_exists_local(&tag)? {
        anyhow::bail!("Tag already exists locally: {}", tag);
    }
    // Remote tag collision:
    if tag_exists_remote("origin", &tag)? {
        anyhow::bail!("Tag already exists on remote origin: {}", tag);
    }

    create_annotated_tag(&tag, &format!("Release {}", tag))?;
    push_tag(&tag)?;

    // Print a helpful URL to the CI runs page (no guessing run id).
    if let Some(repo_https) = origin_https_repo_url()? {
        ui::print_info(&format!(
            "Track progress in GitHub Actions: {}/actions?query=workflow%3ARelease",
            repo_https
        ));
        ui::print_info(&format!(
            "Release page (once published): {}/releases/tag/{}",
            repo_https, tag
        ));
    } else {
        ui::print_info(
            "Track progress in GitHub Actions: (could not derive repo URL from origin remote).",
        );
    }

    ui::print_success(&format!(
        "Release initiated: pushed tag {} (GitHub Actions will build + release + publish).",
        tag
    ));

    Ok(())
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run {} {}", cmd, args.join(" ")))?;

    if !status.success() {
        anyhow::bail!("Command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}

fn is_working_tree_clean() -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to run git status")?;
    if !output.status.success() {
        anyhow::bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout.is_empty())
}

fn remote_url(remote: &str) -> Result<Option<String>> {
    let o = std::process::Command::new("git")
        .args(["remote", "get-url", remote])
        .output()
        .with_context(|| format!("Failed to get remote URL for '{}'", remote))?;

    if o.status.success() {
        Ok(Some(String::from_utf8_lossy(&o.stdout).trim().to_string()))
    } else {
        // If remote doesn't exist, git returns non-zero. Treat as None.
        Ok(None)
    }
}

fn origin_https_repo_url() -> Result<Option<String>> {
    let url = match remote_url("origin")? {
        Some(u) => u,
        None => return Ok(None),
    };

    // Handle common forms:
    // - https://github.com/OWNER/REPO.git
    // - https://github.com/OWNER/REPO
    // - git@github.com:OWNER/REPO.git
    // We normalize to: https://github.com/OWNER/REPO
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

fn read_cargo_version(path: &str) -> Result<String> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version") && trimmed.contains('=') && trimmed.contains('"') {
            // naive but reliable enough for standard Cargo.toml
            // version = "x.y.z"
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    return Ok(trimmed[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    anyhow::bail!("Failed to locate package version in {}", path)
}

fn update_cargo_version_in_toml(path: &str, old: &str, new: &str) -> Result<()> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    let mut out = String::new();
    let mut replaced = false;

    for line in content.lines() {
        if !replaced
            && line.trim_start().starts_with("version")
            && line.contains(&format!("\"{}\"", old))
        {
            out.push_str(&line.replace(&format!("\"{}\"", old), &format!("\"{}\"", new)));
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !replaced {
        anyhow::bail!(
            "Failed to update version in {} (did not find version = \"{}\")",
            path,
            old
        );
    }

    std::fs::write(path, out).with_context(|| format!("Failed to write {}", path))?;
    Ok(())
}

fn bump_semver(current: &str, bump: ReleaseBump) -> Result<String> {
    let parts: Vec<&str> = current.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!(
            "Current version is not semver (expected x.y.z): {}",
            current
        );
    }
    let mut major: u64 = parts[0].parse().context("Invalid major version")?;
    let mut minor: u64 = parts[1].parse().context("Invalid minor version")?;
    let mut patch: u64 = parts[2].parse().context("Invalid patch version")?;

    match bump {
        ReleaseBump::Patch => patch += 1,
        ReleaseBump::Minor => {
            minor += 1;
            patch = 0;
        }
        ReleaseBump::Major => {
            major += 1;
            minor = 0;
            patch = 0;
        }
        ReleaseBump::Custom | ReleaseBump::Back => {
            anyhow::bail!("Invalid bump kind for bump_semver")
        }
    }

    Ok(format!("{}.{}.{}", major, minor, patch))
}

fn current_branch() -> Result<String> {
    let o = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;
    if !o.status.success() {
        anyhow::bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn has_upstream() -> Result<bool> {
    let o = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .output()
        .context("Failed to check upstream")?;
    Ok(o.status.success())
}

fn push_current_branch_with_upstream() -> Result<()> {
    let branch = current_branch()?;
    if has_upstream()? {
        run_cmd("git", &["push"])?;
        return Ok(());
    }

    // No upstream; set it explicitly
    run_cmd("git", &["push", "-u", "origin", &branch])?;
    Ok(())
}

fn push_tags() -> Result<()> {
    run_cmd("git", &["push", "--tags"])
}

fn push_tag(tag: &str) -> Result<()> {
    let t = tag.trim();
    if t.is_empty() {
        anyhow::bail!("Tag name cannot be empty.");
    }
    run_cmd("git", &["push", "origin", t])
}

fn tag_exists_local(tag: &str) -> Result<bool> {
    let o = std::process::Command::new("git")
        .args(["tag", "--list", tag])
        .output()
        .context("Failed to check local tags")?;

    if !o.status.success() {
        anyhow::bail!(
            "git tag --list failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    Ok(!String::from_utf8_lossy(&o.stdout).trim().is_empty())
}

fn tag_exists_remote(remote: &str, tag: &str) -> Result<bool> {
    // `git ls-remote --tags origin refs/tags/vX.Y.Z`
    let refs = format!("refs/tags/{}", tag);
    let o = std::process::Command::new("git")
        .args(["ls-remote", "--tags", remote, &refs])
        .output()
        .with_context(|| format!("Failed to check remote tags on {}", remote))?;

    if !o.status.success() {
        anyhow::bail!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    Ok(!String::from_utf8_lossy(&o.stdout).trim().is_empty())
}

fn create_annotated_tag(tag: &str, message: &str) -> Result<()> {
    run_cmd("git", &["tag", "-a", tag, "-m", message])
}

fn handle_release_failure_recovery(error_message: &str) -> Result<()> {
    ui::print_info("Release did not complete.");
    ui::print_info("No tag was pushed, so CI release/publish was not triggered.");
    ui::print_info(
        "However, the release flow may have already modified files like Cargo.toml / Cargo.lock.",
    );
    ui::print_info("Choose a recovery action below:");

    // Heuristic: if rustfmt failed, offer auto-fix first.
    let mut menu = select("Release recovery");
    menu = menu.item(
        ReleaseFailureAction::RunCargoFmt,
        "Run cargo fmt",
        "Auto-fix formatting, then you can re-run Release",
    );
    menu = menu.item(
        ReleaseFailureAction::RevertReleaseChanges,
        "Revert release changes",
        "Restore Cargo.toml and Cargo.lock to the last committed state",
    );
    menu = menu.item(ReleaseFailureAction::Back, "Back", "Return to main menu");

    let choice = menu.interact()?;

    match choice {
        ReleaseFailureAction::RunCargoFmt => {
            ui::print_info("Running: cargo fmt");
            run_cmd("cargo", &["fmt"])?;
            ui::print_success("Formatting complete. Re-run Release when ready.");
        }
        ReleaseFailureAction::RevertReleaseChanges => {
            // Only revert the two files we touch in the release bump.
            // Use `git restore` which works on modern git; if it fails, fall back to checkout.
            let restore = std::process::Command::new("git")
                .args(["restore", "Cargo.toml", "Cargo.lock"])
                .status();

            match restore {
                Ok(s) if s.success() => {
                    ui::print_success("Reverted Cargo.toml and Cargo.lock.");
                }
                _ => {
                    run_cmd("git", &["checkout", "--", "Cargo.toml", "Cargo.lock"])?;
                    ui::print_success("Reverted Cargo.toml and Cargo.lock.");
                }
            }
        }
        ReleaseFailureAction::Back => {}
    }

    // Extra guidance if formatting was the likely culprit
    if error_message.contains("fmt") {
        ui::print_info("Tip: If this failed due to formatting, run `cargo fmt` and try again.");
    }

    Ok(())
}
