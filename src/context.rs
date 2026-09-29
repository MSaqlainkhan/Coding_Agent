use crate::workspace::Workspace;
use std::process::{Command, Stdio};
use walkdir::WalkDir;

const DEFAULT_IGNORE_DIRS: &[&str] = &[
    ".git", "target", "node_modules", "dist", "build", ".cache", "venv",
];

/// Builds a lightweight project-context summary to give the model orientation
/// without dumping the entire project into the conversation.
pub struct ContextEngine<'a> {
    pub workspace: &'a Workspace,
}

impl<'a> ContextEngine<'a> {
    pub fn new(workspace: &'a Workspace) -> Self {
        Self { workspace }
    }

    pub fn summary(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("Workspace root: {}\n", self.workspace.root.display()));
        out.push_str(&format!("Project name: {}\n", self.workspace.name()));
        out.push_str(&format!("Operating system: {}\n", std::env::consts::OS));

        let project_types = self.workspace.detect_project_types();
        if !project_types.is_empty() {
            out.push_str(&format!("Detected project type(s): {}\n", project_types.join(", ")));
        } else {
            out.push_str("Detected project type(s): unknown\n");
        }

        out.push_str(&format!("Git repository: {}\n", self.workspace.is_git_repo()));
        if self.workspace.is_git_repo() {
            if let Some(status) = self.git_status_summary() {
                out.push_str(&format!("Git status:\n{status}\n"));
            }
        }

        out.push_str("\nTop-level entries:\n");
        for entry in self.top_level_entries() {
            out.push_str(&format!("  {entry}\n"));
        }

        if let Some(readme) = self.readme_excerpt() {
            out.push_str("\nREADME excerpt:\n");
            out.push_str(&readme);
            out.push('\n');
        }

        out
    }

    fn top_level_entries(&self) -> Vec<String> {
        let mut entries = Vec::new();
        for entry in WalkDir::new(&self.workspace.root).max_depth(1).into_iter().flatten() {
            if entry.path() == self.workspace.root {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if DEFAULT_IGNORE_DIRS.contains(&name.as_str()) {
                continue;
            }
            let suffix = if entry.file_type().is_dir() { "/" } else { "" };
            entries.push(format!("{name}{suffix}"));
        }
        entries.sort();
        entries
    }

    fn readme_excerpt(&self) -> Option<String> {
        for name in ["README.md", "README.txt", "README"] {
            let path = self.workspace.root.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let excerpt: String = content.lines().take(20).collect::<Vec<_>>().join("\n");
                return Some(excerpt);
            }
        }
        None
    }

    fn git_status_summary(&self) -> Option<String> {
        let output = Command::new("git")
            .arg("status")
            .arg("--short")
            .current_dir(&self.workspace.root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.trim().is_empty() {
            Some("(clean working tree)".to_string())
        } else {
            Some(text)
        }
    }
}
