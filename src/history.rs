use crate::types::ChatMessage;

/// Maintains conversation state for the session, preserving the relationship
/// between assistant tool calls and their tool results.
#[derive(Debug, Default)]
pub struct History {
    messages: Vec<ChatMessage>,
}

impl History {
    pub fn new() -> Self {
        Self { messages: Vec::new() }
    }

    pub fn push(&mut self, message: ChatMessage) {
        self.messages.push(message);
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Number of user/assistant turns recorded (excludes system/tool messages).
    #[allow(dead_code)]
    pub fn turn_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .count()
    }

    /// Basic compaction: drop all but the most recent `keep_last` messages,
    /// always preserving the leading system message if present.
    pub fn compact(&mut self, keep_last: usize) {
        if self.messages.len() <= keep_last {
            return;
        }
        let system_prefix: Vec<ChatMessage> = self
            .messages
            .iter()
            .take_while(|m| m.role == "system")
            .cloned()
            .collect();

        let tail_start = self.messages.len().saturating_sub(keep_last);
        let tail: Vec<ChatMessage> = self.messages[tail_start..].to_vec();

        let mut compacted = system_prefix;
        compacted.extend(tail);
        self.messages = compacted;
    }

    pub fn render_transcript(&self) -> String {
        let mut out = String::new();
        for m in &self.messages {
            match m.role.as_str() {
                "user" => out.push_str(&format!("You: {}\n", m.content.clone().unwrap_or_default())),
                "assistant" => {
                    if let Some(content) = &m.content {
                        out.push_str(&format!("Agent: {content}\n"));
                    }
                    if let Some(calls) = &m.tool_calls {
                        for c in calls {
                            out.push_str(&format!("  [tool_call] {}({})\n", c.function.name, c.function.arguments));
                        }
                    }
                }
                "tool" => out.push_str(&format!(
                    "  [tool_result:{}] {}\n",
                    m.name.clone().unwrap_or_default(),
                    truncate(m.content.as_deref().unwrap_or(""), 200)
                )),
                _ => {}
            }
        }
        out
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_keeps_system_and_recent_messages() {
        let mut h = History::new();
        h.push(ChatMessage::system("sys"));
        for i in 0..10 {
            h.push(ChatMessage::user(format!("msg {i}")));
        }
        h.compact(3);
        assert_eq!(h.messages()[0].role, "system");
        assert_eq!(h.messages().len(), 4); // system + 3 recent
    }

    #[test]
    fn turn_count_ignores_system_and_tool_messages() {
        let mut h = History::new();
        h.push(ChatMessage::system("sys"));
        h.push(ChatMessage::user("hi"));
        h.push(ChatMessage::assistant_text("hello"));
        h.push(ChatMessage::tool_result("id1", "read_file", "content"));
        assert_eq!(h.turn_count(), 2);
    }
}
