# Saqlain Code

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A terminal-based AI coding agent written in Rust.**

Saqlain Code is in the same spirit as Claude Code and Codex CLI, but it talks to **your own OpenAI-compatible model server** (for example [`gemini-web2api`](https://github.com/Sophomoresty/gemini-web2api)) running locally.

It is not a chatbot. The model decides *which* tools to call, but the Rust program is the only thing that touches your filesystem or runs commands, and it is the sole source of truth for whether anything actually happened. If a tool call isn't made and executed, no file is created, no command is run, and the agent won't claim otherwise.

```
╭────────────────────────────────────────╮
│ Saqlain Code                            │
│ AI Coding Agent                         │
│ Model: gemini-3.5-flash                 │
│ Workspace: my-project                   │
╰────────────────────────────────────────╯

> Fix the compilation errors

🔍 Listing files in .
📖 Reading src/main.rs
✏ Editing src/main.rs
⚙ Running: cargo check

✓ Fixed 2 errors
```

## Table of Contents

- [Features](#features)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Tools Available to the Model](#tools-available-to-the-model)
- [Permissions and Security](#permissions-and-security)
- [Architecture](#architecture)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [License](#license)

## Features

- **Real agent loop**: user request → system prompt + project context → LLM → tool calls → execution → results fed back → repeat until a final answer, the iteration limit, or an error.
- **Filesystem tools**: list, read, write, edit, and delete files.
- **Code search**: uses `ripgrep` when available and falls back to a built-in Rust search otherwise.
- **Command execution**: captures stdout, stderr, exit code, and duration, and feeds them back so the model can debug its own build and test failures.
- **Permission system**: `auto`, `ask`, and `deny` modes. Dangerous commands always require confirmation, whatever the mode.
- **Diffs before writes**: every file creation or edit is shown as a unified diff before you approve it.
- **Path security**: all paths are canonicalized against the workspace root. `../` escapes, absolute paths outside the workspace, and symlink escapes are rejected.
- **Secret protection**: files that look sensitive (`.env`, `*.pem`, `*.key`, `id_rsa`, `credentials.json`, ...) require explicit confirmation before their contents are sent to the model.
- **Hang-proof commands**: shell commands run with stdin closed and a hard timeout, so an interactive prompt can never freeze the agent.
- **Conversation history**: `tool_call_id` linkage between tool calls and results follows the OpenAI tool-calling protocol exactly.
- **Interactive terminal UI** with slash commands, plus a one-shot CLI mode for scripting.

## Quick Start

```bash
# 1. Clone and install
git clone https://github.com/YOUR-USERNAME/saqlain-code.git
cd saqlain-code
cargo install --path .

# 2. Start your OpenAI-compatible model server (default: http://localhost:8081/v1)

# 3. Run the agent from inside any project
cd /path/to/your/project
saqlain-code
```

## Installation

### Requirements

- **Rust** (stable, 1.75 or newer) and **Cargo**
- An **OpenAI-compatible chat completions server** reachable over HTTP, such as [`gemini-web2api`](https://github.com/Sophomoresty/gemini-web2api) on `http://localhost:8081/v1`
- *Optional:* [`ripgrep`](https://github.com/BurntSushi/ripgrep) (`rg`) on your `PATH` for faster code search

### Build and install

```bash
git clone https://github.com/YOUR-USERNAME/saqlain-code.git
cd saqlain-code
cargo build --release
cargo install --path .
```

After installation, `saqlain-code` is available from any directory.

## Configuration

Settings are resolved in this order (highest priority first):

1. **CLI flags**: `--model`, `--base-url`, `--api-key`, `--permission-mode`, `--max-iterations`, `--command-timeout-secs`
2. **Environment variables**: `SAQLAIN_CODE_BASE_URL`, `SAQLAIN_CODE_API_KEY`, `SAQLAIN_CODE_MODEL`
3. **Config file** at `~/.config/saqlain-code/config.toml`:

   ```toml
   base_url = "http://localhost:8081/v1"
   model = "gemini-3.5-flash"
   api_key = "anything"
   permission_mode = "ask"
   max_iterations = 50
   command_timeout_secs = 180
   ```

4. **Built-in defaults**: `http://localhost:8081/v1`, `gemini-3.5-flash`, `ask` mode, 50 iterations, 180-second command timeout

## Usage

Start your model server first, then run Saqlain Code from inside the project you want to work on:

```bash
cd /path/to/your/project

# Interactive mode
saqlain-code

# One-shot task
saqlain-code "fix the compilation errors"

# Override model / server / iteration limit
saqlain-code --model gemini-3.7-flash --base-url http://localhost:8081/v1 --max-iterations 30

# Auto-approve safe file changes and commands (dangerous ones still ask)
saqlain-code --auto

# Verbose logging of HTTP requests, tool calls, and timings
saqlain-code --verbose

# Cap how long any single shell command may run (default: 180s)
saqlain-code --command-timeout-secs 60
```

### Interactive commands

| Command         | Description                                      |
|-----------------|--------------------------------------------------|
| `/help`         | List available commands                          |
| `/status`       | Show model, base URL, and workspace path         |
| `/model [name]` | Show or change the active model                  |
| `/models`       | List known model names                           |
| `/clear`        | Clear conversation history                       |
| `/reset`        | Alias for `/clear`                               |
| `/history`      | Print the conversation transcript                |
| `/diff`         | Run `git diff` in the workspace                  |
| `/files`        | List top-level files in the workspace            |
| `/context`      | Print the project context sent to the model      |
| `/permissions`  | Show the current permission mode                 |
| `/compact`      | Compact conversation history to save tokens      |
| `/quit`         | Exit                                             |

## Tools Available to the Model

| Tool          | Purpose                                                                                     |
|---------------|---------------------------------------------------------------------------------------------|
| `list_files`  | List files and directories under a path, respecting ignore rules                            |
| `read_file`   | Read a file with line numbers                                                               |
| `write_file`  | Create or overwrite a file (shows a diff, asks permission)                                  |
| `edit_file`   | Exact-match find/replace in a file (fails on 0 or >1 matches unless `allow_multiple`)       |
| `delete_file` | Delete a file or directory (always confirmed)                                               |
| `search_code` | Text search across the workspace, using `ripgrep` if available                              |
| `run_command` | Execute a shell command, capturing stdout, stderr, exit code, and duration                  |

`run_command` closes the child's stdin and streams output to your terminal live. A hard timeout (default 180s, set via `--command-timeout-secs` or `command_timeout_secs` in the config) kills any command that hangs, and the model is told it timed out. The system prompt also tells the model to prefer non-interactive flags (`--yes`, `-y`, `CI=true`, and so on) for scaffolding tools.

By default `.git`, `target`, `node_modules`, `dist`, `build`, `.cache`, and `venv` are excluded from listings and search.

## Permissions and Security

- **Permission modes**: `auto`, `ask` (default), and `deny`. Set with `--permission-mode`, the config file, or `--auto` as a shortcut.
- **Dangerous commands** (`rm -rf`, `sudo`, `mkfs`, `dd`, `shutdown`, `reboot`, `chmod -R`, `chown -R`, `curl ... | bash`, `wget ... | bash`, `git reset --hard`, `git clean -fd`, force pushes, and similar) **always** prompt for confirmation, even in `auto` mode. The default answer is "no".
- **Path security**: paths are resolved against the workspace root. `..` escapes, absolute paths outside the workspace, and symlinks resolving outside it are rejected.
- **Secret protection**: files that look like secrets are never sent to the model without an explicit "yes" from you.
- The agent **never commits or pushes to git** on its own.

## Architecture

| Module           | Responsibility                                            |
|------------------|-----------------------------------------------------------|
| `api.rs`         | OpenAI-compatible HTTP client (retries, backoff)          |
| `agent.rs`       | The agent loop                                            |
| `tools.rs`       | Tool schemas and real execution against the filesystem/OS |
| `workspace.rs`   | Workspace root and safe path resolution                   |
| `permissions.rs` | Dangerous-command detection and interactive confirmation  |
| `context.rs`     | Lightweight project context (git status, manifest, README)|
| `history.rs`     | Conversation state, transcript rendering, compaction      |
| `diff.rs`        | Unified diff rendering via the `similar` crate            |
| `ui.rs`          | Terminal output (banner, tool icons, results)             |
| `config.rs`      | CLI args / env vars / config file resolution              |
| `types.rs`       | API and internal data structures                          |
| `errors.rs`      | Centralized error type                                    |
| `cli.rs`         | `clap`-based argument parsing                             |

The Rust application always performs the filesystem and process work. The model can only *request* an action and then see the real result.

## Examples

```
> Create a file named test.cpp containing a hello world program, then compile and run it.

✏ Writing test.cpp
──────────────────────────────────────────────────
File: test.cpp

+ #include <iostream>
+
+ int main() {
+     std::cout << "Hello from Saqlain Code!" << std::endl;
+     return 0;
+ }
──────────────────────────────────────────────────
✓ Successfully created test.cpp (108 bytes)
⚙ Running: g++ test.cpp -o test
✓ Command: g++ test.cpp -o test
⚙ Running: ./test
✓ Command: ./test

Created test.cpp, compiled it successfully, and ran it. Output: Hello from Saqlain Code!
```

## Troubleshooting

- **`network error: ... Connection refused`**: the model server isn't running, or `--base-url` / `SAQLAIN_CODE_BASE_URL` points at the wrong address.
- **`API error (4xx/5xx): ...`**: the server rejected the request. Check that the model name is one your server actually serves.
- **`edit_file` fails with "matches N times"**: make `old_text` longer or more specific so it matches exactly one location, or pass `allow_multiple: true` if that is intended.
- **A command seems stuck on a `[y/N]` prompt**: the permission system flagged the action (dangerous command, sensitive file, or a change while not in `auto` mode). Answer the prompt in the terminal.

## Development

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

### Project layout

```
saqlain-code/
├── Cargo.toml
├── LICENSE
├── README.md
├── src/
│   ├── main.rs          # binary entrypoint, interactive loop, slash commands
│   ├── lib.rs           # re-exports modules for integration tests
│   ├── agent.rs
│   ├── api.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── context.rs
│   ├── diff.rs
│   ├── errors.rs
│   ├── history.rs
│   ├── permissions.rs
│   ├── tools.rs
│   ├── types.rs
│   ├── ui.rs
│   └── workspace.rs
└── tests/
    ├── api_tests.rs     # message/response (de)serialization
    ├── tool_tests.rs    # binary smoke tests (--help / --version)
    └── agent_tests.rs   # agent-loop tests against an in-process mock server
```

### Testing

Unit tests live next to their modules and cover path security, dangerous-command detection, sensitive-file detection, `edit_file` match rules, `run_command` exit-code handling, and permission gating. They run against temporary directories, so your real filesystem is never touched.

`tests/agent_tests.rs` starts a minimal in-process HTTP server that plays back scripted OpenAI-style tool-call responses, then asserts the agent produced real side effects (files actually created on disk, `tool_call_id`s preserved), not just that it said it did.

```bash
cargo test
```

## License

Distributed under the MIT License. See [LICENSE](LICENSE) for details.

## Author

**Saqlain Khan**
