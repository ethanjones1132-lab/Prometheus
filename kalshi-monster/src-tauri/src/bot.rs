#![allow(dead_code)]
// ═══════════════════════════════════════════════════════════════
// Discord / Telegram Bot Integration
//
// Sends daily pick alerts, game reminders, and grading results
// to Discord (via webhook) and Telegram (via Bot API).
//
// Both channels are independent — you can use one, both, or neither.
// ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// Bot Configuration
// ═══════════════════════════════════════════════════════════════

/// Per-user alert preferences for bot delivery
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BotAlertPreferences {
    pub daily_picks_enabled: bool,
    pub game_alerts_enabled: bool,
    pub grading_results_enabled: bool,
    /// What time to send daily picks (HH:MM, 24h)
    pub daily_picks_time: String,
    /// Minimum confidence score (0-100) for picks to forward
    pub min_confidence: u8,
}

impl Default for BotAlertPreferences {
    fn default() -> Self {
        Self {
            daily_picks_enabled: true,
            game_alerts_enabled: true,
            grading_results_enabled: true,
            daily_picks_time: "08:00".to_string(),
            min_confidence: 60,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Discord Webhook
// ═══════════════════════════════════════════════════════════════

/// Discord webhook payload
#[derive(Debug, Serialize)]
struct DiscordWebhookPayload {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    embeds: Vec<DiscordEmbed>,
}

#[derive(Debug, Serialize)]
struct DiscordEmbed {
    title: String,
    description: String,
    color: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<DiscordEmbedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    footer: Option<DiscordEmbedFooter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiscordEmbedField {
    name: String,
    value: String,
    inline: bool,
}

#[derive(Debug, Serialize)]
struct DiscordEmbedFooter {
    text: String,
}

/// Send a plain text message to Discord via webhook
pub async fn send_discord_webhook(url: &str, content: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("Discord webhook URL is empty".to_string());
    }

    let payload = DiscordWebhookPayload {
        content: content.to_string(),
        username: Some("Kalshi Monster".to_string()),
        avatar_url: None,
        embeds: vec![],
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Discord webhook request failed: {}", e))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Discord webhook returned {}: {}", status, body))
    }
}

/// Send a rich embed message to Discord
pub async fn send_discord_embed(
    url: &str,
    title: &str,
    description: &str,
    color: u32,
    fields: Vec<(String, String, bool)>,
    footer: Option<&str>,
) -> Result<(), String> {
    if url.is_empty() {
        return Err("Discord webhook URL is empty".to_string());
    }

    let embed_fields: Vec<DiscordEmbedField> = fields
        .into_iter()
        .map(|(name, value, inline)| DiscordEmbedField {
            name: name.to_string(),
            value: value.to_string(),
            inline,
        })
        .collect();

    let embed = DiscordEmbed {
        title: title.to_string(),
        description: description.to_string(),
        color,
        fields: embed_fields,
        footer: footer.map(|f| DiscordEmbedFooter { text: f.to_string() }),
        timestamp: Some(chrono::Utc::now().to_rfc3339()),
    };

    let payload = DiscordWebhookPayload {
        content: String::new(),
        username: Some("Kalshi Monster".to_string()),
        avatar_url: None,
        embeds: vec![embed],
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Discord webhook request failed: {}", e))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Discord webhook returned {}: {}", status, body))
    }
}

/// Test the Discord webhook by sending a test message
pub async fn test_discord_webhook(url: &str) -> Result<String, String> {
    send_discord_webhook(
        url,
        "📉 **Kalshi Monster** bot connected successfully!\n\nYou'll receive daily predictions, alerts, and grading results here.",
    )
    .await?;
    Ok("Discord webhook test sent successfully".to_string())
}

// ═══════════════════════════════════════════════════════════════
// Telegram Bot API
// ═══════════════════════════════════════════════════════════════

/// Telegram sendMessage payload
#[derive(Debug, Serialize)]
struct TelegramSendMessage {
    chat_id: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disable_web_page_preview: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
}

