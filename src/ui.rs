use crossterm::style::Stylize;

pub fn print_banner(model: &str, workspace_name: &str) {
    let title = "Saqlain Code";
    let subtitle = "AI Coding Agent";
    let model_line = format!("Model: {model}");
    let ws_line = format!("Workspace: {workspace_name}");

    let width = [title.len(), subtitle.len(), model_line.len(), ws_line.len()]
        .into_iter()
        .max()
        .unwrap_or(0)
        + 4;

    println!("╭{}╮", "─".repeat(width));
    println!("│ {} │", pad(title, width - 2).bold());
    println!("│ {:<width$} │", subtitle, width = width - 2);
    println!("│ {:<width$} │", model_line, width = width - 2);
    println!("│ {:<width$} │", ws_line, width = width - 2);
    println!("╰{}╯", "─".repeat(width));
    println!();
}

fn pad(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

pub fn print_tool_call(name: &str, args: &str) {
    let icon = icon_for_tool(name);
    println!("{icon} {}", describe_tool_call(name, args));
}

pub fn print_tool_result(success: bool, summary: &str) {
    if success {
        println!("{} {}", "✓".green(), summary);
    } else {
        println!("{} {}", "✗".red(), summary);
    }
}

pub fn print_agent_text(text: &str) {
    println!("\n{text}\n");
}

#[allow(dead_code)]
pub fn print_info(text: &str) {
    println!("{}", text.dim());
}

pub fn print_error(text: &str) {
    println!("{} {}", "Error:".red().bold(), text);
}

fn icon_for_tool(name: &str) -> &'static str {
    match name {
        "list_files" => "🔍",
        "read_file" => "📖",
        "write_file" => "✏",
        "edit_file" => "✏",
        "delete_file" => "🗑",
        "search_code" => "🔎",
        "run_command" => "⚙",
        _ => "🔧",
    }
}

fn describe_tool_call(name: &str, args: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
    match name {
        "list_files" => format!("Listing files in {}", parsed.get("path").and_then(|v| v.as_str()).unwrap_or(".")),
        "read_file" => format!("Reading {}", parsed.get("path").and_then(|v| v.as_str()).unwrap_or("?")),
        "write_file" => format!("Writing {}", parsed.get("path").and_then(|v| v.as_str()).unwrap_or("?")),
        "edit_file" => format!("Editing {}", parsed.get("path").and_then(|v| v.as_str()).unwrap_or("?")),
        "delete_file" => format!("Deleting {}", parsed.get("path").and_then(|v| v.as_str()).unwrap_or("?")),
        "search_code" => format!("Searching for '{}'", parsed.get("query").and_then(|v| v.as_str()).unwrap_or("?")),
        "run_command" => format!("Running: {}", parsed.get("command").and_then(|v| v.as_str()).unwrap_or("?")),
        other => format!("Calling {other}"),
    }
}
