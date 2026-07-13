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

#[tauri::command]
pub async fn get_bot_config(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<serde_json::Value, String> {
    let config = state.lock().await;
    Ok(serde_json::json!({
        "discord_webhook_url": config.discord_webhook_url,
        "telegram_bot_token": config.telegram_bot_token,
        "telegram_chat_id": config.telegram_chat_id,
        "bot_daily_picks_enabled": config.bot_daily_picks_enabled,
        "bot_game_alerts_enabled": config.bot_game_alerts_enabled,
        "bot_grading_results_enabled": config.bot_grading_results_enabled,
        "bot_daily_picks_time": config.bot_daily_picks_time,
    }))
}

#[tauri::command]
pub async fn save_bot_config(
    bot_settings: serde_json::Value,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
    let mut config = state.lock().await;

    // Secret fields go to the OS credential store; non-secret preferences stay
    // in config.json.
    if let Some(url) = bot_settings.get("discord_webhook_url").and_then(|v| v.as_str()) {
        config.discord_webhook_url = url.to_string();
        config::save_secret(SecretKey::DiscordWebhookUrl, url)?;
    }
    if let Some(token) = bot_settings.get("telegram_bot_token").and_then(|v| v.as_str()) {
        config.telegram_bot_token = token.to_string();
        config::save_secret(SecretKey::TelegramBotToken, token)?;
    }
    if let Some(chat_id) = bot_settings.get("telegram_chat_id").and_then(|v| v.as_str()) {
        config.telegram_chat_id = chat_id.to_string();
    }
    if let Some(enabled) = bot_settings.get("bot_daily_picks_enabled").and_then(|v| v.as_bool()) {
        config.bot_daily_picks_enabled = enabled;
    }
    if let Some(enabled) = bot_settings.get("bot_game_alerts_enabled").and_then(|v| v.as_bool()) {
        config.bot_game_alerts_enabled = enabled;
    }
    if let Some(enabled) = bot_settings.get("bot_grading_results_enabled").and_then(|v| v.as_bool()) {
        config.bot_grading_results_enabled = enabled;
    }
    if let Some(time) = bot_settings.get("bot_daily_picks_time").and_then(|v| v.as_str()) {
        config.bot_daily_picks_time = time.to_string();
    }

    config::save_config(&config).map_err(|e| AppError::Config(e.to_string()))?;
    tracing::info!("Bot configuration saved");
    Ok(())
}

#[tauri::command]
pub async fn test_discord_webhook_cmd(
    url: String,
) -> Result<String, String> {
    crate::bot::test_discord_webhook(&url).await
}

#[tauri::command]
pub async fn test_telegram_bot_cmd(
    bot_token: String,
    chat_id: String,
) -> Result<String, String> {
    crate::bot::test_telegram_bot(&bot_token, &chat_id).await
}

#[tauri::command]
pub async fn send_bot_test_message(
    title: String,
    body: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<String, String> {
    let config = state.lock().await.clone();

    let bot_config = crate::bot::BotDeliveryConfig {
        discord_webhook_url: config.discord_webhook_url,
        telegram_bot_token: config.telegram_bot_token,
        telegram_chat_id: config.telegram_chat_id,
        preferences: crate::bot::BotAlertPreferences::default(),
    };

    crate::bot::send_bot_notification(
        &bot_config,
        &title,
        &body,
        "info",
    ).await?;

    Ok("Test message sent successfully".to_string())
}

// ── ML Predictor Commands ──

/// Train the ML model on historical prediction data
#[tauri::command]
pub async fn ml_train_model(
    db_path: Option<String>,
    output_path: Option<String>,
) -> Result<crate::ml_predictor::MLTrainingResult, String> {
    crate::ml_predictor::train_model(
        db_path.as_deref(),
        output_path.as_deref(),
    ).await
}

/// Generate ML predictions for all pending props
#[tauri::command]
pub async fn ml_predict_batch(
    db_path: Option<String>,
    model_path: Option<String>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::ml_predictor::MLPredictionBatch, String> {
    let batch = crate::ml_predictor::predict_batch(
        db_path.as_deref(),
        model_path.as_deref(),
    ).await?;

    // Save predictions to database
    if batch.status == "ok" && !batch.predictions.is_empty() {
        let model_ver = batch.model_path.clone().unwrap_or_else(|| "unknown".to_string());
        let _ = crate::ml_predictor::save_ml_predictions(
            &db_pool,
            &batch.predictions,
            &model_ver,
        ).await;
    }

    Ok(batch)
}

/// Get ML model status (training info, accuracy, etc.)
#[tauri::command]
pub async fn ml_get_model_status(
    model_path: Option<String>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::ml_predictor::MLModelStatus, String> {
    crate::ml_predictor::get_model_status(&db_pool, model_path.as_deref()).await
}

/// Get stored ML predictions for frontend display
#[tauri::command]
pub async fn ml_get_predictions(
    limit: Option<i64>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<crate::ml_predictor::MLPrediction>, String> {
    let limit = limit.unwrap_or(50);
    crate::ml_predictor::get_stored_ml_predictions(&db_pool, limit).await
}

/// Export feature matrix as CSV for external analysis
#[tauri::command]
pub async fn ml_export_features(
    output_path: Option<String>,
    _db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    // Use the db_pool to get the actual database path
    let _db_path = format!("sqlite://{}", crate::ml_predictor::default_db_path().display());
    crate::ml_predictor::export_features_csv(output_path.as_deref()).await
}

/// Read-only cache state snapshot (does not lock the KalshiClient mutex).
#[derive(Debug, serde::Serialize)]
pub struct KalshiCacheStateResponse {
    pub has_cache: bool,
    pub is_stale: bool,
    pub full_catalog: bool,
    pub market_count: usize,
    pub cache_age_secs: Option<u64>,
    pub fetch_in_progress: bool,
}

#[tauri::command]
pub async fn kalshi_get_cache_state(
    kalshi: State<'_, KalshiState>,
    shared_cache: State<'_, SharedCacheState>,
) -> Result<KalshiCacheStateResponse, String> {
    // Read from shared cache — no client mutex required
    let cached = shared_cache.read().await;
    let (has_cache, is_stale, full_catalog, market_count, cache_age_secs) = match &*cached {
        Some(cache) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let age = (now as i64 - cache.fetched_at as i64).max(0) as u64;
            (
                true,
                age > 60, // CACHE_TTL_SECS
                cache.full_catalog,
                cache.markets.len(),
                Some(age),
            )
        }
        None => (false, false, false, 0, None),
    };
    drop(cached);

    // Check fetch_in_progress through the client (no lock needed; client uses internal RwLock)
    let fetch_in_progress = kalshi.is_fetch_in_progress();

    Ok(KalshiCacheStateResponse {
        has_cache,
        is_stale,
        full_catalog,
        market_count,
        cache_age_secs,
        fetch_in_progress,
    })
}

