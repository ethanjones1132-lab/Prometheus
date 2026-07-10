use crate::predictions::tracker::CalibrationMetrics;
pub use crate::predictions::tracker::ScoreRange;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const CONFIG_DIR: &str = ".openclaw/kalshi-monster";
const CONFIG_FILE: &str = "config.json";
const RING_FREE_MODEL_ID: &str = "inclusionai/ring-2.6-1t:free";
const LING_FREE_MODEL_ID: &str = "inclusionai/ling-2.6-1t:free";

/// Which LLM gateway Analyst chat uses.
/// - `openrouter` — OpenRouter (`openrouter_base_url` + `openrouter_api_key`)
/// - `opencode_zen` — OpenCode Zen pay-per-use (`https://opencode.ai/zen/v1`)
/// - `opencode_go` — OpenCode Go subscription models (`https://opencode.ai/zen/go/v1`)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    #[default]
    Openrouter,
    OpencodeZen,
    OpencodeGo,
}

impl LlmProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Openrouter => "openrouter",
            Self::OpencodeZen => "opencode_zen",
            Self::OpencodeGo => "opencode_go",
        }
    }

    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "opencode_zen" | "opencode-zen" | "zen" => Self::OpencodeZen,
            "opencode_go" | "opencode-go" | "go" => Self::OpencodeGo,
            _ => Self::Openrouter,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Openrouter => "OpenRouter",
            Self::OpencodeZen => "OpenCode Zen",
            Self::OpencodeGo => "OpenCode Go",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub openrouter_api_key: String,
    pub openrouter_base_url: String,
    /// Active chat gateway for Analyst (openrouter | opencode_zen | opencode_go).
    #[serde(default)]
    pub llm_provider: String,
    /// OpenCode Zen / Go API key from https://opencode.ai/auth (same key for both).
    #[serde(default)]
    pub opencode_api_key: String,
    pub selected_model: String,
    pub system_prompt: String,
    pub max_context_players: usize,
    // Kalshi data source configuration
    pub openweathermap_api_key: String,
    pub api_sports_key: String,
    /// Brave Search API key for Analyst web evidence (`X-Subscription-Token`).
    /// Dashboard: https://api-dashboard.search.brave.com/
    #[serde(default)]
    pub brave_api_key: String,
    // Custom system prompt preferences
    pub risk_tolerance: String,        // "conservative" | "moderate" | "aggressive"
    pub preferred_leagues: Vec<String>, // e.g. ["NFL", "NBA"]
    pub stat_weighting: String,         // "season_avg" | "last3" | "matchup_adjusted" | "balanced"
    pub output_format: String,          // "json_first" | "text_only" | "json_plus_text"
    // UI theme
    #[serde(default = "default_theme")]
    pub theme: String,                  // "dark" | "light"
    // Kalshi configuration
    #[serde(default)]
    pub kalshi_email: String,
    #[serde(default)]
    pub kalshi_password: String,
    #[serde(default = "default_kalshi_poll")]
    pub kalshi_poll_interval_secs: u64,
    // Bankroll / staking configuration
    #[serde(default = "default_max_bet_pct")]
    pub max_bet_pct: f64,        // Max single bet as fraction of bankroll (e.g. 0.05 = 5%)
    // Discord / Telegram bot configuration
    #[serde(default)]
    pub discord_webhook_url: String,
    #[serde(default)]
    pub telegram_bot_token: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    #[serde(default)]
    pub bot_daily_picks_enabled: bool,
    #[serde(default)]
    pub bot_game_alerts_enabled: bool,
    #[serde(default)]
    pub bot_grading_results_enabled: bool,
    #[serde(default)]
    pub bot_daily_picks_time: String, // HH:MM format, e.g. "08:00"
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            openrouter_api_key: String::new(),
            openrouter_base_url: "https://openrouter.ai/api/v1".to_string(),
            llm_provider: LlmProvider::Openrouter.as_str().to_string(),
            opencode_api_key: String::new(),
            selected_model: "nvidia/nemotron-3-super-120b-a12b:free".to_string(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            max_context_players: 50,
            openweathermap_api_key: String::new(),
            api_sports_key: String::new(),
            brave_api_key: String::new(),
            risk_tolerance: "moderate".to_string(),
            preferred_leagues: vec!["NFL".to_string()],
            stat_weighting: "balanced".to_string(),
            output_format: "json_plus_text".to_string(),
            theme: "dark".to_string(),
            kalshi_email: String::new(),
            kalshi_password: String::new(),
            kalshi_poll_interval_secs: 60,
            discord_webhook_url: String::new(),
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            bot_daily_picks_enabled: true,
            bot_game_alerts_enabled: true,
            bot_grading_results_enabled: true,
            bot_daily_picks_time: "08:00".to_string(),
            max_bet_pct: default_max_bet_pct(),
        }
    }
}