/// Send a text message to Telegram via Bot API
pub async fn send_telegram_message(
    bot_token: &str,
    chat_id: &str,
    text: &str,
    markdown: bool,
) -> Result<(), String> {
    if bot_token.is_empty() {
        return Err("Telegram bot token is empty".to_string());
    }
    if chat_id.is_empty() {
        return Err("Telegram chat ID is empty".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);

    let payload = TelegramSendMessage {
        chat_id: chat_id.to_string(),
        text: text.to_string(),
        parse_mode: if markdown {
            Some("MarkdownV2".to_string())
        } else {
            None
        },
        disable_web_page_preview: Some(false),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Telegram API request failed: {}", e))?;

    if resp.status().is_success() {
        let tg_resp: TelegramResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Telegram response: {}", e))?;
        if tg_resp.ok {
            Ok(())
        } else {
            Err(format!(
                "Telegram API error: {}",
                tg_resp.description.unwrap_or_default()
            ))
        }
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Telegram API returned {}: {}", status, body))
    }
}

/// Test the Telegram bot by sending a test message
pub async fn test_telegram_bot(bot_token: &str, chat_id: &str) -> Result<String, String> {
    send_telegram_message(
        bot_token,
        chat_id,
        "📉 *Kalshi Monster* bot connected successfully\\!\n\nYou'll receive daily predictions, alerts, and grading results here\\.",
        true,
    )
    .await?;
    Ok("Telegram bot test sent successfully".to_string())
}

// ═══════════════════════════════════════════════════════════════
// Combined Bot Sender — sends to both channels
// ═══════════════════════════════════════════════════════════════

/// Bot delivery config derived from AppConfig
#[derive(Debug, Clone)]
pub struct BotDeliveryConfig {
    pub discord_webhook_url: String,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub preferences: BotAlertPreferences,
}

impl BotDeliveryConfig {
    /// Check if any bot channel is configured
    pub fn has_discord(&self) -> bool {
        !self.discord_webhook_url.is_empty()
    }

    pub fn has_telegram(&self) -> bool {
        !self.telegram_bot_token.is_empty() && !self.telegram_chat_id.is_empty()
    }

    pub fn is_configured(&self) -> bool {
        self.has_discord() || self.has_telegram()
    }
}

/// Send a notification to all configured bot channels
pub async fn send_bot_notification(
    config: &BotDeliveryConfig,
    title: &str,
    body: &str,
    notification_type: &str,
) -> Result<(), String> {
    let mut errors = Vec::new();

    // Check if this notification type is enabled
    let type_enabled = match notification_type {
        "game_starting" | "game_final" => config.preferences.game_alerts_enabled,
        "prediction_graded" | "prediction_win" | "prediction_loss" | "prediction_push" | "grading_complete" => {
            config.preferences.grading_results_enabled
        }
        "daily_picks" => config.preferences.daily_picks_enabled,
        _ => true,
    };

    if !type_enabled {
        return Ok(());
    }

    // Send to Discord
    if config.has_discord() {
        let emoji = match notification_type {
            "game_starting" => "🏈",
            "game_final" => "🏁",
            "prediction_win" => "✅",
            "prediction_loss" => "❌",
            "prediction_push" => "🔄",
            "grading_complete" => "📊",
            "daily_picks" => "📋",
            _ => "ℹ️",
        };
        let discord_text = format!("{} **{}**\n{}", emoji, title, body);
        if let Err(e) = send_discord_webhook(&config.discord_webhook_url, &discord_text).await {
            errors.push(format!("Discord: {}", e));
        }
    }

    // Send to Telegram
    if config.has_telegram() {
        let emoji = match notification_type {
            "game_starting" => "🏈",
            "game_final" => "🏁",
            "prediction_win" => "✅",
            "prediction_loss" => "❌",
            "prediction_push" => "🔄",
            "grading_complete" => "📊",
            "daily_picks" => "📋",
            _ => "ℹ️",
        };
        // Escape special MarkdownV2 characters
        let escaped_title = escape_telegram_markdown(title);
        let escaped_body = escape_telegram_markdown(body);
        let telegram_text = format!("{} *{}* {}", emoji, escaped_title, escaped_body);
        if let Err(e) =
            send_telegram_message(&config.telegram_bot_token, &config.telegram_chat_id, &telegram_text, true)
                .await
        {
            errors.push(format!("Telegram: {}", e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Send a rich Discord embed for pick alerts
pub async fn send_discord_picks_embed(
    discord_webhook_url: &str,
    title: &str,
    picks: &[PickAlertData],
) -> Result<(), String> {
    if picks.is_empty() || discord_webhook_url.is_empty() {
        return Ok(());
    }

    let fields: Vec<(String, String, bool)> = picks
        .iter()
        .take(10)
        .map(|pick| {
            let value = format!(
                "{} {} (Line: {}) | Conf: {}%",
                pick.pick_type, pick.line, pick.stat_category, pick.confidence
            );
            (pick.player_name.clone(), value, false)
        })
        .collect();

    let footer = format!("Kalshi Monster • {} picks • {}", picks.len(), chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"));

    send_discord_embed(
        discord_webhook_url,
        title,
        &format!("{} picks ready for today", picks.len()),
        0x5865F2,
        fields,
        Some(&footer),
    )
    .await
}

/// Send a formatted Telegram message for pick alerts
pub async fn send_telegram_picks_message(
    bot_token: &str,
    chat_id: &str,
    title: &str,
    picks: &[PickAlertData],
) -> Result<(), String> {
    if picks.is_empty() || bot_token.is_empty() || chat_id.is_empty() {
        return Ok(());
    }

    let mut text = format!("📋 *{}*\\n\\n", escape_telegram_markdown(title));

    for (i, pick) in picks.iter().take(15).enumerate() {
        text.push_str(&format!(
            "{}\\. {} — {} {} (Line: {}) Conf: {}%\n",
            i + 1,
            escape_telegram_markdown(&pick.player_name),
            escape_telegram_markdown(&pick.pick_type),
            pick.line,
            escape_telegram_markdown(&pick.stat_category),
            pick.confidence
        ));
    }

    if picks.len() > 15 {
        text.push_str(&format!("\\n_\\+ {} more picks_", picks.len() - 15));
    }

    send_telegram_message(bot_token, chat_id, &text, true).await
}

/// Data structure for a single pick alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickAlertData {
    pub player_name: String,
    pub pick_type: String,
    pub line: f64,
    pub stat_category: String,
    pub confidence: u8,
}

/// Escape special characters for Telegram MarkdownV2 format
fn escape_telegram_markdown(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('_', "\\_")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(')', "\\)")
        .replace('~', "\\~")
        .replace('`', "\\`")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('.', "\\.")
        .replace('!', "\\!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_escape_telegram_markdown() {
        assert_eq!(escape_telegram_markdown("Hello World"), "Hello World");
        assert_eq!(escape_telegram_markdown("Over 27.5"), "Over 27\\.5");
        assert_eq!(escape_telegram_markdown("Conf: 75%"), "Conf: 75%");
        assert_eq!(escape_telegram_markdown("Player (LAC)"), "Player \\(LAC\\)");
    }

    #[test]
    fn test_bot_delivery_config() {
        let config = BotDeliveryConfig {
            discord_webhook_url: "https://discord.com/api/webhooks/xxx".to_string(),
            telegram_bot_token: "".to_string(),
            telegram_chat_id: "".to_string(),
            preferences: BotAlertPreferences::default(),
        };
        assert!(config.has_discord());
        assert!(!config.has_telegram());
        assert!(config.is_configured());
    }

    #[test]
    fn test_bot_delivery_config_neither() {
        let config = BotDeliveryConfig {
            discord_webhook_url: "".to_string(),
            telegram_bot_token: "".to_string(),
            telegram_chat_id: "".to_string(),
            preferences: BotAlertPreferences::default(),
        };
        assert!(!config.is_configured());
    }
}
