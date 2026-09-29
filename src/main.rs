use saqlain_code::agent::{Agent, StepOutcome};
use saqlain_code::cli::Cli;
use saqlain_code::config::Config;
use saqlain_code::context;
use saqlain_code::errors::Result;
use saqlain_code::ui;
use saqlain_code::workspace::Workspace;

use clap::Parser;
use std::io::{self, Write};
use std::process::{Command, Stdio};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        ui::print_error(&e.to_string());
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let filter = if cli.verbose { "debug" } else { "warn" };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .try_init();

    let config = Config::resolve(
        cli.base_url.clone(),
        cli.model.clone(),
        cli.api_key.clone(),
        cli.permission_mode.clone(),
        cli.max_iterations,
        cli.command_timeout_secs,
        cli.auto,
        cli.verbose,
    )?;

    let workspace = Workspace::current_dir()?;
    let mut agent = Agent::new(&config, workspace);

    match cli.task {
        Some(task) => {
            run_single_task(&mut agent, &task).await;
        }
        None => {
            run_interactive(&mut agent).await;
        }
    }

    Ok(())
}

async fn run_single_task(agent: &mut Agent, task: &str) {
    println!("> {task}\n");
    match agent.handle_user_message(task).await {
        Ok(StepOutcome::Final(text)) => ui::print_agent_text(&text),
        Ok(StepOutcome::MaxIterations) => {
            println!("\nAgent stopped because maximum iterations were reached.");
        }
        Ok(StepOutcome::Cancelled) => println!("\nCancelled."),
        Err(e) => ui::print_error(&e.to_string()),
    }
}

async fn run_interactive(agent: &mut Agent) {
    ui::print_banner(agent.model(), &agent.workspace().name());

    loop {
        print!("> ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if let Some(command) = input.strip_prefix('/') {
            if handle_slash_command(agent, command) {
                break;
            }
            continue;
        }

        match agent.handle_user_message(input).await {
            Ok(StepOutcome::Final(text)) => ui::print_agent_text(&text),
            Ok(StepOutcome::MaxIterations) => {
                println!("\nAgent stopped because maximum iterations were reached.");
                print!("Continue? [y/N] ");
                let _ = io::stdout().flush();
                let mut answer = String::new();
                let _ = io::stdin().read_line(&mut answer);
                if answer.trim().eq_ignore_ascii_case("y") {
                    // A subsequent empty nudge lets the agent keep going.
                    if let Ok(StepOutcome::Final(text)) =
                        agent.handle_user_message("Please continue.").await
                    {
                        ui::print_agent_text(&text);
                    }
                }
            }
            Ok(StepOutcome::Cancelled) => println!("\nCancelled."),
            Err(e) => ui::print_error(&e.to_string()),
        }
    }
}

/// Returns true if the interactive loop should exit.
fn handle_slash_command(agent: &mut Agent, command: &str) -> bool {
    let mut parts = command.trim().splitn(2, ' ');
    let name = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim();

    match name.as_str() {
        "quit" | "exit" | "q" => {
            println!("Goodbye.");
            return true;
        }
        "help" => print_help(),
        "status" => print_status(agent),
        "model" => {
            if arg.is_empty() {
                println!("Current model: {}", agent.model());
            } else {
                agent.set_model(arg.to_string());
                println!("Model set to: {arg}");
            }
        }
        "models" => print_known_models(),
        "clear" | "reset" => {
            agent.history_mut().clear();
            println!("Conversation history cleared.");
        }
        "history" => print!("{}", agent.history().render_transcript()),
        "diff" => print_git_diff(agent),
        "files" => print_top_level_files(agent),
        "context" => print_context(agent),
        "permissions" => println!("Permission mode: {:?}", agent.permission_mode()),
        "compact" => {
            agent.history_mut().compact(20);
            println!("History compacted.");
        }
        "" => {}
        other => println!("Unknown command: /{other}. Try /help."),
    }

    false
}

fn print_help() {
    println!(
        "\nAvailable commands:\n\
         /help          Show this help message\n\
         /status        Show model, base URL, and workspace info\n\
         /model [name]  Show or set the active model\n\
         /models        List known model names\n\
         /clear         Clear conversation history\n\
         /reset         Alias for /clear\n\
         /history       Show the conversation transcript\n\
         /diff          Show `git diff` for the workspace\n\
         /files         List top-level files in the workspace\n\
         /context       Show the project context sent to the model\n\
         /permissions   Show the current permission mode\n\
         /compact       Compact conversation history to save tokens\n\
         /quit          Exit saqlain-code\n"
    );
}

fn print_status(agent: &Agent) {
    println!(
        "\nModel:\n{}\n\nBase URL:\n{}\n\nWorkspace:\n{}\n",
        agent.model(),
        agent.base_url(),
        agent.workspace().root.display()
    );
}

fn print_known_models() {
    println!(
        "\nKnown models:\n  gemini-3.7-flash\n  gemini-3.6-flash\n  gemini-3.5-flash\n  \
         gemini-3.5-flash-thinking\n  gemini-3.1-pro\n  gemini-auto\n  \
         gemini-3.5-flash-thinking-lite\n  gemini-flash-lite\n"
    );
}

fn print_git_diff(agent: &Agent) {
    if !agent.workspace().is_git_repo() {
        println!("Not a git repository.");
        return;
    }
    let output = Command::new("git")
        .arg("diff")
        .current_dir(&agent.workspace().root)
        .stdout(Stdio::piped())
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.trim().is_empty() {
                println!("(no changes)");
            } else {
                println!("{text}");
            }
        }
        Err(e) => ui::print_error(&format!("failed to run git diff: {e}")),
    }
}

fn print_top_level_files(agent: &Agent) {
    if let Ok(entries) = std::fs::read_dir(&agent.workspace().root) {
        for entry in entries.flatten() {
            println!("{}", entry.file_name().to_string_lossy());
        }
    }
}

fn print_context(agent: &Agent) {
    let ctx = context::ContextEngine::new(agent.workspace());
    println!("{}", ctx.summary());
}