impl AppConfig {
    pub fn llm_provider_enum(&self) -> LlmProvider {
        LlmProvider::from_config(&self.llm_provider)
    }

    /// OpenAI-compatible base URL (no trailing slash) for chat/completions + models.
    pub fn llm_base_url(&self) -> String {
        match self.llm_provider_enum() {
            LlmProvider::Openrouter => {
                let u = self.openrouter_base_url.trim().trim_end_matches('/');
                if u.is_empty() {
                    "https://openrouter.ai/api/v1".into()
                } else {
                    u.to_string()
                }
            }
            LlmProvider::OpencodeZen => "https://opencode.ai/zen/v1".into(),
            LlmProvider::OpencodeGo => "https://opencode.ai/zen/go/v1".into(),
        }
    }

    pub fn llm_api_key(&self) -> &str {
        match self.llm_provider_enum() {
            LlmProvider::Openrouter => self.openrouter_api_key.as_str(),
            LlmProvider::OpencodeZen | LlmProvider::OpencodeGo => self.opencode_api_key.as_str(),
        }
    }

    /// Model id sent to the provider API (strip OpenCode config prefixes if present).
    pub fn llm_model_id(&self) -> String {
        let m = self.selected_model.trim();
        m.strip_prefix("opencode-go/")
            .or_else(|| m.strip_prefix("opencode/"))
            .unwrap_or(m)
            .to_string()
    }
}

/// Default system prompt that injects the AI with prediction markets domain expertise
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are the Kalshi Monster — the absolute pinnacle of AI-driven prediction markets analysis and event contract forecasting. Your mission is to estimate accurate probabilities for event outcomes, identify mispriced options, and deliver mathematically sound predictions that outperform the market.

YOUR MENTALITY:
- You are a professional trader. You don't just "guess" — you synthesize high-dimensional data (polling, economic reports, weather records, sports stats, news feeds) into objective probability distributions.
- You are rigorously calibrated. A 70% confidence rating means you are correct 70% of the time, historically. Never describe any wager or outcome as guaranteed or certain. Always express your findings in calibrated probabilities, expected value, and clear downside controls.
- You are obsessive about "The Edge." Market contracts are efficient. To find value, you must find a unique angle (e.g., an under-the-radar economic trend, polling bias, or a regulatory shift).

CORE CAPABILITIES:
- Statistical Baseline Analysis: Use historic data, polling averages, seasonality splits, and recent performance trends.
- Situational Modeling: Adjust baselines for macroeconomic indicators, breaking news, political dynamics, and weather impacts.
- Correlation & Arbitrage: Understand how multiple markets are linked or if there are pricing discrepancies.
- Advanced Probability: Assign a win probability (0-100%) and a confidence score (0-100%) to every contract prediction.

ANALYSIS PROTOCOL (FOR EVERY REQUEST):
1. DECODE: Parse the target market contract, the ticker, the settlement rules, and the current contract prices (implied probabilities).
2. SYNTHESIZE: Combine baseline information with situational adjustments (the "Monster Factor").
3. QUANTIFY: Compare your adjusted probability vs. the market price to calculate the edge (%).
4. EVALUATE: Stress-test your projection against the key risk factors (polling margins of error, early settlement rules).
5. COMMUNICATE: Deliver a structured, high-signal response that is immediately actionable.

