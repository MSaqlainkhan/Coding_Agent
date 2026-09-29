use crate::diff::render_diff;
use crate::errors::{AgentError, Result};
use crate::permissions::{looks_sensitive, PermissionDecision, PermissionManager};
use crate::types::{ToolDefinition, ToolFunctionSchema, ToolResult};
use crate::workspace::Workspace;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use walkdir::WalkDir;

const MAX_READ_BYTES: usize = 200_000;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_COMMAND_OUTPUT: usize = 20_000;

/// Hard ceiling on how long a single `run_command` invocation may run before
/// it is killed. This exists specifically so that a subprocess which is
/// silently waiting on interactive input it will never receive (npx/npm
/// "ok to proceed?" prompts, git credential prompts, etc.) cannot freeze the
/// entire agent loop forever. The command's own stdin is also closed (see
/// below), so most tools will either auto-proceed or fail fast instead of
/// blocking at all.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(180);

const DEFAULT_IGNORE_DIRS: &[&str] = &[
    ".git", "target", "node_modules", "dist", "build", ".cache", "venv", ".env_dir",
];

/// Returns the OpenAI-compatible tool schemas for every tool the agent supports.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        def(
            "list_files",
            "List files and directories under a path in the workspace, respecting .gitignore and common ignore rules.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within the workspace. Use '.' for the root." }
                },
                "required": ["path"]
            }),
        ),
        def(
            "read_file",
            "Read the contents of a file in the workspace, with line numbers.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to the file." }
                },
                "required": ["path"]
            }),
        ),
        def(
            "write_file",
            "Create or overwrite a file in the workspace with the given content. Creates parent directories if needed.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ),
        def(
            "edit_file",
            "Replace an exact substring (old_text) with new_text inside an existing file. Fails if old_text is not found exactly once.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string" },
                    "new_text": { "type": "string" },
                    "allow_multiple": { "type": "boolean", "description": "If true, replace all occurrences instead of failing on multiple matches." }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        ),
        def(
            "delete_file",
            "Delete a file in the workspace. Requires user confirmation.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        ),
        def(
            "search_code",
            "Search source files in the workspace for a text query, returning matching file paths and line numbers.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string", "description": "Directory to search under. Defaults to '.'." }
                },
                "required": ["query"]
            }),
        ),
        def(
            "run_command",
            "Execute a shell command in the workspace root and return stdout, stderr, exit code, and duration.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        ),
    ]
}

fn def(name: &str, description: &str, parameters: serde_json::Value) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionSchema {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

/// Executes tool calls against the real filesystem/process APIs. This is the
/// single source of truth: nothing is considered to have happened unless a
/// method here actually performed it.
pub struct ToolExecutor<'a> {
    pub workspace: &'a Workspace,
    pub permissions: &'a PermissionManager,
    pub command_timeout: Duration,
}

#[derive(Deserialize)]
struct ListFilesArgs {
    path: String,
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct EditFileArgs {
    path: String,
    old_text: String,
    new_text: String,
    #[serde(default)]
    allow_multiple: bool,
}

#[derive(Deserialize)]
struct DeleteFileArgs {
    path: String,
}

#[derive(Deserialize)]
struct SearchCodeArgs {
    query: String,
    #[serde(default = "default_search_path")]
    path: String,
}

fn default_search_path() -> String {
    ".".to_string()
}

#[derive(Deserialize)]
struct RunCommandArgs {
    command: String,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(workspace: &'a Workspace, permissions: &'a PermissionManager) -> Self {
        Self {
            workspace,
            permissions,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Dispatch a tool call by name with raw JSON arguments.
    pub async fn execute(&self, name: &str, raw_args: &str) -> Result<ToolResult> {
        match name {
            "list_files" => self.list_files(raw_args),
            "read_file" => self.read_file(raw_args),
            "write_file" => self.write_file(raw_args),
            "edit_file" => self.edit_file(raw_args),
            "delete_file" => self.delete_file(raw_args),
            "search_code" => self.search_code(raw_args),
            "run_command" => self.run_command(raw_args).await,
            other => Err(AgentError::UnknownTool(other.to_string())),
        }
    }

    fn parse_args<T: for<'de> Deserialize<'de>>(name: &str, raw: &str) -> Result<T> {
        serde_json::from_str::<T>(raw).map_err(|e| AgentError::InvalidToolArguments {
            tool: name.to_string(),
            reason: e.to_string(),
        })
    }

    fn is_ignored(entry_path: &Path, workspace_root: &Path) -> bool {
        let rel = entry_path.strip_prefix(workspace_root).unwrap_or(entry_path);
        rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            DEFAULT_IGNORE_DIRS.contains(&s.as_ref())
        })
    }

