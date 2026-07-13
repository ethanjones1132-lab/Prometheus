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

// ═══════════════════════════════════════════════════════════════
// Kalshi Commands — Prediction Market Integration
// ═══════════════════════════════════════════════════════════════

/// Fetch markets filtered by category. Category can be "All", "Sports", "Politics",
/// "Economics", "Crypto", "Finance", "Weather", "Other".
#[tauri::command]
pub async fn kalshi_get_markets(
    category: String,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiMarketSummary>, String> {
    kalshi.get_markets_by_category(&category).await
}

/// Fetch a single market by ticker.
#[tauri::command]
pub async fn kalshi_get_market(
    ticker: String,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::kalshi::KalshiMarketSummary, String> {
    let market = kalshi.fetch_market(&ticker).await?;
    Ok(crate::kalshi::KalshiMarketSummary::from(&market))
}

/// Fetch the orderbook for a market by ticker.
#[tauri::command]
pub async fn kalshi_get_orderbook(
    ticker: String,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::kalshi::KalshiOrderbook, String> {
    kalshi.fetch_orderbook(&ticker).await
}

/// Search markets by keyword across title, ticker, and event ticker.
#[tauri::command]
pub async fn kalshi_search_markets(
    query: String,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiMarketSummary>, String> {
    if query.len() > 200 {
        return Err("Search query too long (max 200 characters)".to_string());
    }
    kalshi.search_markets(&query).await
}

/// Get the top markets by 24h trading volume.
#[tauri::command]
pub async fn kalshi_get_top_markets(
    limit: Option<usize>,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiMarketSummary>, String> {
    let n = limit.unwrap_or(30).min(100);
    kalshi.get_top_markets(n).await
}

/// Build human-readable tape quality hints for the Kalshi dashboard (no extra struct fields).
pub(crate) fn build_kalshi_dashboard_data_quality_notes(
    partial_catalog: bool,
    showing_persisted_snapshot: bool,
    cache_stale: bool,
    fetch_in_progress: bool,
    tape_market_count: usize,
    last_fetch_error: Option<&str>,
) -> Vec<String> {
    let mut notes = if partial_catalog {
        vec!["Partial catalog loaded for fast first paint".to_string()]
    } else {
        vec!["Full catalog cache ready".to_string()]
    };
    if showing_persisted_snapshot {
        notes.push(
            "Instant paint from saved market snapshot; live refresh runs in background"
                .to_string(),
        );
    }
    if cache_stale {
        notes.push(
            "Market tape is older than 60s — use Refresh and snapshot for live prices"
                .to_string(),
        );
    }
    if fetch_in_progress {
        notes.push(
            "Live catalog refresh in progress — tape may update shortly".to_string(),
        );
    }
    if tape_market_count == 0 {
        notes.push(
            "No markets loaded — verify Kalshi API access in Settings and tap Refresh and snapshot"
                .to_string(),
        );
    }
    if let Some(err) = last_fetch_error {
        if !err.is_empty() {
            notes.push(format!("Last catalog fetch error: {err}"));
        }
    }
    notes
}

/// Initial dashboard payload: top markets, category stats, and cache freshness in one IPC call.
#[tauri::command]
pub async fn kalshi_get_dashboard_bootstrap(
    limit: Option<usize>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<KalshiDashboardBootstrap, String> {
    let n = limit.unwrap_or(30).min(100);
    let client = kalshi.as_ref();
    let markets = client.get_top_markets(n).await?;
    let categories = client.category_stats();
    let (cache_status, cache_age_secs, partial_catalog, fetched_at) = client.cache_metadata();
    let last_refresh_at = fetched_at
        .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
        .map(|dt| dt.to_rfc3339());
    let tape_market_count = client.cached_tape_market_count();
    let market_count = tape_market_count.max(markets.len());
    let category_count = categories.len();
    let data_quality_notes = build_kalshi_dashboard_data_quality_notes(
        partial_catalog,
        client.showing_persisted_snapshot(),
        client.is_cache_stale(),
        client.is_fetch_in_progress(),
        tape_market_count,
        client.last_fetch_error().as_deref(),
    );

    let ml_phase3 = crate::ml_predictor::phase3_dashboard_summary(&db_pool).await;

    Ok(KalshiDashboardBootstrap {
        markets,
        categories,
        cache_status,
        cache_age_secs,
        partial_catalog,
        last_refresh_at,
        market_count,
        category_count,
        dashboard_generated_at: chrono::Utc::now().to_rfc3339(),
        data_quality_notes,
        ml_phase3: Some(ml_phase3),
    })
}

/// Get per-category market counts and 24h volumes.
#[tauri::command]
pub async fn kalshi_get_category_stats(
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiCategoryStat>, String> {
    Ok(kalshi.category_stats())
}

/// Analyst chat: whether Kalshi tape is ready for context injection (KB-2a).
#[tauri::command]
pub async fn kalshi_get_chat_context_status(
    kalshi: State<'_, KalshiState>,
) -> Result<crate::chat::kalshi_context::KalshiChatContextStatus, String> {
    Ok(crate::chat::kalshi_context::assess_kalshi_chat_context(&kalshi))
}

/// Get authenticated portfolio balance and positions.
/// Requires kalshi_email + kalshi_password to be configured.
#[tauri::command]
pub async fn kalshi_get_portfolio(
    config: State<'_, Arc<Mutex<AppConfig>>>,
    kalshi: State<'_, KalshiState>,
) -> Result<serde_json::Value, String> {
    // Sync config into client before making auth calls
    {
        let app_cfg = config.lock().await;
        let current = kalshi.config();
        if current.email != app_cfg.kalshi_email
            || current.password != app_cfg.kalshi_password
        {
            let new_cfg = crate::kalshi::kalshi_config_from_app(&app_cfg);
            kalshi.set_config(new_cfg);
            kalshi.invalidate_cache();
        }
    }

    let balance = kalshi.get_balance().await?;
    let positions = kalshi.get_positions().await?;

    Ok(serde_json::json!({
        "balance_cents": balance.balance,
        "balance_dollars": balance.balance as f64 / 100.0,
        "reserved_fees_cents": balance.reserved_fees,
        "positions": positions,
    }))
}

/// Force-refresh the Kalshi market cache, syncing config from app config.
#[tauri::command]
pub async fn kalshi_refresh(
    config: State<'_, Arc<Mutex<AppConfig>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<usize, String> {
    {
        let app_cfg = config.lock().await;
        let new_cfg = crate::kalshi::kalshi_config_from_app(&app_cfg);
        kalshi.set_config(new_cfg);
        kalshi.invalidate_cache();
    }
    let markets = kalshi.fetch_all_markets().await?;
    let summaries: Vec<crate::kalshi::KalshiMarketSummary> =
        markets.iter().map(crate::kalshi::KalshiMarketSummary::from).collect();
    if let Err(e) = crate::kalshi::price_tracker::snapshot_markets(&db_pool, &summaries).await {
        tracing::warn!("kalshi price snapshot on refresh: {}", e);
    }
    Ok(markets.len())
}


mod kalshi_dashboard_bootstrap_tests {
    use super::build_kalshi_dashboard_data_quality_notes;

    #[test]
    fn data_quality_notes_include_stale_and_fetch_hints() {
        let notes = build_kalshi_dashboard_data_quality_notes(true, true, true, true, 0, Some("auth failed"));
        assert!(notes.iter().any(|n| n.contains("Partial catalog")));
        assert!(notes.iter().any(|n| n.contains("saved market snapshot")));
        assert!(notes.iter().any(|n| n.contains("older than 60s")));
        assert!(notes.iter().any(|n| n.contains("refresh in progress")));
        assert!(notes.iter().any(|n| n.contains("No markets loaded")));
        assert!(notes.iter().any(|n| n.contains("Last catalog fetch error")));
    }

    #[test]
    fn data_quality_notes_full_catalog_without_extras() {
        let notes = build_kalshi_dashboard_data_quality_notes(false, false, false, false, 12, None);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("Full catalog"));
    }
}
