use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "saqlain-code",
    version,
    about = "A terminal-based AI coding agent with real tool execution",
    long_about = "saqlain-code connects to an OpenAI-compatible local API server and lets an AI \
model inspect, understand, modify, create, test, and debug software projects directly from the \
terminal. The Rust application executes every tool; the model only decides what to do."
)]
pub struct Cli {
    /// A one-off task to run instead of entering interactive mode.
    pub task: Option<String>,

    /// Model name to use (overrides config/env).
    #[arg(long)]
    pub model: Option<String>,

    /// Base URL of the OpenAI-compatible API server.
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key to send (the local server usually accepts any value).
    #[arg(long)]
    pub api_key: Option<String>,

    /// Permission mode: auto, ask, or deny.
    #[arg(long)]
    pub permission_mode: Option<String>,

    /// Automatically accept safe file changes and commands (dangerous actions still ask).
    #[arg(long)]
    pub auto: bool,

    /// Enable verbose logging of HTTP requests, tool calls, and timings.
    #[arg(long)]
    pub verbose: bool,

    /// Maximum number of tool/LLM iterations per user request.
    #[arg(long)]
    pub max_iterations: Option<usize>,

    /// Maximum seconds a single shell command may run before it is killed
    /// (guards against commands silently waiting on interactive input).
    #[arg(long)]
    pub command_timeout_secs: Option<u64>,
}
