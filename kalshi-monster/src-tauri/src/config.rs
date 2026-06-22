use crate::predictions::tracker::CalibrationMetrics;
pub use crate::predictions::tracker::ScoreRange;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const CONFIG_DIR: &str = ".openclaw/kalshi-monster";
const CONFIG_FILE: &str = "config.json";
const RING_FREE_MODEL_ID: &str = "inclusionai/ring-2.6-1t:free";
const LING_FREE_MODEL_ID: &str = "inclusionai/ling-2.6-1t:free";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub openrouter_api_key: String,
    pub openrouter_base_url: String,
    pub selected_model: String,
    pub system_prompt: String,
    pub max_context_players: usize,
    // Kalshi data source configuration
    pub openweathermap_api_key: String,
    pub api_sports_key: String,
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
            selected_model: "nvidia/nemotron-3-super-120b-a12b:free".to_string(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            max_context_players: 50,
            openweathermap_api_key: String::new(),
            api_sports_key: String::new(),
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

/// Get list of popular OpenRouter models suitable for sports analysis
pub fn available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "nvidia/nemotron-3-super-120b-a12b:free".into(),
            name: "Nemotron 3 Super (free)".into(),
            provider: "NVIDIA".into(),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
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

/// Check if the OpenRouter API key is valid and the model is available
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

    // Check models endpoint to validate API key
    let resp = client
        .get(format!("{}/models", config.openrouter_base_url))
        .header("Authorization", format!("Bearer {}", config.openrouter_api_key))
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
                                    .map_or(false, |id| id == config.selected_model)
                            })
                        })
                        .unwrap_or(true) // If we can't parse, assume available
                } else {
                    true
                };

                ApiStatus {
                    connected: true,
                    model_available,
                    credits_remaining: None, // OpenRouter doesn't expose this in models endpoint
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
