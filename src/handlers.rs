use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::formatting::{send_safe_message, spawn_typing_indicator, split_message};
use crate::lmstudio::LmStudioClient;
use crate::session::SessionStore;

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "Start the bot and get an overview.")]
    Start,
    #[command(description = "Show this help message.")]
    Help,
    #[command(description = "List available models in LM Studio.")]
    Models,
    #[command(description = "Show or switch active model: /model [model_name]")]
    Model(String),
    #[command(description = "Show or set system prompt: /system [prompt]")]
    System(String),
    #[command(description = "Set generation temperature (0.0 - 2.0): /temperature [value]")]
    Temperature(String),
    #[command(description = "List active MCP server integrations.")]
    Mcp,
    #[command(description = "View the current compacted rolling summary.")]
    Summary,
    #[command(description = "Clear current conversation memory.")]
    Clear,
    #[command(description = "Clear current conversation memory (alias for /clear).")]
    Reset,
    #[command(description = "Show current chat settings and LM Studio status.")]
    Status,
}

fn check_auth(user: Option<&teloxide::types::User>, config: &Config) -> bool {
    match user {
        Some(u) => {
            let allowed = config.is_user_allowed(u.id.0 as i64);
            if !allowed {
                warn!(
                    user_id = u.id.0,
                    username = ?u.username,
                    "Unauthorized access attempt rejected"
                );
            }
            allowed
        }
        None => config.authorised_users.is_none(),
    }
}

fn unauthorized_reply(user: Option<&teloxide::types::User>) -> String {
    match user {
        Some(u) => format!(
            "⛔ You are not authorised to use this bot.\nYour Telegram User ID is: `{}`",
            u.id.0
        ),
        None => "⛔ You are not authorised to use this bot.".to_string(),
    }
}

pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    config: Arc<Config>,
    client: LmStudioClient,
    sessions: SessionStore,
) -> ResponseResult<()> {
    if !check_auth(msg.from.as_ref(), &config) {
        send_safe_message(&bot, msg.chat.id, &unauthorized_reply(msg.from.as_ref())).await?;
        return Ok(());
    }

    let chat_id = msg.chat.id;

    match cmd {
        Command::Start => {
            let mcp_info = if client.mcp_servers().is_empty() {
                "None active".to_string()
            } else {
                client.mcp_servers().join(", ")
            };

            let welcome = format!(
                "👋 *Welcome to LM Studio Telegram Bot!*\n\n\
                I am connected to your local LM Studio instance at `{}`.\n\n\
                *Active MCP Integrations:* `{}`\n\n\
                *Quick Guide:*\n\
                • Simply send me a text message to chat with your local LLM.\n\
                • Use /models to see models currently loaded or available in LM Studio.\n\
                • Use /model <name> to switch models for this chat.\n\
                • Use /mcp to see active MCP tools and servers.\n\
                • Use /summary to see your rolling conversation compaction summary.\n\
                • Use /clear or /reset to wipe the conversation history and start fresh.\n\
                • Use /status to view current session settings.\n\
                • Use /help to see all available commands.",
                client.base_url(),
                mcp_info
            );
            send_safe_message(&bot, chat_id, &welcome).await?;
        }

        Command::Help => {
            let help_text = format!(
                "🤖 *LM Studio Bot Commands*\n\n\
                {}\n\n\
                *Tips:*\n\
                - To reset your system prompt to default, run `/system default`\n\
                - To reset model selection to default, run `/model auto`\n\
                - Automatic compaction condenses your conversation every {} turns into a durable summary.",
                Command::descriptions(),
                config.max_history_turns
            );
            send_safe_message(&bot, chat_id, &help_text).await?;
        }

        Command::Models => {
            let typing = spawn_typing_indicator(bot.clone(), chat_id);
            let result = client.fetch_models().await;
            typing.abort();

            match result {
                Ok(models) if models.is_empty() => {
                    let text = "⚠️ LM Studio is running, but no models were reported as loaded.\n\
                        Please open LM Studio, load a model in the *Local Server* tab, and try again.";
                    send_safe_message(&bot, chat_id, text).await?;
                }
                Ok(models) => {
                    let mut text = "🤖 *Available Models in LM Studio:*\n\n".to_string();
                    for m in &models {
                        text.push_str(&format!("• `{}`\n", m));
                    }
                    text.push_str("\nTo select a model, type:\n`/model <model-name>`");
                    send_safe_message(&bot, chat_id, &text).await?;
                }
                Err(err) => {
                    let text = format!("❌ *Failed to fetch models from LM Studio:*\n\n{}", err);
                    send_safe_message(&bot, chat_id, &text).await?;
                }
            }
        }

        Command::Model(arg) => {
            let trimmed = arg.trim();
            let mut store = sessions.write().await;
            let session = store.entry(chat_id).or_default();

            if trimmed.is_empty() {
                let current = session.get_effective_model(config.default_model.as_ref());
                let text = match current {
                    Some(m) => format!("🤖 Current active model for this chat: `{}`\n\nTo change it, use `/model <name>` or `/models` to list options.", m),
                    None => "⚠️ No model is currently configured. Use `/models` to view available models or `/model <name>` to set one.".to_string(),
                };
                send_safe_message(&bot, chat_id, &text).await?;
            } else if trimmed.eq_ignore_ascii_case("auto") || trimmed.eq_ignore_ascii_case("default") {
                session.set_model(None);
                let fallback = config.default_model.as_deref().unwrap_or("auto-detect");
                let text = format!("🔄 Model reset to default/auto (`{}`).", fallback);
                send_safe_message(&bot, chat_id, &text).await?;
            } else {
                session.set_model(Some(trimmed.to_string()));
                let text = format!("✅ Active model set to: `{}`", trimmed);
                send_safe_message(&bot, chat_id, &text).await?;
            }
        }

        Command::System(arg) => {
            let trimmed = arg.trim();
            let mut store = sessions.write().await;
            let session = store.entry(chat_id).or_default();

            if trimmed.is_empty() {
                let current = session.get_effective_system_prompt(&config.default_system_prompt);
                let text = format!(
                    "⚙️ *Current System Prompt:*\n\n```\n{}\n```\n\nTo change it, use `/system <your prompt>` or `/system default` to reset.",
                    current
                );
                send_safe_message(&bot, chat_id, &text).await?;
            } else if trimmed.eq_ignore_ascii_case("default") || trimmed.eq_ignore_ascii_case("reset") {
                session.set_system_prompt(None);
                let text = format!(
                    "🔄 System prompt reset to default:\n\n```\n{}\n```",
                    config.default_system_prompt
                );
                send_safe_message(&bot, chat_id, &text).await?;
            } else {
                session.set_system_prompt(Some(trimmed.to_string()));
                let text = format!("✅ Custom system prompt updated:\n\n```\n{}\n```", trimmed);
                send_safe_message(&bot, chat_id, &text).await?;
            }
        }

        Command::Temperature(arg) => {
            let trimmed = arg.trim();
            let mut store = sessions.write().await;
            let session = store.entry(chat_id).or_default();

            if trimmed.is_empty() {
                let current = session.get_effective_temperature(config.default_temperature);
                let text = format!(
                    "🌡️ Current temperature: `{:.2}` (range: 0.0 - 2.0)\n\nTo change it, use `/temperature <0.0 - 2.0>` or `/temperature default`.",
                    current
                );
                send_safe_message(&bot, chat_id, &text).await?;
            } else if trimmed.eq_ignore_ascii_case("default") || trimmed.eq_ignore_ascii_case("reset") {
                session.set_temperature(None);
                let text = format!("🔄 Temperature reset to default: `{:.2}`", config.default_temperature);
                send_safe_message(&bot, chat_id, &text).await?;
            } else {
                match trimmed.parse::<f32>() {
                    Ok(val) if (0.0..=2.0).contains(&val) => {
                        session.set_temperature(Some(val));
                        let text = format!("✅ Temperature updated to: `{:.2}`", val);
                        send_safe_message(&bot, chat_id, &text).await?;
                    }
                    _ => {
                        let text = "❌ Invalid temperature. Please enter a decimal number between 0.0 (deterministic) and 2.0 (creative). Example: `/temperature 0.7`";
                        send_safe_message(&bot, chat_id, text).await?;
                    }
                }
            }
        }

        Command::Mcp => {
            let servers = client.mcp_servers();
            let text = if servers.is_empty() {
                "ℹ️ *No MCP servers currently active.*\n\n\
                To enable MCP tools, ensure they are configured in `~/.lmstudio/mcp.json` \
                or set `MCP_SERVERS=mcp/<name>` in your `.env` file.".to_string()
            } else {
                let mut s = "🛠️ *Active MCP Servers Attached:*\n\n".to_string();
                for srv in servers {
                    s.push_str(&format!("• `{}`\n", srv));
                }
                s.push_str("\nThese MCP servers are attached to model queries and available for tool calling.");
                s
            };
            send_safe_message(&bot, chat_id, &text).await?;
        }

        Command::Summary => {
            let store = sessions.read().await;
            let session = store.get(&chat_id);
            let text = match session.and_then(|s| s.rolling_summary.as_ref()) {
                Some(summary) if !summary.trim().is_empty() => {
                    format!("🧠 *Current Rolling Conversation Summary:*\n\n{}", summary)
                }
                _ => format!(
                    "ℹ️ No conversation summary has been generated yet for this chat.\n\n\
                    Summaries are automatically created once conversation reaches {} turns to preserve context indefinitely.",
                    config.max_history_turns
                ),
            };
            send_safe_message(&bot, chat_id, &text).await?;
        }

        Command::Clear | Command::Reset => {
            let mut store = sessions.write().await;
            let session = store.entry(chat_id).or_default();
            let count = session.history.len();
            session.clear_history();

            let text = format!(
                "🧹 *Conversation cleared!*\nRemoved {} previous message(s) and cleared compaction memory. Starting fresh.",
                count
            );
            send_safe_message(&bot, chat_id, &text).await?;
        }

        Command::Status => {
            let store = sessions.read().await;
            let session = store.get(&chat_id);

            let model = session
                .and_then(|s| s.active_model.clone())
                .or_else(|| config.default_model.clone())
                .unwrap_or_else(|| "Auto-detect from LM Studio".to_string());

            let history_len = session.map(|s| s.history.len()).unwrap_or(0);
            let temp = session
                .and_then(|s| s.custom_temperature)
                .unwrap_or(config.default_temperature);

            // Test ping to LM Studio
            let lm_status = match client.fetch_models().await {
                Ok(models) => format!("🟢 Connected ({} model(s) available)", models.len()),
                Err(_) => "🔴 Offline or Unreachable".to_string(),
            };

            let mcp_info = if client.mcp_servers().is_empty() {
                "None".to_string()
            } else {
                client.mcp_servers().join(", ")
            };

            let summary_status = match session.and_then(|s| s.rolling_summary.as_ref()) {
                Some(s) => format!("Active ({} chars)", s.len()),
                None => "None".to_string(),
            };

            let status_msg = format!(
                "📊 *Current Chat Status:*\n\n\
                • *LM Studio URL:* `{}`\n\
                • *Server Status:* {}\n\
                • *Active Model:* `{}`\n\
                • *Temperature:* `{:.2}`\n\
                • *MCP Servers:* `{}`\n\
                • *History Length:* `{}` message(s) (Compacts at {})\n\
                • *Rolling Summary:* `{}`\n\
                • *System Prompt:* {}\n",
                client.base_url(),
                lm_status,
                model,
                temp,
                mcp_info,
                history_len,
                config.max_history_turns * 2,
                summary_status,
                if session.and_then(|s| s.custom_system_prompt.as_ref()).is_some() {
                    "Custom"
                } else {
                    "Default"
                }
            );
            send_safe_message(&bot, chat_id, &status_msg).await?;
        }
    }

    Ok(())
}

