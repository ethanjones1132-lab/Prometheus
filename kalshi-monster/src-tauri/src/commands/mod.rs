use crate::chat::openrouter::{self, OpenRouterResponse};
use crate::chat::session;
use crate::config;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::football::data;
use crate::football::live_data;
use crate::football::player_stats;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use crate::predictions::grading::{self, GradingSummary};
use crate::prizepicks::PrizePicksFetcher;
use crate::weather::WeatherClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

type KalshiState = Arc<Mutex<crate::kalshi::KalshiClient>>;
type SharedCacheState = Arc<tokio::sync::RwLock<Option<crate::kalshi::KalshiCache>>>;

#[derive(Debug, serde::Serialize)]
pub struct KalshiDashboardBootstrap {
    pub markets: Vec<crate::kalshi::KalshiMarketSummary>,
    pub categories: Vec<crate::kalshi::KalshiCategoryStat>,
    pub cache_status: String,
    pub cache_age_secs: Option<u64>,
    pub partial_catalog: bool,
    pub last_refresh_at: Option<String>,
    pub market_count: usize,
    pub category_count: usize,
    pub dashboard_generated_at: String,
    pub data_quality_notes: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════
// Tauri Commands — Bridge between frontend and Rust backend
// ═══════════════════════════════════════════════════════════════

// ── Config Commands ──

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<Mutex<AppConfig>>>) -> Result<AppConfig, String> {
    Ok(state.lock().await.clone())
}

#[tauri::command]
pub async fn save_config(
    config: AppConfig,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
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
pub async fn get_security_posture(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<config::SecurityPosture, String> {
    let config = state.lock().await.clone();
    Ok(config::security_posture(&config))
}

#[tauri::command]
pub async fn get_available_models() -> Result<Vec<config::ModelInfo>, String> {
    Ok(config::available_models())
}

// ── Chat Commands ──

#[tauri::command]
pub async fn send_message(
    message: String,
    session_id: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<OpenRouterResponse, String> {
    let config = state.lock().await.clone();

    // Load messages from disk if not in memory
    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }

    // Get existing session messages for context
    let session_messages = {
        let cs = chat_state.lock().await;
        let mut messages = cs.get_messages(&session_id);
        if messages.len() > 24 {
            messages = messages.split_off(messages.len() - 24);
        }
            messages
                .into_iter()
                .map(|m| openrouter::ChatMessage {
                    role: m.role,
                    content: m.content,
                    reasoning: m.reasoning,
                })
                .collect::<Vec<_>>()
    };



    // Fetch Kalshi market context (Kalshi-first — replaces ESPN/Sleeper as default)
    let kalshi_context = {
        let mut kalshi = kalshi.lock().await;
        crate::chat::kalshi_context::build_kalshi_context(
            &mut kalshi,
            &message,
            None, // portfolio not injected by default in non-streaming path
        ).await
    };

    // Send to OpenRouter with enriched context + Kalshi data injection
    let response = openrouter::send_message(
        &config,
        &session_messages,
        message.clone(),
        None, // analysis_context: can be populated by frontend via generate_analysis_context
        Some(&db_pool),
        Some(&kalshi_context),
    )
    .await?;

    // Store user message
    let user_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: message,
        reasoning: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: None,
    };

    // Store assistant response
    let assistant_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: response.content.clone(),
        reasoning: response.reasoning.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: response.tokens_used,
    };

    // Update in-memory chat state
    {
        let mut cs = chat_state.lock().await;
        cs.add_message(&session_id, user_msg.clone());
        cs.add_message(&session_id, assistant_msg.clone());
    }

    // Persist to disk
    let all_messages = {
        let cs = chat_state.lock().await;
        cs.get_messages(&session_id)
    };
    let _ = session::save_session_messages(&session_id, &all_messages);

    // Auto-extract predictions from AI response
    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let record = PredictionRecord {
                prediction: pred,
                outcome: PredictionOutcome::Pending,
                actual_result: None,
                notes: None,
                resolved_at: None,
            };
            let _ = t.save_prediction(record).await;
        }
    }

    Ok(response)
}

