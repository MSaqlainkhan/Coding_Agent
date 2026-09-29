use similar::{ChangeTag, TextDiff};

/// Render a unified-style diff between old and new file contents for display
/// to the user before a change is applied.
pub fn render_diff(path: &str, old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    out.push_str(&"─".repeat(50));
    out.push('\n');
    out.push_str(&format!("File: {path}\n\n"));

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        out.push_str(&format!("{sign} {change}"));
    }

    out.push_str(&"─".repeat(50));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_added_lines() {
        let diff = render_diff("test.txt", "a\nb\n", "a\nb\nc\n");
        assert!(diff.contains("+ c"));
    }
}
