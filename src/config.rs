use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Gemini,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::OpenAI => write!(f, "OpenAI"),
            Provider::Anthropic => write!(f, "Anthropic"),
            Provider::Gemini => write!(f, "Google Gemini"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub provider: Provider,
    pub api_key: String,
    pub model: String,
}

impl Config {
    pub fn get_path() -> Result<PathBuf> {
        let mut path = dirs::config_dir().context("Could not determine config directory")?;
        path.push("git-wiz");
        // Ensure directory exists
        if !path.exists() {
            fs::create_dir_all(&path).context("Failed to create config directory")?;
        }
        path.push("config.json");
        Ok(path)
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::get_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).context("Failed to read config file")?;
        let config: Config =
            serde_json::from_str(&content).context("Failed to parse config file")?;

        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::get_path()?;
        let content = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, content).context("Failed to write config file")?;
        Ok(())
    }
}
