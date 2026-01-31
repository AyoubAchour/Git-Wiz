use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};

/// Release orchestration helpers for a tag-based CI pipeline.
///
/// This module is intentionally UI-agnostic (usable from TUI/CLI).
///
/// Design goals:
/// - Safe defaults: refuse to proceed on a dirty working tree, tag collisions, or missing `origin`.
/// - Deterministic: run preflight checks before bumping versions.
/// - Compatible with your GitHub Actions workflow that triggers on `push` tags matching `v*`.
///
/// NOTE: This module does not talk to GitHub APIs; it only performs local git/cargo operations
/// and can optionally push the release tag to `origin` to trigger CI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpKind {
    Patch,
    Minor,
    Major,
}

impl BumpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BumpKind::Patch => "patch",
            BumpKind::Minor => "minor",
            BumpKind::Major => "major",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightConfig {
    /// Run `cargo fmt --check`
    pub fmt_check: bool,
    /// Run `cargo clippy -- -D warnings`
    pub clippy_deny_warnings: bool,
    /// Run `cargo test --locked`
    pub test_locked: bool,
}

impl Default for PreflightConfig {
    fn default() -> Self {
        Self {
            fmt_check: true,
            clippy_deny_warnings: true,
            test_locked: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub old_version: String,
    pub new_version: String,
    pub tag: String, // "vX.Y.Z"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseGuardrailConfig {
    pub remote: String,                  // usually "origin"
    pub expected_branch: Option<String>, // e.g. Some("master".into())
}

impl Default for ReleaseGuardrailConfig {
    fn default() -> Self {
        Self {
            remote: "origin".to_string(),
            expected_branch: Some("master".to_string()),
        }
    }
}

/// Compute a release plan by reading `Cargo.toml` and applying a semver bump.
pub fn plan_bump(cargo_toml_path: impl AsRef<Path>, bump: BumpKind) -> Result<ReleasePlan> {
    let old_version = read_cargo_package_version(cargo_toml_path.as_ref())?;
    let new_version = bump_semver(&old_version, bump)?;
    Ok(ReleasePlan {
        old_version,
        tag: format!("v{}", new_version),
        new_version,
    })
}

/// Compute a release plan using a custom version string.
/// Validates that it looks like `x.y.z` and differs from current.
pub fn plan_custom(cargo_toml_path: impl AsRef<Path>, new_version: &str) -> Result<ReleasePlan> {
    let new_version = new_version.trim();
    if new_version.is_empty() {
        bail!("New version cannot be empty.");
    }
    let old_version = read_cargo_package_version(cargo_toml_path.as_ref())?;
    validate_semver_3(new_version).context("Invalid custom version")?;
    if old_version == new_version {
        bail!("New version matches current version: {}", new_version);
    }
    Ok(ReleasePlan {
        old_version,
        new_version: new_version.to_string(),
        tag: format!("v{}", new_version),
    })
}

/// Run preflight checks before modifying repository state.
pub fn run_preflight(cfg: &PreflightConfig) -> Result<()> {
    if cfg.fmt_check {
        run_cmd_inherit("cargo", &["fmt", "--check"])
            .context("Release preflight failed: cargo fmt --check")?;
    }
    if cfg.clippy_deny_warnings {
        run_cmd_inherit("cargo", &["clippy", "--", "-D", "warnings"])
            .context("Release preflight failed: cargo clippy -- -D warnings")?;
    }
    if cfg.test_locked {
        run_cmd_inherit("cargo", &["test", "--locked"])
            .context("Release preflight failed: cargo test --locked")?;
    }
    Ok(())
}

/// Guardrails: ensure repo is in a safe state for release.
pub fn assert_release_guardrails(cfg: &ReleaseGuardrailConfig) -> Result<()> {
    ensure_git_repo()?;
    ensure_remote_exists(&cfg.remote)?;
    ensure_clean_working_tree()?;

    if let Some(expected) = &cfg.expected_branch {
        let branch = current_branch()?;
        if branch != *expected {
            bail!(
                "Refusing to release: current branch is '{}' (expected '{}').",
                branch,
                expected
            );
        }
    }

    Ok(())
}

/// Apply the version bump to `Cargo.toml` and refresh lockfile (best-effort).
///
/// This only updates files; it does not commit, tag, or push.
pub fn apply_version_bump(
    cargo_toml_path: impl AsRef<Path>,
    old_version: &str,
    new_version: &str,
) -> Result<()> {
    update_cargo_version_in_toml(cargo_toml_path.as_ref(), old_version, new_version)?;
    // Avoid `cargo update` during releases; just ensure lockfile exists.
    let _ = run_cmd_inherit("cargo", &["generate-lockfile"]);
    Ok(())
}

/// Stage all changes (intended for staging the release bump).
pub fn stage_all() -> Result<()> {
    ensure_git_repo()?;
    let out = run_git_output(&["add", "-A"])?;
    if !out.status.success() {
        bail!(
            "git add -A failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Commit with a multi-line message reliably using `git commit -F <tempfile>`.
pub fn commit_with_message(message: &str) -> Result<()> {
    ensure_git_repo()?;
    crate::git::commit_changes(message)
}

/// Create an annotated tag `tag` with message `tag_message`.
pub fn create_annotated_tag(tag: &str, tag_message: &str) -> Result<()> {
    ensure_git_repo()?;
    let tag = tag.trim();
    if tag.is_empty() {
        bail!("Tag cannot be empty.");
    }

    let out = run_git_output(&["tag", "-a", tag, "-m", tag_message])?;
    if !out.status.success() {
        bail!(
            "git tag -a {} failed: {}",
            tag,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Push a single tag to the remote (e.g., `origin vX.Y.Z`) to trigger CI.
pub fn push_tag(remote: &str, tag: &str) -> Result<()> {
    ensure_git_repo()?;
    let remote = remote.trim();
    let tag = tag.trim();
    if remote.is_empty() {
        bail!("Remote cannot be empty.");
    }
    if tag.is_empty() {
        bail!("Tag cannot be empty.");
    }

    let out = run_git_output(&["push", remote, tag])?;
    if !out.status.success() {
        bail!(
            "git push {} {} failed: {}",
            remote,
            tag,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Check whether a local tag exists.
pub fn tag_exists_local(tag: &str) -> Result<bool> {
    ensure_git_repo()?;
    let tag = tag.trim();
    if tag.is_empty() {
        return Ok(false);
    }

    let out = run_git_output(&["tag", "--list", tag])?;
    if !out.status.success() {
        bail!(
            "git tag --list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Check whether a remote tag exists (uses `git ls-remote --tags`).
pub fn tag_exists_remote(remote: &str, tag: &str) -> Result<bool> {
    ensure_git_repo()?;
    let remote = remote.trim();
    let tag = tag.trim();
    if remote.is_empty() || tag.is_empty() {
        return Ok(false);
    }

    let refs = format!("refs/tags/{}", tag);
    let out = run_git_output(&["ls-remote", "--tags", remote, &refs])?;
    if !out.status.success() {
        bail!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Run the complete tag-based release pipeline locally (safe defaults).
///
/// Steps:
/// 1) guardrails (repo, remote, clean tree, expected branch)
/// 2) preflight checks
/// 3) update Cargo.toml + generate lockfile
/// 4) stage + commit
/// 5) collision checks
/// 6) create annotated tag + push tag
///
/// This is intended to trigger GitHub Actions which builds releases and publishes to crates.io.
///
/// `commit_message` should be a full multi-line commit message.
pub fn run_tag_release(
    cargo_toml_path: impl AsRef<Path>,
    plan: &ReleasePlan,
    commit_message: &str,
    preflight: &PreflightConfig,
    guards: &ReleaseGuardrailConfig,
) -> Result<()> {
    assert_release_guardrails(guards)?;
    run_preflight(preflight)?;

    // Apply bump + stage + commit
    apply_version_bump(
        cargo_toml_path.as_ref(),
        &plan.old_version,
        &plan.new_version,
    )?;
    stage_all()?;
    commit_with_message(commit_message)?;

    // Tag collision checks
    if tag_exists_local(&plan.tag)? {
        bail!("Tag already exists locally: {}", plan.tag);
    }
    if tag_exists_remote(&guards.remote, &plan.tag)? {
        bail!(
            "Tag already exists on remote {}: {}",
            guards.remote,
            plan.tag
        );
    }

    create_annotated_tag(&plan.tag, &format!("Release {}", plan.tag))?;
    push_tag(&guards.remote, &plan.tag)?;

    Ok(())
}

/* ----------------------------- helpers ----------------------------- */

fn ensure_git_repo() -> Result<()> {
    if crate::git::is_repo() {
        Ok(())
    } else {
        bail!("Not a git repository (or git is not installed).");
    }
}

fn ensure_remote_exists(remote: &str) -> Result<()> {
    let remote = remote.trim();
    if remote.is_empty() {
        bail!("Remote cannot be empty.");
    }

    let out = run_git_output(&["remote", "get-url", remote])?;
    if out.status.success() {
        Ok(())
    } else {
        bail!("No '{}' remote found. Add it first.", remote);
    }
}

fn ensure_clean_working_tree() -> Result<()> {
    let out = run_git_output(&["status", "--porcelain"])?;
    if !out.status.success() {
        bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    if out.stdout.is_empty() {
        Ok(())
    } else {
        bail!("Working tree is not clean. Commit or stash your changes before releasing.");
    }
}

fn current_branch() -> Result<String> {
    let out = run_git_output(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if !out.status.success() {
        bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_git_output(args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))
}

fn run_cmd_inherit(cmd: &str, args: &[&str]) -> Result<ExitStatus> {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run {} {}", cmd, args.join(" ")))
        .and_then(|s| {
            if s.success() {
                Ok(s)
            } else {
                bail!("Command failed: {} {}", cmd, args.join(" "));
            }
        })
}

fn read_cargo_package_version(path: &Path) -> Result<String> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    // naive but reliable for common Cargo.toml layouts:
    // [package]
    // version = "x.y.z"
    //
    // NOTE: This does not parse TOML properly; it's a deliberate minimal dependency.
    // If you later want correctness for workspaces and non-standard formatting,
    // replace with `toml_edit`.
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }

        if in_package
            && trimmed.starts_with("version")
            && trimmed.contains('=')
            && trimmed.contains('"')
        {
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    return Ok(trimmed[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }

    bail!("Failed to locate [package] version in {}", path.display())
}

fn update_cargo_version_in_toml(path: &Path, old: &str, new: &str) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut out = String::new();
    let mut in_package = false;
    let mut replaced = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if in_package
            && !replaced
            && trimmed.starts_with("version")
            && line.contains(&format!("\"{}\"", old))
        {
            out.push_str(&line.replace(&format!("\"{}\"", old), &format!("\"{}\"", new)));
            out.push('\n');
            replaced = true;
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    if !replaced {
        bail!(
            "Failed to update version in {} (did not find version = \"{}\" under [package])",
            path.display(),
            old
        );
    }

    fs::write(path, out).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn validate_semver_3(v: &str) -> Result<()> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        bail!("expected x.y.z, got {}", v);
    }
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() {
            bail!("version part {} is empty in {}", i, v);
        }
        // Allow leading zeros (Cargo allows), but require numeric.
        if p.parse::<u64>().is_err() {
            bail!("version part '{}' is not numeric in {}", p, v);
        }
    }
    Ok(())
}

fn bump_semver(current: &str, bump: BumpKind) -> Result<String> {
    validate_semver_3(current)?;
    let parts: Vec<&str> = current.split('.').collect();
    let mut major: u64 = parts[0].parse().unwrap_or(0);
    let mut minor: u64 = parts[1].parse().unwrap_or(0);
    let mut patch: u64 = parts[2].parse().unwrap_or(0);

    match bump {
        BumpKind::Patch => patch += 1,
        BumpKind::Minor => {
            minor += 1;
            patch = 0;
        }
        BumpKind::Major => {
            major += 1;
            minor = 0;
            patch = 0;
        }
    }

    Ok(format!("{}.{}.{}", major, minor, patch))
}
