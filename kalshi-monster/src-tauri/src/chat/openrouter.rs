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

/// Completion budget. Thinking/free models often burn most of a small
/// budget on monologue; 4k caused mid-sentence cutoffs in production logs.
const DEFAULT_MAX_COMPLETION_TOKENS: u32 = 16_384;
/// Auto-continue when the model stops without a deliverable decision.
const MAX_AUTO_CONTINUATIONS: u32 = 2;

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

/// Some gateways (OpenCode Zen free/thinking models) stream only into
/// `delta.reasoning` / `reasoning_content` and leave `delta.content` empty.
/// Without this, Analyst stores an empty assistant bubble that looks blank.
fn coalesce_content_and_reasoning(
    content: String,
    reasoning: Option<String>,
) -> (String, Option<String>) {
    if !content.trim().is_empty() {
        return (content, reasoning);
    }
    match reasoning {
        Some(r) if !r.trim().is_empty() => {
            tracing::info!(
                "LLM returned empty content with {} chars of reasoning — promoting reasoning to content",
                r.len()
            );
            (r, None)
        }
        other => (content, other),
    }
}

/// OpenRouter-only `reasoning` request extension; other gateways may ignore or
/// mishandle it. Keep content-path clean for OpenCode Zen/Go.
fn should_send_reasoning_request(config: &AppConfig, model: &str) -> bool {
    matches!(
        config.llm_provider_enum(),
        crate::config::LlmProvider::Openrouter
    ) && model_supports_reasoning(model)
}

/// True when the model never produced a trade decision deliverable.
/// Used to trigger auto-continue after max_tokens / free-model early stops.
pub fn response_looks_incomplete(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_lowercase();
    let has_json_decision = lower.contains("\"decision\"")
        && (lower.contains("\"take\"")
            || lower.contains("\"pass\"")
            || lower.contains("\"watch\"")
            || t.contains("TAKE")
            || t.contains("PASS")
            || t.contains("WATCH"));
    let has_summary = lower.contains("decision:")
        || lower.contains("**decision**")
        || lower.contains("decision —")
        || lower.contains("decision -");
    if has_json_decision || has_summary {
        return false;
    }
    // Long monologue without a decision, or cut mid-token/sentence
    let last = t.chars().last().unwrap_or(' ');
    last.is_alphanumeric() || last == ',' || last == ':' || t.ends_with("...") || t.len() > 1500
}

