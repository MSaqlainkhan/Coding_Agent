use crate::errors::{AgentError, Result};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ToolDefinition};
use std::time::Duration;

/// OpenAI-compatible chat completion client.
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    verbose: bool,
}

impl ApiClient {
    pub fn new(base_url: String, api_key: String, model: String, verbose: bool) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self { http, base_url, api_key, model, verbose }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Send a chat completion request with up to `max_retries` attempts using exponential backoff.
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatCompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
            temperature: Some(0.2),
            stream: Some(false),
        };

        let max_retries = 3;
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            if self.verbose {
                tracing::debug!(url = %url, model = %self.model, attempt, "sending chat completion request");
            }

            let result = self
                .http
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&request)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let text = resp.text().await?;
                        if self.verbose {
                            tracing::debug!(response = %text, "raw API response");
                        }
                        return serde_json::from_str::<ChatCompletionResponse>(&text)
                            .map_err(AgentError::Json);
                    } else {
                        let body = resp.text().await.unwrap_or_default();
                        let status_code = status.as_u16();
                        if attempt >= max_retries || status_code < 500 {
                            return Err(AgentError::Api { status: status_code, message: body });
                        }
                    }
                }
                Err(e) => {
                    if attempt >= max_retries {
                        return Err(AgentError::Network(e));
                    }
                }
            }

            let backoff_ms = 250u64 * (1u64 << (attempt - 1));
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        }
    }
}