/// Streaming chat command — sends chunks to the frontend via Tauri events.
/// The frontend listens for "stream-chunk" events to build the response in real-time.
#[tauri::command]
pub async fn send_message_stream(
    message: String,
    session_id: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
    app: tauri::AppHandle<tauri::Wry>,
) -> Result<(), String> {
    let config = state.lock().await.clone();

    // Load messages from disk if not in memory
    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }

    let session_messages = {
        let cs = chat_state.lock().await;
        let mut messages = cs.get_messages(&session_id);
        if messages.len() > 24 {
            messages = messages.split_off(messages.len() - 24);
        }
        messages
            .into_iter()
            .map(|m| openrouter::ChatMessage { role: m.role, content: m.content, reasoning: m.reasoning })
            .collect::<Vec<_>>()
    };



    // Create channel for streaming
    let (tx, mut rx) = mpsc::channel::<String>(256);
    let session_id_clone = session_id.clone();
    let app_clone = app.clone();

    // Spawn a task to forward chunks to Tauri events
    let forward_handle = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            if chunk == "__STREAM_DONE__" {
                let _ = app_clone.emit("stream-done", &session_id_clone);
                break;
            }
            if chunk.starts_with("__STREAM_ERROR__:") {
                let error_msg = &chunk["__STREAM_ERROR__:".len()..];
                let _ = app_clone.emit("stream-error", serde_json::json!({
                    "session_id": session_id_clone,
                    "error": error_msg,
                }));
                break;
            }
            if chunk.starts_with("__STREAM_THOUGHT__:") {
                let thought = &chunk["__STREAM_THOUGHT__:".len()..];
                let _ = app_clone.emit("stream-thought", serde_json::json!({
                    "session_id": session_id_clone,
                    "thought": thought,
                }));
                continue;
            }
            let _ = app_clone.emit("stream-chunk", serde_json::json!({
                "session_id": session_id_clone,
                "chunk": chunk,
            }));
        }
    });

    // Fetch live data context (ESPN + Sleeper) — real-time injection
    let kalshi_context = {
        let mut kalshi = kalshi.lock().await;
        crate::chat::kalshi_context::build_kalshi_context(
            &mut kalshi,
            &message,
            None,
        ).await
    };

    // Send to OpenRouter with streaming + Kalshi data injection
    let tx_after_stream = tx.clone();
    let response = match openrouter::stream_message(
        &config,
        &session_messages,
        message.clone(),
        None, // analysis_context: can be populated by frontend via generate_analysis_context
        Some(&db_pool),
        tx,
        Some(&kalshi_context),
    )
    .await
    {
        Ok(response) => response,
        Err(_error) => {
            // stream_message already sent __STREAM_ERROR__ through the channel before returning Err
            let _ = forward_handle.await;
            return Ok(());
        }
    };

    let _ = tx_after_stream.send("__STREAM_DONE__".to_string()).await;

    // Wait for the forwarder to finish
    let _ = forward_handle.await;

    // Store user message
    let user_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: message,
        reasoning: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: None,
    };

    let assistant_msg = session::ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: response.content.clone(),
        reasoning: response.reasoning.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        tokens_used: response.tokens_used,
    };

    {
        let mut cs = chat_state.lock().await;
        cs.add_message(&session_id, user_msg.clone());
        cs.add_message(&session_id, assistant_msg.clone());
    }

    let all_messages = {
        let cs = chat_state.lock().await;
        cs.get_messages(&session_id)
    };
    let _ = session::save_session_messages(&session_id, &all_messages);

    // Auto-extract predictions
    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let record = PredictionRecord {
                prediction: pred,
                outcome: PredictionOutcome::Pending,
                actual_result: None,
                notes: None,
                resolved_at: None,
            };
            let _ = t.save_prediction(record).await;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn new_chat_session(
    name: Option<String>,
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<session::ChatSession, String> {
    let config = state.lock().await;
    let session = session::create_session(name, &config.selected_model)?;
    Ok(session)
}

#[tauri::command]
pub async fn list_chat_sessions() -> Result<Vec<session::ChatSession>, String> {
    session::list_sessions()
}

#[tauri::command]
pub async fn delete_chat_session(session_id: String) -> Result<(), String> {
    session::delete_session(&session_id)
}

#[tauri::command]
pub async fn get_session_messages(
    session_id: String,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
) -> Result<Vec<session::ChatMessage>, String> {
    // Load from disk if not in memory
    {
        let mut cs = chat_state.lock().await;
        if !cs.sessions.contains_key(&session_id) {
            cs.load_from_disk(&session_id);
        }
    }
    let cs = chat_state.lock().await;
    Ok(cs.get_messages(&session_id))
}

// ── Football Data Commands ──

#[tauri::command]
pub async fn search_players(query: String) -> Result<Vec<data::PlayerSearchResult>, String> {
    Ok(data::search_players(&query))
}

#[tauri::command]
pub async fn get_game_schedule(
    week: Option<u32>,
) -> Result<Vec<data::GameInfo>, String> {
    match live_data::fetch_live_schedule().await {
        Ok(schedule) if !schedule.is_empty() => Ok(schedule),
        Ok(_) => Ok(data::get_game_schedule(week)),
        Err(_) => Ok(data::get_game_schedule(week)),
    }
}

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



// ── Weather Commands ──

#[tauri::command]
pub async fn get_game_weather(
    game: String,
    location: String,
    weather: State<'_, Arc<Mutex<WeatherClient>>>,
) -> Result<crate::weather::GameWeather, String> {
    let mut w = weather.lock().await;
    w.get_weather(&game, &location).await
}

// ── Live Data / Sports API Commands ──

/// Get live data source status (which APIs are available)
#[tauri::command]
pub async fn get_data_source_status(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<Vec<crate::football::api_client::DataSourceStatus>, String> {
    let config = state.lock().await.clone();
    let api_config = crate::football::api_client::SportsApiConfig {
        api_sports_key: config.api_sports_key.clone(),
        ..Default::default()
    };
    Ok(crate::football::api_client::check_all_sources(&api_config).await)
}

/// Fetch live NFL scoreboard from ESPN
#[tauri::command]
pub async fn fetch_live_scoreboard(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_scoreboard().await
}

/// Fetch NFL standings from ESPN
#[tauri::command]
pub async fn fetch_nfl_standings(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_standings().await
}

/// Fetch NFL news from ESPN
#[tauri::command]
pub async fn fetch_nfl_news(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_news().await
}

/// Fetch Sleeper NFL state (week, season, etc.)
#[tauri::command]
pub async fn fetch_sleeper_state(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_news().await
}

/// Fetch Sleeper injuries
#[tauri::command]
pub async fn fetch_sleeper_injuries(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_injuries().await
}

/// Fetch Sleeper player stats for a season/week
#[tauri::command]
pub async fn fetch_sleeper_stats(
    season: String,
    week: Option<u32>,
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_player_stats(&season, week).await
}

/// Fetch comprehensive live data context for AI enrichment.
/// Returns a JSON object with schedule, standings, news, and injuries.
#[tauri::command]
pub async fn fetch_live_data_context(
    query: String,
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;

    let mut result = serde_json::json!({});

    // Fetch scoreboard
    match client.espn_scoreboard().await {
        Ok(scoreboard) => {
            result["scoreboard"] = scoreboard;
        }
        Err(e) => {
            result["scoreboard_error"] = serde_json::json!(e);
        }
    }

    // Fetch standings
    match client.espn_standings().await {
        Ok(standings) => {
            result["standings"] = standings;
        }
        Err(e) => {
            result["standings_error"] = serde_json::json!(e);
        }
    }

    // Fetch news
    match client.espn_news().await {
        Ok(news) => {
            result["news"] = news;
        }
        Err(e) => {
            result["news_error"] = serde_json::json!(e);
        }
    }

    // Fetch Sleeper injuries
    match client.sleeper_injuries().await {
        Ok(injuries) => {
            result["injuries"] = injuries;
        }
        Err(e) => {
            result["injuries_error"] = serde_json::json!(e);
        }
    }

    // Fetch Sleeper state
    match client.sleeper_news().await {
        Ok(state) => {
            result["nfl_state"] = state;
        }
        Err(e) => {
            result["nfl_state_error"] = serde_json::json!(e);
        }
    }

    result["query"] = serde_json::json!(query);
    result["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

    Ok(result)
}

// ── Helper Functions ──

/// Input for generating bet recommendations from the frontend
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PickInput {
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: f64,
    pub confidence_score: Option<u8>,
}

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

// ── Multi-Sport Scoreboard Commands ──

/// Fetch the live scoreboard for a specific league from ESPN.
/// Supported leagues: "football" (NFL), "basketball" (NBA), "baseball" (MLB), "hockey" (NHL)
#[tauri::command]
pub async fn fetch_league_scoreboard(
    league: String,
) -> Result<serde_json::Value, String> {
    let (sport, league_code) = match league.to_lowercase().as_str() {
        "nfl" | "football" => ("football", "nfl"),
        "nba" | "basketball" => ("basketball", "nba"),
        "mlb" | "baseball" => ("baseball", "mlb"),
        "nhl" | "hockey" => ("hockey", "nhl"),
        _ => return Err(AppError::Validation(format!("Unsupported league: {}. Use NFL, NBA, MLB, or NHL.", league)).into()),
    };

    let base_url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/{}/{}",
        sport, league_code
    );

    let url = format!("{}/scoreboard", base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Network(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Api(format!("ESPN scoreboard request failed for {}: {}", league, e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Api(format!(
            "ESPN scoreboard returned HTTP {} for {}",
            resp.status(),
            league
        )).into());
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Api(format!("ESPN scoreboard parse error for {}: {}", league, e)))?;

    Ok(serde_json::json!({
        "league": league.to_uppercase(),
        "sport": sport,
        "scoreboard": json,
        "fetched_at": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Fetch scoreboards for all major leagues (NFL, NBA, MLB, NHL) in parallel.
/// Returns a JSON object keyed by league.
#[tauri::command]
pub async fn fetch_all_scoreboards() -> Result<serde_json::Value, String> {
    use futures::future::join_all;

    let leagues = vec!["football", "basketball", "baseball", "hockey"];
    let league_labels = vec!["NFL", "NBA", "MLB", "NHL"];

    let futures = leagues.iter().map(|&league| {
        let (sport, league_code) = match league {
            "football" => ("football", "nfl"),
            "basketball" => ("basketball", "nba"),
            "baseball" => ("baseball", "mlb"),
            "hockey" => ("hockey", "nhl"),
            _ => unreachable!(),
        };
        let url = format!(
            "https://site.api.espn.com/apis/site/v2/sports/{}/{}/scoreboard",
            sport, league_code
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        async move {
            let result = client.get(&url).send().await;
            (league, result)
        }
    });

    let results = join_all(futures).await;
    let mut scoreboards = serde_json::json!({});

    for (i, (_league, result)) in results.into_iter().enumerate() {
        let label = league_labels[i];
        match result {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    scoreboards[label] = json;
                } else {
                    scoreboards[label] = serde_json::json!({ "error": "parse failed" });
                }
            }
            Ok(resp) => {
                scoreboards[label] = serde_json::json!({ "error": format!("HTTP {}", resp.status()) });
            }
            Err(e) => {
                scoreboards[label] = serde_json::json!({ "error": e.to_string() });
            }
        }
    }

    scoreboards["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    Ok(scoreboards)
}

/// Get comprehensive data for a sport/league: scoreboard + standings + news.
/// Returns a JSON object with all available data for the league.
#[tauri::command]
pub async fn get_sport_league_data(
    league: String,
) -> Result<serde_json::Value, String> {
    let (sport, league_code) = match league.to_lowercase().as_str() {
        "nfl" | "football" => ("football", "nfl"),
        "nba" | "basketball" => ("basketball", "nba"),
        "mlb" | "baseball" => ("baseball", "mlb"),
        "nhl" | "hockey" => ("hockey", "nhl"),
        _ => return Err(AppError::Validation(format!("Unsupported league: {}. Use NFL, NBA, MLB, or NHL.", league)).into()),
    };

    let base_url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/{}/{}",
        sport, league_code
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Network(format!("HTTP client error: {}", e)))?;

    let mut result = serde_json::json!({
        "league": league.to_uppercase(),
        "sport": sport,
    });

    // Fetch scoreboard
    let scoreboard_url = format!("{}/scoreboard", base_url);
    match client.get(&scoreboard_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["scoreboard"] = json;
            }
        }
        Ok(resp) => {
            result["scoreboard_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["scoreboard_error"] = serde_json::json!(e.to_string());
        }
    }

    // Fetch standings
    let standings_url = format!("{}/standings", base_url);
    match client.get(&standings_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["standings"] = json;
            }
        }
        Ok(resp) => {
            result["standings_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["standings_error"] = serde_json::json!(e.to_string());
        }
    }

    // Fetch news
    let news_url = format!("{}/news", base_url);
    match client.get(&news_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["news"] = json;
            }
        }
        Ok(resp) => {
            result["news_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["news_error"] = serde_json::json!(e.to_string());
        }
    }

    result["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    Ok(result)
}

/// Key Call: Inject, store, and interpret multi-sport data dynamically
#[tauri::command]
pub async fn inject_sports_data(
    league: String,
    payload: serde_json::Value,
) -> Result<(), String> {
    data::inject_sports_data(&league, payload)
}

// ═══════════════════════════════════════════════════════════════
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

// ── File Upload Commands ──

/// Read a file from disk and return its content as a base64 string along with metadata.
/// The frontend can use this to let users attach files (CSV, JSON, images, etc.) to chat.
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<serde_json::Value, String> {
    use std::fs;
    use std::path::Path;

    let path = Path::new(&path);
    if !path.exists() {
        return Err(AppError::NotFound(format!("File not found: {}", path.display())).into());
    }

    let metadata = fs::metadata(path).map_err(|e| AppError::Io(format!("Failed to read file metadata: {}", e)))?;
    let size = metadata.len();

    // Limit file size to 5MB
    const MAX_SIZE: u64 = 5 * 1024 * 1024;
    if size > MAX_SIZE {
        return Err(AppError::Validation(format!(
            "File too large: {} bytes (max {} bytes / 5MB)",
            size, MAX_SIZE
        )).into());
    }

    let content = fs::read(path).map_err(|e| AppError::Io(format!("Failed to read file: {}", e)))?;
    use base64::{Engine as _, engine::general_purpose};
    let base64_content = general_purpose::STANDARD.encode(&content);

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    Ok(serde_json::json!({
        "name": file_name,
        "path": path.to_string_lossy().to_string(),
        "size": size,
        "extension": extension,
        "content_base64": base64_content,
    }))
}

// ═══════════════════════════════════════════════════════════════
// Live Player Stats Commands — Multi-Sport API Integration
// ═══════════════════════════════════════════════════════════════

/// Fetch live season leaders for a sport league.
/// Returns top players across all major stat categories.
#[tauri::command]
pub async fn fetch_season_leaders(
    league: String,
    season: Option<u32>,
) -> Result<Vec<player_stats::PlayerStatProfile>, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_season_leaders(league).await
}

/// Fetch live player stats by athlete ID.
#[tauri::command]
pub async fn fetch_player_stats_by_id(
    league: String,
    athlete_id: String,
    season: Option<u32>,
) -> Result<player_stats::PlayerStatProfile, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_player_stats(league, &athlete_id).await
}

/// Fetch all players for a team (roster).
#[tauri::command]
pub async fn fetch_team_players(
    league: String,
    team_id: String,
    season: Option<u32>,
) -> Result<player_stats::TeamPlayerStats, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_team_players(league, &team_id).await
}

/// Fetch season leaders as a categorized map.
/// Returns stat categories with top players in each.
#[tauri::command]
pub async fn fetch_season_leaders_map(
    league: String,
    season: Option<u32>,
) -> Result<player_stats::SeasonLeaders, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_season_leaders_map(league).await
}

/// Fetch live player stats for multiple leagues concurrently.
/// Useful for getting a cross-sport overview.
#[tauri::command]
pub async fn fetch_multi_sport_leaders(
    leagues: Vec<String>,
    season: Option<u32>,
) -> Result<serde_json::Value, String> {
    use futures::future::join_all;

    let futures = leagues.into_iter().map(|league_str| {
        let season = season;
        async move {
            let league = match league_str.parse::<live_data::SportLeague>() {
                Ok(l) => l,
                Err(e) => return (league_str, Err(String::from(AppError::Validation(format!("Invalid league: {}", e))))),
            };
            let fetcher = match season {
                Some(s) => player_stats::PlayerStatsFetcher::new(league).unwrap().with_season(s),
                None => player_stats::PlayerStatsFetcher::new(league).unwrap(),
            };
            let result = fetcher.fetch_season_leaders(league).await.map_err(String::from);
            (league.short_name().to_string(), result)
        }
    });

    let results: Vec<(String, Result<Vec<player_stats::PlayerStatProfile>, String>)> =
        join_all(futures).await;

    let mut map = serde_json::Map::new();
    for (name, result) in results {
        match result {
            Ok(profiles) => {
                map.insert(name, serde_json::to_value(profiles).unwrap_or_default());
            }
            Err(e) => {
                map.insert(name, serde_json::json!({ "error": e }));
            }
        }
    }

    Ok(serde_json::Value::Object(map))
}

/// Build a live player stats context string for AI injection.
/// This fetches real-time stats and formats them for the AI prompt.
#[tauri::command]
pub async fn build_live_player_context(
    league: String,
    max_players: Option<usize>,
) -> Result<String, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = player_stats::PlayerStatsFetcher::new(league)?;
    let profiles = fetcher.fetch_season_leaders(league).await?;
    let max = max_players.unwrap_or(15);
    Ok(player_stats::build_live_player_context(&profiles, league, max))
}

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
    let mut client = kalshi.lock().await;
    client.get_markets_by_category(&category).await
}

