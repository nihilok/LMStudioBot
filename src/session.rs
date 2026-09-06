use std::collections::HashMap;
use std::sync::Arc;
use teloxide::types::ChatId;
use tokio::sync::RwLock;

use crate::lmstudio::ChatMessage;

#[derive(Clone, Debug)]
pub struct ChatSession {
    pub active_model: Option<String>,
    pub custom_system_prompt: Option<String>,
    pub custom_temperature: Option<f32>,
    pub last_response_id: Option<String>,
    pub rolling_summary: Option<String>,
    pub history: Vec<ChatMessage>,
}

impl Default for ChatSession {
    fn default() -> Self {
        Self {
            active_model: None,
            custom_system_prompt: None,
            custom_temperature: None,
            last_response_id: None,
            rolling_summary: None,
            history: Vec::new(),
        }
    }
}

impl ChatSession {
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.last_response_id = None;
        self.rolling_summary = None;
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.active_model = model;
        self.last_response_id = None;
    }

    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.custom_system_prompt = prompt;
        self.last_response_id = None;
    }

    pub fn set_temperature(&mut self, temp: Option<f32>) {
        self.custom_temperature = temp;
    }

    pub fn get_effective_model<'a>(&'a self, fallback: Option<&'a String>) -> Option<&'a str> {
        self.active_model
            .as_deref()
            .or_else(|| fallback.map(|s| s.as_str()))
    }

    pub fn get_effective_temperature(&self, fallback: f32) -> f32 {
        self.custom_temperature.unwrap_or(fallback)
    }

    pub fn get_effective_system_prompt<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.custom_system_prompt
            .as_deref()
            .unwrap_or(fallback)
    }

    /// Combines base system prompt with any existing rolling summary
    pub fn get_system_prompt_with_summary(&self, fallback_system: &str) -> String {
        let base = self.get_effective_system_prompt(fallback_system);
        match &self.rolling_summary {
            Some(summary) if !summary.trim().is_empty() => {
                format!(
                    "{}\n\n[Conversation Memory & Summary So Far]\n{}",
                    base, summary.trim()
                )
            }
            _ => base.to_string(),
        }
    }

    /// Checks if conversation history has exceeded the turn limit for compaction
    pub fn needs_compaction(&self, max_turns: usize) -> bool {
        self.history.len() >= max_turns * 2
    }

    /// Formats history for the summarizer prompt
    pub fn format_history_for_summary(&self) -> String {
        let mut out = String::new();
        for msg in &self.history {
            let role_label = if msg.role == "user" { "User" } else { "Assistant" };
            out.push_str(&format!("{}: {}\n\n", role_label, msg.content));
        }
        out
    }

    /// Applies newly generated summary, clears verbatim history, and resets response_id
    /// so the next turn starts a fresh branch in LM Studio with the compacted context.
    pub fn apply_compaction(&mut self, new_summary: String) {
        self.rolling_summary = Some(new_summary);
        self.history.clear();
        self.last_response_id = None;
    }

    /// Records the user query, assistant response, and LM Studio response_id
    pub fn record_exchange(
        &mut self,
        user_text: &str,
        assistant_text: &str,
        response_id: Option<String>,
    ) {
        self.last_response_id = response_id;

        self.history.push(ChatMessage {
            role: "user".to_string(),
            content: user_text.to_string(),
        });
        self.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: assistant_text.to_string(),
        });
    }
}

pub type SessionStore = Arc<RwLock<HashMap<ChatId, ChatSession>>>;

pub fn create_session_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compaction_trigger_and_apply() {
        let mut session = ChatSession::default();
        let max_turns = 3; // 6 messages

        session.record_exchange("u1", "a1", Some("resp_1".to_string()));
        session.record_exchange("u2", "a2", Some("resp_2".to_string()));
        assert!(!session.needs_compaction(max_turns));

        session.record_exchange("u3", "a3", Some("resp_3".to_string()));
        assert!(session.needs_compaction(max_turns));
        assert_eq!(session.history.len(), 6);

        let formatted = session.format_history_for_summary();
        assert!(formatted.contains("User: u1"));
        assert!(formatted.contains("Assistant: a3"));

        session.apply_compaction("- User discussed u1, u2, u3.".to_string());
        assert_eq!(session.history.len(), 0);
        assert!(session.last_response_id.is_none());
        assert_eq!(
            session.rolling_summary.as_deref(),
            Some("- User discussed u1, u2, u3.")
        );

        let full_prompt = session.get_system_prompt_with_summary("Base prompt");
        assert!(full_prompt.contains("Base prompt"));
        assert!(full_prompt.contains("[Conversation Memory & Summary So Far]"));
        assert!(full_prompt.contains("- User discussed u1, u2, u3."));
    }

    #[test]
    fn test_clear_resets_everything() {
        let mut session = ChatSession::default();
        session.record_exchange("u1", "a1", Some("resp_abc".to_string()));
        session.rolling_summary = Some("Old summary".to_string());

        session.clear_history();
        assert!(session.last_response_id.is_none());
        assert!(session.rolling_summary.is_none());
        assert_eq!(session.history.len(), 0);
    }
}