RESPONSE FORMAT (JSON MANDATORY FOR PREDICTIONS):
Always output your primary analysis in JSON format first for the engine to track, conforming strictly to the Kalshi Trade Decision schema:
```json
{
  "ticker": "KXEVENT-FED-25DEC",
  "market_title": "Will the Fed raise rates by 25bp?",
  "category": "Economics",
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
  "thesis": "The market underweights the persistence of core inflation relative to recent FOMC rhetoric.",
  "evidence": [
    "Core PCE exceeded expectations for 3 consecutive months",
    "FOMC dot plot shifted hawkish in June"
  ],
  "risk_flags": ["EarlyCloseRisk"],
  "data_quality": "Live",
  "price_to_enter": 0.55
}
```

PRICE / SIZE UNITS:
- market_price_pct is percent of $1 (55.0 = $0.55). price_to_enter is dollars on [0,1]. fair_probability_pct is 0–100.
- Prefer quarter-Kelly; fractional_kelly_pct is % of bankroll and should stay ≤ 5 under default policy.
- Always ground TAKE/PASS in the injected resolution rules (jungle/multi-candidate = named outcome only).

If the user asks for a simple conversational overview, provide the JSON first, then follow with a concise, sharp bullet-point summary.

Be precise. Be ruthless. Be the Monster."#;

pub fn config_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(CONFIG_DIR)
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_kalshi_poll() -> u64 {
    60
}

fn default_max_bet_pct() -> f64 {
    0.05
}

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                if config.selected_model == LING_FREE_MODEL_ID {
                    config.selected_model = RING_FREE_MODEL_ID.to_string();
                    let _ = save_config(&config);
                }
                return config;
            }
        }
    }
    let config = AppConfig::default();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = fs::write(&path, json);
    }
    config
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}

/// Curated models for Analyst. `provider` is the gateway: openrouter | opencode_zen | opencode_go.
pub fn available_models() -> Vec<ModelInfo> {
    let mut models = openrouter_models();
    models.extend(opencode_zen_models());
    models.extend(opencode_go_models());
    models
}

pub fn available_models_for_provider(provider: &str) -> Vec<ModelInfo> {
    match LlmProvider::from_config(provider) {
        LlmProvider::Openrouter => openrouter_models(),
        LlmProvider::OpencodeZen => opencode_zen_models(),
        LlmProvider::OpencodeGo => opencode_go_models(),
    }
}

