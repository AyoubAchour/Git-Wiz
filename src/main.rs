mod config;
mod generator;
mod git;
mod setup;
mod ui;

use anyhow::Result;
use clap::Parser;
use cliclack::{input, select};
use config::{Config, Provider};
use generator::{AnthropicGenerator, GeminiGenerator, Generator, MockGenerator, OpenAIGenerator};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Optional hint to guide the commit message generation
    #[arg(long)]
    hint: Option<String>,

    /// Force use of the mock generator
    #[arg(long, default_value_t = false)]
    mock: bool,

    /// Re-run the setup wizard
    #[arg(long, default_value_t = false)]
    config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Ensure terminal colors are enabled on Windows
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    let args = Args::parse();

    // 1. Display Banner
    ui::print_banner();

    // 2. Re-configuration request
    if args.config {
        setup::run_setup()?;
        return Ok(());
    }

    // 3. Get the diff
    let diff = match git::get_diff() {
        Ok(d) => {
            ui::print_success(&format!("Found staged changes ({} chars)", d.len()));
            d
        }
        Err(e) => {
            ui::print_error(&format!("{}", e));
            ui::print_outro("Exiting.");
            std::process::exit(1);
        }
    };

    // 4. Setup Generator
    let generator = if args.mock {
        Generator::Mock(MockGenerator::new())
    } else {
        match Config::load()? {
            Some(cfg) => match cfg.provider {
                Provider::OpenAI => Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model)),
                Provider::Anthropic => {
                    Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
                }
                Provider::Gemini => Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model)),
            },
            None => {
                // First run flow
                let cfg = setup::run_setup()?;
                match cfg.provider {
                    Provider::OpenAI => {
                        Generator::OpenAI(OpenAIGenerator::new(cfg.api_key, cfg.model))
                    }
                    Provider::Anthropic => {
                        Generator::Anthropic(AnthropicGenerator::new(cfg.api_key, cfg.model))
                    }
                    Provider::Gemini => {
                        Generator::Gemini(GeminiGenerator::new(cfg.api_key, cfg.model))
                    }
                }
            }
        }
    };

    let current_hint = args.hint;
    let mut current_message = String::new();
    let mut needs_generation = true;

    loop {
        if needs_generation {
            let result: anyhow::Result<String> =
                ui::with_spinner("Thinking...", "Analysis complete", || async {
                    generator.generate(&diff, current_hint.clone()).await
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

        // 5. Display Result
        if !current_message.is_empty() {
            ui::print_commit_preview(&current_message);
        }

        // 6. Interactive Menu
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Action {
            Commit,
            Edit,
            Regenerate,
            Cancel,
        }

        let action = select("What would you like to do?")
            .item(Action::Commit, "Commit", "Run git commit")
            .item(Action::Edit, "Edit", "Refine the message")
            .item(Action::Regenerate, "Regenerate", "Try again")
            .item(Action::Cancel, "Cancel", "Exit")
            .interact()?;

        match action {
            Action::Commit => {
                let result: anyhow::Result<()> = ui::with_spinner(
                    "Committing...",
                    "Changes committed successfully!",
                    || async { git::commit_changes(&current_message) },
                )
                .await;

                match result {
                    Ok(_) => {
                        ui::print_outro("Done.");
                        break;
                    }
                    Err(e) => {
                        ui::print_error(&format!("Commit failed: {}", e));
                        // Loop back to allow edit or cancel
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
            Action::Cancel => {
                ui::print_outro("Operation cancelled.");
                break;
            }
        }
    }

    Ok(())
}
