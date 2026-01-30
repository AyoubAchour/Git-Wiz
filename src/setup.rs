use crate::config::{Config, Provider};
use anyhow::Result;
use cliclack::{input, log, note, password, select};
use colored::*;

pub fn run_setup() -> Result<Config> {
    log::info("Welcome! It looks like this is your first time running the tool.")?;
    log::info("Let's get you set up with a few simple questions.\n")?;

    // 1. Select Provider
    let provider = select("Select your AI Provider")
        .item(
            Provider::Gemini,
            "Google Gemini",
            "Gemini 2.5 / 3 (Pro & Flash)",
        )
        .item(
            Provider::Anthropic,
            "Anthropic",
            "Claude 4.5 (Sonnet / Opus)",
        )
        .item(Provider::OpenAI, "OpenAI", "GPT-5.2")
        .interact()?;

    // 2. Input API Key
    let api_key = password(format!("Enter your {} API Key", provider))
        .mask('•')
        .interact()?;

    // 3. Select Model
    let model = match provider {
        Provider::Gemini => select_model_gemini()?,
        Provider::Anthropic => select_model_anthropic()?,
        Provider::OpenAI => select_model_openai()?,
    };

    let config = Config {
        provider,
        api_key,
        model,
    };

    // 4. Save
    config.save()?;

    log::success("Setup Complete! You are ready to go.")?;

    note(
        "Quick Start Tutorial",
        format!(
            "1. Stage your changes:   {}\n2. Run the wizard:       {}\n3. Review & Commit:      {}",
            "git add <files>".cyan(),
            "git-wiz".cyan(),
            "Follow the interactive menu".cyan()
        ),
    )?;

    Ok(config)
}

fn select_model_gemini() -> Result<String> {
    let selection = select("Select Gemini Model")
        .item(
            "gemini-3-pro-preview",
            "Gemini 3 Pro (Preview)",
            "Most powerful, multimodal",
        )
        .item(
            "gemini-3-flash-preview",
            "Gemini 3 Flash (Preview)",
            "Balanced, fast",
        )
        .item(
            "gemini-2.5-pro",
            "Gemini 2.5 Pro",
            "Stable, advanced reasoning",
        )
        .item(
            "gemini-2.5-flash",
            "Gemini 2.5 Flash",
            "Production workhorse",
        )
        .item("custom", "Other...", "Enter a custom model name")
        .interact()?;

    if selection == "custom" {
        Ok(input("Enter custom model name")
            .placeholder("e.g. gemini-2.5-flash")
            .interact()?)
    } else {
        Ok(selection.to_string())
    }
}

fn select_model_anthropic() -> Result<String> {
    let selection = select("Select Claude Model")
        .item(
            "claude-sonnet-4-5",
            "Claude 4.5 Sonnet",
            "Recommended default",
        )
        .item("claude-opus-4-5", "Claude 4.5 Opus", "Maximum intelligence")
        .item("custom", "Other...", "Enter a custom model name")
        .interact()?;

    if selection == "custom" {
        Ok(input("Enter custom model name")
            .placeholder("e.g. claude-sonnet-4-5")
            .interact()?)
    } else {
        Ok(selection.to_string())
    }
}

fn select_model_openai() -> Result<String> {
    let selection = select("Select OpenAI Model")
        .item("gpt-5.2", "GPT-5.2", "Recommended default")
        .item("custom", "Other...", "Enter a custom model name")
        .interact()?;

    if selection == "custom" {
        Ok(input("Enter custom model name")
            .placeholder("e.g. gpt-5.2")
            .interact()?)
    } else {
        Ok(selection.to_string())
    }
}