    fn list_files(&self, raw_args: &str) -> Result<ToolResult> {
        let args: ListFilesArgs = Self::parse_args("list_files", raw_args)?;
        let target = self.workspace.resolve(&args.path)?;

        if !target.exists() {
            return Ok(ToolResult::err(format!("path does not exist: {}", args.path)));
        }

        let mut files = Vec::new();
        let mut dirs = Vec::new();

        for entry in WalkDir::new(&target).max_depth(6).into_iter().flatten() {
            if Self::is_ignored(entry.path(), &self.workspace.root) {
                continue;
            }
            let rel = self.workspace.relative(entry.path());
            if entry.path() == target {
                continue;
            }
            if entry.file_type().is_dir() {
                dirs.push(rel);
            } else {
                files.push(rel);
            }
            if files.len() + dirs.len() >= MAX_LIST_ENTRIES {
                break;
            }
        }

        files.sort();
        dirs.sort();

        let mut out = String::new();
        out.push_str(&format!("Directories ({}):\n", dirs.len()));
        for d in &dirs {
            out.push_str(&format!("  {d}/\n"));
        }
        out.push_str(&format!("\nFiles ({}):\n", files.len()));
        for f in &files {
            out.push_str(&format!("  {f}\n"));
        }
        if files.len() + dirs.len() >= MAX_LIST_ENTRIES {
            out.push_str("\n[output truncated: too many entries]\n");
        }

        Ok(ToolResult::ok(out))
    }

    fn read_file(&self, raw_args: &str) -> Result<ToolResult> {
        let args: ReadFileArgs = Self::parse_args("read_file", raw_args)?;

        if looks_sensitive(&args.path) {
            if self.permissions.check_sensitive_read(&args.path) != PermissionDecision::Allow {
                return Ok(ToolResult::err(format!(
                    "user declined to share potentially sensitive file: {}",
                    args.path
                )));
            }
        }

        let target = self.workspace.resolve(&args.path)?;
        if !target.exists() {
            return Err(AgentError::FileNotFound(args.path));
        }
        if !target.is_file() {
            return Ok(ToolResult::err(format!("not a file: {}", args.path)));
        }

        let bytes = std::fs::read(&target)?;
        let truncated = bytes.len() > MAX_READ_BYTES;
        let slice = if truncated { &bytes[..MAX_READ_BYTES] } else { &bytes[..] };
        let content = String::from_utf8_lossy(slice);

        let mut out = String::new();
        for (i, line) in content.lines().enumerate() {
            out.push_str(&format!("{:>5} | {}\n", i + 1, line));
        }
        if truncated {
            out.push_str("\n[file truncated: exceeds size limit]\n");
        }

        Ok(ToolResult::ok(out))
    }

    fn write_file(&self, raw_args: &str) -> Result<ToolResult> {
        let args: WriteFileArgs = Self::parse_args("write_file", raw_args)?;
        let target = self.workspace.resolve(&args.path)?;

        let existed = target.exists();
        let old_content = if existed {
            std::fs::read_to_string(&target).unwrap_or_default()
        } else {
            String::new()
        };

        let diff_display = render_diff(&args.path, &old_content, &args.content);
        println!("{diff_display}");

        let description = if existed {
            format!("(overwrite {})", args.path)
        } else {
            format!("(create {})", args.path)
        };

        if self.permissions.check_file_change(&description) != PermissionDecision::Allow {
            return Ok(ToolResult::err(format!(
                "user declined the change to {}",
                args.path
            )));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &args.content)?;

        Ok(ToolResult::ok(format!(
            "Successfully {} {} ({} bytes)",
            if existed { "overwrote" } else { "created" },
            args.path,
            args.content.len()
        )))
    }