/// Fetch a single market by ticker.
#[tauri::command]
pub async fn kalshi_get_market(
    ticker: String,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::kalshi::KalshiMarketSummary, String> {
    let client = kalshi.lock().await;
    let market = client.fetch_market(&ticker).await?;
    Ok(crate::kalshi::KalshiMarketSummary::from(&market))
}

/// Fetch the orderbook for a market by ticker.
#[tauri::command]
pub async fn kalshi_get_orderbook(
    ticker: String,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::kalshi::KalshiOrderbook, String> {
    let client = kalshi.lock().await;
    client.fetch_orderbook(&ticker).await
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
    let mut client = kalshi.lock().await;
    client.search_markets(&query).await
}

/// Get the top markets by 24h trading volume.
#[tauri::command]
pub async fn kalshi_get_top_markets(
    limit: Option<usize>,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiMarketSummary>, String> {
    let n = limit.unwrap_or(30).min(100);
    let mut client = kalshi.lock().await;
    client.get_top_markets(n).await
}

/// Initial dashboard payload: top markets, category stats, and cache freshness in one IPC call.
#[tauri::command]
pub async fn kalshi_get_dashboard_bootstrap(
    limit: Option<usize>,
    kalshi: State<'_, KalshiState>,
) -> Result<KalshiDashboardBootstrap, String> {
    let n = limit.unwrap_or(30).min(100);
    let mut client = kalshi.lock().await;
    let markets = client.get_top_markets(n).await?;
    let categories = client.category_stats();
    let (cache_status, cache_age_secs, partial_catalog, fetched_at) = client.cache_metadata();
    let last_refresh_at = fetched_at
        .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
        .map(|dt| dt.to_rfc3339());
    let market_count = markets.len();
    let category_count = categories.len();
    let data_quality_notes = if partial_catalog {
        vec!["Partial catalog loaded for fast first paint".to_string()]
    } else {
        vec!["Full catalog cache ready".to_string()]
    };

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
    })
}

/// Get per-category market counts and 24h volumes.
#[tauri::command]
pub async fn kalshi_get_category_stats(
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::kalshi::KalshiCategoryStat>, String> {
    let client = kalshi.lock().await;
    Ok(client.category_stats())
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
        let mut client = kalshi.lock().await;
        if client.config.email != app_cfg.kalshi_email
            || client.config.password != app_cfg.kalshi_password
        {
            let new_cfg = crate::kalshi::kalshi_config_from_app(&app_cfg);
            client.config = new_cfg;
            client.invalidate_cache();
        }
    }

    let mut client = kalshi.lock().await;
    let balance = client.get_balance().await?;
    let positions = client.get_positions().await?;

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
        let mut client = kalshi.lock().await;
        let new_cfg = crate::kalshi::kalshi_config_from_app(&app_cfg);
        client.config = new_cfg;
        client.invalidate_cache();
    }
    let mut client = kalshi.lock().await;
    let markets = client.fetch_all_markets().await?;
    let summaries: Vec<crate::kalshi::KalshiMarketSummary> =
        markets.iter().map(crate::kalshi::KalshiMarketSummary::from).collect();
    if let Err(e) = crate::kalshi::price_tracker::snapshot_markets(&db_pool, &summaries).await {
        tracing::warn!("kalshi price snapshot on refresh: {}", e);
    }
    Ok(markets.len())
}

