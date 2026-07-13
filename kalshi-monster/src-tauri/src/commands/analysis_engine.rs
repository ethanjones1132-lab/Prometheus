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
// Analysis Engine Tauri Commands — Expose mathematical analysis
// to the frontend and wire into OpenRouter chat flow
// ═══════════════════════════════════════════════════════════════

/// Input for single prop edge analysis
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct EdgeAnalysisInput {
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub pick_type: String,
    pub projection: f64,
    pub season_avg: f64,
    pub last3_avg: f64,
    pub home_avg: Option<f64>,
    pub away_avg: Option<f64>,
    pub is_home: bool,
    pub defense_rank: Option<u32>,
    pub pace_rank: Option<u32>,
    pub usage_rate: Option<f64>,
    pub opponent_pace_rank: Option<u32>,
    pub park_factor: Option<f64>,
    pub goalie_quality_rank: Option<u32>,
    pub consistency_score: Option<f64>,
}

/// Full analysis result for a single prop (edge + score)
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PropAnalysisResult {
    pub edge: crate::analysis::edge_calculator::EdgeScore,
    pub scored: crate::analysis::prop_scorer::ScoredProp,
}

/// Run the full analysis pipeline on a single prop
#[tauri::command]
pub async fn analyze_prop(input: EdgeAnalysisInput) -> Result<PropAnalysisResult, String> {
    let analysis_input = crate::analysis::context::AnalysisInput {
        player_name: input.player_name,
        stat_category: input.stat_category,
        line: input.line,
        pick_type: input.pick_type,
        projection: input.projection,
        season_avg: input.season_avg,
        last3_avg: input.last3_avg,
        home_avg: input.home_avg,
        away_avg: input.away_avg,
        is_home: input.is_home,
        defense_rank: input.defense_rank,
        pace_rank: input.pace_rank,
        usage_rate: input.usage_rate,
        opponent_pace_rank: input.opponent_pace_rank,
        park_factor: input.park_factor,
        goalie_quality_rank: input.goalie_quality_rank,
        consistency_score: input.consistency_score,
    };

    let (edge, scored) = crate::analysis::context::analyze_single_prop(&analysis_input);

    Ok(PropAnalysisResult { edge, scored })
}

/// Analyze multiple props and return scored + ranked results
#[tauri::command]
pub async fn analyze_multiple_props(
    inputs: Vec<EdgeAnalysisInput>,
) -> Result<crate::analysis::context::AnalysisContext, String> {
    let analysis_inputs: Vec<crate::analysis::context::AnalysisInput> = inputs
        .into_iter()
        .map(|input| crate::analysis::context::AnalysisInput {
            player_name: input.player_name,
            stat_category: input.stat_category,
            line: input.line,
            pick_type: input.pick_type,
            projection: input.projection,
            season_avg: input.season_avg,
            last3_avg: input.last3_avg,
            home_avg: input.home_avg,
            away_avg: input.away_avg,
            is_home: input.is_home,
            defense_rank: input.defense_rank,
            pace_rank: input.pace_rank,
            usage_rate: input.usage_rate,
            opponent_pace_rank: input.opponent_pace_rank,
            park_factor: input.park_factor,
            goalie_quality_rank: input.goalie_quality_rank,
            consistency_score: input.consistency_score,
        })
        .collect();

    Ok(crate::analysis::context::analyze_multiple_props(&analysis_inputs))
}

/// Analyze parlay correlation for a set of picks
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ParlayLegInput {
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: Option<f64>,
    pub confidence_score: Option<u8>,
}

#[tauri::command]
pub async fn analyze_parlay_correlation(
    legs: Vec<ParlayLegInput>,
) -> Result<crate::analysis::parlay_correlation::ParlayAnalysis, String> {
    let picks: Vec<crate::correlation::CorrelationPick> = legs
        .into_iter()
        .map(|leg| crate::correlation::CorrelationPick {
            player_name: leg.player_name,
            team: leg.team,
            opponent: leg.opponent,
            prop_category: leg.prop_category,
            line: leg.line,
            pick_type: leg.pick_type,
            win_probability: leg.win_probability,
            confidence_score: leg.confidence_score,
        })
        .collect();

    Ok(crate::analysis::context::analyze_parlay(&picks))
}

