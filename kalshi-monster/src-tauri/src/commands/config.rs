#![allow(unused_imports)]

use crate::commands::{KalshiState, SharedCacheState, PickInput, KalshiDashboardBootstrap, edge_config_for_pool, emit_chat_kalshi_context};
use crate::chat::openrouter::{self, OpenRouterResponse};
use crate::chat::session;
use crate::config;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::secrets::{AppSecrets, SecretKey};
use crate::football::data;
use crate::football::live_data;
use crate::football::player_stats;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use crate::predictions::grading::{self, GradingSummary};
use crate::weather::WeatherClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

// ── Config Commands ──

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<Mutex<AppConfig>>>) -> Result<AppConfig, String> {
    // Never hand plaintext secrets to the webview — masked shape only.
    Ok(state.lock().await.redacted_for_ipc())
}

#[tauri::command]
pub async fn save_config(
    mut config: AppConfig,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    // If the frontend did not send secret values (common when secrets are loaded
    // separately), preserve the currently cached secrets in memory so they are
    // not accidentally cleared. The redaction mask echoed back by the settings
    // form (which only ever sees masked values) also means "no change".
    {
        let guard = state.lock().await;
        use crate::secrets::is_masked_or_empty as no_change;
        if no_change(&config.openrouter_api_key) {
            config.openrouter_api_key = guard.openrouter_api_key.clone();
        }
        if no_change(&config.opencode_api_key) {
            config.opencode_api_key = guard.opencode_api_key.clone();
        }
        if no_change(&config.openweathermap_api_key) {
            config.openweathermap_api_key = guard.openweathermap_api_key.clone();
        }
        if no_change(&config.api_sports_key) {
            config.api_sports_key = guard.api_sports_key.clone();
        }
        if no_change(&config.brave_api_key) {
            config.brave_api_key = guard.brave_api_key.clone();
        }
        if no_change(&config.fred_api_key) {
            config.fred_api_key = guard.fred_api_key.clone();
        }
        if no_change(&config.kalshi_password) {
            config.kalshi_password = guard.kalshi_password.clone();
        }
        if no_change(&config.discord_webhook_url) {
            config.discord_webhook_url = guard.discord_webhook_url.clone();
        }
        if no_change(&config.telegram_bot_token) {
            config.telegram_bot_token = guard.telegram_bot_token.clone();
        }
    }

    config::save_config(&config).map_err(|e| AppError::Config(e.to_string()))?;
    let mut guard = state.lock().await;
    *guard = config;
    Ok(())
}

#[tauri::command]
pub async fn check_api_status(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<config::ApiStatus, String> {
    let config = state.lock().await.clone();
    Ok(config::check_api_status(&config).await)
}

#[tauri::command]
pub async fn check_integration_secrets_health(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<crate::integration_secrets::IntegrationSecretsHealth, String> {
    let config = state.lock().await.clone();
    Ok(crate::integration_secrets::check_integration_secrets_health(&config).await)
}

#[tauri::command]
pub async fn get_security_posture(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<config::SecurityPosture, String> {
    let config = state.lock().await.clone();
    Ok(config::security_posture(&config))
}

#[tauri::command]
pub async fn get_secrets() -> Result<AppSecrets, String> {
    // Masked shape only — the webview learns whether a secret is set, never its value.
    AppSecrets::load().map(|s| s.redacted())
}

#[tauri::command]
pub async fn save_secret(
    key: String,
    value: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    let secret_key = SecretKey::from_account(&key)
        .ok_or_else(|| format!("Unknown secret key: {}", key))?;
    config::save_secret(secret_key, &value)?;

    // Keep the in-memory config state in sync so existing callers that read
    // secret fields from the cached config continue to see the latest value.
    let mut guard = state.lock().await;
    match secret_key {
        SecretKey::OpenrouterApiKey => guard.openrouter_api_key = value,
        SecretKey::OpencodeApiKey => guard.opencode_api_key = value,
        SecretKey::OpenweathermapApiKey => guard.openweathermap_api_key = value,
        SecretKey::ApiSportsKey => guard.api_sports_key = value,
        SecretKey::BraveApiKey => guard.brave_api_key = value,
        SecretKey::FredApiKey => guard.fred_api_key = value,
        SecretKey::KalshiPassword => guard.kalshi_password = value,
        SecretKey::DiscordWebhookUrl => guard.discord_webhook_url = value,
        SecretKey::TelegramBotToken => guard.telegram_bot_token = value,
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_secret(
    key: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    let secret_key = SecretKey::from_account(&key)
        .ok_or_else(|| format!("Unknown secret key: {}", key))?;
    config::delete_secret(secret_key)?;

    let mut guard = state.lock().await;
    match secret_key {
        SecretKey::OpenrouterApiKey => guard.openrouter_api_key.clear(),
        SecretKey::OpencodeApiKey => guard.opencode_api_key.clear(),
        SecretKey::OpenweathermapApiKey => guard.openweathermap_api_key.clear(),
        SecretKey::ApiSportsKey => guard.api_sports_key.clear(),
        SecretKey::BraveApiKey => guard.brave_api_key.clear(),
        SecretKey::FredApiKey => guard.fred_api_key.clear(),
        SecretKey::KalshiPassword => guard.kalshi_password.clear(),
        SecretKey::DiscordWebhookUrl => guard.discord_webhook_url.clear(),
        SecretKey::TelegramBotToken => guard.telegram_bot_token.clear(),
    }
    Ok(())
}

#[tauri::command]
pub async fn get_available_models(
    provider: Option<String>,
) -> Result<Vec<config::ModelInfo>, String> {
    Ok(match provider {
        Some(p) if !p.trim().is_empty() => config::available_models_for_provider(&p),
        _ => config::available_models(),
    })
}

