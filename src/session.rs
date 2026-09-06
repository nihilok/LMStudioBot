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
    pub history: Vec<ChatMessage>,
}

impl Default for ChatSession {
    fn default() -> Self {
        Self {
            active_model: None,
            custom_system_prompt: None,
            custom_temperature: None,
            history: Vec::new(),
        }
    }
}

impl ChatSession {
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.active_model = model;
    }

    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.custom_system_prompt = prompt;
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

    /// Prepares the full message list for LM Studio, including system prompt and recent history
    pub fn build_messages_for_prompt(&self, fallback_system: &str, user_text: &str) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        let system_prompt = self.get_effective_system_prompt(fallback_system);
        if !system_prompt.trim().is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            });
        }

        messages.extend(self.history.clone());

        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_text.to_string(),
        });

        messages
    }

    /// Records the user query and assistant response, trimming history to max turns
    pub fn record_exchange(&mut self, user_text: &str, assistant_text: &str, max_turns: usize) {
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

        session.record_exchange("u1", "a1", max_turns);
        session.record_exchange("u2", "a2", max_turns);
        assert_eq!(session.history.len(), 4);
        assert_eq!(session.history[0].content, "u1");

        // Adding 3rd exchange should drop u1, a1
        session.record_exchange("u3", "a3", max_turns);
        assert_eq!(session.history.len(), 4);
        assert_eq!(session.history[0].content, "u2");
        assert_eq!(session.history[1].content, "a2");
        assert_eq!(session.history[2].content, "u3");
        assert_eq!(session.history[3].content, "a3");
    }

    #[test]
    fn test_build_messages_for_prompt() {
        let mut session = ChatSession::default();
        session.record_exchange("Hi", "Hello", 5);

        let msgs = session.build_messages_for_prompt("System prompt", "How are you?");
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "System prompt");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "Hi");
        assert_eq!(msgs[2].role, "assistant");
        assert_eq!(msgs[2].content, "Hello");
        assert_eq!(msgs[3].role, "user");
        assert_eq!(msgs[3].content, "How are you?");
    }

    #[test]
    fn test_custom_system_prompt_override() {
        let mut session = ChatSession::default();
        session.set_system_prompt(Some("You are a pirate.".to_string()));

        let msgs = session.build_messages_for_prompt("Default prompt", "Ahoy");
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "You are a pirate.");
    }
}
