//! Asking a language model, with the person's own key. Two kinds of API
//! cover the providers people use:
//!
//! - Anthropic's Messages API (`x-api-key`, `POST /v1/messages`);
//! - the OpenAI-style chat API that OpenAI, DeepSeek, Kimi, Qwen, GLM and a
//!   local Ollama all offer (`Authorization: Bearer`, `POST
//!   /chat/completions`).
//!
//! Nothing here names a model: the person picks one from the provider's own
//! list ([`models`]).

use paperloom_http::{BytesResponse, Http, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Anthropic,
    /// The OpenAI-style chat API.
    OpenAi,
}

impl Protocol {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            _ => None,
        }
    }
}

/// A provider and the person's key for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub protocol: Protocol,
    /// `https://api.anthropic.com`, or an OpenAI-style base such as
    /// `https://api.deepseek.com/v1`.
    pub base_url: String,
    /// Empty for a local server that needs none.
    pub api_key: String,
    pub model: String,
}

impl Provider {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim().trim_end_matches('/'))
    }

    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        match self.protocol {
            Protocol::Anthropic => {
                headers.push(("x-api-key".to_string(), self.api_key.clone()));
                headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
            }
            Protocol::OpenAi if !self.api_key.is_empty() => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", self.api_key)));
            }
            Protocol::OpenAi => {}
        }
        headers
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    #[error("{0}")]
    Network(String),
    /// The provider answered with an error (bad key, unknown model,
    /// rate limit…).
    #[error("{message} (HTTP {status})")]
    Provider { status: u16, message: String },
    /// The model declined to answer.
    #[error("The model declined to answer: {0}")]
    Refused(String),
    #[error("The provider's answer cannot be read: {0}")]
    Unreadable(String),
    #[error("{0}")]
    Setup(String),
}

/// What to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
}

/// The model's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    /// The answer stopped at `max_tokens`.
    pub cut_short: bool,
}

/// A model the provider offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

fn send(
    http: &dyn Http,
    provider: &Provider,
    method: &'static str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, AiError> {
    if provider.base_url.trim().is_empty() {
        return Err(AiError::Setup("Set the provider's address in Settings.".into()));
    }
    if provider.protocol == Protocol::Anthropic && provider.api_key.trim().is_empty() {
        return Err(AiError::Setup("Enter your API key in Settings.".into()));
    }
    let request = Request {
        method,
        url: provider.url(path),
        headers: provider.headers(),
        body: body.map(|b| b.to_string().into_bytes()),
    };
    let response = http.send(request).map_err(|e| AiError::Network(e.0))?;
    read(response)
}

fn read(response: BytesResponse) -> Result<Value, AiError> {
    let value: Option<Value> = serde_json::from_slice(&response.body).ok();
    if !response.is_success() {
        // Both APIs: {"error": {"message": …}}.
        let message = value
            .as_ref()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&response.body).chars().take(300).collect());
        return Err(AiError::Provider { status: response.status, message });
    }
    value.ok_or_else(|| AiError::Unreadable("not JSON".into()))
}