/// Generate compact analysis context string for AI prompt injection
#[tauri::command]
pub async fn generate_analysis_context(
    inputs: Vec<EdgeAnalysisInput>,
) -> Result<String, String> {
    let analysis_inputs: Vec<crate::analysis::context::AnalysisInput> = inputs
        .into_iter()
        .map(|input| crate::analysis::context::AnalysisInput {
            player_name: input.player_name,
            stat_category: input.stat_category,
            line: input.line,
            pick_type: input.pick_type,
            projection: input.projection,
            season_avg: input.season_avg,
            last3_avg: input.last3_avg,
            home_avg: input.home_avg,
            away_avg: input.away_avg,
            is_home: input.is_home,
            defense_rank: input.defense_rank,
            pace_rank: input.pace_rank,
            usage_rate: input.usage_rate,
            opponent_pace_rank: input.opponent_pace_rank,
            park_factor: input.park_factor,
            goalie_quality_rank: input.goalie_quality_rank,
            consistency_score: input.consistency_score,
        })
        .collect();

    let ctx = crate::analysis::context::analyze_multiple_props(&analysis_inputs);
    Ok(ctx.to_prompt_context())
}

/// Get scored props filtered by minimum tier
#[tauri::command]
pub async fn get_scored_props_by_tier(
    inputs: Vec<EdgeAnalysisInput>,
    min_tier: String,
) -> Result<Vec<crate::analysis::prop_scorer::ScoredProp>, String> {
    let analysis_inputs: Vec<crate::analysis::context::AnalysisInput> = inputs
        .into_iter()
        .map(|input| crate::analysis::context::AnalysisInput {
            player_name: input.player_name,
            stat_category: input.stat_category,
            line: input.line,
            pick_type: input.pick_type,
            projection: input.projection,
            season_avg: input.season_avg,
            last3_avg: input.last3_avg,
            home_avg: input.home_avg,
            away_avg: input.away_avg,
            is_home: input.is_home,
            defense_rank: input.defense_rank,
            pace_rank: input.pace_rank,
            usage_rate: input.usage_rate,
            opponent_pace_rank: input.opponent_pace_rank,
            park_factor: input.park_factor,
            goalie_quality_rank: input.goalie_quality_rank,
            consistency_score: input.consistency_score,
        })
        .collect();

    let ctx = crate::analysis::context::analyze_multiple_props(&analysis_inputs);

    let min_tier_enum = match min_tier.as_str() {
        "Elite" => crate::analysis::prop_scorer::PropTier::Elite,
        "Strong" => crate::analysis::prop_scorer::PropTier::Strong,
        "Playable" => crate::analysis::prop_scorer::PropTier::Playable,
        "Marginal" => crate::analysis::prop_scorer::PropTier::Marginal,
        _ => crate::analysis::prop_scorer::PropTier::Avoid,
    };

    let filtered: Vec<crate::analysis::prop_scorer::ScoredProp> = ctx
        .scored_props
        .into_iter()
        .filter(|p| {
            let score = p.composite_score;
            match min_tier_enum {
                crate::analysis::prop_scorer::PropTier::Elite => score >= 80.0,
                crate::analysis::prop_scorer::PropTier::Strong => score >= 65.0,
                crate::analysis::prop_scorer::PropTier::Playable => score >= 50.0,
                crate::analysis::prop_scorer::PropTier::Marginal => score >= 35.0,
                crate::analysis::prop_scorer::PropTier::Avoid => true,
            }
        })
        .collect();

    Ok(filtered)
}

// ── Bet Slip OCR Commands ──

#[tauri::command]
pub async fn create_prediction_from_ocr(
    session_id: String,
    player_name: String,
    stat_category: String,
    line: f64,
    pick_type: String,
    source: String,
    stake: Option<f64>,
    potential_payout: Option<f64>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
) -> Result<String, String> {
    let prediction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let raw_text = format!(
        "[Bet Slip OCR - {}] {} {} {} {}",
        source, player_name, pick_type, line, stat_category
    );

    let notes = match (stake, potential_payout) {
        (Some(s), Some(p)) => Some(format!("Stake: ${:.2}, Potential Payout: ${:.2}", s, p)),
        (Some(s), None) => Some(format!("Stake: ${:.2}", s)),
        (None, Some(p)) => Some(format!("Potential Payout: ${:.2}", p)),
        (None, None) => None,
    };

    let prediction = crate::predictions::tracker::Prediction {
        id: prediction_id.clone(),
        session_id: session_id.clone(),
        raw_text,
        player_name: if player_name.is_empty() { None } else { Some(player_name) },
        pick_type: if pick_type.is_empty() { None } else { Some(pick_type) },
        line: if line > 0.0 { Some(line) } else { None },
        stat_category: if stat_category.is_empty() { None } else { Some(stat_category) },
        confidence: None,
        confidence_score: None,
        probability: None,
        reasoning: None,
        risk: None,
        created_at: now,
        full_decision_json: None,
        entry_price: None,
        model_disagreement: false,
    };

    let record = PredictionRecord {
        prediction,
        outcome: PredictionOutcome::Pending,
        actual_result: None,
        notes,
        resolved_at: None,
    };

    let t = tracker.lock().await;
    t.save_prediction(record).await?;

    Ok(prediction_id)
}


