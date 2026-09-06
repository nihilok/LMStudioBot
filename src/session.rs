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
    pub history: Vec<ChatMessage>,
}

impl Default for ChatSession {
    fn default() -> Self {
        Self {
            active_model: None,
            custom_system_prompt: None,
            custom_temperature: None,
            last_response_id: None,
            history: Vec::new(),
        }
    }
}

impl ChatSession {
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.last_response_id = None;
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

    /// Records the user query, assistant response, and LM Studio response_id
    pub fn record_exchange(
        &mut self,
        user_text: &str,
        assistant_text: &str,
        response_id: Option<String>,
        max_turns: usize,
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

        let max_messages = max_turns * 2;
        if self.history.len() > max_messages {
            let excess = self.history.len() - max_messages;
            self.history.drain(0..excess);
        }
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
    fn test_session_history_trimming() {
        let mut session = ChatSession::default();
        let max_turns = 2; // max 4 messages

        session.record_exchange("u1", "a1", None, max_turns);
        session.record_exchange("u2", "a2", None, max_turns);
        assert_eq!(session.history.len(), 4);
        assert_eq!(session.history[0].content, "u1");

        // Adding 3rd exchange should drop u1, a1
        session.record_exchange("u3", "a3", Some("resp_123".to_string()), max_turns);
        assert_eq!(session.history.len(), 4);
        assert_eq!(session.history[0].content, "u2");
        assert_eq!(session.history[1].content, "a2");
        assert_eq!(session.history[2].content, "u3");
        assert_eq!(session.history[3].content, "a3");
        assert_eq!(session.last_response_id.as_deref(), Some("resp_123"));
    }

    #[test]
    fn test_clear_resets_response_id() {
        let mut session = ChatSession::default();
        session.record_exchange("u1", "a1", Some("resp_abc".to_string()), 5);
        assert_eq!(session.last_response_id.as_deref(), Some("resp_abc"));

        session.clear_history();
        assert!(session.last_response_id.is_none());
        assert_eq!(session.history.len(), 0);
    }
}