/// Asks once and waits for the whole answer.
pub fn complete(http: &dyn Http, provider: &Provider, prompt: &Prompt) -> Result<Answer, AiError> {
    if provider.model.trim().is_empty() {
        return Err(AiError::Setup("Choose a model in Settings.".into()));
    }
    match provider.protocol {
        Protocol::Anthropic => {
            let body = json!({
                "model": provider.model.trim(),
                "max_tokens": prompt.max_tokens,
                "system": prompt.system,
                "messages": [{ "role": "user", "content": prompt.user }],
            });
            let reply = send(http, provider, "POST", "/v1/messages", Some(body))?;
            let stop = reply["stop_reason"].as_str().unwrap_or_default();
            if stop == "refusal" {
                let details = &reply["stop_details"];
                let why = details["explanation"]
                    .as_str()
                    .or_else(|| details["category"].as_str())
                    .unwrap_or("no reason given");
                return Err(AiError::Refused(why.to_string()));
            }
            let text: String = reply["content"]
                .as_array()
                .ok_or_else(|| AiError::Unreadable("no content".into()))?
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect();
            Ok(Answer { text, cut_short: stop == "max_tokens" })
        }
        Protocol::OpenAi => {
            // No token limit is sent: providers disagree on its name.
            let body = json!({
                "model": provider.model.trim(),
                "messages": [
                    { "role": "system", "content": prompt.system },
                    { "role": "user", "content": prompt.user },
                ],
            });
            let reply = send(http, provider, "POST", "/chat/completions", Some(body))?;
            let choice = &reply["choices"][0];
            let finish = choice["finish_reason"].as_str().unwrap_or_default();
            if finish == "content_filter" {
                return Err(AiError::Refused("content filter".into()));
            }
            let text = choice["message"]["content"]
                .as_str()
                .ok_or_else(|| AiError::Unreadable("no message".into()))?
                .to_string();
            Ok(Answer { text, cut_short: finish == "length" })
        }
    }
}

/// Embeds text through an OpenAI-compatible provider, in input order.
pub fn embed(
    http: &dyn Http,
    provider: &Provider,
    model: &str,
    inputs: &[String],
) -> Result<Vec<Vec<f32>>, AiError> {
    if provider.protocol != Protocol::OpenAi {
        return Err(AiError::Setup("Semantic search needs an OpenAI-compatible provider.".into()));
    }
    if model.trim().is_empty() {
        return Err(AiError::Setup("Choose an embedding model in Settings.".into()));
    }
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let reply = send(
        http,
        provider,
        "POST",
        "/embeddings",
        Some(json!({
            "model": model.trim(),
            "input": inputs,
        })),
    )?;
    let invalid = || AiError::Setup("The provider returned invalid embeddings.".into());
    let data = reply["data"].as_array().ok_or_else(invalid)?;
    if data.len() != inputs.len() {
        return Err(invalid());
    }
    let mut ordered = vec![None; inputs.len()];
    let mut dimensions = None;
    for entry in data {
        let index = entry["index"].as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(invalid)?;
        let slot = ordered.get_mut(index).ok_or_else(invalid)?;
        if slot.is_some() {
            return Err(invalid());
        }
        let vector: Vec<f32> = entry["embedding"]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|v| v.as_f64().map(|n| n as f32).filter(|n| n.is_finite()).ok_or_else(invalid))
            .collect::<Result<_, _>>()?;
        if vector.is_empty() || dimensions.is_some_and(|d| d != vector.len()) {
            return Err(invalid());
        }
        dimensions = Some(vector.len());
        *slot = Some(vector);
    }
    ordered.into_iter().map(|v| v.ok_or_else(invalid)).collect()
}

/// The models the provider offers this key, for the person to pick from.
pub fn models(http: &dyn Http, provider: &Provider) -> Result<Vec<ModelInfo>, AiError> {
    let mut found = Vec::new();
    match provider.protocol {
        Protocol::Anthropic => {
            let mut after: Option<String> = None;
            loop {
                let path = match &after {
                    Some(id) => format!("/v1/models?limit=100&after_id={id}"),
                    None => "/v1/models?limit=100".to_string(),
                };
                let page = send(http, provider, "GET", &path, None)?;
                for model in page["data"].as_array().into_iter().flatten() {
                    if let Some(id) = model["id"].as_str() {
                        let name = model["display_name"].as_str().unwrap_or(id);
                        found.push(ModelInfo { id: id.into(), name: name.into() });
                    }
                }
                match (page["has_more"].as_bool(), page["last_id"].as_str()) {
                    (Some(true), Some(last)) => after = Some(last.to_string()),
                    _ => break,
                }
            }
        }
        Protocol::OpenAi => {
            let page = send(http, provider, "GET", "/models", None)?;
            let mut ids: Vec<String> = page["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|model| model["id"].as_str().map(str::to_string))
                .collect();
            ids.sort();
            found.extend(ids.into_iter().map(|id| ModelInfo { name: id.clone(), id }));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