fn continue_user_prompt() -> &'static str {
    "Continue EXACTLY where you left off. Do not restart the analysis. \
     Finish any open thought, then output the required JSON decision block \
     (with decision TAKE/WATCH/PASS) and the DECISION summary. \
     Prefer completing the deliverable over more internal monologue."
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

            // OpenCode Zen free/thinking models often stream ONLY into reasoning_*
            // and never emit delta.content. Mirror those tokens onto the visible
            // content channel so first-token latency is not "wait until done".
            let reasoning_piece = choice
                .delta
                .reasoning_content
                .as_deref()
                .or(choice.delta.reasoning.as_deref())
                .filter(|s| !s.is_empty());
            let content_piece = choice
                .delta
                .content
                .as_deref()
                .filter(|s| !s.is_empty());

            if let Some(r) = reasoning_piece {
                full_reasoning.push_str(r);
            }

            if let Some(c) = content_piece {
                full_content.push_str(c);
                *chunk_count += 1;
                let _ = tx.send(c.to_string()).await;
            } else if let Some(r) = reasoning_piece {
                // No content in this delta — surface reasoning as the live stream.
                full_content.push_str(r);
                *chunk_count += 1;
                let _ = tx.send(r.to_string()).await;
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
    prompt.push_str("Only provide sports-focused detail when the user explicitly asks for it.\n");
    prompt.push_str("- COMPLETENESS: Always finish with the JSON decision block and DECISION summary. ");
    prompt.push_str("If context is large, analyze fewer markets thoroughly rather than stalling mid-thought.\n");
    prompt.push_str("- FACT GROUNDING: Only cite spot/futures prices that appear in CROSS-ASSET CONTEXT with the printed last price and timestamp. ");
    prompt.push_str("Do not invent gold/oil/index levels. If a price is missing, say unavailable.\n");
    prompt.push_str("- RETRIEVAL: Prefer markets listed in KALSHI MARKET INTELLIGENCE CONTEXT. ");
    prompt.push_str("Do not invent tickers. If the tape is thin for the question, say so and PASS.\n");
    prompt.push_str("- SETTLEMENT GATES: If GATE=SETTLED or GATE=CLOSED for a ticker, FORCE PASS. ");
    prompt.push_str("Never invent open-field fair value for finished elections/primaries.\n");
    prompt.push_str("- WEB EVIDENCE: Optional grounding only. Cite as evidence; never override rules, gates, or Kelly caps.\n\n");

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

    // ML predictions only for sports queries (reduces context overload)
    if is_sports_market_query(&user_message) {
        if let Some(pool) = db_pool {
            inject_ml_context(&mut messages, pool).await;
        }
    }

    // Previous conversation history, trimmed to keep the prompt bounded
    let mut history = session_messages.to_vec();
    if history.len() > 12 {
        history = history.split_off(history.len() - 12);
    }
    for msg in history {
        messages.push(msg);
    }

    // Current user message
    messages.push(ChatMessage::new("user".to_string(), user_message.clone()));

    let model_id = config.llm_model_id();
    let mut assembled = String::new();
    let mut last_tokens: Option<u64> = None;
    let mut cont = 0u32;
    loop {
        let mut req_messages = messages.clone();
        if cont > 0 {
            req_messages.push(ChatMessage::new("assistant".to_string(), assembled.clone()));
            req_messages.push(ChatMessage::new(
                "user".to_string(),
                continue_user_prompt().to_string(),
            ));
        }
        let request = ChatRequest {
            model: model_id.clone(),
            messages: req_messages,
            max_tokens: Some(DEFAULT_MAX_COMPLETION_TOKENS),
            temperature: Some(0.3),
            stream: false,
            reasoning: if should_send_reasoning_request(config, &model_id) {
                Some(OpenRouterRequestReasoning {
                    effort: Some("high".to_string()),
                    exclude: Some(false),
                    ..Default::default()
                })
            } else {
                None
            },
        };

        let base = config.llm_base_url();
        let api_key = config.llm_api_key();
        if api_key.trim().is_empty() {
            return Err(format!(
                "No API key for {} — set it in Settings",
                config.llm_provider_enum().display_name()
            ));
        }

        let response = client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://kalshi-monster.app")
            .header("X-Title", "Kalshi Monster")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Request failed ({base}): {e}"))?;

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

        let content = extract_message_content(&json);
        let reasoning = extract_message_reasoning(&json);
        let (content, _) = coalesce_content_and_reasoning(content, reasoning);
        if content.trim().is_empty() && cont == 0 {
            return Err(
                "Model returned an empty response (no content or reasoning). Try another model or provider."
                    .into(),
            );
        }
        if cont == 0 {
            assembled = content;
        } else if !content.trim().is_empty() {
            assembled.push_str("\n\n");
            assembled.push_str(&content);
        }

        last_tokens = json
            .get("usage")
            .and_then(|u| u.get("total_tokens"))
            .and_then(|t| t.as_u64())
            .or(last_tokens);

        if !response_looks_incomplete(&assembled) || cont >= MAX_AUTO_CONTINUATIONS {
            break;
        }
        tracing::info!(
            "LLM response incomplete (len={}); auto-continue {}/{}",
            assembled.len(),
            cont + 1,
            MAX_AUTO_CONTINUATIONS
        );
        cont += 1;
    }

    Ok(OpenRouterResponse {
        content: assembled,
        reasoning: None,
        tokens_used: last_tokens,
        model: model_id,
    })
}

fn extract_message_content(json: &Value) -> String {
    json.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| {
            c.as_str().map(|s| s.to_string()).or_else(|| {
                c.as_array().map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
            })
        })
        .unwrap_or_default()
}

fn extract_message_reasoning(json: &Value) -> Option<String> {
    json.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("reasoning").or_else(|| m.get("reasoning_content")))
        .and_then(|r| r.as_str())
        .map(|r| r.to_string())
}

