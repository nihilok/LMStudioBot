use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
struct NativeIntegration {
    #[serde(rename = "type")]
    pub integration_type: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize)]
struct NativeChatRequest {
    pub model: String,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<NativeIntegration>,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeChatResponse {
    #[serde(default)]
    #[allow(dead_code)]
    pub model_instance_id: Option<String>,
    #[serde(default)]
    pub output: Vec<NativeOutputItem>,
    #[serde(default)]
    pub stats: Option<NativeStats>,
    #[serde(default)]
    pub response_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeOutputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeStats {
    #[serde(default)]
    pub input_tokens: Option<u32>,
    #[serde(default)]
    pub total_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeModelListResponse {
    #[serde(default)]
    pub models: Vec<NativeModelEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeModelEntry {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    #[serde(default)]
    pub loaded_instances: Vec<serde_json::Value>,
    #[serde(default)]
    pub state: Option<String>,
}

impl NativeModelEntry {
    pub fn is_loaded(&self) -> bool {
        !self.loaded_instances.is_empty()
            || self.state.as_deref() == Some("loaded")
    }
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAIModelListResponse {
    #[serde(default)]
    pub data: Vec<OpenAIModelEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAIModelEntry {
    pub id: String,
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

#[derive(Clone)]
pub struct LmStudioClient {
    client: Client,
    base_url: String,
    #[allow(dead_code)]
    api_token: Option<String>,
    mcp_servers: Vec<String>,
}

impl LmStudioClient {
    pub fn new(base_url: String, api_token: Option<String>, mcp_servers: Vec<String>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(ref token) = api_token {
            if let Ok(mut val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
                val.set_sensitive(true);
                headers.insert(AUTHORIZATION, val);
            }
        }

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            api_token,
            mcp_servers,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn mcp_servers(&self) -> &[String] {
        &self.mcp_servers
    }

    /// Fetches all available LLM models from LM Studio, prioritizing currently loaded models
    pub async fn fetch_models(&self) -> Result<Vec<String>, String> {
        // First try native /api/v1/models
        let native_endpoint = format!("{}/api/v1/models", self.base_url);
        if let Ok(resp) = self.client.get(&native_endpoint).send().await {
            if resp.status().is_success() {
                if let Ok(list) = resp.json::<NativeModelListResponse>().await {
                    let mut entries: Vec<NativeModelEntry> = list
                        .models
                        .into_iter()
                        .filter(|m| {
                            // Only include llm models, not embedding
                            m.model_type.as_deref().map_or(true, |t| t == "llm")
                        })
                        .collect();

                    // Sort currently loaded models ahead of unloaded models on disk
                    entries.sort_by_key(|m| !m.is_loaded());

                    let mut keys = Vec::new();
                    for m in entries {
                        if let Some(k) = m.key.or(m.id) {
                            if !keys.contains(&k) {
                                keys.push(k);
                            }
                        }
                    }
                    if !keys.is_empty() {
                        return Ok(keys);
                    }
                }
            }
        }

        // Fallback to OpenAI /v1/models
        let openai_endpoint = format!("{}/v1/models", self.base_url);
        let resp = self
            .client
            .get(&openai_endpoint)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    format!(
                        "Could not connect to LM Studio at {}.\nPlease make sure LM Studio local server is started.",
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

        let model_list: OpenAIModelListResponse = resp.json().await.map_err(|e| {
            format!("Failed to parse models response from LM Studio: {}", e)
        })?;

        let model_ids: Vec<String> = model_list.data.into_iter().map(|m| m.id).collect();
        Ok(model_ids)
    }

    /// Generates a concise rolling summary of conversation history
    pub async fn generate_summary(
        &self,
        model: &str,
        new_dialogue: &str,
        previous_summary: Option<&str>,
    ) -> Result<String, String> {
        let endpoint = format!("{}/api/v1/chat", self.base_url);

        let system_instruction = "You are a concise conversation summarizer. Your task is to extract and condense key context, facts, decisions, and preferences into a brief summary. Omit small talk and pleasantries.";

        let user_prompt = match previous_summary {
            Some(prev) if !prev.trim().is_empty() => {
                format!(
                    "Update the following existing summary by incorporating the new conversation turns.\n\n\
                    [Existing Summary]:\n{}\n\n\
                    [New Conversation Turns]:\n{}\n\n\
                    Provide an updated, concise bulleted summary:",
                    prev.trim(),
                    new_dialogue.trim()
                )
            }
            _ => {
                format!(
                    "Summarize the key facts, user preferences, decisions, and ongoing context from these conversation turns into concise bullet points:\n\n\
                    {}\n\n\
                    Concise Summary:",
                    new_dialogue.trim()
                )
            }
        };

        let request_body = NativeChatRequest {
            model: model.to_string(),
            input: user_prompt,
            system_prompt: Some(system_instruction.to_string()),
            previous_response_id: None,
            temperature: Some(0.3),
            integrations: Vec::new(),
        };

        let chat_resp = self.execute_chat_request(&endpoint, &request_body).await?;

        let summary = chat_resp
            .output
            .into_iter()
            .filter(|item| item.item_type == "message")
            .filter_map(|item| item.content)
            .collect::<Vec<_>>()
            .join("\n\n");

        if summary.trim().is_empty() {
            Err("Empty summary returned from model".to_string())
        } else {
            Ok(summary.trim().to_string())
        }
    }

    /// Sends a chat request to LM Studio's native /api/v1/chat endpoint with MCP support
    pub async fn chat(
        &self,
        model: &str,
        input: &str,
        system_prompt: Option<&str>,
        previous_response_id: Option<&str>,
        temperature: f32,
    ) -> Result<(String, Option<String>, Option<UsageInfo>), String> {
        let endpoint = format!("{}/api/v1/chat", self.base_url);

        let integrations: Vec<NativeIntegration> = self
            .mcp_servers
            .iter()
            .map(|id| NativeIntegration {
                integration_type: "plugin".to_string(),
                id: id.clone(),
            })
            .collect();

        if !integrations.is_empty() {
            info!(
                integrations = ?self.mcp_servers,
                "Attaching MCP plugins to LM Studio request"
            );
        }

        // When previous_response_id is present, the conversation already contains
        // the initial system prompt. Passing system_prompt again causes LM Studio
        // to insert a system message mid-conversation, which triggers Jinja error:
        // "System message must be at the beginning."
        let effective_system_prompt = if previous_response_id.is_none() {
            system_prompt.map(|s| s.to_string())
        } else {
            None
        };

        let request_body = NativeChatRequest {
            model: model.to_string(),
            input: input.to_string(),
            system_prompt: effective_system_prompt,
            previous_response_id: previous_response_id.map(|s| s.to_string()),
            temperature: Some(temperature),
            integrations,
        };

        let mut chat_result = self.execute_chat_request(&endpoint, &request_body).await;

        // Self-healing fallback: if continuation with previous_response_id failed
        // (e.g. expired ID, server restart, template mismatch), retry as a fresh turn.
        // Do NOT retry if the request timed out, as that would double the stall.
        if let Err(ref err) = chat_result {
            if previous_response_id.is_some() && !err.contains("timed out") {
                tracing::warn!(error = %err, "Request with previous_response_id failed. Retrying with fresh conversation context...");
                let fallback_body = NativeChatRequest {
                    model: model.to_string(),
                    input: input.to_string(),
                    system_prompt: system_prompt.map(|s| s.to_string()),
                    previous_response_id: None,
                    temperature: Some(temperature),
                    integrations: request_body.integrations.clone(),
                };
                chat_result = self.execute_chat_request(&endpoint, &fallback_body).await;
            }
        }

        let chat_resp = chat_result?;

        // Extract message texts from output array
        let mut messages = Vec::new();
        let mut executed_tools = Vec::new();

        for item in &chat_resp.output {
            if item.item_type == "message" {
                if let Some(ref text) = item.content {
                    if !text.trim().is_empty() {
                        messages.push(text.clone());
                    }
                }
            } else if item.item_type == "tool_call" {
                if let Some(ref tool_name) = item.tool {
                    executed_tools.push(tool_name.clone());
                }
            }
        }

        if !executed_tools.is_empty() {
            info!(tools = ?executed_tools, "LM Studio executed MCP tool(s)");
        }

        let full_text = if !messages.is_empty() {
            messages.join("\n\n")
        } else {
            "(No textual response generated)".to_string()
        };

        let usage = chat_resp.stats.map(|s| UsageInfo {
            prompt_tokens: s.input_tokens,
            completion_tokens: s.total_output_tokens,
            total_tokens: match (s.input_tokens, s.total_output_tokens) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            },
        });

        Ok((full_text, chat_resp.response_id, usage))
    }

    async fn execute_chat_request(
        &self,
        endpoint: &str,
        request_body: &NativeChatRequest,
    ) -> Result<NativeChatResponse, String> {
        let resp = self
            .client
            .post(endpoint)
            .json(request_body)
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

        let chat_resp: NativeChatResponse = resp.json().await.map_err(|e| {
            format!("Failed to parse response from LM Studio: {}", e)
        })?;

        Ok(chat_resp)
    }
}