pub async fn handle_message(
    bot: Bot,
    msg: Message,
    config: Arc<Config>,
    client: LmStudioClient,
    sessions: SessionStore,
) -> ResponseResult<()> {
    if !check_auth(msg.from.as_ref(), &config) {
        send_safe_message(&bot, msg.chat.id, &unauthorized_reply(msg.from.as_ref())).await?;
        return Ok(());
    }

    let text = match msg.text() {
        Some(t) => t.trim(),
        None => return Ok(()),
    };

    // Ignore commands (they are processed by command handler)
    if text.starts_with('/') {
        return Ok(());
    }

    let chat_id = msg.chat.id;

    // Resolve target model
    let target_model = {
        let store = sessions.read().await;
        let session = store.get(&chat_id);
        session
            .and_then(|s| s.active_model.clone())
            .or_else(|| config.default_model.clone())
    };

    let target_model = match target_model {
        Some(m) => m,
        None => {
            // Auto-detect first model from LM Studio
            match client.fetch_models().await {
                Ok(models) if !models.is_empty() => models[0].clone(),
                Ok(_) => {
                    let err_msg = "⚠️ No model is loaded in LM Studio.\n\
                        Please load a model in LM Studio or choose one using `/model <name>`.";
                    send_safe_message(&bot, chat_id, err_msg).await?;
                    return Ok(());
                }
                Err(err) => {
                    let err_msg = format!("❌ Could not connect to LM Studio to detect model:\n\n{}", err);
                    send_safe_message(&bot, chat_id, &err_msg).await?;
                    return Ok(());
                }
            }
        }
    };

    // Check if session needs compaction (history reached turn threshold)
    let needs_compaction = {
        let store = sessions.read().await;
        store
            .get(&chat_id)
            .map(|s| s.needs_compaction(config.max_history_turns))
            .unwrap_or(false)
    };

    if needs_compaction {
        let (history_text, prev_summary) = {
            let store = sessions.read().await;
            let session = store.get(&chat_id).unwrap();
            (
                session.format_history_for_summary(),
                session.rolling_summary.clone(),
            )
        };

        info!(
            chat_id = %chat_id,
            "Compacting conversation history into rolling summary..."
        );
        let typing_compaction = spawn_typing_indicator(bot.clone(), chat_id);
        let summary_result = client
            .generate_summary(&target_model, &history_text, prev_summary.as_deref())
            .await;
        typing_compaction.abort();

        match summary_result {
            Ok(new_summary) => {
                info!(
                    chat_id = %chat_id,
                    "Conversation compaction succeeded. Rolling summary updated."
                );
                let mut store = sessions.write().await;
                if let Some(session) = store.get_mut(&chat_id) {
                    session.apply_compaction(new_summary);
                }
            }
            Err(e) => {
                warn!(
                    chat_id = %chat_id,
                    "Compaction summary failed: {}. Continuing without compaction.",
                    e
                );
            }
        }
    }

    // Resolve system prompt, temperature, and previous_response_id
    let (temp, system_prompt, prev_response_id) = {
        let store = sessions.read().await;
        let session = store.get(&chat_id);

        let temp = session
            .map(|s| s.get_effective_temperature(config.default_temperature))
            .unwrap_or(config.default_temperature);

        let sys = match session {
            Some(s) => s.get_system_prompt_with_summary(&config.default_system_prompt),
            None => config.default_system_prompt.clone(),
        };

        let prev_id = session.and_then(|s| s.last_response_id.clone());

        (temp, sys, prev_id)
    };

    info!(
        chat_id = %chat_id,
        model = %target_model,
        has_prev_id = prev_response_id.is_some(),
        "Sending prompt to LM Studio..."
    );

    // Start typing indicator loop
    let typing_task = spawn_typing_indicator(bot.clone(), chat_id);

    let chat_result = client
        .chat(
            &target_model,
            text,
            Some(&system_prompt),
            prev_response_id.as_deref(),
            temp,
        )
        .await;

    // Stop typing indicator
    typing_task.abort();

    match chat_result {
        Ok((reply_content, new_response_id, usage)) => {
            // Save turn in session history
            {
                let mut store = sessions.write().await;
                let session = store.entry(chat_id).or_default();
                session.record_exchange(text, &reply_content, new_response_id);
            }

            if let Some(u) = usage {
                info!(
                    prompt_tokens = ?u.prompt_tokens,
                    completion_tokens = ?u.completion_tokens,
                    total_tokens = ?u.total_tokens,
                    "LM Studio generation completed"
                );
            }

            // Split into safe chunks and send
            let chunks = split_message(&reply_content);
            for chunk in chunks {
                if let Err(e) = send_safe_message(&bot, chat_id, &chunk).await {
                    error!("Failed to send reply to Telegram chat {}: {}", chat_id, e);
                }
            }
        }
        Err(err) => {
            error!("LM Studio request failed: {}", err);
            let error_text = format!("❌ *Error generating response:*\n\n{}", err);
            send_safe_message(&bot, chat_id, &error_text).await?;
        }
    }

    Ok(())
}
