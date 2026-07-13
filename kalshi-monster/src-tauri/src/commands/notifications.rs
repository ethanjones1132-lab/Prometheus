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

// Notification Commands
// ═══════════════════════════════════════════════════════════════

use crate::notification::{AppNotification, NotificationSettings};

/// Get all notifications (newest first)
#[tauri::command]
pub async fn get_notifications(
    limit: Option<i64>,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<AppNotification>, String> {
    crate::notification::get_notifications(&pool, limit).await
}

/// Get unread notification count
#[tauri::command]
pub async fn get_unread_notification_count(
    pool: State<'_, Pool<Sqlite>>,
) -> Result<i64, String> {
    crate::notification::get_unread_count(&pool).await
}

/// Mark a notification as read
#[tauri::command]
pub async fn mark_notification_read(
    id: String,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    crate::notification::mark_read(&pool, &id).await
}

/// Mark all notifications as read
#[tauri::command]
pub async fn mark_all_notifications_read(
    pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    crate::notification::mark_all_read(&pool).await
}

/// Dismiss a notification
#[tauri::command]
pub async fn dismiss_notification_cmd(
    id: String,
    pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    crate::notification::dismiss_notification(&pool, &id).await
}

/// Get notification settings
#[tauri::command]
pub async fn get_notification_settings() -> Result<NotificationSettings, String> {
    Ok(crate::notification::load_settings())
}

/// Save notification settings
#[tauri::command]
pub async fn save_notification_settings(
    settings: NotificationSettings,
) -> Result<(), String> {
    crate::notification::save_settings(&settings)?;
    tracing::info!("Notification settings persisted: {:?}", settings);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