async fn inject_ml_context(messages: &mut Vec<ChatMessage>, pool: &Pool<Sqlite>) {
    let ml_preds = ml_predictor::get_stored_ml_predictions(pool, 10)
        .await
        .unwrap_or_default();
    if ml_preds.is_empty() {
        return;
    }
    let ml_status = ml_predictor::get_model_status(pool, None).await.ok();
    let header_detail = ml_status
        .as_ref()
        .map(ml_predictor::format_ml_training_header)
        .unwrap_or_else(|| "N/A samples, CV accuracy: N/A".to_string());
    let mut ml_ctx = format!("## ML MODEL PREDICTIONS ({})\n\n", header_detail);
    ml_ctx.push_str("Sports-query only. Consider alongside your analysis.\n\n");
    for pred in &ml_preds {
        let emoji = if pred.ml_win_probability >= 0.55 {
            "✅"
        } else if pred.ml_win_probability >= 0.45 {
            "⚠️"
        } else {
            "❌"
        };
        let lean = if pred.ml_win_probability >= 0.5 {
            "Lean OVER"
        } else {
            "Lean UNDER"
        };
        ml_ctx.push_str(&format!(
            "  {} {} — {} {} | Line: {:.1} | ML Win Prob: {:.1}% ({})\n",
            emoji,
            pred.player_name,
            pred.ml_prediction,
            pred.stat_category,
            pred.line,
            pred.ml_win_probability * 100.0,
            lean
        ));
    }
    messages.push(ChatMessage::new("system".to_string(), ml_ctx));
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
/// Tightened to explicit leagues, sports, positions, and stat/mechanics terms so
/// non-sports prediction markets do not trigger irrelevant sports data injection.
fn is_sports_market_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let sports_keywords = [
        "sports", "nba", "nfl", "mlb", "nhl", "ufc", "golf", "tennis",
        "basketball", "baseball", "football", "hockey",
        "quarterback", "qb", "running back", "rb", "wide receiver", "wr",
        "passing", "rushing", "receiving", "yards", "touchdown",
        "tip-off", "kickoff", "overtime", "halftime",
    ];
    sports_keywords.iter().any(|kw| lower.contains(kw))
}

/// Builds sports context ONLY when the user explicitly requests a specific league/sport.
/// This replaces the old behavior where sports data was injected by default and the
/// fallback dumped NFL-centric data for any vague sports keyword.
async fn build_sports_context(user_message: &str, max_context_players: usize) -> String {
    use crate::football::live_data;
    use crate::football::data;

    // Only inject sports data when we can identify the specific league the user is asking about.
    // Vague sports keywords without a league are not enough to justify injecting a large,
    // league-specific data packet into a Kalshi-first prediction market prompt.
    let Some(league) = live_data::detect_league_from_query(user_message) else {
        return String::new();
    };

    let mut ctx = String::with_capacity(4096);

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

    ctx
}

