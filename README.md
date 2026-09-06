# LM Studio Telegram Bot (Rust + Teloxide)

A fast, async Telegram bot written in Rust using [Teloxide](https://github.com/teloxide/teloxide) that allows you to chat with local LLMs running on an [LM Studio](https://lmstudio.ai/) local server.

---

## Features

- ⚡ **Native Async Performance**: Built on `tokio`, `reqwest`, and `teloxide`.
- 🔄 **OpenAI-Compatible Local API**: Connects to LM Studio's `/v1/chat/completions` and `/v1/models` endpoints.
- 💬 **Context-Aware Chat**: Remembers previous turns per chat with an automatic sliding window to preserve token limits.
- ⏳ **Interactive Typing Indicators**: Sends continuous `typing...` status to Telegram while the local model is generating tokens.
- ✂️ **Automatic Message Splitting**: Telegram limits messages to 4096 characters. Responses exceeding the limit are split cleanly at paragraph, sentence, or word boundaries.
- 🛡️ **Graceful Markdown Handling**: Attempts Markdown formatting with automatic plain-text fallback to prevent delivery failures caused by unescaped model output.
- 🎛️ **Runtime Controls via Commands**:
  - Switch models dynamically per chat (`/model <name>`)
  - Inspect loaded models in LM Studio (`/models`)
  - Customize system prompt per chat (`/system <prompt>`)
  - Adjust generation temperature (`/temperature <value>`)
  - Reset conversation memory (`/clear` or `/reset`)
  - View session metrics & connection diagnostics (`/status`)
- 🔒 **Access Whitelist**: Optional `ALLOWED_USERS` filter to ensure only authorized Telegram accounts can use your GPU/machine.

---

## Prerequisites

1. **Rust & Cargo**: Rust 1.82+ installed (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`).
2. **LM Studio**: Installed and running on your machine (or accessible over your local network).
3. **Telegram Bot Token**: Created via [@BotFather](https://t.me/BotFather) on Telegram.

---

## Quick Start Guide

### 1. Start LM Studio Local Server

1. Open **LM Studio**.
2. Download or select a model (e.g., LLaMA 3.2, Mistral, Qwen, Phi-3, Gemma).
3. Navigate to the **Developer / Local Server** tab (icon `↔` on the left sidebar).
4. Select your model from the top dropdown and click **Start Server**.
5. Ensure the server URL is `http://localhost:1234` (default) and CORS is enabled if needed.

### 2. Configure Environment

Copy the example environment file:

```bash
cp .env.example .env
```

Edit `.env` and fill in your settings:

```env
# Required: Telegram bot token from @BotFather
TELOXIDE_TOKEN=123456789:ABCdefGhIJKlmNoPQRsTUVwxyZ

# LM Studio Server URL (default: http://localhost:1234/v1)
LM_STUDIO_URL=http://localhost:1234/v1

# Optional: Default model name. If commented or empty, auto-detects from LM Studio
# LM_STUDIO_MODEL=meta-llama-3.1-8b-instruct

# Default system prompt
SYSTEM_PROMPT=You are a helpful, knowledgeable, and friendly AI assistant.

# Temperature (0.0 to 2.0, default 0.7)
TEMPERATURE=0.7

# Conversation memory (number of conversation turns to retain)
MAX_HISTORY_TURNS=10

# Optional: restrict to specific Telegram user IDs (comma-separated)
# ALLOWED_USERS=123456789,987654321
```

> **Tip:** You can find your Telegram user ID by messaging [@userinfobot](https://t.me/userinfobot).

### 3. Build and Run

Run in development mode:

```bash
cargo run
```

Or build an optimized release binary:

```bash
cargo build --release
./target/release/lmstudio-telegram-bot
```

When the bot starts up, it logs connectivity verification with LM Studio and authenticates with Telegram:

```
INFO Starting LM Studio Telegram Bot...
INFO Testing connection to LM Studio at http://localhost:1234/v1...
INFO Connected to LM Studio! Available models (1):
INFO   - meta-llama-3.1-8b-instruct
INFO Telegram bot authenticated as @YourBotUsername
INFO Bot is listening for messages. Press Ctrl+C to stop.
```

---

## Bot Commands

| Command | Description | Example |
|---|---|---|
| `/start` | Welcome message and overview | `/start` |
| `/help` | List all available commands | `/help` |
| `/models` | Query LM Studio for available/loaded models | `/models` |
| `/model [name]` | View active model or switch to another model | `/model llama-3.2-3b-instruct` or `/model auto` |
| `/system [prompt]` | View or customize the system prompt | `/system Speak like a 19th-century detective.` or `/system default` |
| `/temperature [val]` | View or update generation temperature | `/temperature 0.3` (precise) or `/temperature 1.2` (creative) |
| `/clear` or `/reset` | Wipe the conversation history for this chat | `/clear` |
| `/status` | View server connectivity, active model, and session history length | `/status` |

---

## Project Structure

```
.
├── Cargo.toml          # Rust package manifest and dependencies
├── .env.example        # Template for configuration settings
├── .gitignore          # Git ignore rules (targets, .env, logs)
├── README.md           # Project documentation
└── src/
    ├── main.rs         # Bot initialization, dispatcher, tracing setup
    ├── config.rs       # Environment configuration parsing and auth check
    ├── formatting.rs   # UTF-8 safe message chunking & fallback sending
    ├── handlers.rs     # Command & text message handlers
    ├── lmstudio.rs     # LM Studio HTTP client (/v1/models, /v1/chat/completions)
    └── session.rs      # Per-chat session state and conversation history sliding window
```

---

## Running as a Background Service (systemd)

To keep the bot running on a Linux home server or workstation:

1. Copy the binary to `/usr/local/bin/`:
   ```bash
   sudo cp target/release/lmstudio-telegram-bot /usr/local/bin/
   ```

2. Create a systemd service file `/etc/systemd/system/lmstudio-bot.service`:
   ```ini
   [Unit]
   Description=LM Studio Telegram Bot
   After=network.target

   [Service]
   Type=simple
   User=YOUR_USER
   WorkingDirectory=/home/YOUR_USER/Code/LMStudioBot
   ExecStart=/usr/local/bin/lmstudio-telegram-bot
   Restart=on-failure
   RestartSec=5
   EnvironmentFile=/home/YOUR_USER/Code/LMStudioBot/.env

   [Install]
   WantedBy=multi-user.target
   ```

3. Enable and start:
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable --now lmstudio-bot
   ```
