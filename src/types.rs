use serde::{Deserialize, Serialize};

/// A single chat message in OpenAI-compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_function_type() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// Raw JSON-encoded arguments string, as returned by the model.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionSchema,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Permission mode for potentially destructive actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Auto,
    Ask,
    Deny,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Ask
    }
}

impl std::str::FromStr for PermissionMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(PermissionMode::Auto),
            "ask" => Ok(PermissionMode::Ask),
            "deny" => Ok(PermissionMode::Deny),
            other => Err(format!("invalid permission mode: {other}")),
        }
    }
}

/// The outcome of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self { success: true, output: output.into() }
    }
    pub fn err(output: impl Into<String>) -> Self {
        Self { success: false, output: output.into() }
    }
}
