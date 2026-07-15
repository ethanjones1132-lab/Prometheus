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
use crate::kalshi::models::KalshiGradingSummary;
use crate::weather::WeatherClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

// Paper trading journal
// ═══════════════════════════════════════════════════════════════

/// Paper account analytics (cash, equity, win rate, drawdown).
#[tauri::command]
pub async fn paper_get_analytics(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::paper::PaperAnalytics, String> {
    crate::paper::get_analytics(&db_pool, Some(&*kalshi)).await
}

/// Open paper positions aggregated by ticker/side.
#[tauri::command]
pub async fn paper_get_positions(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::paper::PaperPosition>, String> {
    crate::paper::aggregate_positions(&db_pool, Some(&*kalshi)).await
}

/// Settle open paper lots against resolved Kalshi markets.
/// Also grades linked prediction rows and resolves forecast ledger entries.
#[tauri::command]
pub async fn paper_settle_pending(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::paper::PaperSettlementSummary, String> {
    let summary = crate::paper::settle_pending(&db_pool, &kalshi).await?;
    if summary.settled > 0 {
        tracing::info!(
            "paper_settle_pending: {} lots, predictions synced: {}",
            summary.settled,
            summary
                .details
                .iter()
                .filter(|d| d.prediction_outcome.is_some())
                .count()
        );
    }
    Ok(summary)
}

/// Reset paper account and clear lot history.
#[tauri::command]
pub async fn paper_reset_account(
    db_pool: State<'_, Pool<Sqlite>>,
    starting_balance: Option<f64>,
) -> Result<crate::paper::PaperAccount, String> {
    crate::paper::reset_account(&db_pool, starting_balance).await
}

/// Get the latest grading summary
#[tauri::command]
pub async fn kalshi_get_grading_summary(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<KalshiGradingSummary, String> {
    let t = tracker.lock().await;
    Ok(t.get_kalshi_grading_summary().await)
}

/// Export Kalshi predictions as CSV
#[tauri::command]
pub async fn export_kalshi_predictions_csv(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<String, String> {
    let t = tracker.lock().await;
    let all = t.get_kalshi_predictions().await;

    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(&[
        "date", "ticker", "title", "category", "predicted_probability",
        "actual_outcome", "confidence_score", "stake_amount", "pnl",
    ])
    .map_err(|e| AppError::Io(format!("CSV header error: {}", e)))?;

    let mut sorted = all;
    sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    for pred in &sorted {
        wtr.write_record(&[
            pred.created_at.clone(),
            pred.ticker.clone(),
            pred.title.clone(),
            pred.category.clone(),
            pred.predicted_probability.to_string(),
            pred.actual_outcome.clone().unwrap_or_default(),
            pred.confidence_score.map(|s| s.to_string()).unwrap_or_default(),
            pred.stake_amount.to_string(),
            pred.pnl.map(|p| p.to_string()).unwrap_or_default(),
        ])
        .map_err(|e| AppError::Io(format!("CSV row error: {}", e)))?;
    }

    wtr.flush().map_err(|e| AppError::Io(format!("CSV flush error: {}", e)))?;
    let csv_bytes = wtr.into_inner().map_err(|e| AppError::Io(format!("CSV inner error: {}", e)))?;
    String::from_utf8(csv_bytes)
        .map_err(|e| AppError::Serialization(format!("CSV encoding error: {}", e)).into())
}

// ═══════════════════════════════════════════════════════════════
