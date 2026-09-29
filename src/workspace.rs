use crate::errors::{AgentError, Result};
use std::path::{Component, Path, PathBuf};

/// Represents the project workspace root and provides safe path resolution.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Result<Self> {
        let root = root.canonicalize().map_err(AgentError::Io)?;
        Ok(Self { root })
    }

    pub fn current_dir() -> Result<Self> {
        let cwd = std::env::current_dir().map_err(AgentError::Io)?;
        Self::new(cwd)
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    /// Resolve a user/model-provided relative path against the workspace root,
    /// rejecting any path that would escape the workspace (via `..`, absolute
    /// paths pointing elsewhere, or symlink traversal).
    pub fn resolve(&self, requested: &str) -> Result<PathBuf> {
        let requested_path = Path::new(requested);

        // Reject absolute paths outright unless they are already inside the workspace.
        let candidate = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            self.root.join(requested_path)
        };

        // Lexically normalize (removing "." and resolving ".." components)
        // without touching the filesystem yet, so we can reject escapes even
        // for paths that don't exist yet (e.g. write_file targets).
        let mut normalized = PathBuf::new();
        for component in candidate.components() {
            match component {
                Component::ParentDir => {
                    if !normalized.pop() {
                        return Err(AgentError::PathOutsideWorkspace(requested.to_string()));
                    }
                }
                Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }

        if !normalized.starts_with(&self.root) {
            return Err(AgentError::PathOutsideWorkspace(requested.to_string()));
        }

        // If the path already exists, canonicalize to catch symlink escapes.
        if normalized.exists() {
            let canon = normalized.canonicalize().map_err(AgentError::Io)?;
            if !canon.starts_with(&self.root) {
                return Err(AgentError::PathOutsideWorkspace(requested.to_string()));
            }
            return Ok(canon);
        }

        Ok(normalized)
    }

    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Detects likely project type(s) by looking for manifest files at the root.
    pub fn detect_project_types(&self) -> Vec<&'static str> {
        let markers: &[(&str, &str)] = &[
            ("Cargo.toml", "Rust (Cargo)"),
            ("package.json", "Node.js"),
            ("CMakeLists.txt", "C/C++ (CMake)"),
            ("Makefile", "Make"),
            ("requirements.txt", "Python (pip)"),
            ("pyproject.toml", "Python (pyproject)"),
            ("pubspec.yaml", "Dart/Flutter"),
            ("go.mod", "Go"),
            ("pom.xml", "Java (Maven)"),
            ("build.gradle", "Java/Kotlin (Gradle)"),
        ];
        let mut found = Vec::new();
        for (file, label) in markers {
            if self.root.join(file).exists() {
                found.push(*label);
            }
        }
        // *.csproj glob check
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if entry.path().extension().map(|e| e == "csproj").unwrap_or(false) {
                    found.push("C# (.csproj)");
                    break;
                }
            }
        }
        found
    }

    pub fn is_git_repo(&self) -> bool {
        self.root.join(".git").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_relative_path_inside_workspace() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let resolved = ws.resolve("src/main.rs").unwrap();
        assert!(resolved.starts_with(&ws.root));
    }

    #[test]
    fn rejects_parent_dir_escape() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let result = ws.resolve("../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_absolute_path_outside_workspace() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let result = ws.resolve("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn allows_dot_slash_paths() {
        let dir = tempdir().unwrap();
        let ws = Workspace::new(dir.path().to_path_buf()).unwrap();
        let resolved = ws.resolve("./foo.txt").unwrap();
        assert_eq!(resolved, ws.root.join("foo.txt"));
    }
}
