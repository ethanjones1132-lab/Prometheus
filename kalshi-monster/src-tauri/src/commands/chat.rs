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

    // Fetch live data context — Kalshi gates + optional web + Fincept
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

