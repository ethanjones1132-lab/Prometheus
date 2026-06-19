#![allow(dead_code)]
use crate::analysis::context::AnalysisContext;
use crate::config::AppConfig;
use crate::ml_predictor;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Pool, Sqlite};
use std::time::Duration;
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════
// OpenRouter API Integration — Kalshi-First Prediction Market AI
// Supports both streaming and non-streaming modes.
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Default)]
struct OpenRouterRequestReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenRouterRequestReasoning>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl ChatMessage {
    pub fn new(role: String, content: String) -> Self {
        Self {
            role,
            content,
            reasoning: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

fn model_supports_reasoning(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("claude")
        || m.contains("deepseek-r1")
        || m.contains("/r1")
        || m.contains("qwq")
        || m.contains("qvq")
        || m.contains("thinking")
        || m.contains("/o1")
        || m.contains("/o3")
        || m.contains("/o4")
        || m.contains("gemini-2.5")
}

async fn process_stream_line(
    line: &str,
    tx: &mpsc::Sender<String>,
    full_content: &mut String,
    full_reasoning: &mut String,
    chunk_count: &mut usize,
) -> bool {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return false;
    }

    let Some(data) = line.strip_prefix("data:") else {
        return false;
    };
    let data = data.trim_start();
    if data == "[DONE]" {
        return true;
    }

    match serde_json::from_str::<StreamChunk>(data) {
        Ok(chunk) => {
            let Some(choice) = chunk.choices.first() else {
                return false;
            };

            if let Some(r_content) = &choice.delta.reasoning_content {
                full_reasoning.push_str(r_content);
                let _ = tx.send(format!("__STREAM_THOUGHT__:{}", r_content)).await;
            } else if let Some(r_content) = &choice.delta.reasoning {
                full_reasoning.push_str(r_content);
                let _ = tx.send(format!("__STREAM_THOUGHT__:{}", r_content)).await;
            }

            if let Some(content) = &choice.delta.content {
                full_content.push_str(content);
                *chunk_count += 1;
                let _ = tx.send(content.to_string()).await;
            }
        }
        Err(e) => {
            tracing::debug!("Skipping unparseable stream data line: {}", e);
        }
    }

    false
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

/// Build the core system prompt for Kalshi-first prediction market analysis.
fn build_kalshi_system_prompt(config: &AppConfig) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("# KALSHI MONSTER — PREDICTION MARKET INTELLIGENCE ENGINE\n\n");
    prompt.push_str("You are the Kalshi Monster, an elite AI-driven prediction market intelligence system. ");
    prompt.push_str("Your mission is to deliver mathematically rigorous, probability-weighted market assessments ");
    prompt.push_str("for Kalshi event contracts.\n\n");

    if !config.system_prompt.is_empty() {
        prompt.push_str("## USER PREFERENCES\n");
        prompt.push_str(&config.system_prompt);
        prompt.push_str("\n\n");
    }

    prompt.push_str("GUIDING PRINCIPLES:\n");
    prompt.push_str("- Never describe any wager, contract, or forecast as guaranteed, certain, or risk-free. ");
    prompt.push_str("Always express outcomes in calibrated probabilities, expected value (EV), and downside risk controls.\n");
    prompt.push_str("- Prioritize prediction market mechanics: bid-ask spreads, liquidity depth, market microstructure, and settlement risk are as important as the fundamental analysis.\n");
    prompt.push_str("- Default to PASS when the edge is unclear, the spread is too wide, or data quality is poor. ");
    prompt.push_str("A clean no-trade is often the best trade.\n");
    prompt.push_str("- Sports analysis is a subdomain of Kalshi markets, not the primary domain. ");
    prompt.push_str("Only provide sports-focused detail when the user explicitly asks for it.\n\n");

    prompt
}

/// Send a message to OpenRouter with Kalshi-first enriched context.
/// Sports context is only injected if the user explicitly requests it.
pub async fn send_message(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    analysis_context: Option<&AnalysisContext>,
    db_pool: Option<&Pool<Sqlite>>,
    kalshi_context: Option<&str>,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // Kalshi-first system prompt (replaces football-specific enriched prompt)
    let system_prompt = build_kalshi_system_prompt(config);

    // Kalshi decision framework context (always included)
    let decision_context = build_kalshi_decision_context_message();

    // Kalshi market data context (always included — the core intelligence)
    let kalshi_data_msg = kalshi_context.unwrap_or("");

    // Sports context is injected ONLY when the user explicitly asks about sports
    let sports_data = if is_sports_market_query(&user_message) {
        build_sports_context(&user_message, config.max_context_players).await
    } else {
        String::new()
    };

    // Construct messages array: system + Kalshi context + (optional sports) + history + user
    let mut messages = Vec::new();

    // System prompt (highest priority for identity and rules)
    messages.push(ChatMessage::new("system".to_string(), system_prompt));

    // Kalshi decision framework
    messages.push(ChatMessage::new("system".to_string(), decision_context));

    // Kalshi market data (core intelligence)
    if !kalshi_data_msg.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), kalshi_data_msg.to_string()));
    }

    // Sports data (only if user asked for sports)
    if !sports_data.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), sports_data));
    }

    // Analysis Engine computed context (edge, matchup, scoring, correlation)
    if let Some(analysis) = analysis_context {
        let analysis_prompt = analysis.to_prompt_context();
        if !analysis_prompt.is_empty() {
            messages.push(ChatMessage::new("system".to_string(), format!("## ANALYSIS ENGINE COMPUTED CONTEXT\n{analysis_prompt}")));
        }
    }

    // ML Model Predictions — fetch from DB and inject as system context
    if let Some(pool) = db_pool {
        let ml_preds = ml_predictor::get_stored_ml_predictions(pool, 15).await.unwrap_or_default();
        if !ml_preds.is_empty() {
            let ml_status = ml_predictor::get_model_status(pool, None).await.ok();
            let acc_str = ml_status
                .as_ref()
                .and_then(|s| s.cv_accuracy_mean)
                .map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            let samples_str = ml_status
                .as_ref()
                .and_then(|s| s.samples)
                .map_or("N/A".to_string(), |s| s.to_string());
            let mut ml_ctx = format!("## ML MODEL PREDICTIONS (trained on {} samples, CV accuracy: {})\n\n", samples_str, acc_str);
            ml_ctx.push_str("The following are machine-learning generated predictions from your trained model.\n");
            ml_ctx.push_str("Consider these alongside your own analysis — they may confirm or challenge your lean.\n\n");
            for pred in &ml_preds {
                let emoji = if pred.ml_win_probability >= 0.55 { "✅" } else if pred.ml_win_probability >= 0.45 { "⚠️" } else { "❌" };
                let lean = if pred.ml_win_probability >= 0.5 { "Lean OVER" } else { "Lean UNDER" };
                let line_change_str = if pred.line_change.abs() > 0.01 {
                    format!(" | Line change: {:+.1}", pred.line_change)
                } else {
                    String::new()
                };
                ml_ctx.push_str(&format!(
                    "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({}){}\n",
                    emoji, pred.player_name, pred.ml_prediction, pred.stat_category,
                    pred.line, pred.ml_win_probability * 100.0, lean, line_change_str
                ));
            }
            messages.push(ChatMessage::new("system".to_string(), ml_ctx));
        }
    }

    // Previous conversation history, trimmed to keep the prompt bounded
    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }

    // Current user message
    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: false,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header("Authorization", format!("Bearer {}", config.openrouter_api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://kalshi-monster.app")
        .header("X-Title", "Kalshi Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let content = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("No content in response")?
        .to_string();

    let reasoning = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| {
            m.get("reasoning")
                .or_else(|| m.get("reasoning_content"))
        })
        .and_then(|r| r.as_str())
        .map(|r| r.to_string());

    let usage = json.get("usage");
    let tokens_used = usage
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64());

    Ok(OpenRouterResponse {
        content,
        reasoning,
        tokens_used,
        model: config.selected_model.clone(),
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenRouterResponse {
    pub content: String,
    pub reasoning: Option<String>,
    pub tokens_used: Option<u64>,
    pub model: String,
}

impl OpenRouterResponse {
    pub fn new(content: String, model: String) -> Self {
        Self {
            content,
            reasoning: None,
            tokens_used: None,
            model,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Context Builders
// ═══════════════════════════════════════════════════════════════

/// Detects if the user is asking about sports markets.
fn is_sports_market_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let sports_keywords = [
        "sports", "nba", "nfl", "mlb", "nhl", "ufc", "golf", "tennis",
        "player", "quarterback", "qb", "running back", "rb", "wide receiver", "wr",
        "passing", "rushing", "receiving", "yards", "touchdown",
        "basketball", "baseball", "football", "hockey",
        "playoff", "championship",
    ];
    sports_keywords.iter().any(|kw| lower.contains(kw))
}

/// Builds sports context ONLY when the user explicitly requests it.
/// This replaces the old behavior where sports data was injected by default.
async fn build_sports_context(user_message: &str, max_context_players: usize) -> String {
    use crate::football::live_data;
    use crate::football::data;

    let mut ctx = String::with_capacity(4096);

    // Detect the league from the user message
    if let Some(league) = live_data::detect_league_from_query(user_message) {
        let sport_prompt = data::build_multi_sport_system_prompt(league);
        if !sport_prompt.is_empty() {
            ctx.push_str("## SPORTS MARKET CONTEXT (USER REQUESTED)\n");
            ctx.push_str(&sport_prompt);
            ctx.push('\n');
        }

        // Add live data context for the detected league
        let live = live_data::build_live_data_context(user_message, max_context_players).await;
        if !live.is_empty() {
            ctx.push_str("## LIVE SPORTS DATA\n");
            ctx.push_str(&live);
            ctx.push('\n');
        }
    } else {
        // Default: provide general sports market overview if no specific league
        let live = live_data::build_live_data_context(user_message, max_context_players).await;
        if !live.is_empty() {
            ctx.push_str("## LIVE SPORTS DATA (USER REQUESTED)\n");
            ctx.push_str(&live);
            ctx.push('\n');
        }
    }

    ctx
}

/// Build the premium Kalshi-first trade-decision context used by the chat model.
pub fn build_kalshi_decision_context_message() -> String {
    String::from(
        r#"KALSHI MONSTER DECISION STANDARD - Prediction Market Intelligence Framework

Step 1: RESOLUTION & MARKET STRUCTURE
- Identify the exact contract definition, ticker, settlement timeline, provisional rules, early close risk, and what must happen for YES to resolve.
- Never call a wager guaranteed, certain, risk-free, a lock, or a sure thing.
- If contract terms, pricing, or evidence are unclear, name the missing data and reduce confidence.

Step 2: MARKET FRICTION & LIQUIDITY
- Evaluate bid-ask spread, order book depth, stale volume, and practical fill risk.
- Penalize thin or stale markets. A positive theoretical edge can still be a PASS if execution quality is poor.

Step 3: PROBABILITY MODELING & EDGE
- Estimate a fair probability for YES and compare both YES and NO asks.
- Selected side price must be decimal cost, e.g. 0.55 for 55 cents.
- If buying YES: EV ROI = (Fair_Yes / YES_Cost) - 1.0.
- If buying NO: EV ROI = ((1.0 - Fair_Yes) / NO_Cost) - 1.0.
- Recommend BUY only when expected value remains positive after spread, liquidity, and model-risk adjustments.
- If neither side is attractive, output PASS and the price that would make it playable.

Step 4: RISK CONTROL
- Apply shrinkage for extreme probabilities below 10% or above 90%.
- Binary contracts can lose 100% of principal. Size conservatively and note invalidation conditions.
- Call out correlated exposure with related active markets.

Step 5: KELLY SIZING
- Raw Kelly percent = edge / (1.0 - selected_side_cost).
- Prefer quarter Kelly or smaller unless data quality, liquidity, and resolution clarity are excellent.
- Cap or zero the stake for low confidence, wide spreads, thin books, or ambiguous settlement.

RESPONSE FORMAT - Output one JSON block FIRST so the app can render a trade ticket:

```json
{
  "ticker": "KXEVENT-TICKER",
  "market_title": "Human-readable market title",
  "category": "Politics, Economics, Finance, Weather, Sports, Other",
  "contract_side": "YES",
  "market_price_pct": 55.0,
  "fair_probability_pct": 62.0,
  "edge_points": 7.0,
  "spread_cents": 3.0,
  "liquidity_score": 75.0,
  "ev_per_contract_cents": 7.0,
  "ev_roi_pct": 12.7,
  "raw_kelly_pct": 22.4,
  "fractional_kelly_pct": 5.6,
  "recommended_stake_dollars": 56.0,
  "max_position_dollars": 50.0,
  "decision": "TAKE",
  "confidence_tier": "High",
  "thesis": "2-3 sentences explaining market price vs fair probability, spread friction, and order book depth.",
  "evidence": ["Core PCE exceeded expectations", "Market pricing: 55c vs model: 62c"],
  "risk_flags": ["EarlyCloseRisk"],
  "data_quality": "Live",
  "price_to_enter": 0.55
}
```

JSON RULES:
- "decision" must be "TAKE", "WATCH", or "PASS".
- "contract_side" must be "YES", "NO", or "PASS".
- "confidence_tier" must be "High", "Medium", "Low", or "None".
- "data_quality" must be "Live", "Fresh", "Stale", "Inferential", or "Speculative".
- "risk_flags" can include: SpreadExceedsEdge, InsufficientLiquidity, CorrelatedExposure, ProvisionalSettlement, EarlyCloseRisk, ExtremeProbability, AmbiguousResolution, StaleData, ConcentrationRisk.
- JSON must be valid. No trailing commas. Place it FIRST in the response.

After the JSON, provide a concise readable summary:
DECISION: [TAKE/WATCH/PASS] [YES/NO] at [price]
PRICE VS FAIR: [market]% vs [fair]%
EDGE: [edge points] pts, [EV ROI]% EV ROI
SIZE: [raw Kelly]% raw Kelly, [fractional Kelly]% recommended
WHY: [specific quantitative and qualitative thesis]
RISK CONTROL: [key risk flags and invalidation conditions]

Be selective. PASS is a premium outcome when the price is not good enough."#,
    )
}

/// Stream a message to OpenRouter with Kalshi-first context.
/// Sports context is only injected when the user explicitly asks about sports.
pub async fn stream_message(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    analysis_context: Option<&AnalysisContext>,
    db_pool: Option<&Pool<Sqlite>>,
    tx: mpsc::Sender<String>,
    kalshi_context: Option<&str>,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // Kalshi-first system prompt
    let system_prompt = build_kalshi_system_prompt(config);

    // Kalshi decision framework
    let decision_context = build_kalshi_decision_context_message();

    // Kalshi market data
    let kalshi_data_msg = kalshi_context.unwrap_or("");

    // Sports context only if user explicitly asks
    let sports_data = if is_sports_market_query(&user_message) {
        build_sports_context(&user_message, config.max_context_players).await
    } else {
        String::new()
    };

    // Construct messages array
    let mut messages = Vec::new();

    // System prompt
    messages.push(ChatMessage::new("system".to_string(), system_prompt));

    // Kalshi decision framework
    messages.push(ChatMessage::new("system".to_string(), decision_context));

    // Kalshi market data
    if !kalshi_data_msg.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), kalshi_data_msg.to_string()));
    }

    // Sports context (only if user asked)
    if !sports_data.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), sports_data));
    }

    // Analysis Engine
    if let Some(analysis) = analysis_context {
        let analysis_prompt = analysis.to_prompt_context();
        if !analysis_prompt.is_empty() {
            messages.push(ChatMessage::new("system".to_string(), format!("## ANALYSIS ENGINE COMPUTED CONTEXT\n{analysis_prompt}")));
        }
    }

    // ML Model Predictions
    if let Some(pool) = db_pool {
        let ml_preds = ml_predictor::get_stored_ml_predictions(pool, 15).await.unwrap_or_default();
        if !ml_preds.is_empty() {
            let ml_status = ml_predictor::get_model_status(pool, None).await.ok();
            let acc_str = ml_status
                .as_ref()
                .and_then(|s| s.cv_accuracy_mean)
                .map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            let samples_str = ml_status
                .as_ref()
                .and_then(|s| s.samples)
                .map_or("N/A".to_string(), |s| s.to_string());
            let mut ml_ctx = format!("## ML MODEL PREDICTIONS (trained on {} samples, CV accuracy: {})\n\n", samples_str, acc_str);
            ml_ctx.push_str("The following are machine-learning generated predictions from your trained model.\n");
            ml_ctx.push_str("Consider these alongside your own analysis — they may confirm or challenge your lean.\n\n");
            for pred in &ml_preds {
                let emoji = if pred.ml_win_probability >= 0.55 { "✅" } else if pred.ml_win_probability >= 0.45 { "⚠️" } else { "❌" };
                let lean = if pred.ml_win_probability >= 0.5 { "Lean OVER" } else { "Lean UNDER" };
                ml_ctx.push_str(&format!(
                    "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({})\n",
                    emoji, pred.player_name, pred.ml_prediction, pred.stat_category,
                    pred.line, pred.ml_win_probability * 100.0, lean
                ));
            }
            messages.push(ChatMessage::new("system".to_string(), ml_ctx));
        }
    }

    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }
    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: true,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header("Authorization", format!("Bearer {}", config.openrouter_api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://kalshi-monster.app")
        .header("X-Title", "Kalshi Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        let _ = tx.send(format!("__STREAM_ERROR__:API error ({}): {}", status, error_body)).await;
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let mut stream = response.bytes_stream();
    let mut full_content = String::new();
    let mut full_reasoning = String::new();
    let mut tokens_used: Option<u64> = None;
    let mut chunk_count: usize = 0;
    let mut raw_data = String::new();
    let mut line_buffer: Vec<u8> = Vec::new();
    let mut done_received = false;

    'stream_loop: while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                if !full_content.is_empty() || !full_reasoning.is_empty() {
                    tracing::warn!("Stream read error after partial content; preserving streamed response: {}", e);
                    break 'stream_loop;
                }
                let _ = tx.send(format!("__STREAM_ERROR__:Stream error: {}", e)).await;
                return Err(format!("Stream error: {}", e));
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        raw_data.push_str(&text);
        line_buffer.extend_from_slice(&bytes);

        while let Some(newline_index) = line_buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = line_buffer.drain(..=newline_index).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(&['\r', '\n'][..]);
            if process_stream_line(
                line,
                &tx,
                &mut full_content,
                &mut full_reasoning,
                &mut chunk_count,
            )
            .await
            {
                done_received = true;
                break 'stream_loop;
            }
        }
    }

    if !done_received && !line_buffer.is_empty() {
        let line = String::from_utf8_lossy(&line_buffer);
        let line = line.trim_end_matches(&['\r', '\n'][..]);
        if process_stream_line(
            line,
            &tx,
            &mut full_content,
            &mut full_reasoning,
            &mut chunk_count,
        )
        .await
        {
            // done
        }
    }

    if tokens_used.is_none() {
        tokens_used = Some((full_content.len() / 4) as u64);
    }

    let reasoning_val = if full_reasoning.is_empty() { None } else { Some(full_reasoning) };

    Ok(OpenRouterResponse {
        content: full_content,
        reasoning: reasoning_val,
        tokens_used,
        model: config.selected_model.clone(),
    })
}