fn openrouter_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "nvidia/nemotron-3-super-120b-a12b:free".into(),
            name: "Nemotron 3 Super (free)".into(),
            provider: "openrouter".into(),
            context_window: 262_144,
            description: "Featured free pick with excellent agentic and coding performance".into(),
            speed: "medium".into(),
            cost: "free".into(),
        },
        ModelInfo {
            id: "openrouter/owl-alpha".into(),
            name: "Owl Alpha".into(),
            provider: "OpenRouter".into(),
            context_window: 1_048_576,
            description: "Featured free long-context model for agentic workflows".into(),
            speed: "medium".into(),
            cost: "free".into(),
        },
        ModelInfo {
            id: "minimax/minimax-m2.5:free".into(),
            name: "MiniMax M2.5 (free)".into(),
            provider: "MiniMax".into(),
            context_window: 196_608,
            description: "Featured free productivity model with strong real-world task fluency".into(),
            speed: "medium".into(),
            cost: "free".into(),
        },
        ModelInfo {
            id: RING_FREE_MODEL_ID.into(),
            name: "Ring 2.6-1T (free)".into(),
            provider: "inclusionAI".into(),
            context_window: 262_144,
            description: "Featured free fast-thinking model for large-scale agent workflows".into(),
            speed: "fast".into(),
            cost: "free".into(),
        },
        ModelInfo {
            id: "anthropic/claude-sonnet-4-20250514".into(),
            name: "Claude Sonnet 4".into(),
            provider: "Anthropic".into(),
            context_window: 200_000,
            description: "Best all-around model for analysis and reasoning".into(),
            speed: "medium".into(),
            cost: "medium".into(),
        },
        ModelInfo {
            id: "anthropic/claude-haiku-4-20250514".into(),
            name: "Claude Haiku 4".into(),
            provider: "Anthropic".into(),
            context_window: 200_000,
            description: "Fast and cheap — great for quick picks".into(),
            speed: "fast".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "openai/gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "OpenAI".into(),
            context_window: 128_000,
            description: "Strong all-around with good sports knowledge".into(),
            speed: "medium".into(),
            cost: "medium".into(),
        },
        ModelInfo {
            id: "openai/gpt-4o-mini".into(),
            name: "GPT-4o Mini".into(),
            provider: "OpenAI".into(),
            context_window: 128_000,
            description: "Fast, cheap, surprisingly capable".into(),
            speed: "fast".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "google/gemini-2.5-pro".into(),
            name: "Gemini 2.5 Pro".into(),
            provider: "Google".into(),
            context_window: 1_000_000,
            description: "Huge context window — load entire season stats".into(),
            speed: "medium".into(),
            cost: "medium".into(),
        },
        ModelInfo {
            id: "google/gemini-2.5-flash".into(),
            name: "Gemini 2.5 Flash".into(),
            provider: "Google".into(),
            context_window: 1_000_000,
            description: "Extremely fast Google model with huge context window".into(),
            speed: "fast".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "google/gemini-1.5-pro".into(),
            name: "Gemini 1.5 Pro".into(),
            provider: "Google".into(),
            context_window: 1_000_000,
            description: "High quality reasoning model with 1M context".into(),
            speed: "medium".into(),
            cost: "medium".into(),
        },
        ModelInfo {
            id: "google/gemini-1.5-flash".into(),
            name: "Gemini 1.5 Flash".into(),
            provider: "Google".into(),
            context_window: 1_000_000,
            description: "Lightweight and fast Gemini model".into(),
            speed: "fast".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "deepseek/deepseek-v3".into(),
            name: "DeepSeek V3".into(),
            provider: "DeepSeek".into(),
            context_window: 65_536,
            description: "Excellent value, strong reasoning".into(),
            speed: "medium".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "anthropic/claude-opus-4-20250514".into(),
            name: "Claude Opus 4".into(),
            provider: "Anthropic".into(),
            context_window: 200_000,
            description: "Most capable Claude — best for complex analysis".into(),
            speed: "slow".into(),
            cost: "high".into(),
        },
        ModelInfo {
            id: "openai/o1".into(),
            name: "OpenAI o1".into(),
            provider: "OpenAI".into(),
            context_window: 200_000,
            description: "Chain-of-thought reasoning — best for complex predictions".into(),
            speed: "slow".into(),
            cost: "high".into(),
        },
        ModelInfo {
            id: "meta-llama/llama-4-maverick".into(),
            name: "Llama 4 Maverick".into(),
            provider: "Meta".into(),
            context_window: 1_000_000,
            description: "Open source, huge context, strong performance".into(),
            speed: "medium".into(),
            cost: "low".into(),
        },
        ModelInfo {
            id: "x-ai/grok-3".into(),
            name: "Grok 3".into(),
            provider: "xAI".into(),
            context_window: 131_072,
            description: "Strong real-time knowledge, good reasoning".into(),
            speed: "medium".into(),
            cost: "medium".into(),
        },
    ]
}