/// Build the premium Kalshi-first trade-decision context used by the chat model.
pub fn build_kalshi_decision_context_message() -> String {
    String::from(
        r#"KALSHI MONSTER DECISION STANDARD - Prediction Market Intelligence Framework

Step 1: RESOLUTION & MARKET STRUCTURE
- Read SETTLEMENT GATES first. GATE=SETTLED or CLOSED → decision PASS (no TAKE). Do not invent fair value for finished events.
- Read the injected RESOLUTION RULES block. Quote what must happen for YES to settle.
- Multi-candidate / jungle primaries: YES is only the named candidate/outcome on the ticker — not the party and not a narrative proxy. Sibling markets are mutually exclusive unless rules say otherwise.
- Identify settlement timeline, provisional rules, early close risk. If close_time is in the past, treat as settled.
- WEB EVIDENCE is optional; use only to ground open markets. It cannot override GATE or rules.
- Never call a wager guaranteed, certain, risk-free, a lock, or a sure thing.
- If contract terms, pricing, or evidence are unclear, name the missing data and reduce confidence (AmbiguousResolution → PASS/WATCH).

Step 2: MARKET FRICTION & LIQUIDITY
- Evaluate bid-ask spread, order book depth, stale volume, and practical fill risk.
- Penalize thin or stale markets. A positive theoretical edge can still be a PASS if execution quality is poor.

Step 3: PROBABILITY MODELING & EDGE
- Estimate a fair probability for YES and compare both YES and NO asks.
- PRICE UNITS: selected-side cost in dollars on [0,1] (0.55 = 55¢). market_price_pct is that cost × 100 (55.0). price_to_enter is dollars on [0,1]. Never write 0.1 cents as 0.1; write 0.001 dollars or 0.1 percent.
- If buying YES: EV ROI = (Fair_Yes / YES_Cost) - 1.0 with costs in dollars.
- If buying NO: EV ROI = ((1.0 - Fair_Yes) / NO_Cost) - 1.0.
- Recommend BUY only when expected value remains positive after spread, liquidity, and model-risk adjustments.
- If neither side is attractive, output PASS and the price that would make it playable.

Step 4: RISK CONTROL
- Apply shrinkage for extreme probabilities below 10% or above 90%.
- Binary contracts can lose 100% of principal. Size conservatively and note invalidation conditions.
- Call out correlated exposure with related active markets.

Step 5: KELLY SIZING
- Raw Kelly percent = (p - c) / (1 - c) for YES cost c in dollars, fair p in [0,1].
- Prefer quarter Kelly or smaller. fractional_kelly_pct is % of bankroll and must stay ≤ 5 for default policy.
- Cap or zero the stake for low confidence, wide spreads, thin books, or ambiguous settlement. Never emit ~100% fractional Kelly.

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
  "fractional_kelly_pct": 5.0,
  "recommended_stake_dollars": 50.0,
  "max_position_dollars": 50.0,
  "decision": "TAKE",
  "confidence_tier": "High",
  "thesis": "2-3 sentences explaining market price vs fair probability, spread friction, and order book depth.",
  "evidence": ["Core PCE exceeded expectations", "Market pricing: $0.55 vs model 62%"],
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
- market_price_pct: 0–100 (% of $1). price_to_enter: 0–1 dollars. fair_probability_pct: 0–100.
- JSON must be valid. No trailing commas. Place it FIRST in the response.

After the JSON, provide a concise readable summary:
DECISION: [TAKE/WATCH/PASS] [YES/NO] at [$0.xx]
PRICE VS FAIR: [market]% vs [fair]%
EDGE: [edge points] pts, [EV ROI]% EV ROI
SIZE: [raw Kelly]% raw Kelly, [fractional Kelly]% recommended (≤5% bankroll default)
WHY: [specific quantitative and qualitative thesis]
RULES CHECK: [what settles YES; mutual exclusivity if multi-candidate]
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

    if is_sports_market_query(&user_message) {
        if let Some(pool) = db_pool {
            inject_ml_context(&mut messages, pool).await;
        }
    }

    let mut history = session_messages.to_vec();
    if history.len() > 12 {
        history = history.split_off(history.len() - 12);
    }
    for msg in history {
        messages.push(msg);
    }
    messages.push(ChatMessage::new("user".to_string(), user_message));

    let model_id = config.llm_model_id();
    let base = config.llm_base_url();
    let api_key = config.llm_api_key();
    if api_key.trim().is_empty() {
        let _ = tx
            .send(format!(
                "__STREAM_ERROR__:No API key for {} — set it in Settings",
                config.llm_provider_enum().display_name()
            ))
            .await;
        return Err(format!(
            "No API key for {}",
            config.llm_provider_enum().display_name()
        ));
    }

    let mut assembled = String::new();
    let mut tokens_used: Option<u64> = None;
    let mut cont = 0u32;

    loop {
        let mut req_messages = messages.clone();
        if cont > 0 {
            req_messages.push(ChatMessage::new("assistant".to_string(), assembled.clone()));
            req_messages.push(ChatMessage::new(
                "user".to_string(),
                continue_user_prompt().to_string(),
            ));
            let notice = "\n\n---\n*(continuing incomplete response…)*\n\n";
            assembled.push_str(notice);
            let _ = tx.send(notice.to_string()).await;
        }

        let request = ChatRequest {
            model: model_id.clone(),
            messages: req_messages,
            max_tokens: Some(DEFAULT_MAX_COMPLETION_TOKENS),
            temperature: Some(0.3),
            stream: true,
            reasoning: if should_send_reasoning_request(config, &model_id) {
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
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://kalshi-monster.app")
            .header("X-Title", "Kalshi Monster")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Request failed ({base}): {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            let _ = tx
                .send(format!("__STREAM_ERROR__:API error ({status}): {error_body}"))
                .await;
            return Err(format!("API error ({status}): {error_body}"));
        }

        let mut stream = response.bytes_stream();
        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut chunk_count: usize = 0;
        let mut line_buffer: Vec<u8> = Vec::new();
        let mut done_received = false;

        'stream_loop: while let Some(chunk_result) = stream.next().await {
            let bytes = match chunk_result {
                Ok(b) => b,
                Err(e) => {
                    if !full_content.is_empty() || !full_reasoning.is_empty() {
                        tracing::warn!(
                            "Stream read error after partial content; preserving: {e}"
                        );
                        break 'stream_loop;
                    }
                    let _ = tx.send(format!("__STREAM_ERROR__:Stream error: {e}")).await;
                    return Err(format!("Stream error: {e}"));
                }
            };
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
            let _ = process_stream_line(
                line,
                &tx,
                &mut full_content,
                &mut full_reasoning,
                &mut chunk_count,
            )
            .await;
        }

        let reasoning_val = if full_reasoning.is_empty()
            || full_content.contains(full_reasoning.as_str())
        {
            None
        } else if full_content.trim().is_empty() {
            Some(full_reasoning)
        } else {
            None
        };
        let (piece, _) = coalesce_content_and_reasoning(full_content, reasoning_val);
        if cont == 0 {
            assembled = piece;
        } else if !piece.trim().is_empty() {
            // piece was already streamed live; keep assembled in sync
            if !assembled.ends_with(&piece) {
                assembled.push_str(&piece);
            }
        }

        tokens_used = Some(assembled.len() as u64 / 4);

        if assembled.trim().is_empty() {
            let msg = "Model returned an empty streamed response. Try another model.";
            let _ = tx.send(format!("__STREAM_ERROR__:{msg}")).await;
            return Err(msg.into());
        }

        if !response_looks_incomplete(&assembled) || cont >= MAX_AUTO_CONTINUATIONS {
            break;
        }
        tracing::info!(
            "stream incomplete (len={}); auto-continue {}/{}",
            assembled.len(),
            cont + 1,
            MAX_AUTO_CONTINUATIONS
        );
        cont += 1;
    }

    Ok(OpenRouterResponse {
        content: assembled,
        reasoning: None,
        tokens_used,
        model: model_id,
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

    let model_id = config.llm_model_id();
    let request = ChatRequest {
        model: model_id.clone(),
        messages,
        max_tokens: Some(DEFAULT_MAX_COMPLETION_TOKENS),
        temperature: Some(0.3),
        stream: false,
        reasoning: if should_send_reasoning_request(config, &model_id) {
            Some(OpenRouterRequestReasoning {
                effort: Some("high".to_string()),
                exclude: Some(false),
                ..Default::default()
            })
        } else {
            None
        },
    };

    let base = config.llm_base_url();
    let api_key = config.llm_api_key();
    if api_key.trim().is_empty() {
        return Err(format!(
            "No API key for {}",
            config.llm_provider_enum().display_name()
        ));
    }

    let response = client
        .post(format!("{base}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://kalshi-monster.app")
        .header("X-Title", "Kalshi Monster")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed ({base}): {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error ({}): {}", status, error_body));
    }

    let json: Value = response.json().await.map_err(|e| format!("Failed to parse response: {}", e))?;

    let content = extract_message_content(&json);
    let reasoning = extract_message_reasoning(&json);
    let (content, reasoning) = coalesce_content_and_reasoning(content, reasoning);
    if content.trim().is_empty() {
        return Err("Model returned an empty response (no content or reasoning).".into());
    }

    let usage = json.get("usage");
    let tokens_used = usage.and_then(|u| u.get("total_tokens")).and_then(|t| t.as_u64());

    Ok(OpenRouterResponse {
        content,
        reasoning,
        tokens_used,
        model: model_id,
    })
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_promotes_reasoning_when_content_empty() {
        let (c, r) = coalesce_content_and_reasoning(String::new(), Some("thinking hard".into()));
        assert_eq!(c, "thinking hard");
        assert!(r.is_none());
    }

    #[test]
    fn coalesce_keeps_content_when_present() {
        let (c, r) = coalesce_content_and_reasoning("hello".into(), Some("secret thoughts".into()));
        assert_eq!(c, "hello");
        assert_eq!(r.as_deref(), Some("secret thoughts"));
    }

    #[test]
    fn coalesce_empty_both_stays_empty() {
        let (c, r) = coalesce_content_and_reasoning("   ".into(), Some("  ".into()));
        assert_eq!(c.trim(), "");
        assert!(r.is_some());
    }

    #[test]
    fn incomplete_without_decision() {
        assert!(response_looks_incomplete(
            "Let's look at Harris's market. I need to stress test"
        ));
        assert!(response_looks_incomplete(""));
    }

    #[test]
    fn complete_with_json_decision() {
        let t = r#"{"ticker":"KX","decision":"PASS","contract_side":"PASS"}
DECISION: PASS"#;
        assert!(!response_looks_incomplete(t));
    }
}