/// Send a message with pre-built context strings.
/// Used by the model comparison feature.
pub async fn send_message_with_context(
    config: &AppConfig,
    session_messages: &[ChatMessage],
    user_message: String,
    system_prompt: &str,
    kalshi_context_msg: &str,
    sports_context_msg: &str,
) -> Result<OpenRouterResponse, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut messages = Vec::new();

    messages.push(ChatMessage::new("system".to_string(), system_prompt.to_string()));
    messages.push(ChatMessage::new("system".to_string(), build_kalshi_decision_context_message()));

    if !kalshi_context_msg.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), kalshi_context_msg.to_string()));
    }
    if !sports_context_msg.is_empty() {
        messages.push(ChatMessage::new("system".to_string(), sports_context_msg.to_string()));
    }

    let mut history = session_messages.to_vec();
    if history.len() > 20 {
        history = history.split_off(history.len() - 20);
    }
    for msg in history {
        messages.push(msg);
    }

    messages.push(ChatMessage::new("user".to_string(), user_message));

    let request = ChatRequest {
        model: config.selected_model.clone(),
        messages,
        max_tokens: Some(4096),
        temperature: Some(0.3),
        stream: false,
        reasoning: if model_supports_reasoning(&config.selected_model) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let response = client
        .post(format!("{}/chat/completions", config.openrouter_base_url))
        .header("Authorization", format!("Bearer {}", config.openrouter_api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://kalshi-monster.app")
        .header("X-Title", "Kalshi Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let json: Value = response.json().await.map_err(|e| format!("Failed to parse response: {}", e))?;

    let content = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("No content in response")?
        .to_string();

    let usage = json.get("usage");
    let tokens_used = usage.and_then(|u| u.get("total_tokens")).and_then(|t| t.as_u64());

    Ok(OpenRouterResponse {
        content,
        reasoning: None,
        tokens_used,
        model: config.selected_model.clone(),
    })
}


