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

// ── Fincept sidecar bridge (Phase 1) ──

#[tauri::command]
pub async fn get_fincept_bridge_status(
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
) -> Result<crate::fincept_bridge::FinceptBridgeStatus, String> {
    Ok(bridge.status().await)
}

#[tauri::command]
pub async fn fincept_bridge_start_dev(
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
) -> Result<crate::fincept_bridge::FinceptBridgeStatus, String> {
    bridge.start_dev_sidecar().await?;
    Ok(bridge.status().await)
}

#[tauri::command]
pub async fn fincept_bridge_stop(
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
) -> Result<crate::fincept_bridge::FinceptBridgeStatus, String> {
    bridge.stop().await;
    Ok(bridge.status().await)
}

#[tauri::command]
pub async fn get_fincept_market_tracker(
    category: Option<String>,
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
) -> Result<serde_json::Value, String> {
    let path = match category.as_deref() {
        None | Some("") => "/api/v1/market/tracker".to_string(),
        Some(cat) => format!("/api/v1/market/tracker/{cat}"),
    };
    bridge.get_json(&path).await
}

// ════════════════════════════════════════════════════════════════════════════════
