use crate::api::ApiClient;
use crate::config::Config;
use crate::context::ContextEngine;
use crate::errors::{AgentError, Result};
use crate::history::History;
use crate::permissions::PermissionManager;
use crate::tools::{tool_definitions, ToolExecutor};
use crate::types::{ChatMessage, PermissionMode, ToolCall};
use crate::ui;
use crate::workspace::Workspace;

pub const SYSTEM_PROMPT_TEMPLATE: &str = r#"You are an autonomous software engineering agent named Saqlain Code.

You work inside the user's workspace and have access to tools that are executed by a Rust
program, not by you. The Rust program is the sole source of truth for whether an action
actually happened.

Rules you must always follow:
- Never claim to have created, modified, deleted, or run anything unless the corresponding
  tool call actually returned success. Do not describe hypothetical actions as if they happened.
- Inspect existing code before modifying it. Prefer minimal, targeted changes and preserve the
  existing architecture unless the user asks otherwise.
- After making changes, run relevant build/test commands when appropriate, and read compiler or
  test errors carefully before attempting a fix.
- Never modify files outside the workspace, and never expose secrets (API keys, passwords,
  private keys, seed phrases) unnecessarily.
- Ask the user for clarification when a request is ambiguous instead of guessing.
- Use tools to take action. Do not simply describe what you would do — call the tool.
- Only report success after you observe a successful tool result in the conversation.
- Every shell command you run has its stdin closed and a hard timeout. Never rely on a
  command prompting you interactively — it cannot receive input and will simply time out.
  Always add the non-interactive flag for scaffolding/install tools, for example:
  `npm create vite@latest . -- --yes --template react-ts`, `npm install --yes`,
  `create-next-app . --yes --typescript --no-git`, `git clone --quiet`. If a command times out,
  do not repeat it unchanged — find and use its non-interactive equivalent instead.
- Prefer several small, verifiable steps (create files directly with write_file when you already
  know their exact contents, rather than invoking an interactive scaffolding wizard) over a
  single large command whose behavior you cannot fully predict.

Workspace context:
{context}
"#;

pub struct Agent {
    api: ApiClient,
    workspace: Workspace,
    permissions: PermissionManager,
    history: History,
    max_iterations: usize,
    command_timeout: std::time::Duration,
    verbose: bool,
}

pub enum StepOutcome {
    /// The model produced a final text response with no further tool calls.
    Final(String),
    /// The maximum number of iterations was reached.
    MaxIterations,
    /// The user cancelled the operation (e.g. via Ctrl+C).
    #[allow(dead_code)]
    Cancelled,
}

impl Agent {
    pub fn new(config: &Config, workspace: Workspace) -> Self {
        let api = ApiClient::new(
            config.base_url.clone(),
            config.api_key.clone(),
            config.model.clone(),
            config.verbose,
        );
        let permissions = PermissionManager::new(config.permission_mode);

        let mut history = History::new();
        let context = ContextEngine::new(&workspace).summary();
        let system_prompt = SYSTEM_PROMPT_TEMPLATE.replace("{context}", &context);
        history.push(ChatMessage::system(system_prompt));

        Self {
            api,
            workspace,
            permissions,
            history,
            max_iterations: config.max_iterations,
            command_timeout: std::time::Duration::from_secs(config.command_timeout_secs),
            verbose: config.verbose,
        }
    }

    pub fn model(&self) -> &str {
        self.api.model()
    }

    pub fn set_model(&mut self, model: String) {
        self.api.set_model(model);
    }

    pub fn base_url(&self) -> &str {
        self.api.base_url()
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permissions.mode
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// Run one full user request through the agent loop:
    /// LLM -> (tool calls -> execute -> feed results -> LLM)* -> final text.
    pub async fn handle_user_message(&mut self, user_input: &str) -> Result<StepOutcome> {
        self.history.push(ChatMessage::user(user_input.to_string()));

        let tools = tool_definitions();

        for iteration in 0..self.max_iterations {
            if self.verbose {
                tracing::debug!(iteration, "agent loop iteration");
            }

            let response = self
                .api
                .chat_completion(self.history.messages(), &tools)
                .await?;

            let Some(choice) = response.choices.into_iter().next() else {
                return Err(AgentError::Other("API returned no choices".to_string()));
            };

            let message = choice.message;

            // Case 1: the model wants to call tools.
            if let Some(tool_calls) = message.tool_calls.clone() {
                if !tool_calls.is_empty() {
                    // If the model also included commentary text alongside the tool
                    // calls, show it to the user before executing anything.
                    if let Some(text) = &message.content {
                        if !text.trim().is_empty() {
                            ui::print_agent_text(text);
                        }
                    }

                    self.history
                        .push(ChatMessage::assistant_tool_calls(tool_calls.clone()));

                    for call in &tool_calls {
                        let executor = ToolExecutor::new(&self.workspace, &self.permissions)
                            .with_command_timeout(self.command_timeout);
                        let (success, output) = Self::run_tool(&executor, call).await;
                        let summary = first_line(&output);
                        ui::print_tool_result(success, &summary);
                        self.history.push(ChatMessage::tool_result(
                            call.id.clone(),
                            call.function.name.clone(),
                            output,
                        ));
                    }

                    continue;
                }
            }

            // Case 2: plain text final response.
            let text = message.content.clone().unwrap_or_default();
            self.history.push(ChatMessage::assistant_text(text.clone()));
            return Ok(StepOutcome::Final(text));
        }

        Ok(StepOutcome::MaxIterations)
    }

    async fn run_tool(executor: &ToolExecutor<'_>, call: &ToolCall) -> (bool, String) {
        ui::print_tool_call(&call.function.name, &call.function.arguments);

        let result = executor.execute(&call.function.name, &call.function.arguments).await;

        match result {
            Ok(tool_result) => (tool_result.success, tool_result.output),
            Err(e) => (false, format!("Error: {e}")),
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::tempdir;

    fn test_config() -> Config {
        Config {
            base_url: "http://localhost:0/v1".to_string(),
            model: "gemini-3.5-flash".to_string(),
            api_key: "anything".to_string(),
            permission_mode: PermissionMode::Auto,
            max_iterations: 5,
            command_timeout_secs: 30,
            verbose: false,
            auto: true,
        }
    }

    #[test]
    fn agent_seeds_system_prompt_with_context() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let agent = Agent::new(&test_config(), ws);
        assert_eq!(agent.history().messages()[0].role, "system");
        assert!(agent
            .history()
            .messages()[0]
            .content
            .as_ref()
            .unwrap()
            .contains("Workspace root"));
    }

    #[tokio::test]
    async fn agent_reports_network_error_gracefully() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let mut agent = Agent::new(&test_config(), ws);
        // No server is running on port 0/unreachable, so this must return an error,
        // not panic and not fake a successful response.
        let result = agent.handle_user_message("hello").await;
        assert!(result.is_err());
    }
}