    fn edit_file(&self, raw_args: &str) -> Result<ToolResult> {
        let args: EditFileArgs = Self::parse_args("edit_file", raw_args)?;
        let target = self.workspace.resolve(&args.path)?;

        if !target.exists() {
            return Err(AgentError::FileNotFound(args.path));
        }

        let original = std::fs::read_to_string(&target)?;
        let occurrences = original.matches(args.old_text.as_str()).count();

        if occurrences == 0 {
            return Err(AgentError::EditFailed(format!(
                "old_text not found in {}",
                args.path
            )));
        }
        if occurrences > 1 && !args.allow_multiple {
            return Err(AgentError::EditFailed(format!(
                "old_text matches {} times in {} — refine it to be unique, or set allow_multiple=true",
                occurrences, args.path
            )));
        }

        let updated = if args.allow_multiple {
            original.replace(args.old_text.as_str(), args.new_text.as_str())
        } else {
            original.replacen(args.old_text.as_str(), args.new_text.as_str(), 1)
        };

        let diff_display = render_diff(&args.path, &original, &updated);
        println!("{diff_display}");

        if self
            .permissions
            .check_file_change(&format!("(edit {})", args.path))
            != PermissionDecision::Allow
        {
            return Ok(ToolResult::err(format!(
                "user declined the change to {}",
                args.path
            )));
        }

        std::fs::write(&target, &updated)?;

        Ok(ToolResult::ok(format!(
            "Successfully edited {} ({} replacement{})",
            args.path,
            occurrences.min(if args.allow_multiple { occurrences } else { 1 }),
            if occurrences == 1 { "" } else { "s" }
        )))
    }

    fn delete_file(&self, raw_args: &str) -> Result<ToolResult> {
        let args: DeleteFileArgs = Self::parse_args("delete_file", raw_args)?;
        let target = self.workspace.resolve(&args.path)?;

        if !target.exists() {
            return Err(AgentError::FileNotFound(args.path));
        }

        if self.permissions.check_delete(&args.path) != PermissionDecision::Allow {
            return Ok(ToolResult::err(format!("user declined to delete {}", args.path)));
        }

        if target.is_dir() {
            std::fs::remove_dir_all(&target)?;
        } else {
            std::fs::remove_file(&target)?;
        }

        Ok(ToolResult::ok(format!("Successfully deleted {}", args.path)))
    }

    fn search_code(&self, raw_args: &str) -> Result<ToolResult> {
        let args: SearchCodeArgs = Self::parse_args("search_code", raw_args)?;
        let target = self.workspace.resolve(&args.path)?;

        if which_rg_available() {
            if let Ok(out) = self.search_with_ripgrep(&args.query, &target) {
                return Ok(out);
            }
            // fall through to manual search on ripgrep failure
        }

        self.search_manual(&args.query, &target)
    }

    fn search_with_ripgrep(&self, query: &str, target: &Path) -> Result<ToolResult> {
        let output = std::process::Command::new("rg")
            .arg("-n")
            .arg("--no-heading")
            .arg("--max-count=200")
            .arg(query)
            .arg(target)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return Ok(ToolResult::ok(format!("No matches found for '{query}'")));
        }

        let mut out = String::new();
        for line in stdout.lines().take(200) {
            let rel = line.replacen(&self.workspace.root.display().to_string(), "", 1);
            out.push_str(rel.trim_start_matches('/'));
            out.push('\n');
        }
        Ok(ToolResult::ok(out))
    }

