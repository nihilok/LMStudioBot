use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ParseMode};
use tokio::task::JoinHandle;
use tracing::warn;

const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4000;

/// Splits a long text message into chunks safe for Telegram's character limits.
pub fn split_message(text: &str) -> Vec<String> {
    if text.len() <= TELEGRAM_MAX_MESSAGE_LENGTH {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= TELEGRAM_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        // Search for a suitable split point within the first TELEGRAM_MAX_MESSAGE_LENGTH bytes
        let slice = &remaining[..TELEGRAM_MAX_MESSAGE_LENGTH];

        // 1. Try double newline (paragraph boundary)
        let split_pos = if let Some(pos) = slice.rfind("\n\n") {
            pos + 2
        // 2. Try single newline
        } else if let Some(pos) = slice.rfind('\n') {
            pos + 1
        // 3. Try space (word boundary)
        } else if let Some(pos) = slice.rfind(' ') {
            pos + 1
        // 4. Fallback to valid UTF-8 char boundary
        } else {
            let mut cut = TELEGRAM_MAX_MESSAGE_LENGTH;
            while !remaining.is_char_boundary(cut) && cut > 0 {
                cut -= 1;
            }
            cut
        };

        let (chunk, rest) = remaining.split_at(split_pos);
        chunks.push(chunk.to_string());
        remaining = rest;
    }

    chunks
}

/// Attempts to send a message chunk using Markdown.
/// If Telegram returns a parse error (common with raw LLM markdown),
/// it falls back to sending as plain text.
pub async fn send_safe_message(bot: &Bot, chat_id: ChatId, text: &str) -> Result<Message, teloxide::RequestError> {
    #[allow(deprecated)]
    let parse_mode = ParseMode::Markdown;

    match bot
        .send_message(chat_id, text)
        .parse_mode(parse_mode)
        .await
    {
        Ok(msg) => Ok(msg),
        Err(err) => {
            warn!(
                "Failed to send with Markdown formatting ({}). Falling back to plain text.",
                err
            );
            bot.send_message(chat_id, text).await
        }
    }
}

/// Spawns a background task that sends "typing..." action periodically until aborted.
pub fn spawn_typing_indicator(bot: Bot, chat_id: ChatId) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_message() {
        let short = "Hello, world!";
        let chunks = split_message(short);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], short);
    }

    #[test]
    fn test_split_long_message_with_paragraphs() {
        let paragraph = "A".repeat(2500);
        let text = format!("{}\n\n{}", paragraph, paragraph);
        let chunks = split_message(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], format!("{}\n\n", paragraph));
        assert_eq!(chunks[1], paragraph);
    }

    #[test]
    fn test_split_message_utf8_boundary() {
        // Multi-byte character 🦀 (4 bytes)
        let crab = "🦀".repeat(1200); // 4800 bytes
        let chunks = split_message(&crab);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= TELEGRAM_MAX_MESSAGE_LENGTH);
            // Verify valid utf8
            assert!(std::str::from_utf8(chunk.as_bytes()).is_ok());
        }
    }
}
