mod config;
mod formatting;
mod handlers;
mod lmstudio;
mod session;

use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::handlers::{handle_command, handle_message, Command};
use crate::lmstudio::LmStudioClient;
use crate::session::create_session_store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Initialize tracing / logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting LM Studio Telegram Bot...");

    // Load configuration
    let config = match Config::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Configuration Error: {}", e);
            eprintln!("\nPlease make sure you set the required environment variables.");
            eprintln!("You can create a .env file based on .env.example.");
            std::process::exit(1);
        }
    };

    let client = LmStudioClient::new(config.lm_studio_url.clone());
    let sessions = create_session_store();

    // Test connectivity to LM Studio
    info!("Testing connection to LM Studio at {}...", config.lm_studio_url);
    match client.fetch_models().await {
        Ok(models) => {
            if models.is_empty() {
                warn!(
                    "Connected to LM Studio, but no models are loaded yet! \
                    Please load a model in LM Studio."
                );
            } else {
                info!("Connected to LM Studio! Available models ({}):", models.len());
                for m in &models {
                    info!("  - {}", m);
                }
            }
        }
        Err(err) => {
            warn!(
                "Could not connect to LM Studio on startup:\n{}\n\
                (The bot will start anyway and attempt to reconnect when messages are received)",
                err
            );
        }
    }

    let bot = Bot::new(&config.bot_token);

    // Verify Telegram bot identity
    match bot.get_me().await {
        Ok(me) => {
            info!("Telegram bot authenticated as @{}", me.username());
        }
        Err(err) => {
            warn!(
                "Failed to get bot identity from Telegram. Check TELOXIDE_TOKEN: {}",
                err
            );
        }
    }

    let handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<Command>()
                .endpoint(handle_command),
        )
        .branch(
            dptree::entry()
                .filter(|msg: Message| msg.text().is_some())
                .endpoint(handle_message),
        );

    info!("Bot is listening for messages. Press Ctrl+C to stop.");

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![config, client, sessions])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