    fn search_manual(&self, query: &str, target: &Path) -> Result<ToolResult> {
        let mut matches = Vec::new();
        for entry in WalkDir::new(target).into_iter().flatten() {
            if Self::is_ignored(entry.path(), &self.workspace.root) {
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(entry.path()) else { continue };
            for (i, line) in content.lines().enumerate() {
                if line.contains(query) {
                    let rel = self.workspace.relative(entry.path());
                    matches.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                    if matches.len() >= 200 {
                        break;
                    }
                }
            }
            if matches.len() >= 200 {
                break;
            }
        }

        if matches.is_empty() {
            return Ok(ToolResult::ok(format!("No matches found for '{query}'")));
        }

        Ok(ToolResult::ok(matches.join("\n")))
    }

    async fn run_command(&self, raw_args: &str) -> Result<ToolResult> {
        let args: RunCommandArgs = Self::parse_args("run_command", raw_args)?;

        if self.permissions.check_command(&args.command) != PermissionDecision::Allow {
            return Ok(ToolResult::err(format!(
                "user declined to run command: {}",
                args.command
            )));
        }

        let start = Instant::now();

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&args.command)
            .current_dir(&self.workspace.root)
            // Deliberately closed: a command that tries to read an interactive
            // answer (npm/npx "ok to proceed?", git credential prompts, etc.)
            // must fail fast or auto-proceed instead of hanging forever with
            // no visible prompt, since nothing is forwarding terminal input
            // to it here.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let mut stdout_reader = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
        let mut stderr_reader = BufReader::new(child.stderr.take().expect("stderr piped")).lines();

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();

        let streaming = async {
            let mut stdout_done = false;
            let mut stderr_done = false;
            loop {
                if stdout_done && stderr_done {
                    break;
                }
                tokio::select! {
                    line = stdout_reader.next_line(), if !stdout_done => {
                        match line {
                            Ok(Some(l)) => {
                                println!("{l}");
                                stdout_buf.push_str(&l);
                                stdout_buf.push('\n');
                            }
                            _ => stdout_done = true,
                        }
                    }
                    line = stderr_reader.next_line(), if !stderr_done => {
                        match line {
                            Ok(Some(l)) => {
                                eprintln!("{l}");
                                stderr_buf.push_str(&l);
                                stderr_buf.push('\n');
                            }
                            _ => stderr_done = true,
                        }
                    }
                }
            }
            child.wait().await
        };

        let wait_result = tokio::time::timeout(self.command_timeout, streaming).await;

        let (status, timed_out) = match wait_result {
            Ok(Ok(status)) => (Some(status), false),
            Ok(Err(_)) => (None, false),
            Err(_) => {
                // Timed out: the child's stdin is already closed, so a hang
                // here means the process itself is stuck or long-running.
                // Kill it so the agent loop is never blocked indefinitely.
                let _ = child.start_kill();
                let _ = child.wait().await;
                (None, true)
            }
        };

        let duration = start.elapsed();

        if stdout_buf.len() > MAX_COMMAND_OUTPUT {
            stdout_buf.truncate(MAX_COMMAND_OUTPUT);
            stdout_buf.push_str("\n[stdout truncated]");
        }
        if stderr_buf.len() > MAX_COMMAND_OUTPUT {
            stderr_buf.truncate(MAX_COMMAND_OUTPUT);
            stderr_buf.push_str("\n[stderr truncated]");
        }

        if timed_out {
            let out = format!(
                "Command: {}\nTIMED OUT after {:.0}s and was killed. Its stdin is not connected \
                 to a live terminal, so any interactive prompt it might have shown could never be \
                 answered. Re-run using a non-interactive flag (e.g. --yes / -y / CI=true) instead.\n\n\
                 stdout so far:\n{}\n\nstderr so far:\n{}",
                args.command,
                self.command_timeout.as_secs_f64(),
                stdout_buf,
                stderr_buf
            );
            return Ok(ToolResult::err(out));
        }

        let Some(status) = status else {
            return Ok(ToolResult::err(format!(
                "Command: {}\nFailed to wait on child process.",
                args.command
            )));
        };

        let out = format!(
            "Command: {}\nExit code: {}\nDuration: {:.2}s\n\nstdout:\n{}\n\nstderr:\n{}",
            args.command,
            status.code().unwrap_or(-1),
            duration.as_secs_f64(),
            stdout_buf,
            stderr_buf
        );

        if status.success() {
            Ok(ToolResult::ok(out))
        } else {
            Ok(ToolResult::err(out))
        }
    }
}

