use anyhow::{Context, Result, bail};
use std::process::Command;

pub fn is_repo() -> bool {
    Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn get_diff() -> Result<String> {
    if !is_repo() {
        bail!("Not a git repository (or git is not installed).");
    }

    let output = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let diff = String::from_utf8(output.stdout).context("git diff output was not valid UTF-8")?;

    if diff.trim().is_empty() {
        bail!("No staged changes found. Did you forget to 'git add'?");
    }

    Ok(diff)
}

pub fn commit_changes(message: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg(message)
        .output()
        .context("Failed to execute git commit")?;

    if !output.status.success() {
        bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
