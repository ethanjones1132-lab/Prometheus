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

// ── Prediction Commands ──

#[tauri::command]
pub async fn get_session_predictions(
    session_id: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    Ok(t.get_session_predictions(&session_id).await)
}

#[tauri::command]
pub async fn get_all_predictions(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    Ok(t.get_all_predictions().await)
}

#[tauri::command]
pub async fn get_prediction_stats(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<config::PredictionStats, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let stats = t.get_stats(&all);
    Ok(config::PredictionStats::from_tracker(&stats))
}

/// Get predictions filtered by confidence score range
#[tauri::command]
pub async fn get_predictions_by_confidence(
    min_score: Option<u8>,
    max_score: Option<u8>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<PredictionRecord>, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let min = min_score.unwrap_or(0);
    let max = max_score.unwrap_or(100);
    Ok(all
        .into_iter()
        .filter(|r| {
            r.prediction.confidence_score.map_or(false, |s| s >= min && s <= max)
        })
        .collect())
}

// ── Trend Commands ──

#[tauri::command]
pub async fn get_overall_trend(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<crate::predictions::tracker::OverallTrend, String> {
    let t = tracker.lock().await;
    Ok(t.get_overall_trend().await)
}

#[tauri::command]
pub async fn get_player_trend(
    player_name: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Option<crate::predictions::tracker::PlayerTrend>, String> {
    let t = tracker.lock().await;
    Ok(t.get_player_trend(&player_name).await)
}

#[tauri::command]
pub async fn get_stat_category_trend(
    stat_category: String,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Option<crate::predictions::tracker::StatCategoryTrend>, String> {
    let t = tracker.lock().await;
    Ok(t.get_stat_category_trend(&stat_category).await)
}

#[tauri::command]
pub async fn get_trend_player_list(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<(String, u32)>, String> {
    let t = tracker.lock().await;
    Ok(t.get_player_list().await)
}

#[tauri::command]
pub async fn get_trend_stat_category_list(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<Vec<(String, u32)>, String> {
    let t = tracker.lock().await;
    Ok(t.get_stat_category_list().await)
}

#[tauri::command]
pub async fn update_prediction_outcome(
    prediction_id: String,
    outcome: String,
    actual_result: Option<f64>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<(), String> {
    let outcome = outcome.parse::<PredictionOutcome>()
        .map_err(|e| AppError::Validation(format!("Invalid outcome: {}", e)))?;
    let t = tracker.lock().await;
    t.update_outcome(&prediction_id, outcome, actual_result).await
}



// ── Helper Functions ──

/// Detect the league from message content
#[allow(dead_code)]
fn detect_league_from_message(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    if lower.contains("nfl") || lower.contains("football") || lower.contains("nfl") {
        Some("football".to_string())
    } else if lower.contains("nba") || lower.contains("basketball") {
        Some("basketball".to_string())
    } else if lower.contains("mlb") || lower.contains("baseball") {
        Some("baseball".to_string())
    } else if lower.contains("nhl") || lower.contains("hockey") {
        Some("hockey".to_string())
    } else {
        None // Default: let the fetcher decide
    }
}

// ── Parlay Commands ──

/// A parlay leg derived from a prediction, formatted for the frontend parlay builder.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ParlayLeg {
    pub id: String,
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub confidence: String,
    pub confidence_score: Option<u8>,
    pub win_probability: Option<f64>,
    pub reasoning: Option<String>,
    pub risk: Option<String>,
}

impl From<&crate::predictions::tracker::PredictionRecord> for ParlayLeg {
    fn from(record: &crate::predictions::tracker::PredictionRecord) -> Self {
        let p = &record.prediction;
        ParlayLeg {
            id: p.id.clone(),
            player_name: p.player_name.clone().unwrap_or_default(),
            team: String::new(),
            opponent: String::new(),
            prop_category: p.stat_category.clone().unwrap_or_default(),
            line: p.line.unwrap_or(0.0),
            pick_type: p.pick_type.clone().unwrap_or_default(),
            confidence: p.confidence.clone().unwrap_or_default(),
            confidence_score: p.confidence_score,
            win_probability: p.probability,
            reasoning: p.reasoning.clone(),
            risk: p.risk.clone(),
        }
    }
}

/// Get all pending predictions formatted as parlay legs.
/// Optionally filter by minimum confidence score.
#[tauri::command]
pub async fn get_parlay_legs(
    min_confidence: Option<u8>,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<Vec<ParlayLeg>, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;

    let legs: Vec<ParlayLeg> = all
        .iter()
        .filter(|r| r.outcome == PredictionOutcome::Pending)
        .filter(|r| {
            if let Some(min) = min_confidence {
                r.prediction.confidence_score.map_or(false, |s| s >= min)
            } else {
                true
            }
        })
        .map(|r| ParlayLeg::from(r))
        .filter(|l| !l.player_name.is_empty() && !l.prop_category.is_empty())
        .collect();

    Ok(legs)
}

// ── Model Comparison Command ──

/// Send the same message to multiple models and return all responses.
/// Useful for comparing how different models analyze the same prop.
#[tauri::command]
pub async fn compare_models(
    message: String,
    models: Vec<String>,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    _chat_state: State<'_, Arc<Mutex<crate::chat::session::ChatState>>>,
) -> Result<Vec<openrouter::OpenRouterResponse>, String> {
    let config = state.lock().await.clone();

    // Load session messages for context (use empty for comparison)
    let session_messages: Vec<openrouter::ChatMessage> = Vec::new();

    let system_prompt = config.system_prompt.clone();
    let kalshi_context_msg = String::new();
    let sports_context_msg = String::new();

    // Send to all models concurrently using join_all
    let futures = models.into_iter().map(|model| {
        let mut model_config = config.clone();
        model_config.selected_model = model.clone();
        let session_messages = session_messages.clone();
        let message = message.clone();
        let system_prompt = system_prompt.clone();
        let kalshi_context_msg = kalshi_context_msg.clone();
        let sports_context_msg = sports_context_msg.clone();

        async move {
            let result = openrouter::send_message_with_context(
                &model_config,
                &session_messages,
                message,
                &system_prompt,
                &kalshi_context_msg,
                &sports_context_msg,
            ).await;

            match result {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!("Model {} failed: {}", model, e);
                    openrouter::OpenRouterResponse {
                        content: format!("Error: {}", e),
                        reasoning: None,
                        tokens_used: None,
                        model: model.clone(),
                    }
                }
            }
        }
    });

    let results: Vec<openrouter::OpenRouterResponse> = futures::future::join_all(futures).await;
    Ok(results)
}

// ── Automated Grading Commands ──

/// Grade all pending predictions using ESPN boxscore data.
/// Fetches completed games from the ESPN scoreboard, retrieves boxscores,
/// and compares actual player stats against prop lines.
#[tauri::command]
pub async fn grade_pending_predictions(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<GradingSummary, String> {
    // Collect pending predictions
    let pending = {
        let t = tracker.lock().await;
        let all = t.get_all_predictions().await;
        all.into_iter()
            .filter(|r| r.outcome == PredictionOutcome::Pending)
            .filter(|r| {
                r.prediction.player_name.is_some()
                    && r.prediction.stat_category.is_some()
                    && r.prediction.line.is_some()
                    && r.prediction.pick_type.is_some()
            })
            .map(|r| {
                (
                    r.prediction.id.clone(),
                    r.prediction.player_name.clone().unwrap_or_default(),
                    r.prediction.pick_type.clone().unwrap_or_default(),
                    r.prediction.line.unwrap_or(0.0),
                    r.prediction.stat_category.clone().unwrap_or_default(),
                    r.prediction.session_id.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    if pending.is_empty() {
        return Ok(GradingSummary {
            total_pending: 0,
            graded: 0,
            skipped: 0,
            wins: 0,
            losses: 0,
            pushes: 0,
            unresolved: 0,
            results: vec![],
            fetched_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    // Run grading
    let summary = grading::grade_all_pending(&pending).await;

    // Apply graded outcomes to the tracker
    let t = tracker.lock().await;
    for result in &summary.results {
        match result.outcome.as_str() {
            "Win" | "Loss" | "Push" => {
                let outcome = result
                    .outcome
                    .parse::<PredictionOutcome>()
                    .unwrap_or(PredictionOutcome::Pending);
                let _ = t.update_outcome(&result.prediction_id, outcome, result.actual_result).await;
            }
            _ => {}
        }
    }

    Ok(summary)
}

/// Export all predictions as CSV.
/// Returns the CSV content as a string so the frontend can download it.
#[tauri::command]
pub async fn export_predictions_csv(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<String, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;

    let mut wtr = csv::Writer::from_writer(Vec::new());

    // Write header
    wtr.write_record(&[
        "date", "player", "team", "pick_type", "line", "stat_category",
        "confidence", "confidence_score", "outcome", "actual_result",
    ])
    .map_err(|e| AppError::Io(format!("CSV header error: {}", e)))?;

    // Sort newest first
    let mut sorted = all;
    sorted.sort_by(|a, b| {
        b.prediction
            .created_at
            .cmp(&a.prediction.created_at)
    });

    for record in &sorted {
        let p = &record.prediction;
        wtr.write_record(&[
            p.created_at.clone(),
            p.player_name.clone().unwrap_or_default(),
            String::new(), // team not tracked yet
            p.pick_type.clone().unwrap_or_default(),
            p.line.map(|l| l.to_string()).unwrap_or_default(),
            p.stat_category.clone().unwrap_or_default(),
            p.confidence.clone().unwrap_or_default(),
            p.confidence_score.map(|s| s.to_string()).unwrap_or_default(),
            record.outcome.to_string(),
            record.actual_result.map(|r| r.to_string()).unwrap_or_default(),
        ])
        .map_err(|e| AppError::Io(format!("CSV row error: {}", e)))?;
    }

    wtr.flush().map_err(|e| AppError::Io(format!("CSV flush error: {}", e)))?;
    let csv_bytes = wtr.into_inner().map_err(|e| AppError::Io(format!("CSV inner error: {}", e)))?;
    String::from_utf8(csv_bytes)
        .map_err(|e| AppError::Serialization(format!("CSV encoding error: {}", e)).into())
}

/// Get grading status — returns info about the last grading run
/// and how many predictions are currently pending.
#[tauri::command]
pub async fn get_grading_status(
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<serde_json::Value, String> {
    let t = tracker.lock().await;
    let all = t.get_all_predictions().await;
    let pending_count = all
        .iter()
        .filter(|r| r.outcome == PredictionOutcome::Pending)
        .filter(|r| {
            r.prediction.player_name.is_some()
                && r.prediction.stat_category.is_some()
                && r.prediction.line.is_some()
                && r.prediction.pick_type.is_some()
        })
        .count();

    Ok(serde_json::json!({
        "total_predictions": all.len(),
        "pending_gradable": pending_count,
        "message": if pending_count > 0 {
            format!("{} pending predictions ready to grade", pending_count)
        } else {
            "No pending predictions to grade".to_string()
        }
    }))
}

// ── Bankroll Management Commands ──

/// Get the current bankroll configuration
#[tauri::command]
pub async fn get_bankroll_config() -> Result<crate::bankroll::BankrollConfig, String> {
    Ok(crate::bankroll::load_bankroll_config())
}

/// Save bankroll configuration
#[tauri::command]
pub async fn save_bankroll_config(
    config: crate::bankroll::BankrollConfig,
) -> Result<(), String> {
    crate::bankroll::save_bankroll_config(&config)
}

/// Get a summary of bankroll status (ROI, P&L, etc.)
#[tauri::command]
pub async fn get_bankroll_summary(
    config: crate::bankroll::BankrollConfig,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::bankroll::BankrollSummary, String> {
    crate::bankroll::get_bankroll_summary_synced(&config, &db_pool).await
}

/// Generate bet recommendations for a list of picks
#[tauri::command]
pub async fn recommend_bets(
    bankroll_config: crate::bankroll::BankrollConfig,
    picks: Vec<crate::commands::PickInput>,
) -> Result<Vec<crate::bankroll::BetRecommendation>, String> {
    let inputs: Vec<crate::bankroll::PickInput> = picks
        .into_iter()
        .map(|p| crate::bankroll::PickInput {
            player_name: p.player_name,
            prop_category: p.prop_category,
            line: p.line,
            pick_type: p.pick_type,
            win_probability: p.win_probability,
            confidence_score: p.confidence_score,
        })
        .collect();
    Ok(crate::bankroll::recommend_multiple_bets(&bankroll_config, &inputs))
}

/// Generate a parlay recommendation with correlation adjustment
#[tauri::command]
pub async fn recommend_parlay(
    bankroll_config: crate::bankroll::BankrollConfig,
    legs: Vec<crate::commands::PickInput>,
    correlation_factor: f64,
) -> Result<crate::bankroll::ParlayRecommendation, String> {
    let inputs: Vec<crate::bankroll::PickInput> = legs
        .into_iter()
        .map(|p| crate::bankroll::PickInput {
            player_name: p.player_name,
            prop_category: p.prop_category,
            line: p.line,
            pick_type: p.pick_type,
            win_probability: p.win_probability,
            confidence_score: p.confidence_score,
        })
        .collect();
    Ok(crate::bankroll::recommend_parlay(&bankroll_config, &inputs, correlation_factor))
}

/// Record a bet result and update the bankroll
#[tauri::command]
pub async fn record_bankroll_result(
    mut config: crate::bankroll::BankrollConfig,
    stake: f64,
    won: bool,
    odds: Option<f64>,
) -> Result<crate::bankroll::BankrollConfig, String> {
    crate::bankroll::record_result(&mut config, stake, won, odds);
    crate::bankroll::save_bankroll_config(&config).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(config)
}

/// Refresh the historical Brier score in the bankroll config from graded predictions in the DB.
/// This populates `historical_brier` (P3) so VolatilityAdjustedKelly can use real calibration data
/// to shrink stakes when past LLM forecasts were miscalibrated. Returns the computed brier (0.0 if none).
#[tauri::command]
pub async fn refresh_historical_brier(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<f64, String> {
    let mut config = crate::bankroll::load_bankroll_config();
    let brier = crate::bankroll::compute_historical_brier(&db_pool).await?;
    config.historical_brier = brier;
    crate::bankroll::save_bankroll_config(&config).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(brier)
}

