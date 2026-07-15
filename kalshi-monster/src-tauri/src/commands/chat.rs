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

// ── Chat Commands ──

#[tauri::command]
pub async fn send_message(
    message: String,
    session_id: String,
    state: State<'_, Arc<Mutex<AppConfig>>>,
    chat_state: State<'_, Arc<Mutex<session::ChatState>>>,
    tracker: State<'_, Arc<Mutex<PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    fincept: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
    db_pool: State<'_, Pool<Sqlite>>,
    app: tauri::AppHandle<tauri::Wry>,
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



    // Fetch Kalshi market context + gated web evidence (tape gates first)
    let kalshi_context = {
        emit_chat_kalshi_context(&app, &session_id, &kalshi);
        let built = crate::chat::kalshi_context::build_kalshi_context_full(
            &kalshi,
            &message,
            None, // portfolio not injected by default in non-streaming path
        )
        .await;
        let any_open = !built.open_markets.is_empty();
        let web = crate::chat::web_context::gather_web_evidence(
            &message,
            &built.open_markets,
            any_open,
            Some(config.brave_api_key.as_str()),
        )
        .await;
        let mut ctx = built.context;
        ctx.push_str(&web.to_prompt_block());
        crate::chat::fincept_context::append_fincept_context_for_query(
            fincept.inner().as_ref(),
            &mut ctx,
            &message,
        )
        .await;
        // Local graded history — confidence tempering only (no math change).
        ctx.push_str(&crate::chat::track_record::track_record_prompt_block(&db_pool).await);
        // Phase A: Fincept agent priors for a few open retrieved markets.
        let edge_cfg = edge_config_for_pool(&db_pool).await;
        crate::chat::agent_priors::append_agent_priors_for_chat(
            &mut ctx,
            kalshi.as_ref(),
            fincept.inner().as_ref(),
            &db_pool,
            &edge_cfg,
            &built.open_markets,
        )
        .await;
        ctx
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

    // Auto-extract predictions; ledger via edge pipeline when possible (Phase A3).
    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let _ = persist_chat_forecast_unified(
                &db_pool,
                &pred,
                &kalshi,
                fincept.inner().as_ref(),
            )
            .await;
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
    fincept: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
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
    let forward_handle = tauri::async_runtime::spawn(async move {
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

    // Fetch live data context — Kalshi gates + optional web + Fincept + agent priors
    let kalshi_context = {
        emit_chat_kalshi_context(&app, &session_id, &kalshi);
        let built = crate::chat::kalshi_context::build_kalshi_context_full(
            &kalshi,
            &message,
            None,
        )
        .await;
        let any_open = !built.open_markets.is_empty();
        let web = crate::chat::web_context::gather_web_evidence(
            &message,
            &built.open_markets,
            any_open,
            Some(config.brave_api_key.as_str()),
        )
        .await;
        let mut ctx = built.context;
        ctx.push_str(&web.to_prompt_block());
        crate::chat::fincept_context::append_fincept_context_for_query(
            fincept.inner().as_ref(),
            &mut ctx,
            &message,
        )
        .await;
        ctx.push_str(&crate::chat::track_record::track_record_prompt_block(&db_pool).await);
        let edge_cfg = edge_config_for_pool(&db_pool).await;
        crate::chat::agent_priors::append_agent_priors_for_chat(
            &mut ctx,
            kalshi.as_ref(),
            fincept.inner().as_ref(),
            &db_pool,
            &edge_cfg,
            &built.open_markets,
        )
        .await;
        ctx
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

    // Auto-extract predictions; ledger via edge pipeline when possible (Phase A3).
    {
        let t = tracker.lock().await;
        let extracted = t.extract_predictions(&session_id, &response.content);
        for pred in extracted {
            let _ = persist_chat_forecast_unified(
                &db_pool,
                &pred,
                &kalshi,
                fincept.inner().as_ref(),
            )
            .await;
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

/// Prefer edge-pipeline forecast (sidecar p_model) when the bridge is online;
/// always attach LLM fair as secondary metadata. Falls back to LLM-only row
/// when sidecar/tape unavailable — never invents agent probabilities.
async fn persist_chat_forecast_unified(
    pool: &Pool<Sqlite>,
    pred: &crate::predictions::tracker::Prediction,
    kalshi: &KalshiState,
    bridge: &crate::fincept_bridge::FinceptBridge,
) -> Option<i64> {
    let blob = pred.full_decision_json.as_deref()?;
    let decision = crate::chat::decision_extract::parse_kalshi_decision_blob(blob).ok()?;
    if crate::chat::decision_schema::KalshiTradeDecision::is_placeholder_ticker(&decision.ticker)
    {
        return None;
    }
    let keep = matches!(
        decision.decision,
        crate::chat::decision_schema::DecisionAction::TAKE
            | crate::chat::decision_schema::DecisionAction::WATCH
    ) || decision.model_disagreement
        || decision.edge_points.abs() >= 3.0;
    if !keep {
        return None;
    }

    let llm_fair = (decision.fair_probability_pct / 100.0).clamp(0.01, 0.99);
    let llm_meta = serde_json::json!({
        "source": "chat_llm",
        "ticker": decision.ticker,
        "decision": format!("{:?}", decision.decision),
        "side": format!("{:?}", decision.contract_side),
        "fair_probability_pct": decision.fair_probability_pct,
        "market_price_pct": decision.market_price_pct,
        "edge_points": decision.edge_points,
        "thesis": decision.thesis,
        "risk_flags": decision.risk_flags,
    });

    // Path 1: full edge pipeline (agents + shrink + ledger) — preferred.
    let status = bridge.status().await;
    if status.online {
        match crate::edge_engine::opinion_input::build_analyze_input(
            kalshi.as_ref(),
            pool,
            &decision.ticker,
        )
        .await
        {
            Ok(mut input) => {
                input.flags.push("source=chat_extract".into());
                let cfg = edge_config_for_pool(pool).await;
                match crate::edge_engine::pipeline::analyze_and_log_forecast(
                    pool, bridge, input, &cfg,
                )
                .await
                {
                    Ok(edge) => {
                        // Annotate the just-written row with LLM fair (best-effort update).
                        let _ = annotate_forecast_with_llm(pool, edge.forecast_id, &llm_meta, llm_fair)
                            .await;
                        tracing::info!(
                            "chat forecast via edge pipeline: id={} ticker={} p_model={:?}",
                            edge.forecast_id,
                            decision.ticker,
                            edge.p_model
                        );
                        return Some(edge.forecast_id);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "chat edge pipeline failed for {}: {e} — LLM-only fallback",
                            decision.ticker
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "chat opinion input failed for {}: {e} — LLM-only fallback",
                    decision.ticker
                );
            }
        }
    }

    // Path 2: LLM-only row (sidecar offline / tape miss). p_model is LLM fair;
    // agent_breakdown marks source so calibration can filter if needed.
    let p_market = (decision.market_price_pct / 100.0).clamp(0.01, 0.99);
    let p_final = llm_fair;
    let verdict = match (&decision.decision, &decision.contract_side) {
        (
            crate::chat::decision_schema::DecisionAction::TAKE,
            crate::chat::decision_schema::ContractSide::YES,
        ) => "trade_yes",
        (
            crate::chat::decision_schema::DecisionAction::TAKE,
            crate::chat::decision_schema::ContractSide::NO,
        ) => "trade_no",
        _ => "pass",
    };
    let reasons = serde_json::json!([
        "source=chat_llm_fallback",
        format!("decision={:?}", decision.decision),
        format!("edge_points={:.2}", decision.edge_points),
    ])
    .to_string();
    let close_time = kalshi
        .as_ref()
        .find_cached_market(&decision.ticker)
        .and_then(|m| m.close_time.clone())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let stake = if decision.recommended_stake_dollars > 0.0 {
        Some(decision.recommended_stake_dollars)
    } else {
        None
    };
    let breakdown = serde_json::json!({
        "signals": [],
        "llm": llm_meta,
        "note": "sidecar offline or analyze failed; p_model is LLM fair only",
    })
    .to_string();

    match crate::kalshi::forecast::insert_forecast(
        pool,
        &decision.ticker,
        &chrono::Utc::now().to_rfc3339(),
        &close_time,
        p_market,
        Some(llm_fair),
        p_final,
        verdict,
        &reasons,
        stake,
        Some(&breakdown),
    )
    .await
    {
        Ok(id) => {
            tracing::info!(
                "chat forecast LLM-only fallback: id={id} ticker={}",
                decision.ticker
            );
            Some(id)
        }
        Err(e) => {
            tracing::warn!("chat forecast insert failed: {e}");
            None
        }
    }
}

/// Merge LLM metadata into an existing forecast's agent_breakdown JSON.
async fn annotate_forecast_with_llm(
    pool: &Pool<Sqlite>,
    forecast_id: i64,
    llm_meta: &serde_json::Value,
    llm_fair: f64,
) -> Result<(), String> {
    let row = sqlx::query("SELECT agent_breakdown FROM forecasts WHERE id = ?1")
        .bind(forecast_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut root = if let Some(r) = row {
        let raw: Option<String> = sqlx::Row::get(&r, "agent_breakdown");
        raw.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if let Some(obj) = root.as_object_mut() {
        obj.insert("llm".into(), llm_meta.clone());
        obj.insert("llm_fair".into(), serde_json::json!(llm_fair));
    }
    let s = root.to_string();
    sqlx::query("UPDATE forecasts SET agent_breakdown = ?1 WHERE id = ?2")
        .bind(s)
        .bind(forecast_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
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
pub async fn rename_chat_session(
    session_id: String,
    new_name: String,
) -> Result<session::ChatSession, String> {
    session::rename_session(&session_id, &new_name)
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