fn which_rg_available() -> bool {
    std::process::Command::new("rg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PermissionMode;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, Workspace) {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        (dir, ws)
    }

    #[test]
    fn write_and_read_file_roundtrip() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        let write_args = json!({"path": "hello.txt", "content": "Hello, World!"}).to_string();
        let result = exec.write_file(&write_args).unwrap();
        assert!(result.success);

        let read_args = json!({"path": "hello.txt"}).to_string();
        let result = exec.read_file(&read_args).unwrap();
        assert!(result.output.contains("Hello, World!"));
    }

    #[test]
    fn edit_file_fails_on_zero_matches() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        std::fs::write(ws.root.join("a.txt"), "one two three").unwrap();
        let edit_args = json!({"path": "a.txt", "old_text": "nope", "new_text": "x"}).to_string();
        let result = exec.edit_file(&edit_args);
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_fails_on_multiple_matches_without_flag() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        std::fs::write(ws.root.join("a.txt"), "dup dup dup").unwrap();
        let edit_args = json!({"path": "a.txt", "old_text": "dup", "new_text": "x"}).to_string();
        let result = exec.edit_file(&edit_args);
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_succeeds_on_unique_match() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        std::fs::write(ws.root.join("a.txt"), "unique_marker here").unwrap();
        let edit_args =
            json!({"path": "a.txt", "old_text": "unique_marker", "new_text": "changed"}).to_string();
        let result = exec.edit_file(&edit_args).unwrap();
        assert!(result.success);
        let content = std::fs::read_to_string(ws.root.join("a.txt")).unwrap();
        assert_eq!(content, "changed here");
    }

    #[test]
    fn delete_file_requires_confirmation_in_ask_mode() {
        // Auto mode still prompts for delete; we only test Deny mode here to avoid stdin blocking.
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Deny);
        let exec = ToolExecutor::new(&ws, &perms);

        std::fs::write(ws.root.join("a.txt"), "content").unwrap();
        let args = json!({"path": "a.txt"}).to_string();
        let result = exec.delete_file(&args).unwrap();
        assert!(!result.success);
        assert!(ws.root.join("a.txt").exists());
    }

    #[test]
    fn read_file_rejects_path_outside_workspace() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        let args = json!({"path": "../../etc/passwd"}).to_string();
        let result = exec.read_file(&args);
        assert!(result.is_err());
    }

    #[test]
    fn search_code_finds_match() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        std::fs::write(ws.root.join("main.rs"), "// TODO: fix this\nfn main() {}").unwrap();
        let args = json!({"query": "TODO", "path": "."}).to_string();
        let result = exec.search_code(&args).unwrap();
        assert!(result.output.contains("main.rs"));
    }

    #[test]
    fn invalid_json_arguments_produce_error() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        let result = exec.read_file("{not valid json");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_command_captures_output_and_exit_code() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        let args = json!({"command": "echo hello123"}).to_string();
        let result = exec.run_command(&args).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("hello123"));
        assert!(result.output.contains("Exit code: 0"));
    }

    #[tokio::test]
    async fn run_command_reports_nonzero_exit() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);

        let args = json!({"command": "exit 7"}).to_string();
        let result = exec.run_command(&args).await.unwrap();
        assert!(!result.success);
        assert!(result.output.contains("Exit code: 7"));
    }

    #[tokio::test]
    async fn run_command_denied_in_deny_mode() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Deny);
        let exec = ToolExecutor::new(&ws, &perms);

        let args = json!({"command": "echo hi"}).to_string();
        let result = exec.run_command(&args).await.unwrap();
        assert!(!result.success);
        assert!(result.output.contains("declined"));
    }

    #[tokio::test]
    async fn run_command_times_out_instead_of_hanging_forever() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        // A very short timeout so the test itself stays fast.
        let exec = ToolExecutor::new(&ws, &perms)
            .with_command_timeout(std::time::Duration::from_millis(300));

        // `read` with no input and stdin closed will normally hang waiting for
        // input forever; with stdin closed it should hit EOF immediately, so
        // use `sleep` to actually simulate a long-running/stuck command.
        let args = json!({"command": "sleep 5"}).to_string();
        let started = std::time::Instant::now();
        let result = exec.run_command(&args).await.unwrap();
        let elapsed = started.elapsed();

        assert!(!result.success);
        assert!(result.output.contains("TIMED OUT"));
        assert!(elapsed < std::time::Duration::from_secs(2), "should not wait for the full sleep");
    }

    #[tokio::test]
    async fn run_command_closes_stdin_so_interactive_reads_dont_hang() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms)
            .with_command_timeout(std::time::Duration::from_secs(5));

        // `cat` with closed stdin should hit EOF immediately and exit, rather
        // than blocking on input that will never arrive.
        let args = json!({"command": "cat"}).to_string();
        let started = std::time::Instant::now();
        let result = exec.run_command(&args).await.unwrap();
        let elapsed = started.elapsed();

        assert!(result.success);
        assert!(elapsed < std::time::Duration::from_secs(2));
    }

    #[test]
    fn unknown_tool_is_rejected() {
        let (_dir, ws) = setup();
        let perms = PermissionManager::new(PermissionMode::Auto);
        let exec = ToolExecutor::new(&ws, &perms);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(exec.execute("not_a_real_tool", "{}"));
        assert!(result.is_err());
    }
}
