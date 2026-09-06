use std::collections::HashSet;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub bot_token: String,
    pub lm_studio_url: String,
    pub default_model: Option<String>,
    pub default_system_prompt: String,
    pub max_history_turns: usize,
    pub default_temperature: f32,
    pub allowed_users: Option<HashSet<i64>>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bot_token = env::var("TELOXIDE_TOKEN")
            .or_else(|_| env::var("TELEGRAM_BOT_TOKEN"))
            .map_err(|_| "Missing TELOXIDE_TOKEN or TELEGRAM_BOT_TOKEN environment variable".to_string())?;

        let mut lm_studio_url = env::var("LM_STUDIO_URL")
            .unwrap_or_else(|_| "http://localhost:1234/v1".to_string());
        // Strip trailing slash if present
        if lm_studio_url.ends_with('/') {
            lm_studio_url.pop();
        }

        let default_model = env::var("LM_STUDIO_MODEL")
            .ok()
            .and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });

        let default_system_prompt = env::var("SYSTEM_PROMPT")
            .unwrap_or_else(|_| "You are a helpful, knowledgeable, and friendly AI assistant.".to_string());

        let max_history_turns = env::var("MAX_HISTORY_TURNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10); // 10 turns = 10 user + 10 assistant messages

        let default_temperature = env::var("TEMPERATURE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.7);

        let allowed_users = env::var("ALLOWED_USERS")
            .ok()
            .and_then(|s| {
                let s = s.trim();
                if s.is_empty() {
                    None
                } else {
                    let set: HashSet<i64> = s
                        .split(',')
                        .filter_map(|id_str| id_str.trim().parse::<i64>().ok())
                        .collect();
                    if set.is_empty() { None } else { Some(set) }
                }
            });

        Ok(Config {
            bot_token,
            lm_studio_url,
            default_model,
            default_system_prompt,
            max_history_turns,
            default_temperature,
            allowed_users,
        })
    }

    pub fn is_user_allowed(&self, user_id: i64) -> bool {
        match &self.allowed_users {
            Some(allowed) => allowed.contains(&user_id),
            None => true,
        }
    }
}