// ═══════════════════════════════════════════════════════════════
// Kalshi Prediction Tracking — Performance & Grading
// ═══════════════════════════════════════════════════════════════

use crate::kalshi::models::{KalshiPrediction, KalshiPredictionStats, KalshiGradingSummary};

/// Get all Kalshi predictions
#[tauri::command]
pub async fn kalshi_get_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<Vec<KalshiPrediction>, String> {
    let t = tracker.lock().await;
    Ok(t.get_kalshi_predictions().await)
}

/// Get Kalshi prediction stats
#[tauri::command]
pub async fn kalshi_get_prediction_stats(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<KalshiPredictionStats, String> {
    let t = tracker.lock().await;
    let all = t.get_kalshi_predictions().await;
    Ok(t.get_kalshi_stats(&all).await)
}

/// Grade pending Kalshi predictions against resolved market outcomes
#[tauri::command]
pub async fn kalshi_grade_pending_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
) -> Result<KalshiGradingSummary, String> {
    let t = tracker.lock().await;
    let client = kalshi.lock().await;
    crate::kalshi::grade_pending_predictions(&t, &client).await
}

/// Portfolio-aware Kelly stake scaling for a proposed Kalshi trade.
#[tauri::command]
pub async fn kalshi_compute_stake_adjustment(
    ticker: String,
    category: String,
    contract_side: String,
    recommended_stake: f64,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::StakeAdjustment, String> {
    let pending = {
        let t = tracker.lock().await;
        t.get_kalshi_predictions().await
    };
    let mut exposures = crate::kalshi::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );

    if let Ok(positions) = kalshi.lock().await.get_positions().await {
        exposures.extend(crate::kalshi::exposures_from_positions(&positions));
    }

    let mut adjustment = crate::kalshi::compute_stake_adjustment(
        &ticker,
        &category,
        Some(&contract_side),
        recommended_stake,
        &exposures,
    );

    let bankroll = crate::bankroll::load_bankroll_config();
    match crate::bankroll::get_bankroll_summary_synced(&bankroll, &db_pool).await {
        Ok(summary) => {
            adjustment.remaining_daily = summary.remaining_daily;
            adjustment.remaining_weekly = summary.remaining_weekly;
            adjustment.bankroll_cap = summary.remaining_daily.min(summary.remaining_weekly);
            let (capped_stake, warning) = crate::bankroll::apply_bankroll_cap(
                adjustment.adjusted_recommended_stake,
                &summary,
            );
            if capped_stake < adjustment.adjusted_recommended_stake {
                let old = adjustment.adjusted_recommended_stake;
                adjustment.adjusted_recommended_stake = capped_stake;
                adjustment.kelly_scale = if old > 0.0 {
                    (capped_stake / old).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            if let Some(warning) = warning {
                adjustment.warnings.push(warning);
            }
        }
        Err(e) => {
            tracing::warn!("bankroll cap sync skipped for stake adjustment: {}", e);
        }
    }

    Ok(adjustment)
}

#[tauri::command]
pub async fn kalshi_get_calibration_status(
    raw_probability_pct: f64,
) -> Result<crate::analysis::calibration::CalibrationStatus, String> {
    Ok(crate::analysis::calibration::calibration_status_for_probability(
        raw_probability_pct,
    ))
}

/// Snapshot current Kalshi market prices into local history.
#[tauri::command]
pub async fn kalshi_snapshot_prices(
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::KalshiSnapshotBatch, String> {
    let mut client = kalshi.lock().await;
    let markets = client.fetch_all_markets().await?;
    let summaries: Vec<crate::kalshi::KalshiMarketSummary> =
        markets.iter().map(crate::kalshi::KalshiMarketSummary::from).collect();
    crate::kalshi::price_tracker::snapshot_markets(&db_pool, &summaries).await
}

/// Fetch stored price/spread history for a ticker.
#[tauri::command]
pub async fn kalshi_get_price_history(
    ticker: String,
    limit: Option<i64>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::KalshiPriceHistory, String> {
    crate::kalshi::price_tracker::get_price_history(&db_pool, &ticker, limit.unwrap_or(200)).await
}

/// Record a paper-trade decision with calibration + correlation-adjusted sizing.
#[tauri::command]
pub async fn kalshi_record_paper_decision(
    session_id: String,
    mut decision: crate::chat::decision_schema::KalshiTradeDecision,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    let bankroll = crate::bankroll::load_bankroll_config();
    let bankroll_summary = match crate::bankroll::get_bankroll_summary_synced(&bankroll, &db_pool).await {
        Ok(summary) => Some(summary),
        Err(e) => {
            tracing::warn!("bankroll cap sync skipped for paper decision: {}", e);
            None
        }
    };
    let pending = {
        let t = tracker.lock().await;
        t.get_kalshi_predictions().await
    };
    let mut exposures = crate::kalshi::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );
    if let Ok(positions) = kalshi.lock().await.get_positions().await {
        exposures.extend(crate::kalshi::exposures_from_positions(&positions));
    }

    let side = format!("{:?}", decision.contract_side);
    let raw_stake = if decision.recommended_stake_dollars > 0.0 {
        decision.recommended_stake_dollars
    } else {
        bankroll.total_bankroll * (decision.fractional_kelly_pct / 100.0)
    };
    let mut adj = crate::kalshi::compute_stake_adjustment(
        &decision.ticker,
        &decision.category,
        Some(&side),
        raw_stake,
        &exposures,
    );
    decision.compute_risk_adjusted(
        bankroll.total_bankroll,
        bankroll.kelly_fraction,
        adj.kelly_scale,
        true,
    );

    if let Some(summary) = &bankroll_summary {
        let (capped_stake, warning) =
            crate::bankroll::apply_bankroll_cap(decision.recommended_stake_dollars, summary);
        if capped_stake < decision.recommended_stake_dollars {
            let old_stake = decision.recommended_stake_dollars;
            decision.recommended_stake_dollars = capped_stake;
            decision.max_position_dollars = decision.max_position_dollars.min(capped_stake);
            if !decision.risk_flags.contains(&crate::chat::decision_schema::RiskFlag::BankrollLimitExceeded) {
                decision.risk_flags.push(crate::chat::decision_schema::RiskFlag::BankrollLimitExceeded);
            }
            if let Some(warning) = warning {
                adj.warnings.push(warning.clone());
                if !decision.thesis.is_empty() {
                    decision.thesis.push(' ');
                }
                decision.thesis.push_str(&warning);
            }
            tracing::info!(
                "paper decision capped by bankroll: {} ${:.2} -> ${:.2}",
                decision.ticker,
                old_stake,
                capped_stake
            );
        }
    }

    let prediction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let decision_json = serde_json::to_string(&decision)
        .map_err(|e| format!("serialize decision: {}", e))?;
    let pick_type = match decision.contract_side {
        crate::chat::decision_schema::ContractSide::YES => Some("Over".to_string()),
        crate::chat::decision_schema::ContractSide::NO => Some("Under".to_string()),
        crate::chat::decision_schema::ContractSide::PASS => None,
    };

    let prediction = crate::predictions::tracker::Prediction {
        id: prediction_id.clone(),
        session_id,
        raw_text: decision_json.clone(),
        player_name: Some(decision.ticker.clone()),
        pick_type,
        line: Some(decision.recommended_stake_dollars),
        stat_category: Some(decision.category.clone()),
        confidence: Some(format!("{:?}", decision.confidence_tier)),
        confidence_score: None,
        probability: Some(decision.fair_probability_pct),
        reasoning: if decision.thesis.is_empty() {
            None
        } else {
            Some(decision.thesis.clone())
        },
        risk: if adj.warnings.is_empty() {
            None
        } else {
            Some(adj.warnings.join("; "))
        },
        created_at: now,
        full_decision_json: Some(decision_json.clone()),
        entry_price: Some(decision.market_price_pct),
        model_disagreement: decision.model_disagreement,
    };

    let record = PredictionRecord {
        prediction,
        outcome: PredictionOutcome::Pending,
        actual_result: None,
        notes: Some(format!(
            "Paper trade: {:?} {} @ {:.2} (kelly_scale {:.0}%)",
            decision.contract_side,
            decision.ticker,
            decision.price_to_enter,
            adj.kelly_scale * 100.0
        )),
        resolved_at: None,
    };

    let t = tracker.lock().await;
    t.save_prediction(record).await?;

    if decision.contract_side != crate::chat::decision_schema::ContractSide::PASS {
        let entry_cents = crate::paper::normalize_entry_cents(decision.price_to_enter);
        let stake = decision.recommended_stake_dollars.max(0.0);
        if stake > 0.0 && entry_cents > 0.0 && entry_cents < 100.0 {
            let qty = stake / (entry_cents / 100.0);
            let side = format!("{:?}", decision.contract_side);
            let trade_input = crate::paper::PaperTradeInput {
                ticker: decision.ticker.clone(),
                title: decision.market_title.clone(),
                category: decision.category.clone(),
                side,
                qty,
                entry_price_cents: entry_cents,
                source: crate::paper::PaperTradeSource::Manual,
                decision_json: Some(decision_json),
            };
            match crate::paper::place_trade(&db_pool, trade_input).await {
                Ok(lot) => {
                    tracing::info!(
                        "paper lot opened: {} {:?} qty {:.2} @ {:.1}c",
                        lot.ticker,
                        decision.contract_side,
                        lot.qty,
                        lot.entry_price_cents
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "paper lot not opened for {} (prediction {} saved): {}",
                        decision.ticker,
                        prediction_id,
                        e
                    );
                }
            }
        }
    }

    Ok(prediction_id)
}

// ═══════════════════════════════════════════════════════════════
// Paper trading journal
// ═══════════════════════════════════════════════════════════════

/// Paper account analytics (cash, equity, win rate, drawdown).
#[tauri::command]
pub async fn paper_get_analytics(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::paper::PaperAnalytics, String> {
    let client = kalshi.lock().await;
    crate::paper::get_analytics(&db_pool, Some(&*client)).await
}

/// Open paper positions aggregated by ticker/side.
#[tauri::command]
pub async fn paper_get_positions(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<Vec<crate::paper::PaperPosition>, String> {
    let client = kalshi.lock().await;
    crate::paper::aggregate_positions(&db_pool, Some(&*client)).await
}

/// Settle open paper lots against resolved Kalshi markets.
#[tauri::command]
pub async fn paper_settle_pending(
    db_pool: State<'_, Pool<Sqlite>>,
    kalshi: State<'_, KalshiState>,
) -> Result<crate::paper::PaperSettlementSummary, String> {
    let client = kalshi.lock().await;
    crate::paper::settle_pending(&db_pool, &client).await
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
// Odds Comparison — Multi-Source Real-Time Line Comparison
// ═══════════════════════════════════════════════════════════════

/// Normalized line from any source, used for cross-source comparison.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceLine {
    pub source: String,
    pub line: Option<f64>,
    pub projection: Option<f64>,
    pub over_odds: Option<f64>,
    pub under_odds: Option<f64>,
    pub implied_probability: Option<f64>,
    pub last_updated: Option<String>,
    pub raw_data: serde_json::Value,
}

/// A single player's prop compared across multiple sources.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerOddsComparison {
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub stat_category: String,
    pub league: String,
    pub game_time: Option<String>,
    pub sources: Vec<SourceLine>,
    pub best_over: Option<BestOdds>,
    pub best_under: Option<BestOdds>,
    pub line_spread: f64,
    pub has_arbitrage: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BestOdds {
    pub source: String,
    pub value: f64,
}

/// An arbitrage or value opportunity detected across sources.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArbitrageOpportunity {
    pub player_name: String,
    pub stat_category: String,
    pub league: String,
    pub arb_type: String,
    pub description: String,
    pub profit_pct: Option<f64>,
    pub legs: Vec<ArbitrageLeg>,
    pub confidence: String,
    pub detected_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArbitrageLeg {
    pub source: String,
    pub pick: String,
    pub line: f64,
    pub odds: Option<f64>,
    pub implied_probability: Option<f64>,
}

/// Full odds comparison response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OddsComparisonData {
    pub comparisons: Vec<PlayerOddsComparison>,
    pub arbitrage_opportunities: Vec<ArbitrageOpportunity>,
    pub sources_queried: Vec<String>,
    pub sources_available: Vec<String>,
    pub sources_failed: Vec<SourceFailure>,
    pub fetched_at: String,
    pub query_league: Option<String>,
    pub query_player: Option<String>,
    pub total_props_found: usize,
    pub total_with_multiple_sources: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceFailure {
    pub source: String,
    pub error: String,
}

// Odds comparison and other commands removed for Kalshi Monster

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

    if let Some(url) = bot_settings.get("discord_webhook_url").and_then(|v| v.as_str()) {
        config.discord_webhook_url = url.to_string();
    }
    if let Some(token) = bot_settings.get("telegram_bot_token").and_then(|v| v.as_str()) {
        config.telegram_bot_token = token.to_string();
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

// ═══════════════════════════════════════════════════════════════
// Line Movement Tracking Commands
// ═══════════════════════════════════════════════════════════════

/// Take a snapshot of current PrizePicks props for line movement tracking
#[tauri::command]
pub async fn snapshot_line_movements(
    fetcher: State<'_, Arc<Mutex<PrizePicksFetcher>>>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<serde_json::Value, String> {
    let props = {
        let mut f = fetcher.lock().await;
        f.fetch_props(None, true).await?
    };

    let result = crate::line_tracker::snapshot_props(
        &db_pool,
        &props.props,
        &props.source.to_string(),
    )
    .await?;

    Ok(serde_json::json!({
        "snapshots_taken": result.snapshots_taken,
        "new_props": result.new_props,
        "updated_props": result.updated_props,
        "snapshot_at": result.snapshot_at,
    }))
}

/// Get line movement summaries with filtering
#[tauri::command]
pub async fn get_line_movements(
    filter: crate::line_tracker::LineMovementFilter,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::line_tracker::LineMovementPage, String> {
    crate::line_tracker::get_line_summaries(&db_pool, &filter).await
}

/// Get detailed line history for a specific prop
#[tauri::command]
pub async fn get_line_detail(
    prop_key: String,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<crate::line_tracker::LineDetailHistory>, String> {
    crate::line_tracker::get_line_detail(&db_pool, &prop_key).await
}

/// Get list of tracked leagues
#[tauri::command]
pub async fn get_tracked_line_leagues(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<String>, String> {
    crate::line_tracker::get_tracked_leagues(&db_pool).await
}

/// Get list of tracked stat categories
#[tauri::command]
pub async fn get_tracked_line_stat_categories(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<String>, String> {
    crate::line_tracker::get_tracked_stat_categories(&db_pool).await
}

/// Get the latest snapshot timestamp
#[tauri::command]
pub async fn get_latest_line_snapshot(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<String>, String> {
    crate::line_tracker::get_latest_snapshot_time(&db_pool).await
}

/// Prune old line movement snapshots
#[tauri::command]
pub async fn prune_line_movements(
    retention_days: i64,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<u64, String> {
    crate::line_tracker::prune_old_snapshots(&db_pool, retention_days).await
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

    // Check fetch_in_progress through the client (brief lock)
    let fetch_in_progress = {
        let client = kalshi.lock().await;
        client.is_fetch_in_progress()
    };

    Ok(KalshiCacheStateResponse {
        has_cache,
        is_stale,
        full_catalog,
        market_count,
        cache_age_secs,
        fetch_in_progress,
    })
}
