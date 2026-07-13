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

// Breaker State Commands (Phase 3 productization)
// ════════════════════════════════════════════════════════════════════════════════

/// Get the current circuit breaker state (Phase 3, plan §6.4 breaker latches).
/// Returns the persisted breaker latch state: stake scaling active, live trading disabled,
/// and paper-mode forced flags.  The caller re-evaluates the full [`BreakerDecision`]
/// each tick with fresh inputs; this command only returns the latched portion.
#[tauri::command]
pub async fn kalshi_get_breaker_state(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::breakers::BreakerState, String> {
    crate::edge_engine::persistence::load_breaker_state(&db_pool).await
}

/// Manual re-enable of the 25% drawdown breaker (plan §6.4 row 3).
/// Clears the `live_trading_disabled` latch so live orders can resume on the
/// next evaluation pass.  If the drawdown is still ≥ 25%, the breaker
/// re-latches immediately on the next call to [`evaluate_breakers`].
///
/// Persists the cleared state and returns the updated [`BreakerState`].
#[tauri::command]
pub async fn kalshi_manual_reenable_breaker(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::breakers::BreakerState, String> {
    let current = crate::edge_engine::persistence::load_breaker_state(&db_pool).await?;
    let cleared = current.manual_reenable();
    crate::edge_engine::persistence::save_breaker_state(&db_pool, &cleared).await?;
    tracing::info!("[Kalshi] breaker manually re-enabled (live_trading_disabled cleared)");
    Ok(cleared)
}

/// Run one §6.4 breaker evaluation from live paper + calibration inputs; persists latches.
#[tauri::command]
pub async fn kalshi_evaluate_breakers(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::breakers::BreakerDecision, String> {
    crate::edge_engine::persistence::evaluate_and_persist_breakers(&db_pool).await
}

/// Load gate + breaker state and return combined live-order eligibility (§6.5).
pub async fn load_live_order_eligibility(
    db_pool: &Pool<Sqlite>,
) -> Result<crate::edge_engine::execution_guard::LiveOrderEligibility, String> {
    let resolved = crate::kalshi::forecast::resolved_forecasts_for_calibration(db_pool).await?;
    let paper_pnl = crate::paper::get_analytics(db_pool, None)
        .await
        .ok()
        .map(|a| a.realized_pnl)
        .unwrap_or(0.0);
    let gate = crate::edge_engine::calibration::evaluate_gate(
        &resolved,
        paper_pnl,
        &crate::edge_engine::calibration::GateConfig::default(),
    );
    let breakers = crate::edge_engine::persistence::evaluate_and_persist_breakers(db_pool).await?;
    Ok(crate::edge_engine::execution_guard::evaluate_live_order_eligibility(
        gate.passed,
        &gate.conditions,
        &breakers,
    ))
}

/// §6.5 order-path guard — Phase 5 `place_order` must call this before any live fill.
pub async fn guard_live_order_path(db_pool: &Pool<Sqlite>) -> Result<(), String> {
    let eligibility = load_live_order_eligibility(db_pool).await?;
    crate::edge_engine::execution_guard::assert_live_order_allowed(&eligibility)
}

#[tauri::command]
pub async fn kalshi_get_live_order_eligibility(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::execution_guard::LiveOrderEligibility, String> {
    load_live_order_eligibility(&db_pool).await
}

/// Phase 5 placeholder: proves the live order path consults §6.5 before any API call.
#[tauri::command]
pub async fn kalshi_guard_live_order_path(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<(), String> {
    guard_live_order_path(&db_pool).await
}

