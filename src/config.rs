use std::collections::HashSet;
use std::env;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Config {
    pub bot_token: String,
    pub lm_studio_url: String,
    pub api_token: Option<String>,
    pub mcp_servers: Vec<String>,
    pub default_model: Option<String>,
    pub default_system_prompt: String,
    pub max_history_turns: usize,
    pub default_temperature: f32,
    pub authorised_users: Option<HashSet<i64>>,
}

/// Parses user IDs from environment string.
/// Supports formats:
/// - Array format: `[123456789]` or `[123456789, 987654321]`
/// - JSON string array: `["123456789"]`
/// - Comma-separated: `123456789, 987654321`
/// - Single ID: `123456789`
pub fn parse_user_ids(input: &str) -> Option<HashSet<i64>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Attempt JSON array parsing first
    if let Ok(vec) = serde_json::from_str::<Vec<i64>>(trimmed) {
        let set: HashSet<i64> = vec.into_iter().collect();
        if !set.is_empty() {
            return Some(set);
        }
    }

    if let Ok(vec) = serde_json::from_str::<Vec<String>>(trimmed) {
        let set: HashSet<i64> = vec
            .into_iter()
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        if !set.is_empty() {
            return Some(set);
        }
    }

    // Strip square brackets and quotes if present
    let cleaned = trimmed
        .trim_start_matches('[')
        .trim_end_matches(']');

    let set: HashSet<i64> = cleaned
        .split(',')
        .filter_map(|item| {
            let item = item.trim().trim_matches('"').trim_matches('\'');
            item.parse::<i64>().ok()
        })
        .collect();

    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

/// Automatically detects MCP servers configured in ~/.lmstudio/mcp.json
fn auto_detect_mcp_servers() -> Vec<String> {
    let mut servers = Vec::new();
    if let Ok(home) = env::var("HOME") {
        let mcp_path = Path::new(&home).join(".lmstudio/mcp.json");
        if let Ok(content) = std::fs::read_to_string(mcp_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(map) = val.get("mcpServers").and_then(|s| s.as_object()) {
                    for key in map.keys() {
                        let id = if key.starts_with("mcp/") {
                            key.clone()
                        } else {
                            format!("mcp/{}", key)
                        };
                        servers.push(id);
                    }
                }
            }
        }
    }
    servers
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bot_token = env::var("TELOXIDE_TOKEN")
            .or_else(|_| env::var("TELEGRAM_BOT_TOKEN"))
            .map_err(|_| "Missing TELOXIDE_TOKEN or TELEGRAM_BOT_TOKEN environment variable".to_string())?;

        let mut lm_studio_url = env::var("LM_STUDIO_URL")
            .unwrap_or_else(|_| "http://localhost:1234".to_string());
        // Strip trailing slash if present
        if lm_studio_url.ends_with('/') {
            lm_studio_url.pop();
        }
        // Normalize: if user specified /v1 at the end, strip it so base URL is http://host:port
        if lm_studio_url.ends_with("/v1") {
            lm_studio_url.truncate(lm_studio_url.len() - 3);
        }

        let api_token = env::var("LM_STUDIO_API_TOKEN")
            .or_else(|_| env::var("LM_API_TOKEN"))
            .ok()
            .and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });

        let mcp_servers = match env::var("MCP_SERVERS") {
            Ok(val) => {
                let trimmed = val.trim();
                if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
                    Vec::new()
                } else {
                    trimmed
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .map(|s| {
                            let s = s.trim().trim_matches('"').trim_matches('\'').to_string();
                            if s.starts_with("mcp/") { s } else { format!("mcp/{}", s) }
                        })
                        .filter(|s| !s.is_empty() && s != "mcp/")
                        .collect()
                }
            }
            Err(_) => auto_detect_mcp_servers(),
        };

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

        // Supports AUTHORISED_USERS, AUTHORIZED_USERS, or ALLOWED_USERS
        let raw_users = env::var("AUTHORISED_USERS")
            .or_else(|_| env::var("AUTHORIZED_USERS"))
            .or_else(|_| env::var("ALLOWED_USERS"))
            .ok();

        let authorised_users = raw_users.and_then(|val| parse_user_ids(&val));

        Ok(Config {
            bot_token,
            lm_studio_url,
            api_token,
            mcp_servers,
            default_model,
            default_system_prompt,
            max_history_turns,
            default_temperature,
            authorised_users,
        })
    }

    pub fn is_user_allowed(&self, user_id: i64) -> bool {
        match &self.authorised_users {
            Some(authorised) => authorised.contains(&user_id),
            None => true,
        }
    }

    #[allow(dead_code)]
    pub fn is_user_authorised(&self, user_id: i64) -> bool {
        self.is_user_allowed(user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_ids_array_format() {
        let parsed = parse_user_ids("[123456789]").unwrap();
        assert!(parsed.contains(&123456789));
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn test_parse_user_ids_multiple_in_array() {
        let parsed = parse_user_ids("[123456789, 987654321]").unwrap();
        assert!(parsed.contains(&123456789));
        assert!(parsed.contains(&987654321));
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_parse_user_ids_comma_separated() {
        let parsed = parse_user_ids("123456789, 987654321").unwrap();
        assert!(parsed.contains(&123456789));
        assert!(parsed.contains(&987654321));
    }

    #[test]
    fn test_parse_user_ids_single() {
        let parsed = parse_user_ids("123456789").unwrap();
        assert!(parsed.contains(&123456789));
    }

    #[test]
    fn test_parse_user_ids_empty() {
        assert!(parse_user_ids("").is_none());
        assert!(parse_user_ids("   ").is_none());
        assert!(parse_user_ids("[]").is_none());
    }
}
