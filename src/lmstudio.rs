use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub stream: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatCompletionResponse {
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessageResponse,
    #[serde(default)]
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChatMessageResponse {
    #[serde(default)]
    #[allow(dead_code)]
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UsageInfo {
    #[serde(default)]
    pub prompt_tokens: Option<u32>,
    #[serde(default)]
    pub completion_tokens: Option<u32>,
    #[serde(default)]
    pub total_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelListResponse {
    pub data: Vec<ModelEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelEntry {
    pub id: String,
}

#[derive(Clone)]
pub struct LmStudioClient {
    client: Client,
    base_url: String,
}

impl LmStudioClient {
    pub fn new(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // Local LLM generation may take time for big prompts
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Self { client, base_url }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetches all available models from LM Studio /v1/models
    pub async fn fetch_models(&self) -> Result<Vec<String>, String> {
        let endpoint = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&endpoint)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    format!(
                        "Could not connect to LM Studio at {}.\nPlease make sure LM Studio is open and the local server is started (Developer tab / Start Server).",
                        self.base_url
                    )
                } else {
                    format!("Error requesting models from LM Studio: {}", e)
                }
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("LM Studio returned error {status}: {body}"));
        }

        let model_list: ModelListResponse = resp.json().await.map_err(|e| {
            format!("Failed to parse models response from LM Studio: {}", e)
        })?;

        let model_ids: Vec<String> = model_list.data.into_iter().map(|m| m.id).collect();
        Ok(model_ids)
    }

    /// Sends a chat completion request to LM Studio
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        temperature: f32,
    ) -> Result<(String, Option<UsageInfo>), String> {
        let endpoint = format!("{}/chat/completions", self.base_url);
        let request_body = ChatCompletionRequest {
            model: model.to_string(),
            messages,
            temperature,
            stream: false,
        };

        let resp = self
            .client
            .post(&endpoint)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    format!(
                        "Could not connect to LM Studio at {}.\nPlease verify LM Studio server is running.",
                        self.base_url
                    )
                } else if e.is_timeout() {
                    "Request to LM Studio timed out. The local model took too long to generate a response.".to_string()
                } else {
                    format!("LM Studio connection error: {}", e)
                }
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("LM Studio returned HTTP {status}: {body}"));
        }

        let completion: ChatCompletionResponse = resp.json().await.map_err(|e| {
            format!("Failed to parse chat response from LM Studio: {}", e)
        })?;

        let choice = completion
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| "LM Studio returned an empty choices list.".to_string())?;

        let content = choice
            .message
            .content
            .unwrap_or_else(|| "(Empty response from model)".to_string());

        Ok((content, completion.usage))
    }
}
