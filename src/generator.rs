use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

pub struct MockGenerator;

impl MockGenerator {
    pub fn new() -> Self {
        Self
    }

    pub async fn generate(&self, _diff: &str, hint: Option<String>) -> Result<String> {
        // Simulate network latency/thinking time
        sleep(Duration::from_millis(1000)).await;

        let subject = if let Some(h) = hint {
            format!("feat: {}", h)
        } else {
            "feat(core): initialize project structure".to_string()
        };

        Ok(format!(
            "{}\n\n- Added git diff capture\n- Implemented mock AI generator\n- Set up basic CLI flow",
            subject
        ))
    }
}

pub struct OpenAIGenerator {
    client: Client,
    api_key: String,
    model: String,
}

impl OpenAIGenerator {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn generate(&self, diff: &str, hint: Option<String>) -> Result<String> {
        let system_prompt = "You are a senior developer. \
            Write a commit message following the Conventional Commits specification. \
            The format should be:\n\
            <type>(<scope>): <subject>\n\n\
            <body>\n\n\
            <footer>\n\
            Only output the commit message itself, no wrapper text or markdown code blocks.";

        let user_prompt = format!(
            "Here is the git diff:\n\n{}\n\n{}",
            diff,
            if let Some(h) = hint {
                format!("Focus on this context: {}", h)
            } else {
                String::new()
            }
        );

        let request_body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": 0.7
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to OpenAI")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            bail!("OpenAI API error: {}", error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .context("Invalid response format from OpenAI")?
            .trim()
            .to_string();

        Ok(clean_response(content))
    }
}

pub struct AnthropicGenerator {
    client: Client,
    api_key: String,
    model: String,
}

impl AnthropicGenerator {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn generate(&self, diff: &str, hint: Option<String>) -> Result<String> {
        let system_prompt = "You are a senior developer. \
            Write a commit message following the Conventional Commits specification. \
            Only output the commit message itself, no wrapper text or markdown code blocks.";

        let user_prompt = format!(
            "Here is the git diff:\n\n{}\n\n{}",
            diff,
            if let Some(h) = hint {
                format!("Focus on this context: {}", h)
            } else {
                String::new()
            }
        );

        let request_body = json!({
            "model": self.model,
            "max_tokens": 1024,
            "system": system_prompt,
            "messages": [
                {"role": "user", "content": user_prompt}
            ]
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Anthropic")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            bail!("Anthropic API error: {}", error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        let content = response_json["content"][0]["text"]
            .as_str()
            .context("Invalid response format from Anthropic")?
            .trim()
            .to_string();

        Ok(clean_response(content))
    }
}

pub struct GeminiGenerator {
    client: Client,
    api_key: String,
    model: String,
}

impl GeminiGenerator {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn generate(&self, diff: &str, hint: Option<String>) -> Result<String> {
        let system_prompt = "You are a senior developer. \
            Write a commit message following the Conventional Commits specification. \
            Only output the commit message itself, no wrapper text or markdown code blocks.";

        let user_prompt = format!(
            "Here is the git diff:\n\n{}\n\n{}",
            diff,
            if let Some(h) = hint {
                format!("Focus on this context: {}", h)
            } else {
                String::new()
            }
        );

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let request_body = json!({
            "systemInstruction": {
                "parts": [ {"text": system_prompt} ]
            },
            "contents": [
                {
                    "parts": [ {"text": user_prompt} ]
                }
            ]
        });

        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Gemini")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            bail!("Gemini API error: {}", error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Gemini response")?;

        let content = response_json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Invalid response format from Gemini")?
            .trim()
            .to_string();

        Ok(clean_response(content))
    }
}

fn clean_response(content: String) -> String {
    content
        .replace("```git commit", "")
        .replace("```commit", "")
        .replace("```", "")
        .trim()
        .to_string()
}

pub enum Generator {
    Mock(MockGenerator),
    OpenAI(OpenAIGenerator),
    Anthropic(AnthropicGenerator),
    Gemini(GeminiGenerator),
}

impl Generator {
    pub async fn generate(&self, diff: &str, hint: Option<String>) -> Result<String> {
        match self {
            Generator::Mock(g) => g.generate(diff, hint).await,
            Generator::OpenAI(g) => g.generate(diff, hint).await,
            Generator::Anthropic(g) => g.generate(diff, hint).await,
            Generator::Gemini(g) => g.generate(diff, hint).await,
        }
    }
}
