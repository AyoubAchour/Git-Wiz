use cliclack::{intro, log, outro, spinner};
use colored::*;
use std::future::Future;

pub fn print_banner() {
    // Clear the screen for a fresh start
    print!("{}[2J", 27 as char);
    // intro prints the title in a nice styled badge
    intro(" GIT WIZ ").ok();
    log::remark("The Rational AI Pair Programmer's CLI").ok();
}

pub fn print_success(message: &str) {
    log::success(message).ok();
}

pub fn print_error(message: &str) {
    log::error(message).ok();
}

pub fn print_info(message: &str) {
    log::info(message).ok();
}

#[allow(dead_code)]
pub fn print_warn(message: &str) {
    log::warning(message).ok();
}

pub fn print_commit_preview(message: &str) {
    println!();
    println!("  {}", "┌  Generated Commit Message".dimmed());
    for line in message.lines() {
        println!("  {}  {}", "│".dimmed(), line.cyan());
    }
    println!("  {}", "└──────────────────────────".dimmed());
    println!();
}

pub fn print_outro(msg: &str) {
    outro(msg).ok();
}

pub async fn with_spinner<F, Fut, T, E>(start_msg: &str, success_msg: &str, f: F) -> Result<T, E>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let s = spinner();
    s.start(start_msg);
    let result = f().await;
    match &result {
        Ok(_) => s.stop(success_msg),
        Err(_) => s.stop("Failed"),
    }
    result
}