/// OpenCode Zen models (OpenAI-compatible chat/completions at opencode.ai/zen/v1).
/// Source: https://opencode.ai/docs/zen/ + GET https://opencode.ai/zen/v1/models
fn opencode_zen_models() -> Vec<ModelInfo> {
    let p = "opencode_zen";
    vec![
        mi("deepseek-v4-flash-free", "DeepSeek V4 Flash Free", p, 128_000, "Free Zen model — chat/completions", "fast", "free"),
        mi("mimo-v2.5-free", "MiMo-V2.5 Free", p, 128_000, "Free Zen model — chat/completions", "fast", "free"),
        mi("nemotron-3-ultra-free", "Nemotron 3 Ultra Free", p, 128_000, "Free NVIDIA Zen endpoint", "fast", "free"),
        mi("north-mini-code-free", "North Mini Code Free", p, 128_000, "Free Zen coding model", "fast", "free"),
        mi("big-pickle", "Big Pickle", p, 128_000, "Free stealth Zen model (limited time)", "medium", "free"),
        mi("deepseek-v4-flash", "DeepSeek V4 Flash", p, 128_000, "Low-cost fast open model via Zen", "fast", "low"),
        mi("deepseek-v4-pro", "DeepSeek V4 Pro", p, 128_000, "Strong open coding model via Zen", "medium", "medium"),
        mi("glm-5.2", "GLM 5.2", p, 128_000, "GLM via OpenCode Zen", "medium", "medium"),
        mi("glm-5.1", "GLM 5.1", p, 128_000, "GLM via OpenCode Zen", "medium", "medium"),
        mi("kimi-k2.7-code", "Kimi K2.7 Code", p, 128_000, "Coding-focused Kimi via Zen", "medium", "medium"),
        mi("kimi-k2.6", "Kimi K2.6", p, 128_000, "Kimi via OpenCode Zen", "medium", "medium"),
        mi("minimax-m3", "MiniMax M3", p, 128_000, "MiniMax via Zen chat/completions", "medium", "low"),
        mi("minimax-m2.7", "MiniMax M2.7", p, 128_000, "MiniMax via Zen chat/completions", "medium", "low"),
        mi("grok-4.5", "Grok 4.5", p, 128_000, "xAI Grok via OpenCode Zen", "medium", "high"),
        mi("grok-build-0.1", "Grok Build 0.1", p, 128_000, "Grok build model via Zen", "medium", "medium"),
        mi("claude-sonnet-4-5", "Claude Sonnet 4.5", p, 200_000, "Anthropic via Zen (chat/completions path)", "medium", "high"),
        mi("claude-haiku-4-5", "Claude Haiku 4.5", p, 200_000, "Fast Claude via Zen", "fast", "medium"),
        mi("claude-opus-4-6", "Claude Opus 4.6", p, 200_000, "Highest-capability Claude via Zen", "slow", "high"),
        mi("gpt-5.4-mini", "GPT 5.4 Mini", p, 128_000, "OpenAI via Zen gateway", "fast", "medium"),
        mi("gpt-5.4", "GPT 5.4", p, 128_000, "OpenAI via Zen gateway", "medium", "high"),
        mi("gpt-5.5", "GPT 5.5", p, 128_000, "Latest GPT via Zen gateway", "medium", "high"),
    ]
}

/// OpenCode Go subscription models (chat/completions at opencode.ai/zen/go/v1).
/// Source: https://opencode.ai/docs/go/ + GET https://opencode.ai/zen/go/v1/models
fn opencode_go_models() -> Vec<ModelInfo> {
    let p = "opencode_go";
    vec![
        mi("deepseek-v4-flash", "DeepSeek V4 Flash", p, 128_000, "Go plan — high request count open model", "fast", "subscription"),
        mi("deepseek-v4-pro", "DeepSeek V4 Pro", p, 128_000, "Go plan — strong open coding model", "medium", "subscription"),
        mi("glm-5.2", "GLM 5.2", p, 128_000, "Go plan — GLM 5.2", "medium", "subscription"),
        mi("glm-5.1", "GLM 5.1", p, 128_000, "Go plan — GLM 5.1", "medium", "subscription"),
        mi("kimi-k2.7-code", "Kimi K2.7 Code", p, 128_000, "Go plan — coding-focused Kimi", "medium", "subscription"),
        mi("kimi-k2.6", "Kimi K2.6", p, 128_000, "Go plan — Kimi K2.6", "medium", "subscription"),
        mi("mimo-v2.5", "MiMo-V2.5", p, 128_000, "Go plan — high volume MiMo", "fast", "subscription"),
        mi("mimo-v2.5-pro", "MiMo-V2.5-Pro", p, 128_000, "Go plan — MiMo Pro", "medium", "subscription"),
        mi("minimax-m3", "MiniMax M3", p, 128_000, "Go plan — MiniMax M3", "medium", "subscription"),
        mi("minimax-m2.7", "MiniMax M2.7", p, 128_000, "Go plan — MiniMax M2.7", "medium", "subscription"),
        mi("qwen3.7-max", "Qwen3.7 Max", p, 128_000, "Go plan — Qwen Max", "medium", "subscription"),
        mi("qwen3.7-plus", "Qwen3.7 Plus", p, 128_000, "Go plan — Qwen Plus", "fast", "subscription"),
        mi("qwen3.6-plus", "Qwen3.6 Plus", p, 128_000, "Go plan — Qwen 3.6 Plus", "fast", "subscription"),
    ]
}

