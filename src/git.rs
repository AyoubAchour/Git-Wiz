use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSource {
    Staged,
    Unstaged,
    Both,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub bytes: usize,
}

pub fn is_repo() -> bool {
    Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))
}

fn run_git_status(args: &[&str]) -> Result<std::process::ExitStatus> {
    Command::new("git")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))
}

fn ensure_repo() -> Result<()> {
    if !is_repo() {
        bail!("Not a git repository (or git is not installed).");
    }
    Ok(())
}

pub fn get_diff(source: DiffSource) -> Result<String> {
    ensure_repo()?;

    match source {
        DiffSource::Staged => get_diff_staged(),
        DiffSource::Unstaged => get_diff_unstaged(),
        DiffSource::Both => {
            let staged = get_diff_staged_allow_empty()?;
            let unstaged = get_diff_unstaged_allow_empty()?;

            if staged.trim().is_empty() && unstaged.trim().is_empty() {
                bail!("No staged or unstaged changes found.");
            }

            let combined = match (staged.trim().is_empty(), unstaged.trim().is_empty()) {
                (false, true) => staged,
                (true, false) => unstaged,
                (false, false) => format!(
                    "--- STAGED ---\n{}\n\n--- UNSTAGED ---\n{}",
                    staged, unstaged
                ),
                (true, true) => unreachable!(),
            };

            Ok(combined)
        }
    }
}

pub fn get_diff_staged() -> Result<String> {
    ensure_repo()?;
    let diff = get_diff_staged_allow_empty()?;

    if diff.trim().is_empty() {
        bail!("No staged changes found. Did you forget to 'git add'?");
    }

    Ok(diff)
}

pub fn get_diff_unstaged() -> Result<String> {
    ensure_repo()?;
    let diff = get_diff_unstaged_allow_empty()?;

    if diff.trim().is_empty() {
        bail!("No unstaged changes found.");
    }

    Ok(diff)
}

pub fn get_diff_staged_allow_empty() -> Result<String> {
    ensure_repo()?;
    let output = run_git(&["diff", "--cached"])?;

    if !output.status.success() {
        bail!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout).context("git diff --cached output was not valid UTF-8")
}

pub fn get_diff_unstaged_allow_empty() -> Result<String> {
    ensure_repo()?;
    let output = run_git(&["diff"])?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout).context("git diff output was not valid UTF-8")
}

pub fn get_diff_allow_empty(source: DiffSource) -> Result<String> {
    ensure_repo()?;

    match source {
        DiffSource::Staged => get_diff_staged_allow_empty(),
        DiffSource::Unstaged => get_diff_unstaged_allow_empty(),
        DiffSource::Both => {
            let staged = get_diff_staged_allow_empty()?;
            let unstaged = get_diff_unstaged_allow_empty()?;

            if staged.trim().is_empty() && unstaged.trim().is_empty() {
                return Ok(String::new());
            }

            let combined = match (staged.trim().is_empty(), unstaged.trim().is_empty()) {
                (false, true) => staged,
                (true, false) => unstaged,
                (false, false) => format!(
                    "--- STAGED ---\n{}\n\n--- UNSTAGED ---\n{}",
                    staged, unstaged
                ),
                (true, true) => unreachable!(),
            };

            Ok(combined)
        }
    }
}

pub fn stage_patch() -> Result<()> {
    ensure_repo()?;
    let status = run_git_status(&["add", "-p"])?;
    if !status.success() {
        bail!("git add -p failed.");
    }
    Ok(())
}

pub fn stage_all() -> Result<()> {
    ensure_repo()?;
    let output = run_git(&["add", "-A"])?;
    if !output.status.success() {
        bail!(
            "git add -A failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn unstage_patch() -> Result<()> {
    ensure_repo()?;

    // Prefer `git restore --staged -p` (newer), fallback to `git reset -p`.
    let status = Command::new("git")
        .args(["restore", "--staged", "-p", "."])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) | Err(_) => {
            let status = run_git_status(&["reset", "-p"])?;
            if !status.success() {
                bail!("Failed to unstage interactively (git restore --staged -p / git reset -p).");
            }
            Ok(())
        }
    }
}

pub fn unstage_all() -> Result<()> {
    ensure_repo()?;

    // Prefer `git restore --staged .`, fallback to `git reset`.
    let output = Command::new("git")
        .args(["restore", "--staged", "."])
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(_) | Err(_) => {
            let o = run_git(&["reset"])?;
            if !o.status.success() {
                bail!(
                    "Failed to unstage all changes: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Ok(())
        }
    }
}

pub fn diff_summary(source: DiffSource) -> Result<DiffSummary> {
    ensure_repo()?;

    let bytes = match source {
        DiffSource::Staged => get_diff_staged_allow_empty()?.len(),
        DiffSource::Unstaged => get_diff_unstaged_allow_empty()?.len(),
        DiffSource::Both => {
            let a = get_diff_staged_allow_empty()?.len();
            let b = get_diff_unstaged_allow_empty()?.len();
            a + b
        }
    };

    // Use numstat for insertions/deletions and file count.
    // For Both, combine cached + working-tree.
    let mut summary = DiffSummary {
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        bytes,
    };

    let mut accumulate_numstat = |args: &[&str]| -> Result<()> {
        let o = run_git(args)?;
        if !o.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&o.stderr)
            );
        }
        let text = String::from_utf8(o.stdout).context("git numstat output was not valid UTF-8")?;
        for line in text.lines() {
            // Format: <insertions>\t<deletions>\t<path>
            // Binary files can show '-' for counts.
            let mut parts = line.split('\t');
            let ins = parts.next().unwrap_or("").trim();
            let del = parts.next().unwrap_or("").trim();
            let path = parts.next().unwrap_or("").trim();

            if path.is_empty() {
                continue;
            }
            summary.files_changed += 1;

            if let Ok(n) = ins.parse::<usize>() {
                summary.insertions += n;
            }
            if let Ok(n) = del.parse::<usize>() {
                summary.deletions += n;
            }
        }
        Ok(())
    };

    match source {
        DiffSource::Staged => accumulate_numstat(&["diff", "--cached", "--numstat"])?,
        DiffSource::Unstaged => accumulate_numstat(&["diff", "--numstat"])?,
        DiffSource::Both => {
            accumulate_numstat(&["diff", "--cached", "--numstat"])?;
            accumulate_numstat(&["diff", "--numstat"])?;
        }
    }

    Ok(summary)
}

pub fn commit_changes(message: &str) -> Result<()> {
    ensure_repo()?;

    // Use a temp file + `git commit -F` to reliably preserve multi-line messages.
    let mut path: PathBuf = std::env::temp_dir();
    let unique = format!(
        "git-wiz-commit-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    path.push(unique);

    fs::write(&path, message).with_context(|| {
        format!(
            "Failed to write temp commit message file: {}",
            path.display()
        )
    })?;

    let output = Command::new("git")
        .arg("commit")
        .arg("-F")
        .arg(&path)
        .output()
        .context("Failed to execute git commit")?;

    // Best-effort cleanup (ignore errors)
    let _ = fs::remove_file(&path);

    if !output.status.success() {
        bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