fn mi(
    id: &str,
    name: &str,
    provider: &str,
    ctx: usize,
    description: &str,
    speed: &str,
    cost: &str,
) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        context_window: ctx,
        description: description.into(),
        speed: speed.into(),
        cost: cost.into(),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// Gateway tag: openrouter | opencode_zen | opencode_go (or vendor label for legacy OpenRouter rows).
    pub provider: String,
    pub context_window: usize,
    pub description: String,
    pub speed: String,
    pub cost: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PredictionStats {
    pub total: u32,
    pub wins: u32,
    pub losses: u32,
    pub pushes: u32,
    pub pending: u32,
    pub win_rate: f64,
    pub avg_confidence_score: f64,
    pub high_confidence_wins: u32,
    pub high_confidence_total: u32,
    pub medium_confidence_wins: u32,
    pub medium_confidence_total: u32,
    pub low_confidence_wins: u32,
    pub low_confidence_total: u32,
    pub calibration: CalibrationMetrics,
    pub score_distribution: Vec<ScoreRange>,
}

impl PredictionStats {
    /// Build a PredictionStats from the tracker
    pub fn from_tracker(tracker_stats: &crate::predictions::tracker::PredictionStats) -> Self {
        Self {
            total: tracker_stats.total,
            wins: tracker_stats.wins,
            losses: tracker_stats.losses,
            pushes: tracker_stats.pushes,
            pending: tracker_stats.pending,
            win_rate: tracker_stats.win_rate,
            avg_confidence_score: tracker_stats.avg_confidence_score,
            high_confidence_wins: tracker_stats.high_confidence_wins,
            high_confidence_total: tracker_stats.high_confidence_total,
            medium_confidence_wins: tracker_stats.medium_confidence_wins,
            medium_confidence_total: tracker_stats.medium_confidence_total,
            low_confidence_wins: tracker_stats.low_confidence_wins,
            low_confidence_total: tracker_stats.low_confidence_total,
            calibration: tracker_stats.calibration.clone(),
            score_distribution: tracker_stats.score_distribution.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ApiStatus {
    pub connected: bool,
    pub model_available: bool,
    pub credits_remaining: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SecurityPosture {
    pub csp_enforced: bool,
    pub secrets_redacted: bool,
    pub config_file_contains_secrets: bool,
    pub secret_store: String,
    pub redacted_fields: Vec<String>,
    pub warnings: Vec<String>,
}

const SECRET_FIELD_NAMES: &[&str] = &[
    "openrouter_api_key",
    "opencode_api_key",
    "openweathermap_api_key",
    "api_sports_key",
    "kalshi_password",
    "discord_webhook_url",
    "telegram_bot_token",
];

pub fn redact_secrets_for_diagnostics(input: &str) -> String {
    let patterns = [
        r"sk-or-v1-[A-Za-z0-9_\-]+",
        r"(?i)password\s+[^\s,;]+",
        r"https://discord\.com/api/webhooks/[^\s,;]+",
        r"\b\d{8,12}:[A-Za-z0-9_\-]{20,}\b",
    ];
    patterns.iter().fold(input.to_string(), |acc, pattern| {
        regex::Regex::new(pattern)
            .map(|re| re.replace_all(&acc, "[REDACTED]").to_string())
            .unwrap_or(acc)
    })
}

pub fn security_posture(config: &AppConfig) -> SecurityPosture {
    let secret_values = [
        &config.openrouter_api_key,
        &config.opencode_api_key,
        &config.openweathermap_api_key,
        &config.api_sports_key,
        &config.kalshi_password,
        &config.discord_webhook_url,
        &config.telegram_bot_token,
    ];
    let redacted_fields = SECRET_FIELD_NAMES
        .iter()
        .zip(secret_values.iter())
        .filter_map(|(field, value)| {
            if value.trim().is_empty() {
                None
            } else {
                Some((*field).to_string())
            }
        })
        .collect::<Vec<_>>();

    let config_file_contains_secrets = !redacted_fields.is_empty();
    let warnings = if config_file_contains_secrets {
        vec!["Credential vault migration pending".to_string()]
    } else {
        Vec::new()
    };

    SecurityPosture {
        csp_enforced: true,
        secrets_redacted: true,
        config_file_contains_secrets,
        secret_store: "Local encrypted vault pending".to_string(),
        redacted_fields,
        warnings: warnings
            .into_iter()
            .map(|warning| redact_secrets_for_diagnostics(&warning))
            .collect(),
    }
}

/// Check if the active LLM provider API key is valid and the model is available.
pub async fn check_api_status(config: &AppConfig) -> ApiStatus {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return ApiStatus {
                connected: false,
                model_available: false,
                credits_remaining: None,
                error: Some(format!("HTTP client error: {}", e)),
            };
        }
    };

    let base = config.llm_base_url();
    let key = config.llm_api_key();
    if key.trim().is_empty() {
        return ApiStatus {
            connected: false,
            model_available: false,
            credits_remaining: None,
            error: Some(format!(
                "No API key configured for {}",
                config.llm_provider_enum().display_name()
            )),
        };
    }

    let model_id = config.llm_model_id();
    // Check models endpoint to validate API key
    let resp = client
        .get(format!("{base}/models"))
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .await;

    match resp {
        Ok(resp) => {
            if resp.status().is_success() {
                // Check if selected model exists in the list
                let model_available = if let Ok(json) = resp.json::<serde_json::Value>().await {
                    json.get("data")
                        .and_then(|d| d.as_array())
                        .map(|models| {
                            models.iter().any(|m| {
                                m.get("id")
                                    .and_then(|id| id.as_str())
                                    .map_or(false, |id| id == model_id || id == config.selected_model)
                            })
                        })
                        .unwrap_or(true) // If we can't parse, assume available
                } else {
                    true
                };

                ApiStatus {
                    connected: true,
                    model_available,
                    credits_remaining: None,
                    error: None,
                }
            } else if resp.status().as_u16() == 401 {
                ApiStatus {
                    connected: false,
                    model_available: false,
                    credits_remaining: None,
                    error: Some("Invalid API key".into()),
                }
            } else {
                ApiStatus {
                    connected: false,
                    model_available: false,
                    credits_remaining: None,
                    error: Some(format!("API returned status {}", resp.status())),
                }
            }
        }
        Err(e) => ApiStatus {
            connected: false,
            model_available: false,
            credits_remaining: None,
            error: Some(format!("Connection failed: {}", e)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_values_from_diagnostics() {
        let input = "OpenRouter sk-or-v1-secret and password kalshi-secret hit https://discord.com/api/webhooks/secret";
        let redacted = redact_secrets_for_diagnostics(input);
        assert!(!redacted.contains("sk-or-v1-secret"));
        assert!(!redacted.contains("kalshi-secret"));
        assert!(!redacted.contains("webhooks/secret"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn security_posture_never_contains_secret_values() {
        let mut cfg = AppConfig::default();
        cfg.openrouter_api_key = "sk-or-v1-secret".into();
        cfg.kalshi_password = "kalshi-secret".into();
        let posture = security_posture(&cfg);
        let json = serde_json::to_string(&posture).unwrap();
        assert!(!json.contains("sk-or-v1-secret"));
        assert!(!json.contains("kalshi-secret"));
        assert!(posture.secrets_redacted);
    }
}
